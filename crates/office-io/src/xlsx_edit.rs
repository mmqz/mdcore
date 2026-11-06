//! XLSX 原位编辑引擎（#177——[`XlsxEditor`]，xlsx 版 DocxEditor）。
//!
//! ## 定位（与既有三面的边界）
//!
//! - **不做读-改-重建**：[`crate::io`] 读面（calamine）+ [`crate::write`]
//!   写面（rust_xlsxwriter 从零构建）的「重建」路径丢样式/文档元数据
//!   （C3 能力宣称边界 + FIDELITY v2 §4 定谳——结构性限制非实现
//!   缺陷）。本模块走**单元格级外科手术**（docx_edit 同款纪律）：
//!   解压 → 门 1/2/3（编辑面恒严格）→ workbook.xml + rels 解析表名 →
//!   `xl/worksheets/sheetN.xml` 单趟扫描建行/格索引 → 只改目标
//!   `<c>` 元素 → 其余 XML 原样 → 重打包（非目标部件 `raw_copy_file`
//!   逐字节透传）。
//! - **样式/合并/列宽保真机制**：目标格的 `s` 属性（样式索引）原样
//!   保留；`<cols>`/`<mergeCells>`/styles.xml/sharedStrings.xml 等
//!   非手术区零接触（字节透传）——保真不靠「会写」而靠「不碰」。
//!
//! ## v1 编辑面（单元格级，#177 最小可行口径）
//!
//! - 值三形态：文本（[`CellEdit::Text`]——恒写 inlineStr，见下）/数字
//!   （[`CellEdit::Number`]）/布尔（[`CellEdit::Boolean`]）。
//! - **公式格拒改**（[`XlsxEditError::FormulaCell`]——`<f>` 子元素在
//!   场即拒，含 shared formula 自闭合形）：不做公式重算（#177 边界），
//!   缓存值与公式结构零接触。公式编辑 = 后续加性扩展（docx 域编辑
//!   #140 同构路线）。
//! - **文本恒写 inlineStr**（`t="inlineStr"` + `<is><t>`）：Excel/
//!   LibreOffice/calamine 全支持；sharedStrings.xml 完全不动（旧
//!   shared 串可能留置为无引用串——无害垃圾，部件自洽性不变）。
//!   新格插行/插格同理——**绝不追加 sharedStrings.xml**（那会动
//!   第二个部件 + count/uniqueCount 对账面）。
//! - 行内格插入（目标格所在 `<row>` 在场但 `<c>` 缺席）与整行插入
//!   （`<row>` 缺席——按行号升序落位）均支持；`<sheetData/>` 自闭合
//!   空形态展开为正常形态。
//!
//! ## 已知边界（v1 登记——宁缺毋错）
//!
//! - `dimension`/`row/@spans` 是优化 hint（ECMA-376 非规范强制），
//!   插入超出声明范围的格**不更新**它们——Excel/LibreOffice 打开
//!   时自算（xlsxwriter 亦可省略两属性，宽容面实测）；
//! - `t="s"` 格编辑后旧串留置（上述无害垃圾）；
//! - 多表名映射同一 worksheet 部件的畸形簿：两张表都有待落账编辑时
//!   save 拒（[`XlsxEditError::InvalidInput`]——对部件二次手术的坐标
//!   系漂移面，真实工作簿不重名部件）；
//! - chartsheet/macroSheet 等非 worksheets 目录部件的表：可列出、
//!   拒改（[`XlsxEditError::NotEditable`]）；
//! - 控制字符（XML 1.0 非法面）与 U+FFFE/U+FFFF typed 拒；NaN/Inf
//!   拒（docx_edit 文本约定同口径——`_xHHHH_` 转义是加性扩展面）。
//!
//! ## 落账模型（save 时一次性落账——docx_edit 同构）
//!
//! 编辑存**待落账态**（`pending`），`rows`/cells 索引恒为 open() 时
//! 的原文坐标系——`cell_info` 读 pending 优先；同一格后写胜。
//! `save_to_vec` 对每张有编辑的表做单趟手术（编辑格互不重叠——
//! 单元格坐标唯一性保证；插入按行/列升序落位），未编辑区域与
//! 未编辑部件零接触。零编辑 save = 全部件 raw_copy 透传。
//!
//! ## 容器防线（与读路径同门，恒严格——docx_edit 同口径）
//!
//! 门 0（`io::LIMIT_INPUT_BYTES` 输入上限）→ 门 1（`io::prescan`
//! xlsx 族声明值预扫）→ 门 2+3（`io::verify_gates` 尺寸诚实 + 部件
//! 良构——**编辑面不放松**：结构畸形部件拒绝编辑，不做兼容降级）
//! → `xl/workbook.xml` 在场（fail-closed）。扫描沿用 quick-xml 0.41
//! `buffer_position` 补偿口径（docx_edit fwd_to_lt/fwd_to_gt/
//! back_to_lt 同款）。

use std::collections::{BTreeMap, HashMap};
use std::io::Read as _;

use crate::io;
use quick_xml::events::Event;

// ---------------------------------------------------------------------------
// 公开类型
// ---------------------------------------------------------------------------

/// 单元格写值（v1 三形态——#177 最小可行口径；与 [`crate::write`]
/// 的 CellValue 同名不共享：写面含公式/样式复合语义，编辑面是窄面）。
#[derive(Debug, Clone, PartialEq)]
pub enum CellEdit {
    /// 文本（写为 inlineStr——模块文档「恒写 inlineStr」节）。
    Text(String),
    /// 数值（有限 f64；NaN/Inf typed 拒于 set 时）。
    Number(f64),
    /// 布尔。
    Boolean(bool),
}

/// 单元格当前值（读面——`cell_info` 产物；pending 优先，原文坐标
/// 系兜底）。文本形态 = 存储面忠实转译（shared string 解析后 /
/// v 原文 / inline 文本），不做显示语义（numFmt/日期面外——
/// [`crate::io::CellData`] 的三分纪律同源：原始值优先）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellInfo {
    /// 文本形态的当前值（shared 串解析 / v 原文 / inline 文本；
    /// 数字格 = v 原文串；空格 = 空串）。
    pub text: String,
    /// 公式格（`<f>` 在场——含 shared 自闭合无文本形）。
    pub is_formula: bool,
    /// 公式文本（`<f>` 元素文本；shared 形/无文本 = None——
    /// 读面公式文本不带 `=` 前缀，office-io FIDELITY §1 口径）。
    pub formula: Option<String>,
    /// 样式索引（`<c>@s` 原文；缺省 None）。
    pub style: Option<String>,
    /// 单元格类型属性原文（`<c>@t`——s/inlineStr/str/b/e/d/缺省）。
    pub t: Option<String>,
}

/// 工作表条目（`sheets()` 产物——簿序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetInfo {
    /// 表名（workbook.xml `@name` 原文）。
    pub name: String,
    /// 可编辑（表名解析到 worksheets 部件；chartsheet/断链 = false
    /// ——可列可读、拒改）。
    pub editable: bool,
}

/// 编辑面错误（拒绝/容器/部件/IO 四分——`crate::io::ReadError`
/// 与 `crate::write::WriteError` 的编辑面同构风格）。
#[derive(Debug)]
pub enum XlsxEditError {
    /// 容器防线拒绝（门 0/1/2/3——与读路径同一错误面）。
    Container(io::ReadError),
    /// 部件缺失/不可解析（fail-closed）。
    InvalidPart { part: String, detail: String },
    /// 输入验证失败（坐标/值/形态——typed，可修输入重试）。
    InvalidInput(String),
    /// 公式格拒改（v1 边界——含 shared formula 自闭合形）。
    FormulaCell { sheet: String, cell: String },
    /// 表不可编辑（无 worksheets 部件/非 worksheet 面/部件断链）。
    NotEditable { sheet: String, detail: String },
    /// 重打包 IO 失败（zip 层）。
    Io(String),
}

impl std::fmt::Display for XlsxEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XlsxEditError::Container(e) => write!(f, "容器防线拒绝: {e}"),
            XlsxEditError::InvalidPart { part, detail } => {
                write!(f, "部件不可用: {part}: {detail}")
            }
            XlsxEditError::InvalidInput(m) => write!(f, "无效输入: {m}"),
            XlsxEditError::FormulaCell { sheet, cell } => {
                write!(f, "公式格拒改（v1 不做公式重算）: {sheet}!{cell}")
            }
            XlsxEditError::NotEditable { sheet, detail } => {
                write!(f, "表不可编辑: {sheet}: {detail}")
            }
            XlsxEditError::Io(m) => write!(f, "IO 错误: {m}"),
        }
    }
}

impl std::error::Error for XlsxEditError {}

impl From<io::ReadError> for XlsxEditError {
    fn from(e: io::ReadError) -> Self {
        XlsxEditError::Container(e)
    }
}

// ---------------------------------------------------------------------------
// 坐标解析（A1 形态——XFD1048576 边界）
// ---------------------------------------------------------------------------

/// A1 坐标 → (行 1 基, 列 0 基)。合法面：1-3 个字母 + 1-7 位数字、
/// 列 ≤ XFD(16384)、行 ≤ 1048576；否则 None（typed 拒入口）。
pub(crate) fn parse_cell_ref(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut col: u32 = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        col = col
            .checked_mul(26)?
            .checked_add(u32::from(bytes[i].to_ascii_uppercase() - b'A' + 1))?;
        i += 1;
        if i > 3 {
            return None;
        }
    }
    if i == 0 || col == 0 {
        return None;
    }
    let mut row: u32 = 0;
    let mut digits = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        row = row
            .checked_mul(10)?
            .checked_add(u32::from(bytes[i] - b'0'))?;
        i += 1;
        digits += 1;
        if digits > 7 {
            return None;
        }
    }
    if i != bytes.len() || row == 0 {
        return None;
    }
    if col > 16_384 || row > 1_048_576 {
        return None;
    }
    Some((row, col - 1))
}

/// 列号（0 基）→ 字母（0→A、25→Z、26→AA、16383→XFD）。
pub(crate) fn col_letters(mut col0: u32) -> String {
    col0 += 1;
    let mut out = String::new();
    while col0 > 0 {
        let rem = ((col0 - 1) % 26) as u8;
        out.insert(0, char::from(b'A' + rem));
        col0 = (col0 - 1) / 26;
    }
    out
}

// ---------------------------------------------------------------------------
// 值校验与序列化
// ---------------------------------------------------------------------------

/// 文本面校验（docx_edit 文本约定同口径）：XML 1.0 非法控制字符
/// （<0x20 除 \t\n\r）与 U+FFFE/U+FFFF 非字符 typed 拒。
fn validate_text(s: &str) -> Result<(), XlsxEditError> {
    for ch in s.chars() {
        let c = ch as u32;
        let illegal_ctrl = c < 0x20 && c != 0x09 && c != 0x0A && c != 0x0D;
        if illegal_ctrl || c == 0xFFFE || c == 0xFFFF {
            return Err(XlsxEditError::InvalidInput(format!(
                "文本含 XML 1.0 非法字符 U+{c:04X}（控制字符/非字符面 v1 拒）"
            )));
        }
    }
    Ok(())
}

/// 文本实体转义（#276 审计 B-6：收敛到 mdc-office 消毒面单源
/// xml_entities::escape_xml_text——与 docx_edit 同批，消除三源漂移；
/// 旧本地实现 `&`/`<`/`>` 语义逐字符一致）。
fn escape_text(s: &str) -> String {
    mdc_office::sanitize::xml_entities::escape_xml_text(s)
}

/// 数字 → v 文本（Rust f64 Display = 最短往返表示；整数值无小数尾
/// ——42.0 → "42"，Excel 数值面惯例；NaN/Inf 已在 set 时拒）。
fn fmt_number(n: f64) -> String {
    format!("{n}")
}

/// 新 `<c>` 元素（保留原 `s` 属性——样式索引零接触；ref 恒规范
/// 大写形态）。
fn emit_cell(ref_str: &str, s: Option<&str>, v: &CellEdit) -> String {
    let s_attr = match s {
        Some(x) => format!(" s=\"{}\"", escape_text(x)),
        None => String::new(),
    };
    match v {
        CellEdit::Text(t) => format!(
            "<c r=\"{ref_str}\"{s_attr} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            escape_text(t)
        ),
        CellEdit::Number(n) => format!("<c r=\"{ref_str}\"{s_attr}><v>{}</v></c>", fmt_number(*n)),
        CellEdit::Boolean(b) => format!(
            "<c r=\"{ref_str}\"{s_attr} t=\"b\"><v>{}</v></c>",
            if *b { 1 } else { 0 }
        ),
    }
}

// ---------------------------------------------------------------------------
// 扫描模型（原文坐标系——open 后不变）
// ---------------------------------------------------------------------------

/// `<c>` 元素扫描条目。
#[derive(Debug, Clone)]
struct CellScan {
    /// 元素节点域（含开闭标签；自闭合为全域）。
    node: (usize, usize),
    /// 列号（0 基——`@r` 解析；缺省推断）。
    col: u32,
    /// `@s` 样式索引原文。
    s: Option<String>,
    /// `@t` 类型属性原文。
    t: Option<String>,
    /// `<f>` 子元素在场（含 shared 自闭合形——set 拒改锚）。
    has_formula: bool,
    /// `<f>` 元素文本（shared 形/无文本 = None）。
    formula: Option<String>,
    /// `<v>` 元素文本（反转义后）。
    v_text: Option<String>,
    /// inline string `<is><t>` 文本（反转义后）。
    is_text: Option<String>,
}

/// `<row>` 元素扫描条目。
#[derive(Debug, Clone)]
struct RowScan {
    /// 元素节点域。
    node: (usize, usize),
    /// 行号（`@r`；缺省 = 前行 +1 推断——Excel 产物恒显式）。
    r: u32,
    /// 开标签尾（内容区起点；自闭合形无意义）。
    content_open: usize,
    /// 闭标签 `<` 位（内容区终点；自闭合形无意义）。
    content_close: usize,
    /// 自闭合 `<row .../>`。
    self_closed: bool,
    /// 行内 `<c>`（文档序）。
    cells: Vec<CellScan>,
}

/// `<sheetData>` 几何（手术锚）。
#[derive(Debug, Clone)]
struct SheetDataGeom {
    /// 元素节点域。
    node: (usize, usize),
    /// 开标签尾（正常形内容区起点）。
    content_open: usize,
    /// 闭标签 `<` 位（正常形内容区终点）。
    content_close: usize,
    /// 自闭合 `<sheetData/>`（空表形态——手术时展开）。
    self_closed: bool,
}

/// 单表扫描产物（原文坐标系）。
#[derive(Debug)]
struct ScannedSheet {
    /// sheet 部件原文（UTF-8）。
    xml: String,
    /// 行（文档序）。
    rows: Vec<RowScan>,
    /// 行号 → rows 下标（重复 r 后写胜——畸形防御）。
    row_idx: HashMap<u32, usize>,
    /// sheetData 几何（缺失 = None = 该表拒改）。
    sheetdata: Option<SheetDataGeom>,
}

/// 工作簿表条目（open 时建立；`scanned` 惰性——首次触及该表才扫）。
#[derive(Debug)]
struct SheetEntry {
    /// 表名（workbook.xml `@name`）。
    name: String,
    /// worksheet 部件 zip 名（如 `xl/worksheets/sheet1.xml`）；
    /// None = 解析不到/非 worksheets 面/断链——可列拒改。
    part: Option<String>,
    /// 惰性扫描产物。
    scanned: Option<ScannedSheet>,
}

// ---------------------------------------------------------------------------
// 位置补偿（quick-xml 0.41 buffer_position 边界口径——docx_edit
// fwd_to_lt/fwd_to_gt/back_to_lt 同款复制：两模块各自私有，不建
// 跨格式抽象（三域纪律），语义恒同（起点前向修 '<'、终点前向修
// '>'、闭标签起点由终点回扫）。
// ---------------------------------------------------------------------------

fn fwd_to_lt(bytes: &[u8], from: usize) -> usize {
    let mut i = from.min(bytes.len());
    while i < bytes.len() && bytes[i] != b'<' {
        i += 1;
    }
    i
}

fn fwd_to_gt(bytes: &[u8], from: usize) -> usize {
    let mut i = from.min(bytes.len());
    if i > 0 && bytes[i - 1] == b'>' {
        return i;
    }
    while i < bytes.len() && bytes[i] != b'>' {
        i += 1;
    }
    if i < bytes.len() {
        i += 1;
    }
    i
}

fn back_to_lt(bytes: &[u8], end_after: usize) -> usize {
    let mut i = end_after.min(bytes.len());
    while i > 0 {
        if bytes[i - 1] == b'<' {
            return i - 1;
        }
        i -= 1;
    }
    0
}

fn local_name(e: &quick_xml::events::BytesStart<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn local_name_end(e: &quick_xml::events::BytesEnd<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

/// 按完整属性名取值（含前缀形态如 `r:id`——quick-xml 属性名不展开
/// 前缀，原文匹配；实体反转义）。
fn attr_raw(e: &quick_xml::events::BytesStart<'_>, want: &str) -> Option<String> {
    for a in e.attributes().with_checks(false) {
        let a = a.ok()?;
        if a.key.as_ref() == want.as_bytes() {
            return Some(String::from_utf8_lossy(&a.value).into_owned());
        }
    }
    None
}

fn part_err(part: &str, detail: impl Into<String>) -> XlsxEditError {
    XlsxEditError::InvalidPart {
        part: part.to_string(),
        detail: detail.into(),
    }
}

// ---------------------------------------------------------------------------
// workbook.xml / rels 扫描（表名 → 部件映射）
// ---------------------------------------------------------------------------

/// workbook.xml `<sheet>` 条目（name + r:id）。
fn scan_workbook_sheets(xml: &str) -> Vec<(String, Option<String>)> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local_name(&e) == "sheet" {
                    let name = attr_raw(&e, "name").unwrap_or_default();
                    let rid = attr_raw(&e, "r:id");
                    out.push((name, rid));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break, // 门 3 已保证良构——此处防御性截断
        }
    }
    out
}

/// rels `<Relationship>` 条目（Id → Target；外部引用 = None——
/// TargetMode="External" 不入部件面）。
fn scan_rels(xml: &str) -> Vec<(String, Option<String>)> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local_name(&e) == "Relationship" {
                    let id = attr_raw(&e, "Id").unwrap_or_default();
                    let target = attr_raw(&e, "Target");
                    let external = attr_raw(&e, "TargetMode")
                        .map(|m| m.eq_ignore_ascii_case("external"))
                        .unwrap_or(false);
                    out.push((id, if external { None } else { target }));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    out
}

/// rels Target → zip 部件名（相对 `xl/` 基；`/` 前缀 = 包根绝对；
/// `..`/`.` 段正规化，逃逸包根 = None）。**只接受 worksheets 目录**
/// （倒数第二段；chartsheet/macroSheet/dialogSheet → None 拒改）。
fn resolve_worksheet_part(target: &str) -> Option<String> {
    let t = target.trim();
    if t.is_empty() {
        return None;
    }
    let mut segs: Vec<&str> = Vec::new();
    if let Some(stripped) = t.strip_prefix('/') {
        segs.extend(stripped.split('/'));
    } else {
        segs.push("xl");
        segs.extend(t.split('/'));
    }
    let mut norm: Vec<&str> = Vec::new();
    for s in segs {
        match s {
            ".." => {
                norm.pop()?; // 逃逸包根 = None
            }
            "." | "" => {}
            _ => norm.push(s),
        }
    }
    let is_worksheet = norm.len() >= 2
        && norm[norm.len() - 2].eq_ignore_ascii_case("worksheets")
        && norm[norm.len() - 1].ends_with(".xml");
    if is_worksheet {
        Some(norm.join("/"))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// sheet.xml 单趟扫描（行/格索引 + sheetData 几何）
// ---------------------------------------------------------------------------

fn scan_sheet(part: &str, xml: &str) -> Result<ScannedSheet, XlsxEditError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let bytes = xml.as_bytes();
    let mut rows: Vec<RowScan> = Vec::new();
    let mut sheetdata: Option<SheetDataGeom> = None;
    // 开放元素游标（sheetData 内 row 内 c——良构保证不嵌套错位）。
    let mut open_row: Option<usize> = None;
    let mut open_cell: Option<(usize, usize)> = None;
    // 文本收集槽（f/v/is 内 t——wt_open 模式）。
    let mut f_open: Option<(usize, usize)> = None;
    let mut v_open: Option<(usize, usize)> = None;
    let mut t_open: Option<(usize, usize)> = None;
    let mut in_is = false;
    // @r 缺省推断（文档序 +1；Excel 产物恒显式——防御面）。
    let mut last_row_no: u32 = 0;
    let mut last_col_no: u32 = 0;

    macro_rules! close_text {
        ($open:expr, $dest:expr, $part:expr, $what:literal) => {
            if let Some((node_start, open)) = $open.take() {
                let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                let close_start = back_to_lt(bytes, end);
                if close_start >= open {
                    let raw = &xml[open..close_start];
                    let decoded = quick_xml::escape::unescape(raw).map_err(|e| {
                        part_err($part, format!("{} text entity decode failed: {e}", $what))
                    })?;
                    $dest = Some(decoded.into_owned());
                }
                let _ = node_start;
            }
        };
    }

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "sheetData" => {
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        // Start 事件：闭标签几何在 End 填充。
                        sheetdata = Some(SheetDataGeom {
                            node: (node_start, 0),
                            content_open,
                            content_close: 0,
                            self_closed: false,
                        });
                    }
                    "row" => {
                        if open_row.is_some() {
                            return Err(part_err(part, "nested <row> (malformed)"));
                        }
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        let r = attr_raw(&e, "r")
                            .and_then(|v| v.trim().parse::<u32>().ok())
                            .unwrap_or_else(|| last_row_no.saturating_add(1));
                        last_row_no = r;
                        rows.push(RowScan {
                            node: (node_start, 0),
                            r,
                            content_open,
                            content_close: 0,
                            self_closed: false,
                            cells: Vec::new(),
                        });
                        open_row = Some(rows.len() - 1);
                        last_col_no = 0;
                    }
                    "c" => {
                        let ri = open_row
                            .ok_or_else(|| part_err(part, "<c> outside <row> (malformed)"))?;
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        let col = attr_raw(&e, "r")
                            .and_then(|v| parse_cell_ref(&v).map(|(_, c)| c))
                            .unwrap_or_else(|| last_col_no.saturating_add(1));
                        last_col_no = col;
                        rows[ri].cells.push(CellScan {
                            node: (node_start, 0),
                            col,
                            s: attr_raw(&e, "s"),
                            t: attr_raw(&e, "t"),
                            has_formula: false,
                            formula: None,
                            v_text: None,
                            is_text: None,
                        });
                        open_cell = Some((ri, rows[ri].cells.len() - 1));
                        in_is = false;
                        // content_open = c 开标签尾（text 槽基点）。
                        let _ = content_open;
                    }
                    "f" => {
                        if let Some((ri, ci)) = open_cell {
                            rows[ri].cells[ci].has_formula = true;
                            let node_start = fwd_to_lt(bytes, pos_before);
                            let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                            f_open = Some((node_start, content_open));
                        }
                    }
                    "v" => {
                        if open_cell.is_some() {
                            let node_start = fwd_to_lt(bytes, pos_before);
                            let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                            v_open = Some((node_start, content_open));
                        }
                    }
                    "is" => {
                        in_is = true;
                    }
                    "t" if in_is && open_cell.is_some() => {
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        t_open = Some((node_start, content_open));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "sheetData" => {
                        // 自闭合空表形态：`<sheetData/>`——节点完整。
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let node_end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        sheetdata = Some(SheetDataGeom {
                            node: (node_start, node_end),
                            content_open: node_end,
                            content_close: node_end,
                            self_closed: true,
                        });
                    }
                    "row" => {
                        // 自闭合空行（带格式属性无格）。
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let node_end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        let r = attr_raw(&e, "r")
                            .and_then(|v| v.trim().parse::<u32>().ok())
                            .unwrap_or_else(|| last_row_no.saturating_add(1));
                        last_row_no = r;
                        rows.push(RowScan {
                            node: (node_start, node_end),
                            r,
                            content_open: node_end,
                            content_close: node_end,
                            self_closed: true,
                            cells: Vec::new(),
                        });
                        last_col_no = 0;
                    }
                    "c" => {
                        let ri = open_row
                            .ok_or_else(|| part_err(part, "<c> outside <row> (malformed)"))?;
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let node_end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        let col = attr_raw(&e, "r")
                            .and_then(|v| parse_cell_ref(&v).map(|(_, c)| c))
                            .unwrap_or_else(|| last_col_no.saturating_add(1));
                        last_col_no = col;
                        rows[ri].cells.push(CellScan {
                            node: (node_start, node_end),
                            col,
                            s: attr_raw(&e, "s"),
                            t: attr_raw(&e, "t"),
                            has_formula: false,
                            formula: None,
                            v_text: None,
                            is_text: None,
                        });
                    }
                    "f" => {
                        // shared formula 自闭合形：`<f t="shared" si="0"/>`
                        // ——无文本但为公式格。
                        if let Some((ri, ci)) = open_cell {
                            rows[ri].cells[ci].has_formula = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name_end(&e);
                match name.as_str() {
                    "sheetData" => {
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        if let Some(g) = sheetdata.as_mut() {
                            g.node.1 = end;
                            g.content_close = back_to_lt(bytes, end);
                        }
                    }
                    "row" => {
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        if let Some(ri) = open_row.take() {
                            rows[ri].node.1 = end;
                            rows[ri].content_close = back_to_lt(bytes, end);
                        }
                    }
                    "c" => {
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        if let Some((ri, ci)) = open_cell.take() {
                            rows[ri].cells[ci].node.1 = end;
                        }
                        in_is = false;
                    }
                    "f" => {
                        if let Some((ri, ci)) = open_cell {
                            close_text!(f_open, rows[ri].cells[ci].formula, part, "f");
                        }
                    }
                    "v" => {
                        if let Some((ri, ci)) = open_cell {
                            close_text!(v_open, rows[ri].cells[ci].v_text, part, "v");
                        }
                    }
                    "t" => {
                        if let Some((ri, ci)) = open_cell {
                            if in_is {
                                let mut dest: Option<String> = None;
                                close_text!(t_open, dest, part, "is/t");
                                if let Some(t) = dest {
                                    rows[ri].cells[ci].is_text = Some(format!(
                                        "{}{}",
                                        rows[ri].cells[ci].is_text.clone().unwrap_or_default(),
                                        t
                                    ));
                                }
                            }
                        }
                    }
                    "is" => {
                        in_is = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(part_err(part, format!("sheet scan failed: {e}")));
            }
        }
    }
    if sheetdata.is_none() {
        // sheetData 缺失 = 非法 worksheet（ECMA 必选元素）——该表拒改。
        return Err(part_err(part, "sheetData element missing"));
    }
    let mut row_idx = HashMap::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        row_idx.insert(r.r, i);
    }
    Ok(ScannedSheet {
        xml: xml.to_string(),
        rows,
        row_idx,
        sheetdata,
    })
}

// ---------------------------------------------------------------------------
// sharedStrings 惰性解析（cell_info 读面用——写面恒不触此部件）
// ---------------------------------------------------------------------------

fn scan_shared_strings(xml: &str) -> Vec<String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let bytes = xml.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut in_si = false;
    let mut t_open: Option<(usize, usize)> = None;
    let mut cur = String::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => match local_name(&e).as_str() {
                "si" => {
                    in_si = true;
                    cur = String::new();
                }
                "t" if in_si => {
                    let node_start = fwd_to_lt(bytes, pos_before);
                    let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                    t_open = Some((node_start, content_open));
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if local_name(&e).as_str() == "si" {
                    out.push(String::new()); // 自闭合空条目（占索引位）
                }
            }
            Ok(Event::End(e)) => match local_name_end(&e).as_str() {
                "si" => {
                    in_si = false;
                    out.push(std::mem::take(&mut cur));
                }
                "t" => {
                    if let Some((_, open)) = t_open.take() {
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        let close_start = back_to_lt(bytes, end);
                        if close_start >= open {
                            if let Ok(decoded) =
                                quick_xml::escape::unescape(&xml[open..close_start])
                            {
                                cur.push_str(decoded.as_ref());
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break, // 读面容错（门 3 严格模式已保证良构——防御）
        }
    }
    out
}

// ---------------------------------------------------------------------------
// XlsxEditor
// ---------------------------------------------------------------------------

/// XLSX 原位编辑器（模块文档——单元格级读改写，其余零接触）。
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use mdc_office_io::xlsx_edit::{CellEdit, XlsxEditor};
/// let bytes = std::fs::read("a.xlsx")?;
/// let mut ed = XlsxEditor::open(&bytes)?;
/// let sheet = ed.sheets().iter().position(|s| s.name == "Sheet1")
///     .ok_or("no Sheet1")?;
/// ed.set_cell(sheet, "B2", CellEdit::Text("新值".into()))?;
/// std::fs::write("a.xlsx", ed.save_to_vec()?)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct XlsxEditor {
    /// 原始字节（open 时的输入——save 重打包原料，恒不变）。
    bytes: Vec<u8>,
    /// 表条目（簿序；扫描惰性）。
    sheets: Vec<SheetEntry>,
    /// sharedStrings 解析缓存（惰性——首次 cell_info 命中 t="s"）。
    shared: Vec<String>,
    shared_loaded: bool,
    /// 待落账编辑（与 sheets 等长；(行, 列) BTreeMap = 行列序天然有序）。
    pending: Vec<BTreeMap<(u32, u32), CellEdit>>,
}

impl XlsxEditor {
    /// 打开工作簿（门 0/1/2/3 恒严格 + workbook.xml 在场 fail-closed）。
    pub fn open(bytes: &[u8]) -> Result<Self, XlsxEditError> {
        if bytes.len() as u64 > io::LIMIT_INPUT_BYTES {
            return Err(XlsxEditError::Container(io::ReadError::PrescanRejected {
                reason: "input size over limit",
                entry: None,
            }));
        }
        let cursor = std::io::Cursor::new(bytes);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| io::ReadError::ContainerCorrupt(format!("{e}")))?;
        io::prescan(&mut za)?;
        let mut warnings = Vec::new();
        io::verify_gates(&mut za, true, &mut warnings)?;
        drop(za);

        // 重开 zip 读 workbook.xml + rels（门已过——by_name 直接读）。
        let cursor = std::io::Cursor::new(bytes);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| io::ReadError::ContainerCorrupt(format!("{e}")))?;
        let mut wb_xml = String::new();
        match za.by_name("xl/workbook.xml") {
            Ok(mut zf) => {
                zf.read_to_string(&mut wb_xml)
                    .map_err(|e| part_err("xl/workbook.xml", format!("read/UTF-8 failed: {e}")))?;
            }
            Err(_) => {
                return Err(part_err("xl/workbook.xml", "part missing (fail-closed)"));
            }
        }
        let mut rels_xml = String::new();
        if let Ok(mut zf) = za.by_name("xl/_rels/workbook.xml.rels") {
            zf.read_to_string(&mut rels_xml)
                .map_err(|e| part_err("xl/_rels/workbook.xml.rels", format!("read failed: {e}")))?;
        }
        let rels = scan_rels(&rels_xml);
        let sheet_meta = scan_workbook_sheets(&wb_xml);
        let sheets: Vec<SheetEntry> = sheet_meta
            .into_iter()
            .map(|(name, rid)| {
                let part = rid
                    .as_ref()
                    .and_then(|id| rels.iter().find(|(i, _)| i == id))
                    .and_then(|(_, t)| t.clone())
                    .and_then(|t| resolve_worksheet_part(&t));
                SheetEntry {
                    name,
                    part,
                    scanned: None,
                }
            })
            .collect();
        let n = sheets.len();
        Ok(Self {
            bytes: bytes.to_vec(),
            sheets,
            shared: Vec::new(),
            shared_loaded: false,
            pending: vec![BTreeMap::new(); n],
        })
    }

    /// 表清单（簿序——与 `cell_info`/`set_cell` 的 sheet 下标同源）。
    pub fn sheets(&self) -> Vec<SheetInfo> {
        self.sheets
            .iter()
            .map(|s| SheetInfo {
                name: s.name.clone(),
                editable: s.part.is_some(),
            })
            .collect()
    }

    /// 单元格当前值（pending 优先；空格/越界 = None）。
    ///
    /// shared string 惰性解析（首次命中 t="s" 才读 sharedStrings.xml
    /// ——编辑面恒不写该部件）。
    pub fn cell_info(
        &mut self,
        sheet: usize,
        cell_ref: &str,
    ) -> Result<Option<CellInfo>, XlsxEditError> {
        let (row, col) = parse_cell_ref(cell_ref)
            .ok_or_else(|| XlsxEditError::InvalidInput(format!("非法单元格坐标: {cell_ref}")))?;
        if sheet >= self.sheets.len() {
            return Err(XlsxEditError::InvalidInput(format!(
                "表下标越界: {sheet}（共 {} 表）",
                self.sheets.len()
            )));
        }
        self.ensure_scanned(sheet)?;
        // 字段拆借（bytes 读 + sheets 写不冲突）。
        let Self {
            bytes,
            sheets,
            shared,
            shared_loaded,
            pending,
        } = self;
        let entry = &mut sheets[sheet];

        // pending 优先（落账模型）。
        if let Some(v) = pending[sheet].get(&(row, col)) {
            let style =
                find_cell(entry.scanned.as_ref().unwrap(), row, col).and_then(|c| c.s.clone());
            let text = match v {
                CellEdit::Text(t) => t.clone(),
                CellEdit::Number(n) => fmt_number(*n),
                CellEdit::Boolean(b) => (if *b { "TRUE" } else { "FALSE" }).to_string(),
            };
            return Ok(Some(CellInfo {
                text,
                is_formula: false,
                formula: None,
                style,
                t: None,
            }));
        }

        let Some(cs) = find_cell(entry.scanned.as_ref().unwrap(), row, col) else {
            return Ok(None);
        };
        // t="s" → shared 索引解析（惰性装载）。
        let text = if cs.t.as_deref() == Some("s") {
            if !*shared_loaded {
                let mut sxml = String::new();
                let cursor = std::io::Cursor::new(&bytes[..]);
                let mut za = zip::ZipArchive::new(cursor)
                    .map_err(|e| io::ReadError::ContainerCorrupt(format!("{e}")))?;
                if let Ok(mut zf) = za.by_name("xl/sharedStrings.xml") {
                    zf.read_to_string(&mut sxml).map_err(|e| {
                        part_err("xl/sharedStrings.xml", format!("read failed: {e}"))
                    })?;
                    *shared = scan_shared_strings(&sxml);
                }
                *shared_loaded = true;
            }
            cs.v_text
                .as_deref()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .and_then(|i| shared.get(i).cloned())
                .unwrap_or_default()
        } else if cs.t.as_deref() == Some("inlineStr") {
            cs.is_text.clone().unwrap_or_default()
        } else {
            cs.v_text.clone().unwrap_or_default()
        };
        Ok(Some(CellInfo {
            text,
            is_formula: cs.has_formula,
            formula: cs.formula.clone(),
            style: cs.s.clone(),
            t: cs.t.clone(),
        }))
    }

    /// 设置单元格值（待落账——`save_to_vec` 时手术）。后写胜。
    ///
    /// 拒改面：公式格（含 shared 自闭合形）/ 控制字符 / NaN/Inf /
    /// 非法坐标 / 无 worksheet 部件的表。
    pub fn set_cell(
        &mut self,
        sheet: usize,
        cell_ref: &str,
        value: CellEdit,
    ) -> Result<(), XlsxEditError> {
        let (row, col) = parse_cell_ref(cell_ref)
            .ok_or_else(|| XlsxEditError::InvalidInput(format!("非法单元格坐标: {cell_ref}")))?;
        if sheet >= self.sheets.len() {
            return Err(XlsxEditError::InvalidInput(format!(
                "表下标越界: {sheet}（共 {} 表）",
                self.sheets.len()
            )));
        }
        match &value {
            CellEdit::Text(t) => validate_text(t)?,
            CellEdit::Number(n) => {
                if !n.is_finite() {
                    return Err(XlsxEditError::InvalidInput(
                        "数值 NaN/Inf 拒（XML 数值面不可表达）".into(),
                    ));
                }
            }
            CellEdit::Boolean(_) => {}
        }
        self.ensure_scanned(sheet)?;
        let entry = &self.sheets[sheet];
        if entry.part.is_none() {
            return Err(XlsxEditError::NotEditable {
                sheet: entry.name.clone(),
                detail: "无 worksheets 部件（chartsheet/断链/路径逃逸）".into(),
            });
        }
        let scanned = entry.scanned.as_ref().unwrap();
        if let Some(cs) = find_cell(scanned, row, col) {
            if cs.has_formula {
                return Err(XlsxEditError::FormulaCell {
                    sheet: entry.name.clone(),
                    cell: format!("{}{}", col_letters(col), row),
                });
            }
        }
        self.pending[sheet].insert((row, col), value);
        Ok(())
    }

    /// 保存（待落账手术 + 非目标部件逐字节透传；零编辑 = 恒等重打包）。
    pub fn save_to_vec(&self) -> Result<Vec<u8>, XlsxEditError> {
        let mut replacements: Vec<(String, Vec<u8>)> = Vec::new();
        for (i, entry) in self.sheets.iter().enumerate() {
            if self.pending[i].is_empty() {
                continue;
            }
            let part = entry.part.as_ref().unwrap();
            // 畸形防御：多表名映射同一部件且都有编辑 → 拒（模块文档
            // 边界节）。
            if replacements.iter().any(|(p, _)| p == part) {
                return Err(XlsxEditError::InvalidInput(format!(
                    "多表映射同一部件且均有编辑（{part}）——v1 拒，避免坐标系漂移"
                )));
            }
            let scanned = entry.scanned.as_ref().unwrap();
            let new_xml = apply_cell_edits(&entry.name, scanned, &self.pending[i])?;
            replacements.push((part.clone(), new_xml.into_bytes()));
        }
        repack_xlsx(&self.bytes, &replacements)
    }

    /// 惰性扫描某表部件（幂等）。
    fn ensure_scanned(&mut self, sheet: usize) -> Result<(), XlsxEditError> {
        if self.sheets[sheet].scanned.is_some() {
            return Ok(());
        }
        // 字段拆借：bytes 只读 + sheets[sheet] 可变。
        let Self { bytes, sheets, .. } = self;
        let entry = &mut sheets[sheet];
        if entry.part.is_none() {
            // 无部件表不扫（set 拒；cell_info 返回 None 兜底——空扫描
            // 产物即可）。
            entry.scanned = Some(ScannedSheet {
                xml: String::new(),
                rows: Vec::new(),
                row_idx: HashMap::new(),
                sheetdata: None,
            });
            return Ok(());
        }
        let part = entry.part.clone().unwrap();
        let cursor = std::io::Cursor::new(&bytes[..]);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| io::ReadError::ContainerCorrupt(format!("{e}")))?;
        let mut xml = String::new();
        match za.by_name(&part) {
            Ok(mut zf) => {
                zf.read_to_string(&mut xml)
                    .map_err(|e| part_err(&part, format!("read/UTF-8 failed: {e}")))?;
            }
            Err(_) => {
                return Err(XlsxEditError::NotEditable {
                    sheet: entry.name.clone(),
                    detail: format!("部件不在 zip（rels 断链）: {part}"),
                });
            }
        }
        entry.scanned = Some(scan_sheet(&part, &xml)?);
        Ok(())
    }
}

/// 行列定位（row_idx + 行内线性列扫——行内格数真实文件个位到十位级）。
fn find_cell(scanned: &ScannedSheet, row: u32, col: u32) -> Option<&CellScan> {
    let ri = *scanned.row_idx.get(&row)?;
    scanned.rows[ri].cells.iter().find(|c| c.col == col)
}

// ---------------------------------------------------------------------------
// 手术（全部待落账编辑 → 新 sheet.xml）
// ---------------------------------------------------------------------------

/// 行内格发射（替换 + 按列序插入；row 编辑集按 col 升序——BTreeMap
/// 保证）。`cells` = 该行 (col, &CellEdit) 序列。
fn emit_row_cells(out: &mut String, xml: &str, row: &RowScan, edits: &[(u32, &CellEdit)]) {
    let mut it = edits.iter().peekable();
    for cell in &row.cells {
        // 列序插入：先于本格的编辑格发射。
        while let Some((c, _)) = it.peek() {
            if *c < cell.col {
                let (c, v) = *it.next().unwrap();
                out.push_str(&emit_cell(&format!("{}{}", col_letters(c), row.r), None, v));
            } else {
                break;
            }
        }
        if let Some((c, _)) = it.peek() {
            if *c == cell.col {
                // 替换：保留原 s 属性（样式零接触）。
                let (c, v) = *it.next().unwrap();
                out.push_str(&emit_cell(
                    &format!("{}{}", col_letters(c), row.r),
                    cell.s.as_deref(),
                    v,
                ));
                continue;
            }
        }
        // 未编辑格：字节透传。
        out.push_str(&xml[cell.node.0..cell.node.1]);
    }
    // 行尾追加（编辑列大于行内全部既有列）。
    for (c, v) in it {
        out.push_str(&emit_cell(
            &format!("{}{}", col_letters(*c), row.r),
            None,
            v,
        ));
    }
}

/// 单趟手术：全部编辑 → 新 sheet.xml（未编辑区域零接触）。
fn apply_cell_edits(
    sheet_name: &str,
    scanned: &ScannedSheet,
    edits: &BTreeMap<(u32, u32), CellEdit>,
) -> Result<String, XlsxEditError> {
    let Some(geom) = scanned.sheetdata.as_ref() else {
        return Err(XlsxEditError::NotEditable {
            sheet: sheet_name.to_string(),
            detail: "sheetData 元素缺失（非法 worksheet——拒改）".into(),
        });
    };
    let xml = &scanned.xml;
    // 编辑按行分组（BTreeMap 键序 = 行升序、行内列升序）。
    let mut edits_by_row: BTreeMap<u32, Vec<(u32, &CellEdit)>> = BTreeMap::new();
    for ((r, c), v) in edits {
        edits_by_row.entry(*r).or_default().push((*c, v));
    }

    let mut out = String::with_capacity(xml.len() + 256);
    out.push_str(&xml[..geom.node.0]);
    if geom.self_closed {
        // 自闭合空表展开：`<sheetData/>` → `<sheetData>…</sheetData>`
        // （全部为行插入）。
        out.push_str("<sheetData>");
        for (r, cells) in &edits_by_row {
            let mut row_xml = String::new();
            row_xml.push_str(&format!("<row r=\"{r}\">"));
            for (c, v) in cells {
                row_xml.push_str(&emit_cell(&format!("{}{}", col_letters(*c), r), None, v));
            }
            row_xml.push_str("</row>");
            out.push_str(&row_xml);
        }
        out.push_str("</sheetData>");
    } else {
        // 原开标签保留（含属性/空白原文）。
        out.push_str(&xml[geom.node.0..geom.content_open]);
        for row in &scanned.rows {
            // 行插入：行号小于本行的缺失行编辑先落（Excel 行序升序——
            // 首个 r 更大行前插入，等价正确落位）。
            let before: Vec<u32> = edits_by_row.range(..row.r).map(|(k, _)| *k).collect();
            for r in before {
                let cells = edits_by_row.remove(&r).unwrap();
                let mut row_xml = format!("<row r=\"{r}\">");
                for (c, v) in &cells {
                    row_xml.push_str(&emit_cell(&format!("{}{}", col_letters(*c), r), None, v));
                }
                row_xml.push_str("</row>");
                out.push_str(&row_xml);
            }
            match edits_by_row.remove(&row.r) {
                Some(edits) => {
                    // 本行手术：开标签原文 + 格序列 + 闭标签原文。
                    if row.self_closed {
                        // `<row r="5"/>` → 展开（属性段保留）。
                        let inner = &xml[row.node.0..row.node.1 - 1]; // 除 '>'
                        let attrs = inner.strip_suffix('/').unwrap_or(inner);
                        out.push_str(attrs);
                        out.push('>');
                        emit_row_cells(&mut out, xml, row, &edits);
                        out.push_str("</row>");
                    } else {
                        out.push_str(&xml[row.node.0..row.content_open]);
                        emit_row_cells(&mut out, xml, row, &edits);
                        out.push_str(&xml[row.content_close..row.node.1]);
                    }
                }
                None => {
                    // 未编辑行：字节透传。
                    out.push_str(&xml[row.node.0..row.node.1]);
                }
            }
        }
        // 行尾：行号大于全部既有行的插入行。
        for (r, cells) in &edits_by_row {
            let mut row_xml = format!("<row r=\"{r}\">");
            for (c, v) in cells {
                row_xml.push_str(&emit_cell(&format!("{}{}", col_letters(*c), r), None, v));
            }
            row_xml.push_str("</row>");
            out.push_str(&row_xml);
        }
        out.push_str(&xml[geom.content_close..geom.node.1]);
    }
    out.push_str(&xml[geom.node.1..]);
    Ok(out)
}

// ---------------------------------------------------------------------------
// 重打包（docx_edit repack_parts 同构——替换部件重写 + 其余
// raw_copy_file 逐字节透传）
// ---------------------------------------------------------------------------

fn repack_xlsx(bytes: &[u8], replacements: &[(String, Vec<u8>)]) -> Result<Vec<u8>, XlsxEditError> {
    use std::io::Write as _;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| XlsxEditError::Io(format!("open zip: {e}")))?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let name = {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| XlsxEditError::Io(format!("zip index: {e}")))?;
            entry.name().to_string()
        };
        match replacements.iter().find(|(p, _)| *p == name) {
            Some((_, data)) => {
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| XlsxEditError::Io(format!("zip start: {e}")))?;
                writer
                    .write_all(data)
                    .map_err(|e| XlsxEditError::Io(format!("zip write: {e}")))?;
            }
            None => {
                let entry = archive
                    .by_index_raw(i)
                    .map_err(|e| XlsxEditError::Io(format!("zip index: {e}")))?;
                writer
                    .raw_copy_file(entry)
                    .map_err(|e| XlsxEditError::Io(format!("zip copy: {e}")))?;
            }
        }
    }
    let out = writer
        .finish()
        .map_err(|e| XlsxEditError::Io(format!("zip finish: {e}")))?;
    Ok(out.into_inner())
}

// ---------------------------------------------------------------------------
// 测试（核心面：坐标/路径解析 + 手术形态 + 拒改面 + 零接触透传；
// 保真矩阵与真实语料往返 = tests/xlsx_edit_roundtrip.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── 夹具基建（最小 xlsx zip——部件全可控）──────────────────

    const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/worksheets/sheet2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>";
    const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";

    fn workbook_xml(sheets: &[(&str, &str)]) -> String {
        let items: Vec<String> = sheets
            .iter()
            .map(|(name, rid)| format!("<sheet name=\"{name}\" sheetId=\"1\" r:id=\"{rid}\"/>"))
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>{}</sheets></workbook>",
            items.join("")
        )
    }

    fn rels_xml(entries: &[(&str, &str)]) -> String {
        let items: Vec<String> = entries
            .iter()
            .map(|(id, target)| {
                format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"{target}\"/>")
            })
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
            items.join("")
        )
    }

    fn sheet1_xml(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{body}</sheetData></worksheet>"
        )
    }

    /// parts：(zip 名, 内容)。workbook/rels 由调用方给全。
    fn build_zip(parts: &[(&str, String)]) -> Vec<u8> {
        use std::io::Write as _;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
        let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut e = ar.by_name(name).unwrap();
        let mut v = vec![];
        std::io::Read::read_to_end(&mut e, &mut v).unwrap();
        v
    }

    /// 标准两表夹具（sheet1 富格 + sheet2 普通）。
    fn build_fixture() -> Vec<u8> {
        let sheet1 = sheet1_xml(
            "<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" s=\"3\" t=\"s\"><v>1</v></c><c r=\"C1\" s=\"7\"><v>3.5</v></c></row>\
             <row r=\"2\"><c r=\"A2\" s=\"3\"><v>10</v></c><c r=\"B2\" t=\"b\"><v>1</v></c></row>\
             <row r=\"4\"><c r=\"A4\"><f>SUM(A1:A2)</f><v>10</v></c></row>"
        );
        let sheet2 = sheet1_xml("<row r=\"1\"><c r=\"A1\"><v>1</v></c></row>");
        build_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES.to_string()),
            ("_rels/.rels", ROOT_RELS.to_string()),
            (
                "xl/workbook.xml",
                workbook_xml(&[("Data", "rId1"), ("Aux", "rId2")]),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                rels_xml(&[
                    ("rId1", "worksheets/sheet1.xml"),
                    ("rId2", "worksheets/sheet2.xml"),
                ]),
            ),
            ("xl/worksheets/sheet1.xml", sheet1),
            ("xl/worksheets/sheet2.xml", sheet2),
        ])
    }

    // ── 坐标/路径解析 ─────────────────────────────────────────

    #[test]
    fn cell_ref_parsing_boundaries() {
        assert_eq!(parse_cell_ref("A1"), Some((1, 0)));
        assert_eq!(parse_cell_ref("XFD1048576"), Some((1048576, 16383)));
        assert_eq!(parse_cell_ref("AA27"), Some((27, 26)));
        assert_eq!(parse_cell_ref(" b2 "), Some((2, 1)), "前后空白容忍");
        assert_eq!(parse_cell_ref("A0"), None, "行 0 非法");
        assert_eq!(parse_cell_ref("A1048577"), None, "行越界");
        assert_eq!(parse_cell_ref("XFE1"), None, "列越界（XFD 之后）");
        assert_eq!(parse_cell_ref("1A"), None, "字母前缀缺失");
        assert_eq!(parse_cell_ref("A"), None, "数字缺失");
        assert_eq!(parse_cell_ref(""), None);
        assert_eq!(parse_cell_ref("AAAA1"), None, "超 3 字母");
        assert_eq!(parse_cell_ref("A1B"), None, "尾部垃圾");
        for (col, letters) in [
            (0u32, "A"),
            (25, "Z"),
            (26, "AA"),
            (701, "ZZ"),
            (702, "AAA"),
            (16383, "XFD"),
        ] {
            assert_eq!(col_letters(col), letters, "col {col}");
        }
    }

    #[test]
    fn worksheet_part_resolution() {
        assert_eq!(
            resolve_worksheet_part("worksheets/sheet1.xml"),
            Some("xl/worksheets/sheet1.xml".into()),
            "相对 xl/ 基"
        );
        assert_eq!(
            resolve_worksheet_part("/xl/worksheets/sheet1.xml"),
            Some("xl/worksheets/sheet1.xml".into()),
            "包根绝对"
        );
        assert_eq!(
            resolve_worksheet_part("xl/../worksheets/sheet9.xml"),
            Some("xl/worksheets/sheet9.xml".into()),
            ".. 段正规化"
        );
        assert_eq!(resolve_worksheet_part("../escape.xml"), None, "逃逸包根");
        assert_eq!(
            resolve_worksheet_part("chartsheets/sheet1.xml"),
            None,
            "chartsheet 拒"
        );
        assert_eq!(resolve_worksheet_part(""), None);
    }

    // ── 基本流：替换/插格/插行 + 零接触 ───────────────────────

    #[test]
    fn replace_cell_three_value_forms() {
        let bytes = build_fixture();
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        let sheets = ed.sheets();
        assert_eq!(sheets[0].name, "Data");
        assert!(sheets[0].editable);
        // 三形态替换：数字格 / 布尔格 / 新文本（shared 串格 → inlineStr）。
        ed.set_cell(0, "A1", CellEdit::Text("改后标题".into()))
            .unwrap();
        ed.set_cell(0, "C1", CellEdit::Number(42.0)).unwrap();
        ed.set_cell(0, "B2", CellEdit::Boolean(false)).unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
        assert!(
            xml.contains(
                "<c r=\"A1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">改后标题</t></is></c>"
            ),
            "shared 串格改文本 → inlineStr：{xml}"
        );
        assert!(
            xml.contains("<c r=\"C1\" s=\"7\"><v>42</v></c>"),
            "数字替换保留 s 属性：{xml}"
        );
        assert!(
            xml.contains("<c r=\"B2\" t=\"b\"><v>0</v></c>"),
            "布尔 false → v=0：{xml}"
        );
        // 零接触断言：未编辑格/行 + sheet2 + 容器部件逐字节恒等。
        assert!(
            xml.contains("<c r=\"B1\" s=\"3\" t=\"s\"><v>1</v></c>"),
            "未编辑格透传"
        );
        assert!(
            xml.contains("<row r=\"4\"><c r=\"A4\"><f>SUM(A1:A2)</f><v>10</v></c></row>"),
            "公式行透传"
        );
        assert_eq!(
            part_bytes(&saved, "xl/worksheets/sheet2.xml"),
            part_bytes(&bytes, "xl/worksheets/sheet2.xml"),
            "未编辑表部件字节恒等"
        );
        assert_eq!(
            part_bytes(&saved, "xl/workbook.xml"),
            part_bytes(&bytes, "xl/workbook.xml"),
            "workbook.xml 字节恒等"
        );
    }

    #[test]
    fn zero_edit_save_is_byte_identical() {
        let bytes = build_fixture();
        let ed = XlsxEditor::open(&bytes).unwrap();
        let saved = ed.save_to_vec().unwrap();
        assert_eq!(
            saved, bytes,
            "零编辑 = 全部件 raw_copy 透传（文件级字节恒等）"
        );
    }

    #[test]
    fn insert_cell_into_existing_row_keeps_order() {
        let bytes = build_fixture();
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        // B2 与 A2 之间无格：row 2 有 A2/B2，C2 缺 → 列序落位 B2 后。
        ed.set_cell(0, "C2", CellEdit::Number(7.0)).unwrap();
        // A1 前插（col 0 最小）。
        ed.set_cell(0, "A1", CellEdit::Number(1.0)).unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
        let row2 = xml
            .split("<row r=\"2\"")
            .nth(1)
            .unwrap()
            .split("</row>")
            .next()
            .unwrap();
        let pos_a = row2.find("r=\"A2\"").unwrap();
        let pos_b = row2.find("r=\"B2\"").unwrap();
        let pos_c = row2.find("r=\"C2\"").unwrap();
        assert!(pos_a < pos_b && pos_b < pos_c, "行内列升序：{row2}");
        assert!(
            row2.contains("<c r=\"C2\"><v>7</v></c>"),
            "新格形态：{row2}"
        );
    }

    #[test]
    fn insert_missing_row_at_sorted_position() {
        let bytes = build_fixture();
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        // 行 3 完全缺失（fixture 行序 1/2/4）→ 插在 2 与 4 之间。
        ed.set_cell(0, "B3", CellEdit::Text("新行".into())).unwrap();
        // 行 9（大于全部既有行）→ sheetData 尾。
        ed.set_cell(0, "A9", CellEdit::Number(1.0)).unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
        let p2 = xml.find("<row r=\"2\"").unwrap();
        let p3 = xml.find("<row r=\"3\"").unwrap();
        let p4 = xml.find("<row r=\"4\"").unwrap();
        let p9 = xml.find("<row r=\"9\"").unwrap();
        assert!(p2 < p3 && p3 < p4 && p4 < p9, "行序升序落位");
        assert!(xml.contains("<row r=\"3\"><c r=\"B3\" t=\"inlineStr\"><is><t xml:space=\"preserve\">新行</t></is></c></row>"), "新行形态");
    }

    #[test]
    fn selfclosed_sheetdata_expands() {
        let sheet1 = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData/></worksheet>";
        let bytes = build_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES.to_string()),
            ("_rels/.rels", ROOT_RELS.to_string()),
            ("xl/workbook.xml", workbook_xml(&[("Empty", "rId1")])),
            (
                "xl/_rels/workbook.xml.rels",
                rels_xml(&[("rId1", "worksheets/sheet1.xml")]),
            ),
            ("xl/worksheets/sheet1.xml", sheet1.to_string()),
        ]);
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        ed.set_cell(0, "A1", CellEdit::Boolean(true)).unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
        assert!(
            xml.contains(
                "<sheetData><row r=\"1\"><c r=\"A1\" t=\"b\"><v>1</v></c></row></sheetData>"
            ),
            "自闭合展开：{xml}"
        );
    }

    #[test]
    fn selfclosed_row_expands_with_attrs() {
        let sheet1 = sheet1_xml("<row r=\"1\" ht=\"18\" customHeight=\"1\"/><row r=\"2\"/>");
        let bytes = build_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES.to_string()),
            ("_rels/.rels", ROOT_RELS.to_string()),
            ("xl/workbook.xml", workbook_xml(&[("S", "rId1")])),
            (
                "xl/_rels/workbook.xml.rels",
                rels_xml(&[("rId1", "worksheets/sheet1.xml")]),
            ),
            ("xl/worksheets/sheet1.xml", sheet1),
        ]);
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        ed.set_cell(0, "A1", CellEdit::Number(5.0)).unwrap();
        ed.set_cell(0, "B2", CellEdit::Number(6.0)).unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
        assert!(
            xml.contains(
                "<row r=\"1\" ht=\"18\" customHeight=\"1\"><c r=\"A1\"><v>5</v></c></row>"
            ),
            "自闭合行展开保留属性：{xml}"
        );
        assert!(
            xml.contains("<row r=\"2\"><c r=\"B2\"><v>6</v></c></row>"),
            "无属性自闭合行展开：{xml}"
        );
    }

    // ── 拒改面 ────────────────────────────────────────────────

    #[test]
    fn formula_cell_rejected() {
        let bytes = build_fixture();
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        // A4 = <f>SUM</f> 公式格 → 拒。
        let err = ed.set_cell(0, "A4", CellEdit::Number(1.0)).unwrap_err();
        assert!(matches!(err, XlsxEditError::FormulaCell { .. }), "{err}");
        // shared formula 自闭合形（f 无文本）同拒。
        let sheet1 =
            sheet1_xml("<row r=\"1\"><c r=\"A1\"><f t=\"shared\" si=\"0\"/><v>1</v></c></row>");
        let bytes2 = build_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES.to_string()),
            ("_rels/.rels", ROOT_RELS.to_string()),
            ("xl/workbook.xml", workbook_xml(&[("S", "rId1")])),
            (
                "xl/_rels/workbook.xml.rels",
                rels_xml(&[("rId1", "worksheets/sheet1.xml")]),
            ),
            ("xl/worksheets/sheet1.xml", sheet1),
        ]);
        let mut ed2 = XlsxEditor::open(&bytes2).unwrap();
        let err2 = ed2.set_cell(0, "A1", CellEdit::Number(2.0)).unwrap_err();
        assert!(matches!(err2, XlsxEditError::FormulaCell { .. }), "{err2}");
    }

    #[test]
    fn invalid_values_rejected() {
        let bytes = build_fixture();
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        assert!(matches!(
            ed.set_cell(0, "A1", CellEdit::Number(f64::NAN)),
            Err(XlsxEditError::InvalidInput(_))
        ));
        assert!(matches!(
            ed.set_cell(0, "A1", CellEdit::Number(f64::INFINITY)),
            Err(XlsxEditError::InvalidInput(_))
        ));
        assert!(matches!(
            ed.set_cell(0, "A1", CellEdit::Text("\u{0001}bad".into())),
            Err(XlsxEditError::InvalidInput(_))
        ));
        assert!(matches!(
            ed.set_cell(0, "A1", CellEdit::Text("\u{FFFE}".into())),
            Err(XlsxEditError::InvalidInput(_))
        ));
        assert!(matches!(
            ed.set_cell(0, "XFE1", CellEdit::Number(1.0)),
            Err(XlsxEditError::InvalidInput(_))
        ));
        assert!(matches!(
            ed.set_cell(9, "A1", CellEdit::Number(1.0)),
            Err(XlsxEditError::InvalidInput(_))
        ));
        // 合法面：\t\n\r 保留（XML 1.0 预定义）。
        ed.set_cell(0, "A1", CellEdit::Text("a\tb\nc".into()))
            .unwrap();
    }

    #[test]
    fn non_worksheet_sheet_rejected() {
        // chartsheet 形态表：可列、拒改。
        let bytes = build_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES.to_string()),
            ("_rels/.rels", ROOT_RELS.to_string()),
            (
                "xl/workbook.xml",
                workbook_xml(&[("Data", "rId1"), ("Chart", "rId9")]),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                rels_xml(&[
                    ("rId1", "worksheets/sheet1.xml"),
                    ("rId9", "chartsheets/sheet1.xml"),
                ]),
            ),
            (
                "xl/worksheets/sheet1.xml",
                sheet1_xml("<row r=\"1\"><c r=\"A1\"><v>1</v></c></row>"),
            ),
        ]);
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        let sheets = ed.sheets();
        assert!(sheets[0].editable, "worksheets 面可编辑");
        assert!(!sheets[1].editable, "chartsheet 不可编辑");
        let err = ed.set_cell(1, "A1", CellEdit::Number(1.0)).unwrap_err();
        assert!(matches!(err, XlsxEditError::NotEditable { .. }), "{err}");
        // 无部件表 cell_info → None 兜底（不 panic）。
        assert!(ed.cell_info(1, "A1").unwrap().is_none());
    }

    #[test]
    fn broken_container_rejected() {
        assert!(matches!(
            XlsxEditor::open(b"not a zip"),
            Err(XlsxEditError::Container(_))
        ));
        // 非 xlsx zip（无 workbook.xml）→ fail-closed。
        let bytes = build_zip(&[("[Content_Types].xml", CONTENT_TYPES.to_string())]);
        assert!(matches!(
            XlsxEditor::open(&bytes),
            Err(XlsxEditError::InvalidPart { .. })
        ));
    }

    // ── cell_info 读面 ────────────────────────────────────────

    #[test]
    fn cell_info_reads_all_storage_forms() {
        let shared = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"3\" uniqueCount=\"3\"><si><t>标题</t></si><si><r><t>富文本</t></r><r><t>拼接</t></r></si><si><t>第三串</t></si></sst>";
        let sheet1 = sheet1_xml(
            "<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\" s=\"2\"><v>1</v></c>\
             <c r=\"C1\" t=\"inlineStr\"><is><t>内联</t></is></c><c r=\"D1\"><v>3.25</v></c>\
             <c r=\"E1\" t=\"b\"><v>1</v></c><c r=\"F1\" t=\"e\"><v>#DIV/0!</v></c>\
             <c r=\"G1\"><f>A1&amp;B1</f><v>10</v></c><c r=\"H1\" s=\"9\"/></row>",
        );
        let bytes = build_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES.to_string()),
            ("_rels/.rels", ROOT_RELS.to_string()),
            ("xl/workbook.xml", workbook_xml(&[("S", "rId1")])),
            (
                "xl/_rels/workbook.xml.rels",
                rels_xml(&[("rId1", "worksheets/sheet1.xml")]),
            ),
            ("xl/sharedStrings.xml", shared.to_string()),
            ("xl/worksheets/sheet1.xml", sheet1),
        ]);
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        let a1 = ed.cell_info(0, "A1").unwrap().unwrap();
        assert_eq!(a1.text, "标题");
        assert!(!a1.is_formula);
        let b1 = ed.cell_info(0, "B1").unwrap().unwrap();
        assert_eq!(b1.text, "富文本拼接", "rich text 多 r/t 拼接");
        assert_eq!(b1.style.as_deref(), Some("2"));
        let c1 = ed.cell_info(0, "C1").unwrap().unwrap();
        assert_eq!(c1.text, "内联");
        assert_eq!(c1.t.as_deref(), Some("inlineStr"));
        let d1 = ed.cell_info(0, "D1").unwrap().unwrap();
        assert_eq!(d1.text, "3.25");
        let e1 = ed.cell_info(0, "E1").unwrap().unwrap();
        assert_eq!(e1.text, "1");
        let f1 = ed.cell_info(0, "F1").unwrap().unwrap();
        assert_eq!(f1.text, "#DIV/0!");
        let g1 = ed.cell_info(0, "G1").unwrap().unwrap();
        assert!(g1.is_formula);
        assert_eq!(g1.formula.as_deref(), Some("A1&B1"), "实体反转义");
        let h1 = ed.cell_info(0, "H1").unwrap().unwrap();
        assert_eq!(h1.text, "", "styled 空格");
        assert_eq!(h1.style.as_deref(), Some("9"));
        // 越界/空格 → None。
        assert!(ed.cell_info(0, "Z99").unwrap().is_none());
        // pending 优先：set 后 cell_info 读新值。
        ed.set_cell(0, "A1", CellEdit::Text("新".into())).unwrap();
        let a1 = ed.cell_info(0, "A1").unwrap().unwrap();
        assert_eq!(a1.text, "新");
        assert!(!a1.is_formula);
        // 后写胜。
        ed.set_cell(0, "A1", CellEdit::Number(8.0)).unwrap();
        assert_eq!(ed.cell_info(0, "A1").unwrap().unwrap().text, "8");
    }

    #[test]
    fn multi_sheet_edits_and_xml_declaration_kept() {
        let bytes = build_fixture();
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        ed.set_cell(0, "A1", CellEdit::Number(1.0)).unwrap();
        ed.set_cell(1, "A1", CellEdit::Text("二表".into())).unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml1 = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
        let xml2 = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet2.xml")).unwrap();
        assert!(xml1.starts_with("<?xml version=\"1.0\""), "XML 声明保留");
        assert!(xml1.contains("<c r=\"A1\"><v>1</v></c>"));
        assert!(xml2.contains("二表"));
    }

    #[test]
    fn escaped_and_special_text_roundtrip_form() {
        let bytes = build_fixture();
        let mut ed = XlsxEditor::open(&bytes).unwrap();
        ed.set_cell(0, "A1", CellEdit::Text("<b>&amp; \"quote\" </t>".into()))
            .unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
        assert!(
            xml.contains("&lt;b&gt;&amp;amp; &quot;quote&quot; &lt;/t&gt;")
                || xml.contains("&lt;b&gt;&amp;amp; \"quote\" &lt;/t&gt;"),
            "实体转义形态：{xml}"
        );
    }
}
