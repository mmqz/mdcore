//! 多级列表自动编号（S1 任务 2：MultiLevelNumbering + 自动计算序号）。
//!
//! ## 定位与边界
//!
//! `word/numbering.xml` 的完整层级承载（读面）+ **序号自动计算器**
//! （[`NumberingTracker`]——Word 多级编号语义）+ 写面自定义多级编号
//! 定义（[`DocxWriter::define_numbering`]/[`DocxWriter::
//! numbered_list_item`]，本模块对 [`crate::docx::DocxWriter`] 的分文件
//! impl）。
//!
//! 与 [`crate::docx`] 既有编号面（r4-t3 numPr 读面承载
//! [`crate::docx::ParagraphNumbering`] + t20 写面列表抽象）**互补**：
//! - 既有面 = 段落级 `w:numPr` 透传 + 二分列表抽象（ordered decimal
//!   outline / bullet `•`——t20 固定几何）；
//! - 本模块 = **定义面全承载**（start/lvlText/lvlRestart/isLgl/suffix/
//!   pStyle/multiLevelType + num 桥 + lvlOverride 双形态）与**序号
//!   计算**（跨段计数/级联重置/lvlRestart 约束/isLegalNumberingStyle/
//!   numFmt 渲染集）——「格式不丢」的编号语义核心。
//!
//! ## 实现路线：自有解析层
//!
//! 引擎 `docx-rs` 0.4.22 对 numbering.xml 有读面，但三处不可达
//! （源级一手核验）：
//!
//! 1. `Start.val`/`LevelText.val`/`LevelRestart.val` **私有字段无公开
//!    getter**（与 styles.rs 同因——`Name`/`BasedOn` 族）；
//! 2. `w:multilevel @w:type`（hybridMultilevel/singleLevel——引擎
//!    `AbstractNumbering.multi_level_type` 字段在但 **reader 无分支**
//!    恒 None）；
//! 3. `w:pStyle`（级别的段落样式绑定）经 `Level.pstyle`（Option<
//!    ParagraphStyle>——`ParagraphStyle` 的 val 私有，同 ①）。
//!
//! numbering.xml 与 styles.xml 同族简单 XML → 自有 quick-xml 解析层
//! （[`crate::io`] 门 3 同族技术）。引擎读面保留不动（`Docx.numberings`
//! 供引擎自身），本层是编号定义的唯一消费真源。
//!
//! ## 序号计算语义（Word 多级编号——可验证行为面）
//!
//! [`NumberingTracker::feed`] 对每个列表段（numId, ilvl）：
//!
//! 1. **实例独立**：不同 numId 独立计数（r4-t3 语义镜像）；首见
//!    numId 时各级计数器初始化为 `start − 1`（**override 后的有效
//!    start**——整级替换 > startOverride 只改 start）；
//! 2. **推进**：本 ilvl 计数 +1；
//! 3. **级联重置**：j > ilvl 各级按 `lvlRestart[j]` 约束重置——
//!    `r = lvlRestart[j]` 缺省 `1`：`0` = **永不重置**（持续计数）；
//!    `r ≥ 1` = 仅当**第 `j − r` 级**产生新项时重置（r=1 即默认
//!    「上一级新项时重置」）；
//! 4. **非列表段不打断**（Word 真语义：编号实例贯穿文档；r5-t1
//!    mdcore-wasm 侧「中断清零」是那层的降级决策，本层不继承——
//!    消费方需要文档边界重置时显式调 [`NumberingTracker::reset`]）；
//! 5. **渲染**：`lvlText` 的 `%1..%9` → 计数按该级 numFmt 格式化
//!    （`isLegalNumberingStyle` = 全部 decimal——层级引用恒阿拉伯
//!    数字）；非占位文字原样；**跳级**（首项即深层）时父级计数为
//!    `start−1`（渲染 `0`——Word/LO 同行为，登记）；`%k` 引用超
//!    ilvl 的未定义级 → 空串（Word 渲染空，登记）；
//! 6. **numFmt 渲染集**（`ST_NumberFormat` 常用子集，其余降级
//!    decimal 可诊断[与 r4-t3 kind 解析同界]）：`decimal` /
//!    `zeroDecimal`（≥2 位补零，≥100 原样——LO arabic-zero 语料
//!    形态）/ `upperRoman` / `lowerRoman`（标准减法制）/ `upperLetter`
//!    / `lowerLetter`（26 进制：27→AA——Word 行为）/ `ordinal`
//!    （1st/2nd/3rd + 11th/12th/13th 特例）/ `none`（空串——显式
//!    去编号）/ `bullet`（lvlText 字面输出，无计数渲染）。
//!
//! 悬空 numId / ilvl 超定义域 → `feed` 返回 `None`（与
//! [`crate::docx::ParagraphNumbering::kind`] = None 同语义——结构
//! 面经既有 `read_docx` 承载，消费方两通道对齐）。
//!
//! ## 面外登记
//!
//! - `w:rPr`/`w:pPr`（编号级别字符/段落格式面——level 上的
//!   rPr[pPr 内嵌段落标记]与 pPr 缩进之外的 spacing/jc——**缩进已
//!   承载**[w:ind 四属性]，rPr 整面不承载[styles.rs 的
//!   StyleRunProps 是样式链语义，此处无合并需求]；
//! - `w:tabs`（级别制表位——suffix=tab 的位置由渲染器定，模型级
//!   不承载）；
//! - 写面 `w:pStyle` 级别绑定（level styles 冷门面；读面承载）；
//! - `w:numStyleLink`/`w:styleLink` 悬空 → 该 numId 无有效层级
//!   （`None`——numStyleLink 的二跳解析[styles.xml 的 numbering
//!   样式再指回 abstract]不承载，与 r4-t3 numStyleLink 同界登记）。

use std::collections::HashMap;
use std::io::Read as _;

use quick_xml::events::Event;

use crate::docx::{ParagraphIndent, ParagraphIndentSpecial};

// ---------------------------------------------------------------------------
// 公开模型（读面）
// ---------------------------------------------------------------------------

/// 序号与文本的分隔（`w:suff`；缺省 tab）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuffixKind {
    Tab,
    Space,
    Nothing,
}

impl SuffixKind {
    fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("space") => SuffixKind::Space,
            Some("nothing") => SuffixKind::Nothing,
            // 缺省与 "tab" 同（ST_LevelSuffix 缺省 tab）。
            _ => SuffixKind::Tab,
        }
    }
}

/// 单个编号级别（`w:lvl` 全语义位——override 已并入前的原始形态
/// 归 [`AbstractDef`]；本类型同时是**有效级表**的元素形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct LevelDef {
    pub ilvl: usize,
    /// `w:start`（1 基；缺省 1）。
    pub start: usize,
    /// 原生 numFmt 值（`decimal`/`upperRoman`/`bullet`/`none`/…
    /// ——透传，渲染降级见模块文档）。
    pub num_fmt: String,
    /// `w:lvlText`（`%1..%9` 占位 + 字面）。
    pub lvl_text: String,
    /// `w:lvlRestart`（None = 缺省 1；Some(0) = 永不重置）。
    pub lvl_restart: Option<u32>,
    /// `w:isLgl`（legal numbering——层级引用恒 decimal）。
    pub is_lgl: bool,
    pub suffix: SuffixKind,
    /// `w:pStyle`（级别的段落样式绑定——读面承载）。
    pub pstyle: Option<String>,
    /// `w:ind`（级别缩进四属性）。
    pub indent: Option<ParagraphIndent>,
}

/// 抽象编号定义（`w:abstractNum`）。
#[derive(Debug, Clone, PartialEq)]
pub struct AbstractDef {
    pub id: usize,
    pub levels: Vec<LevelDef>,
    /// `w:multilevel @w:type`（引擎读面缺失项——本层承载）。
    pub multi_level_type: Option<String>,
    /// `w:styleLink`（abstractNum 间的样式互指——悬空/二跳不解析）。
    pub style_link: Option<String>,
    /// `w:numStyleLink`（同上）。
    pub num_style_link: Option<String>,
}

/// `w:num` 桥的 `w:lvlOverride` 双形态。
#[derive(Debug, Clone, PartialEq)]
pub struct LvlOverrideDef {
    pub ilvl: usize,
    /// `w:startOverride`（只改 start——**重启语义**：Word 把
    /// startOverride 当「从此处重启」的静态值并入，非动态事件）。
    pub start_override: Option<usize>,
    /// `w:lvl` 整级替换形态。
    pub level_override: Option<LevelDef>,
}

/// `w:num` 桥。
#[derive(Debug, Clone, PartialEq)]
pub struct NumDef {
    pub num_id: usize,
    pub abstract_id: usize,
    pub overrides: Vec<LvlOverrideDef>,
}

/// numbering.xml 全表（读入形态）。
#[derive(Debug, Clone, Default)]
pub struct NumberingTable {
    abstracts: HashMap<usize, AbstractDef>,
    nums: HashMap<usize, NumDef>,
}

/// 单段计算结果（序号文本 + 分隔）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemNumber {
    /// 渲染后的序号（如 `"1."`/`"2.a"`/`"•"`——含 lvlText 的字面
    /// 部分，不含 suffix）。
    pub text: String,
    pub suffix: SuffixKind,
}

// ---------------------------------------------------------------------------
// 有效级表（override 并入）
// ---------------------------------------------------------------------------

impl NumberingTable {
    /// 手工组装面（doctest/高级消费方——读入口为
    /// [`read_docx_numbering`]；重复 id 后胜）。
    pub fn add_abstract(&mut self, def: AbstractDef) {
        self.abstracts.insert(def.id, def);
    }

    pub fn add_num(&mut self, def: NumDef) {
        self.nums.insert(def.num_id, def);
    }

    pub fn abstract_def(&self, id: usize) -> Option<&AbstractDef> {
        self.abstracts.get(&id)
    }

    pub fn num_def(&self, num_id: usize) -> Option<&NumDef> {
        self.nums.get(&num_id)
    }

    /// 全部 abstractNumId（升序——语料统计/诊断面）。
    pub fn all_abstract_ids(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.abstracts.keys().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn num_ids(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.nums.keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// numId → **有效级表**（num 桥 → abstractNum → override 并入）。
    ///
    /// - numId 无 `w:num` 桥 / abstractId 悬空 / numStyleLink·styleLink
    ///   在场（二跳面外）→ `None`；
    /// - `lvlOverride`：整级替换（`w:lvl` 形态）优先于 `startOverride`
    ///   （只改 start 的浅覆盖——两者可并存于不同 ilvl）；
    /// - ilvl 升序（0 基表，槽位 = 定义级）。
    pub fn effective_levels(&self, num_id: usize) -> Option<Vec<LevelDef>> {
        let num = self.nums.get(&num_id)?;
        let abs = self.abstracts.get(&num.abstract_id)?;
        if abs.style_link.is_some() || abs.num_style_link.is_some() {
            return None; // 二跳面外（模块文档）
        }
        let mut levels = abs.levels.clone();
        for o in &num.overrides {
            if let Some(slot) = levels.iter_mut().find(|l| l.ilvl == o.ilvl) {
                if let Some(replacement) = &o.level_override {
                    *slot = replacement.clone();
                } else if let Some(new_start) = o.start_override {
                    slot.start = new_start;
                }
            }
        }
        Some(levels)
    }
}

// ---------------------------------------------------------------------------
// 序号计算器
// ---------------------------------------------------------------------------

/// 多级列表序号自动计算器（跨段状态机——Word 语义，见模块文档）。
///
/// ```
/// use mdc_office_io::numbering::{
///     AbstractDef, LevelDef, NumDef, NumberingTable, NumberingTracker, SuffixKind,
/// };
/// let mut t = NumberingTable::default();
/// t.add_abstract(AbstractDef {
///     id: 7, levels: vec![
///         LevelDef { ilvl: 0, start: 1, num_fmt: "decimal".into(), lvl_text: "%1.".into(),
///             lvl_restart: None, is_lgl: false, suffix: SuffixKind::Tab, pstyle: None, indent: None },
///         LevelDef { ilvl: 1, start: 1, num_fmt: "lowerLetter".into(), lvl_text: "%1.%2.".into(),
///             lvl_restart: None, is_lgl: false, suffix: SuffixKind::Tab, pstyle: None, indent: None },
///     ], multi_level_type: None, style_link: None, num_style_link: None,
/// });
/// t.add_num(NumDef { num_id: 3, abstract_id: 7, overrides: vec![] });
/// let mut tr = NumberingTracker::new(&t);
/// assert_eq!(tr.feed(3, 0).unwrap().text, "1.");
/// assert_eq!(tr.feed(3, 1).unwrap().text, "1.a.");
/// assert_eq!(tr.feed(3, 1).unwrap().text, "1.b.");
/// assert_eq!(tr.feed(3, 0).unwrap().text, "2.");   // 上级新项 → 子级重置
/// assert_eq!(tr.feed(3, 1).unwrap().text, "2.a.");
/// ```
#[derive(Debug)]
pub struct NumberingTracker {
    defs: HashMap<usize, Vec<LevelDef>>,
    counters: HashMap<usize, Vec<i64>>,
}

impl NumberingTracker {
    /// 从读入表预解析有效级表（`effective_levels` 全量）。
    pub fn new(table: &NumberingTable) -> Self {
        let defs = table
            .num_ids()
            .into_iter()
            .filter_map(|id| table.effective_levels(id).map(|lv| (id, lv)))
            .collect::<HashMap<_, _>>();
        Self {
            defs,
            counters: HashMap::new(),
        }
    }

    /// 显式重置某 numId（消费方控制——文档边界/编辑器 undo 场景；
    /// 非列表段**不**触发（Word 语义，模块文档）。
    pub fn reset(&mut self, num_id: usize) {
        self.counters.remove(&num_id);
    }

    /// 喂入一个列表段，返回渲染序号（None = numId 悬空/ilvl 超定义
    /// 域——同 `ParagraphNumbering::kind`=None 降级语义）。
    pub fn feed(&mut self, num_id: usize, ilvl: usize) -> Option<ItemNumber> {
        let levels = self.defs.get(&num_id)?;
        let def = levels.iter().find(|l| l.ilvl == ilvl)?;
        // 首见：初始化（start−1，首项 +1 → start）。
        // entry API（clippy map_entry——contains_key+insert 反模式）。
        self.counters
            .entry(num_id)
            .or_insert_with(|| levels.iter().map(|l| l.start as i64 - 1).collect());
        let cur = self.counters.get_mut(&num_id)?;
        let slot = cur.get_mut(ilvl)?;
        *slot += 1;

        // 级联重置（j > ilvl）：r 缺省 1（上一级新项）；0 = 永不；
        // r ≥ 1：第 j−r 级新项时重置。
        for j in (ilvl + 1)..levels.len() {
            let lj = &levels[j];
            let r = lj.lvl_restart.unwrap_or(1);
            if r == 0 {
                continue;
            }
            if ilvl + r as usize == j {
                cur[j] = lj.start as i64 - 1;
            }
        }

        // 渲染 lvlText（bullet 字面；none 空串）。
        let text = render_lvl_text(def, levels, &self.counters[&num_id]);
        Some(ItemNumber {
            text,
            suffix: def.suffix,
        })
    }
}

/// `%k` → 该级计数按 numFmt 渲染（isLgl → 恒 decimal）。
fn render_lvl_text(def: &LevelDef, levels: &[LevelDef], counters: &[i64]) -> String {
    if def.num_fmt == "none" {
        return String::new();
    }
    if def.num_fmt == "bullet" {
        return def.lvl_text.clone();
    }
    let mut out = String::new();
    let chars: Vec<char> = def.lvl_text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            if let Some(d) = chars[i + 1].to_digit(10) {
                let k = d as usize; // 1..9
                let idx = k - 1;
                match levels.iter().position(|l| l.ilvl == idx) {
                    Some(pos) if pos < counters.len() => {
                        let value = counters[idx];
                        // isLgl 是**渲染级**属性（w:isLgl on level i）：本
                        // lvlText 中所有 % 引用恒 decimal——被引用级自身
                        // 的 numFmt/isLgl 不参与（OOXML §17.9.3 + Word
                        // 实测；实施钉定）。
                        let fmt = if def.is_lgl {
                            "decimal"
                        } else {
                            &levels[idx].num_fmt
                        };
                        out.push_str(&format_counter(value, fmt));
                        i += 2;
                        continue;
                    }
                    // %k 引用超 ilvl 的未定义级：空串（Word 行为，登记）。
                    _ => {
                        i += 2;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// numFmt 渲染集（降级 decimal 登记见模块文档）。
fn format_counter(value: i64, num_fmt: &str) -> String {
    let v = value.max(0);
    match num_fmt {
        "none" => String::new(),
        "bullet" => String::new(),
        "zeroDecimal" => {
            if v < 10 {
                format!("0{v}")
            } else {
                format!("{v}")
            }
        }
        "upperRoman" => to_roman(v, true),
        "lowerRoman" => to_roman(v, false),
        "upperLetter" => to_letters(v, true),
        "lowerLetter" => to_letters(v, false),
        // #266：中文序号（标书正文「一、/（一）/①」多级骨架的主于）。
        // chineseCounting 与 chineseCountingThousand 在 1-9999 区间渲染
        // 恒同（差异是千位以上古法——列表序号不可达）；
        // chineseLegalSimplified = 大写数字（合同/财务惯例）。
        "chineseCounting" | "chineseCountingThousand" => to_chinese_number(v, false),
        "chineseLegalSimplified" => to_chinese_number(v, true),
        "ordinal" => {
            let n = v % 100;
            let suffix = if (11..=13).contains(&n) {
                "th"
            } else {
                match n % 10 {
                    1 => "st",
                    2 => "nd",
                    3 => "rd",
                    _ => "th",
                }
            };
            format!("{v}{suffix}")
        }
        // decimal 及其余（locale 系 russianLower 等）：降级 decimal。
        _ => format!("{v}"),
    }
}

/// 中文序号渲染（1-9999——列表序号实际域；超域降级阿拉伯数字可诊断）。
///
/// - 小写：零一二三四五六七八九 + 十百千；大写（legal）：零壹贰叁肆
///   伍陆柒捌玖 + 拾佰仟。
/// - 11-19：十X（非「一十X」——Word 同行为）；零位压缩：连续零位
///   只出一个零（1001 → 一千零一）；末尾零不发射（110 → 一百一十）。
fn to_chinese_number(v: i64, legal: bool) -> String {
    const DIGITS: [[&str; 10]; 2] = [
        ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"],
        ["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"],
    ];
    const UNITS: [[&str; 4]; 2] = [["", "十", "百", "千"], ["", "拾", "佰", "仟"]];
    let table = if legal { 1 } else { 0 };
    if v <= 0 {
        return DIGITS[table][0].to_string();
    }
    if v >= 10000 {
        return format!("{v}");
    }
    let digits = &DIGITS[table];
    let units = &UNITS[table];
    let mut ds = [0usize; 4];
    let mut rest = v as usize;
    let mut len = 0usize;
    while rest > 0 {
        ds[len] = rest % 10;
        rest /= 10;
        len += 1;
    }
    let mut out = String::new();
    let mut pending_zero = false;
    for pos in (0..len).rev() {
        let d = ds[pos];
        if d == 0 {
            pending_zero = true;
            continue;
        }
        if pending_zero {
            out.push_str(digits[0]);
            pending_zero = false;
        }
        let leading_ten = pos == 1 && len == 2 && d == 1;
        if !leading_ten {
            out.push_str(digits[d]);
        }
        out.push_str(units[pos]);
    }
    out
}

fn to_roman(value: i64, upper: bool) -> String {
    if value == 0 {
        return "0".into();
    }
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut rest = value;
    let mut out = String::new();
    for (n, sym) in TABLE {
        while rest >= n {
            out.push_str(sym);
            rest -= n;
        }
    }
    if upper {
        out
    } else {
        out.to_lowercase()
    }
}

fn to_letters(value: i64, upper: bool) -> String {
    // 26 进制：1=A … 26=Z, 27=AA（Word 行为）。
    if value == 0 {
        return "0".into();
    }
    let mut v = value;
    let mut rev = Vec::new();
    while v > 0 {
        let rem = ((v - 1) % 26) as u8;
        rev.push(b'A' + rem);
        v = (v - 1) / 26;
    }
    let s: String = rev.iter().rev().map(|&b| b as char).collect();
    if upper {
        s
    } else {
        s.to_lowercase()
    }
}

// ---------------------------------------------------------------------------
// 自有解析层（numbering.xml）
// ---------------------------------------------------------------------------

/// 属性 local-name 比较（`w:val` 等带前缀形态——同 style.rs 纪律）。
fn key_is(a: &quick_xml::events::attributes::Attribute<'_>, name: &[u8]) -> bool {
    let k = a.key.as_ref();
    let local = match k.iter().position(|&b| b == b':') {
        Some(i) => &k[i + 1..],
        None => k,
    };
    local == name
}

fn attr_str(a: &quick_xml::events::attributes::Attribute<'_>) -> Option<String> {
    let raw = String::from_utf8_lossy(&a.value);
    let unescaped = quick_xml::escape::unescape(&raw).ok()?;
    Some(unescaped.into_owned())
}

fn find_attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    for attr in e.attributes() {
        let a = attr.ok()?;
        if key_is(&a, name) {
            return attr_str(&a);
        }
    }
    None
}

fn find_attr_usize(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<usize> {
    find_attr(e, name).and_then(|v| v.parse().ok())
}

fn local_of(e: &quick_xml::events::BytesStart<'_>) -> String {
    let q = e.name();
    let q = q.as_ref();
    let local = match q.iter().position(|&b| b == b':') {
        Some(i) => &q[i + 1..],
        None => q,
    };
    String::from_utf8_lossy(local).into_owned()
}

fn on_off(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<bool> {
    for attr in e.attributes() {
        let a = attr.ok()?;
        if key_is(&a, name) {
            let v = attr_str(&a)?.to_ascii_lowercase();
            return Some(!matches!(v.as_str(), "0" | "false" | "off"));
        }
    }
    Some(true)
}

fn absorb_ind(e: &quick_xml::events::BytesStart<'_>) -> Option<ParagraphIndent> {
    let get = |names: &[&[u8]]| -> Option<i32> {
        for n in names {
            if let Some(v) = find_attr(e, n) {
                if let Ok(i) = v.parse::<i32>() {
                    return Some(i);
                }
            }
        }
        None
    };
    let left = get(&[b"left", b"start"]);
    let right = get(&[b"right", b"end"]);
    let first = get(&[b"firstLine"]);
    let hanging = get(&[b"hanging"]);
    if left.is_none() && right.is_none() && first.is_none() && hanging.is_none() {
        return None;
    }
    let special = match (first, hanging) {
        (Some(f), _) => Some(ParagraphIndentSpecial::FirstLine(f)),
        (None, Some(h)) => Some(ParagraphIndentSpecial::Hanging(h)),
        (None, None) => None,
    };
    Some(ParagraphIndent {
        left,
        right,
        special,
    })
}

/// 解析 `word/numbering.xml`（门 3 已验良构——本层语义提取）。
pub(crate) fn parse_numbering_part(
    data: &[u8],
    part: &str,
    strict: bool,
    warnings: &mut Vec<crate::io::CompatibilityWarning>,
) -> Result<NumberingTable, crate::io::ReadError> {
    let mut table = NumberingTable::default();

    let mut cur_abstract: Option<AbstractDef> = None;
    let mut cur_level: Option<LevelDef> = None;
    let mut cur_num: Option<NumDef> = None;
    let mut cur_override: Option<LvlOverrideDef> = None;
    // override 内嵌 w:lvl 的 ilvl 上下文（w:lvlOverride/w:lvl）。
    let mut in_override_lvl = false;

    let mut reader = quick_xml::Reader::from_reader(data);
    let mut buf = Vec::new();
    loop {
        let ev = reader.read_event_into(&mut buf);
        match ev {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let el = local_of(&e);
                match el.as_str() {
                    "abstractNum" => {
                        cur_abstract = Some(AbstractDef {
                            id: find_attr_usize(&e, b"abstractNumId").unwrap_or(0),
                            levels: Vec::new(),
                            multi_level_type: None,
                            style_link: None,
                            num_style_link: None,
                        });
                    }
                    "multilevel" => {
                        if let Some(a) = cur_abstract.as_mut() {
                            a.multi_level_type = find_attr(&e, b"type");
                        }
                    }
                    "styleLink" => {
                        if let Some(a) = cur_abstract.as_mut() {
                            a.style_link = find_attr(&e, b"val");
                        }
                    }
                    "numStyleLink" => {
                        if let Some(a) = cur_abstract.as_mut() {
                            a.num_style_link = find_attr(&e, b"val");
                        }
                    }
                    "num" => {
                        cur_num = Some(NumDef {
                            num_id: find_attr_usize(&e, b"numId").unwrap_or(0),
                            abstract_id: 0,
                            overrides: Vec::new(),
                        });
                    }
                    "abstractNumId" => {
                        if let Some(n) = cur_num.as_mut() {
                            n.abstract_id = find_attr_usize(&e, b"val").unwrap_or(0);
                        }
                    }
                    "lvlOverride" => {
                        cur_override = Some(LvlOverrideDef {
                            ilvl: find_attr_usize(&e, b"ilvl").unwrap_or(0),
                            start_override: None,
                            level_override: None,
                        });
                    }
                    "startOverride" => {
                        if let Some(o) = cur_override.as_mut() {
                            o.start_override = find_attr_usize(&e, b"val");
                        }
                    }
                    "lvl" => {
                        let ilvl = find_attr_usize(&e, b"ilvl").unwrap_or(0);
                        cur_level = Some(LevelDef {
                            ilvl,
                            start: find_attr_usize(&e, b"start")
                                .or_else(|| find_attr(&e, b"start").and_then(|v| v.parse().ok()))
                                .unwrap_or(1),
                            num_fmt: "decimal".into(),
                            lvl_text: String::new(),
                            lvl_restart: None,
                            is_lgl: false,
                            suffix: SuffixKind::Tab,
                            pstyle: None,
                            indent: None,
                        });
                        if cur_override.is_some() {
                            in_override_lvl = true;
                        }
                    }
                    "start" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.start = find_attr_usize(&e, b"val").unwrap_or(l.start);
                        }
                    }
                    "numFmt" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.num_fmt = find_attr(&e, b"val").unwrap_or_else(|| "decimal".into());
                        }
                    }
                    "lvlText" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.lvl_text = find_attr(&e, b"val").unwrap_or_default();
                        }
                    }
                    "lvlRestart" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.lvl_restart =
                                find_attr(&e, b"val").and_then(|v| v.parse::<u32>().ok());
                        }
                    }
                    "isLgl" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.is_lgl = on_off(&e, b"val").unwrap_or(true);
                        }
                    }
                    "suff" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.suffix = SuffixKind::parse(find_attr(&e, b"val").as_deref());
                        }
                    }
                    "pStyle" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.pstyle = find_attr(&e, b"val");
                        }
                    }
                    "ind" => {
                        if let Some(l) = cur_level.as_mut() {
                            l.indent = absorb_ind(&e);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let q = e.name();
                let q = q.as_ref();
                let local = match q.iter().position(|&b| b == b':') {
                    Some(i) => &q[i + 1..],
                    None => q,
                };
                let el = String::from_utf8_lossy(local).into_owned();
                match el.as_str() {
                    "lvl" => {
                        if let Some(l) = cur_level.take() {
                            if in_override_lvl {
                                // w:lvlOverride/w:lvl：整级替换形态。
                                in_override_lvl = false;
                                if let Some(o) = cur_override.as_mut() {
                                    o.level_override = Some(l);
                                }
                            } else if let Some(a) = cur_abstract.as_mut() {
                                a.levels.push(l);
                            }
                        }
                    }
                    "lvlOverride" => {
                        if let Some(o) = cur_override.take() {
                            if let Some(n) = cur_num.as_mut() {
                                n.overrides.push(o);
                            }
                        }
                    }
                    "abstractNum" => {
                        if let Some(a) = cur_abstract.take() {
                            table.abstracts.insert(a.id, a);
                        }
                    }
                    "num" => {
                        if let Some(n) = cur_num.take() {
                            table.nums.insert(n.num_id, n);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                let detail = format!("numbering part '{part}' XML stream error");
                if strict {
                    return Err(crate::io::ReadError::InvalidPart {
                        part: part.to_string(),
                        detail,
                    });
                }
                warnings.push(crate::io::CompatibilityWarning {
                    part: part.to_string(),
                    kind: crate::io::WarningKind::MalformedXml,
                    detail: format!("compat mode: {detail} (partial carry)"),
                });
                break;
            }
            Ok(_) => {}
        }
        buf.clear();
    }
    Ok(table)
}

// ---------------------------------------------------------------------------
// 读入口（容器门复用 + numbering.xml 定位）
// ---------------------------------------------------------------------------

/// 读 DOCX 编号定义（文件路径入口，严格模式默认）。
pub fn read_docx_numbering(path: &str) -> Result<NumberingBundle, DocxNumberingError> {
    read_docx_numbering_with_options(path, &crate::io::ReadOptions::default())
}

/// 读 DOCX 编号定义（带选项）。
pub fn read_docx_numbering_with_options(
    path: &str,
    opts: &crate::io::ReadOptions,
) -> Result<NumberingBundle, DocxNumberingError> {
    let meta =
        std::fs::metadata(path).map_err(|e| DocxNumberingError::Io(format!("stat {path}: {e}")))?;
    if meta.len() > crate::io::LIMIT_INPUT_BYTES_DOCX {
        return Err(DocxNumberingError::Container(
            crate::io::ReadError::PrescanRejected {
                reason: "input size over limit",
                entry: None,
            },
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| DocxNumberingError::Io(format!("read {path}: {e}")))?;
    read_docx_numbering_from_bytes_with_options(&bytes, opts)
}

/// 读 DOCX 编号定义（字节入口，严格模式）。
pub fn read_docx_numbering_from_bytes(bytes: &[u8]) -> Result<NumberingBundle, DocxNumberingError> {
    read_docx_numbering_from_bytes_with_options(bytes, &crate::io::ReadOptions::default())
}

/// 读 DOCX 编号定义（字节入口，带选项）。
///
/// 门结构与 [`crate::docx::read_docx`] 同族；numbering.xml 经
/// document.xml.rels 的 `/numbering` 关系定位（多关系按 target 排序
/// 取第一——确定性）。无 numbering 关系 = 无编号部件（空表 + 无警告
/// ——非列表文档常态）；关系在场但部件缺失 = 悬空（严格拒 / 兼容
/// `BrokenReference` 警告 + 空表）。
pub fn read_docx_numbering_from_bytes_with_options(
    bytes: &[u8],
    opts: &crate::io::ReadOptions,
) -> Result<NumberingBundle, DocxNumberingError> {
    use crate::docx::{build_rels_bundle, normalize_part_path, RelsBundle};
    let strict = !opts.compat;
    if bytes.len() as u64 > crate::io::LIMIT_INPUT_BYTES_DOCX {
        return Err(DocxNumberingError::Container(
            crate::io::ReadError::PrescanRejected {
                reason: "input size over limit",
                entry: None,
            },
        ));
    }
    let cursor = std::io::Cursor::new(bytes);
    let mut za = zip::ZipArchive::new(cursor).map_err(|e| {
        DocxNumberingError::Container(crate::io::ReadError::ContainerCorrupt(format!("{e}")))
    })?;
    crate::io::prescan_docx(&mut za)?;
    let mut warnings = Vec::new();
    crate::io::verify_gates(&mut za, strict, &mut warnings)?;

    let rels_cursor = std::io::Cursor::new(bytes);
    let mut rels_za = zip::ZipArchive::new(rels_cursor).map_err(|e| {
        DocxNumberingError::Container(crate::io::ReadError::ContainerCorrupt(format!("{e}")))
    })?;
    let bundle: RelsBundle = build_rels_bundle(&mut rels_za, strict, &mut warnings)?;
    let doc_rels = &bundle.document;
    let part = doc_rels.as_ref().and_then(|doc| {
        let mut targets: Vec<String> = doc
            .iter()
            .filter(|r| r.rel_type.ends_with("/numbering"))
            .filter(|r| r.target_mode.is_none())
            .map(|r| r.target.clone())
            .collect();
        targets.sort();
        targets.into_iter().next().map(|t| {
            let joined = if t.starts_with('/') {
                t.trim_start_matches('/').to_string()
            } else {
                format!("word/{t}")
            };
            normalize_part_path(&joined).unwrap_or(joined)
        })
    });

    let table = match part {
        Some(p) => match za.by_name(&p) {
            Ok(mut zf) => {
                let mut data = Vec::new();
                zf.read_to_end(&mut data).map_err(|e| {
                    DocxNumberingError::Container(crate::io::ReadError::ContainerCorrupt(format!(
                        "read {p}: {e}"
                    )))
                })?;
                parse_numbering_part(&data, &p, strict, &mut warnings)
                    .map_err(DocxNumberingError::Container)?
            }
            Err(_) => {
                if strict {
                    return Err(DocxNumberingError::Container(
                        crate::io::ReadError::InvalidPart {
                            part: p,
                            detail: "relationship target part missing from package (dangling)"
                                .into(),
                        },
                    ));
                }
                warnings.push(crate::io::CompatibilityWarning {
                    part: p,
                    kind: crate::io::WarningKind::BrokenReference,
                    detail: "compat mode: numbering part missing (dangling; empty table)".into(),
                });
                NumberingTable::default()
            }
        },
        None => NumberingTable::default(),
    };
    Ok(NumberingBundle { table, warnings })
}

/// 编号读结果捆。
#[derive(Debug, Clone)]
pub struct NumberingBundle {
    pub table: NumberingTable,
    pub warnings: Vec<crate::io::CompatibilityWarning>,
}

/// 编号读入口错误（三分——同 style.rs 族）。
#[derive(Debug)]
pub enum DocxNumberingError {
    Container(crate::io::ReadError),
    Io(String),
}

impl std::fmt::Display for DocxNumberingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocxNumberingError::Container(e) => write!(f, "docx numbering read: container: {e}"),
            DocxNumberingError::Io(e) => write!(f, "docx numbering read: {e}"),
        }
    }
}

impl std::error::Error for DocxNumberingError {}

impl From<crate::io::ReadError> for DocxNumberingError {
    fn from(e: crate::io::ReadError) -> Self {
        DocxNumberingError::Container(e)
    }
}

// ---------------------------------------------------------------------------
// 写面：NumberingDef + DocxWriter 分文件 impl
// ---------------------------------------------------------------------------

/// 写侧单级规格（读面 [`LevelDef`] 的写侧对偶——`pstyle` 不设
/// [读面承载，写面绑定冷门面登记]）。
#[derive(Debug, Clone, PartialEq)]
pub struct LevelSpec {
    /// 0-8（与 [`crate::docx::MAX_LIST_LEVEL`] 同域）。
    pub ilvl: usize,
    /// numFmt 白名单（见 [`NUM_FMT_WHITELIST`]）。
    pub num_fmt: String,
    /// `%1..%9` 占位 + 字面（`%k` 引用 k ≤ ilvl+1——写面校验）。
    pub lvl_text: String,
    pub start: usize,
    pub lvl_restart: Option<u32>,
    pub is_lgl: bool,
    pub suffix: SuffixKind,
}

impl LevelSpec {
    /// 便捷构造（decimal 缺省集合的常用形）。
    pub fn decimal(ilvl: usize, lvl_text: impl Into<String>) -> Self {
        Self {
            ilvl,
            num_fmt: "decimal".into(),
            lvl_text: lvl_text.into(),
            start: 1,
            lvl_restart: None,
            is_lgl: false,
            suffix: SuffixKind::Tab,
        }
    }
}

/// numFmt 白名单（写面——`ST_NumberFormat` 常用子集；读面渲染降级
/// decimal 的格式同样以 decimal 入白名单[恒渲染等价]）。
pub const NUM_FMT_WHITELIST: [&str; 13] = [
    "decimal",
    "zeroDecimal",
    "upperRoman",
    "lowerRoman",
    "upperLetter",
    "lowerLetter",
    "ordinal",
    "none",
    "bullet",
    // decimalFullWidth 等显示宽变体：与 decimal 渲染等价（Word 全角
    // 数字显示——模型级降级 decimal 入白名单）。
    "decimalFullWidth",
    // #266：中文序号系（读面渲染 format_counter 同名分支——写面定义
    // 与读面渲染对齐，往返闭环）。
    "chineseCounting",
    "chineseCountingThousand",
    "chineseLegalSimplified",
];

/// 写侧多级编号定义（`w:abstractNum` + `w:num` 桥同 id——与
/// ORDERED_NUM_ID=100/BULLET_NUM_ID=101 同模式）。
#[derive(Debug, Clone, PartialEq)]
pub struct NumberingDef {
    /// numId = abstractNumId（避开内建 1 与预置 100/101——校验拒）。
    pub id: usize,
    pub levels: Vec<LevelSpec>,
}

impl crate::docx::DocxWriter {
    /// 定义自定义多级编号（numbering.xml 增量——`w:abstractNum` +
    /// `w:num` 桥同 id）。
    ///
    /// 校验（typed `InvalidInput`——写面自洽）：
    /// - id ∉ {1, 100, 101}∪已定义（内建 1 引擎常量 + t20 两预置）；
    /// - levels 非空、ilvl 升序唯一 0-8；
    /// - `num_fmt` ∈ [`NUM_FMT_WHITELIST`]；
    /// - `lvl_text` 的 `%k`：k ≤ ilvl+1（引用更深级 = Word 不产形，
    ///   拒）；无 `%` 占位 = bullet 之外的恒字面（合法——numFmt
    ///   `none` 的空 lvlText 常态）；
    /// - `start ≥ 1`。
    ///
    /// 级别缩进几何 = 预置同构（`720×(ilvl+1)` 左 + 360 悬挂——
    /// [`crate::docx::QUOTE_INDENT_STEP`]/[`crate::docx::
    /// LIST_HANGING_INDENT`] 单一真源）。
    pub fn define_numbering(&mut self, def: NumberingDef) -> Result<(), crate::docx::DocxError> {
        if def.id == 1
            || def.id == crate::docx::ORDERED_NUM_ID
            || def.id == crate::docx::BULLET_NUM_ID
        {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "numbering id {} collides with built-in (1) or preset (100/101)",
                def.id
            )));
        }
        if self.defined_numberings().contains_key(&def.id) {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "numbering id {} already defined in this writer",
                def.id
            )));
        }
        if def.levels.is_empty() {
            return Err(crate::docx::DocxError::InvalidInput(
                "numbering levels must be non-empty".into(),
            ));
        }
        let mut prev_ilvl: Option<usize> = None;
        for l in &def.levels {
            if l.ilvl > crate::docx::MAX_LIST_LEVEL {
                return Err(crate::docx::DocxError::InvalidInput(format!(
                    "level ilvl must be 0-{}, got {}",
                    crate::docx::MAX_LIST_LEVEL,
                    l.ilvl
                )));
            }
            if let Some(p) = prev_ilvl {
                if l.ilvl <= p {
                    return Err(crate::docx::DocxError::InvalidInput(format!(
                        "level ilvl must be strictly ascending (got {} after {})",
                        l.ilvl, p
                    )));
                }
            }
            prev_ilvl = Some(l.ilvl);
            if !NUM_FMT_WHITELIST.contains(&l.num_fmt.as_str()) {
                return Err(crate::docx::DocxError::InvalidInput(format!(
                    "numFmt {:?} not in whitelist (decimal/zeroDecimal/upperRoman/lowerRoman/upperLetter/lowerLetter/ordinal/none/bullet/decimalFullWidth)",
                    l.num_fmt
                )));
            }
            if l.start == 0 {
                return Err(crate::docx::DocxError::InvalidInput(
                    "level start must be >= 1".into(),
                ));
            }
            // lvlText %k：k ≤ ilvl+1。
            let chars: Vec<char> = l.lvl_text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '%' && i + 1 < chars.len() {
                    if let Some(d) = chars[i + 1].to_digit(10) {
                        if (d as usize) > l.ilvl + 1 {
                            return Err(crate::docx::DocxError::InvalidInput(format!(
                                "lvlText %{} at ilvl {} references a deeper level (max %{})",
                                d,
                                l.ilvl,
                                l.ilvl + 1
                            )));
                        }
                        i += 2;
                        continue;
                    }
                }
                i += 1;
            }
        }

        let mut abs = docx_rs::AbstractNumbering::new(def.id);
        for l in &def.levels {
            let mut lv = docx_rs::Level::new(
                l.ilvl,
                docx_rs::Start::new(l.start),
                docx_rs::NumberFormat::new(l.num_fmt.clone()),
                docx_rs::LevelText::new(l.lvl_text.clone()),
                docx_rs::LevelJc::new("left"),
            )
            .suffix(match l.suffix {
                SuffixKind::Tab => docx_rs::LevelSuffixType::Tab,
                SuffixKind::Space => docx_rs::LevelSuffixType::Space,
                SuffixKind::Nothing => docx_rs::LevelSuffixType::Nothing,
            })
            .indent(
                Some(crate::docx::QUOTE_INDENT_STEP * (l.ilvl as i32 + 1)),
                Some(docx_rs::SpecialIndentType::Hanging(
                    crate::docx::LIST_HANGING_INDENT,
                )),
                None,
                None,
            );
            if let Some(r) = l.lvl_restart {
                lv = lv.level_restart(r);
            }
            if l.is_lgl {
                lv.is_lgl = Some(docx_rs::IsLgl::new());
            }
            abs = abs.add_level(lv);
        }
        let docx = std::mem::take(self.docx_mut());
        let docx = docx.add_abstract_numbering(abs);
        let docx = docx.add_numbering(docx_rs::Numbering::new(def.id, def.id));
        *self.docx_mut() = docx;
        self.defined_numberings_mut()
            .insert(def.id, def.levels.last().unwrap().ilvl);
        Ok(())
    }

    /// 引用自定义编号定义的列表段（`w:numPr` 引用
    /// [`Self::define_numbering`] 注册的 id——预置 100/101 走既有
    /// [`Self::list_item`]）。
    ///
    /// ilvl typed 验证（0-8 + 在该定义的级域内——超域拒绝）；
    /// 悬空 numId 拒绝（写面自洽）。段落级 `w:ind` 与预置同构
    /// （t22 续行几何先例：显式直挂对 pPr 应用不全的渲染器更鲁棒）。
    pub fn numbered_list_item(
        &mut self,
        runs: &[crate::docx::RunSpec],
        num_id: usize,
        ilvl: usize,
    ) -> Result<(), crate::docx::DocxError> {
        if !self.defined_numberings().contains_key(&num_id) {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "numbering id {num_id} is not registered (define_numbering first)"
            )));
        }
        if ilvl > crate::docx::MAX_LIST_LEVEL {
            return Err(crate::docx::DocxError::Parse(format!(
                "list ilvl must be 0-{}, got {ilvl}",
                crate::docx::MAX_LIST_LEVEL
            )));
        }
        // 级域校验（引用该定义之外的级 = 悬空 numPr——写面自洽）。
        let max_ilvl = self.defined_numberings()[&num_id];
        if ilvl > max_ilvl {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "numbering {num_id} defines levels up to ilvl {max_ilvl} (got {ilvl})"
            )));
        }
        let mut p = crate::docx::specs_to_paragraph(runs).numbering(
            docx_rs::NumberingId::new(num_id),
            docx_rs::IndentLevel::new(ilvl),
        );
        p = p.indent(
            Some(crate::docx::QUOTE_INDENT_STEP * (ilvl as i32 + 1)),
            Some(docx_rs::SpecialIndentType::Hanging(
                crate::docx::LIST_HANGING_INDENT,
            )),
            None,
            None,
        );
        let docx = std::mem::take(self.docx_mut());
        *self.docx_mut() = docx.add_paragraph(p);
        self.count_specs(1, runs);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 测试（内嵌——解析器/计算器/写读全环）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 手搓最小 OPC 包（numbering.xml 参数化；styles 关系恒在——
    /// 引擎包无 numbering 时 rels 不写该行）。
    fn build_test_docx(numbering_xml: &str) -> Vec<u8> {
        use std::io::Write as _;
        let ct = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#;
        let root_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let doc = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>t</w:t></w:r></w:p></w:body></w:document>"#;
        let styles = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#;
        let doc_rels: &[u8] = if numbering_xml.is_empty() {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
        } else {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>"#
        };
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let add = |zw: &mut zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>>,
                       name: &str,
                       data: &[u8]|
             -> zip::result::ZipResult<()> {
                zw.start_file(name, zip::write::SimpleFileOptions::default())?;
                zw.write_all(data)?;
                Ok(())
            };
            add(&mut zw, "[Content_Types].xml", ct).unwrap();
            add(&mut zw, "_rels/.rels", root_rels).unwrap();
            add(&mut zw, "word/document.xml", doc).unwrap();
            add(&mut zw, "word/styles.xml", styles).unwrap();
            add(&mut zw, "word/_rels/document.xml.rels", doc_rels).unwrap();
            if !numbering_xml.is_empty() {
                add(&mut zw, "word/numbering.xml", numbering_xml.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        buf.into_inner()
    }

    fn three_level_xml() -> String {
        // 3 级（decimal / lowerLetter / lowerRoman）+ lvlRestart 变体 +
        // lvlOverride（level1 整级替换 + level2 startOverride）。
        r#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="7"><w:multilevel w:type="hybridMultilevel"/>
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlRestart w:val="1"/><w:suff w:val="tab"/><w:pStyle w:val="ListPara"/><w:ind left="720" hanging="360"/></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%1.%2."/><w:suff w:val="space"/></w:lvl>
<w:lvl w:ilvl="2"><w:start w:val="2"/><w:numFmt w:val="lowerRoman"/><w:lvlText w:val="%1.%2.%3."/><w:isLgl/><w:suff w:val="nothing"/><w:ind left="2160" hanging="360"/></w:lvl>
</w:abstractNum>
<w:num w:numId="3"><w:abstractNumId w:val="7"/>
<w:lvlOverride w:ilvl="1"><w:startOverride w:val="5"/></w:lvlOverride>
<w:lvlOverride w:ilvl="2"><w:lvl w:ilvl="2"><w:start w:val="9"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="[%1-%2-%3]"/><w:suff w:val="space"/></w:lvl></w:lvlOverride>
</w:num>
<w:num w:numId="4"><w:abstractNumId w:val="7"/></w:num>
</w:numbering>"#
        .to_string()
    }

    /// #266：中文序号渲染（chineseCounting 系）——Word 行为对照。
    #[test]
    fn chinese_counting_rendering() {
        // 1-9 基数。
        for (v, expect) in [(1, "\u{4e00}"), (2, "\u{4e8c}"), (9, "\u{4e5d}")] {
            assert_eq!(to_chinese_number(v, false), expect);
        }
        // 10-19：十X（一十 → 十）。
        assert_eq!(to_chinese_number(10, false), "\u{5341}");
        assert_eq!(to_chinese_number(11, false), "\u{5341}\u{4e00}");
        assert_eq!(to_chinese_number(19, false), "\u{5341}\u{4e5d}");
        // 20-99：二十X。
        assert_eq!(to_chinese_number(20, false), "\u{4e8c}\u{5341}");
        assert_eq!(to_chinese_number(21, false), "\u{4e8c}\u{5341}\u{4e00}");
        assert_eq!(to_chinese_number(99, false), "\u{4e5d}\u{5341}\u{4e5d}");
        // 百/千 + 零位压缩（1001 → 一千零一；110 → 一百一十）。
        assert_eq!(to_chinese_number(100, false), "\u{4e00}\u{767e}");
        assert_eq!(
            to_chinese_number(110, false),
            "\u{4e00}\u{767e}\u{4e00}\u{5341}"
        );
        assert_eq!(
            to_chinese_number(101, false),
            "\u{4e00}\u{767e}\u{96f6}\u{4e00}"
        );
        assert_eq!(
            to_chinese_number(1001, false),
            "\u{4e00}\u{5343}\u{96f6}\u{4e00}"
        );
        assert_eq!(
            to_chinese_number(1111, false),
            "\u{4e00}\u{5343}\u{4e00}\u{767e}\u{4e00}\u{5341}\u{4e00}"
        );
        // 大写（chineseLegalSimplified）。
        assert_eq!(to_chinese_number(11, true), "\u{62fe}\u{58f9}");
        assert_eq!(to_chinese_number(21, true), "\u{8d30}\u{62fe}\u{58f9}");
        // 0 = 跳级首项面（零）；超域降级阿拉伯。
        assert_eq!(to_chinese_number(0, false), "\u{96f6}");
        assert_eq!(to_chinese_number(10000, false), "10000");
    }

    /// #266：format_counter 分派——chineseCounting/Thousand 同渲染、
    /// LegalSimplified 大写、其余不变。
    #[test]
    fn format_counter_chinese_dispatch() {
        assert_eq!(format_counter(3, "chineseCounting"), "\u{4e09}");
        assert_eq!(
            format_counter(13, "chineseCountingThousand"),
            "\u{5341}\u{4e09}"
        );
        assert_eq!(format_counter(3, "chineseLegalSimplified"), "\u{53c1}");
        // 既有集不回归。
        assert_eq!(format_counter(3, "decimal"), "3");
        assert_eq!(format_counter(3, "upperRoman"), "III");
        assert_eq!(format_counter(3, "lowerLetter"), "c");
    }

    /// #266：tracker 端到端——中文多级（一、/（一）/1.）的 lvlText
    /// 渲染 + 级联重置（三层层级推进）。
    #[test]
    fn tracker_chinese_multilevel_feed() {
        let mut table = NumberingTable::default();
        table.add_abstract(AbstractDef {
            id: 7,
            levels: vec![
                LevelDef {
                    ilvl: 0,
                    start: 1,
                    num_fmt: "chineseCounting".into(),
                    lvl_text: "%1\u{3001}".into(),
                    lvl_restart: None,
                    is_lgl: false,
                    suffix: SuffixKind::Nothing,
                    pstyle: None,
                    indent: None,
                },
                LevelDef {
                    ilvl: 1,
                    start: 1,
                    num_fmt: "chineseCounting".into(),
                    lvl_text: "\u{FF08}%2\u{FF09}".into(),
                    lvl_restart: None,
                    is_lgl: false,
                    suffix: SuffixKind::Nothing,
                    pstyle: None,
                    indent: None,
                },
                LevelDef {
                    ilvl: 2,
                    start: 1,
                    num_fmt: "decimal".into(),
                    lvl_text: "%3.".into(),
                    lvl_restart: None,
                    is_lgl: false,
                    suffix: SuffixKind::Nothing,
                    pstyle: None,
                    indent: None,
                },
            ],
            multi_level_type: None,
            style_link: None,
            num_style_link: None,
        });
        table.add_num(NumDef {
            num_id: 3,
            abstract_id: 7,
            overrides: vec![],
        });
        let mut t = NumberingTracker::new(&table);
        // 一、 → （一） → 1. → 2. → （二） → 1. → 二、
        assert_eq!(t.feed(3, 0).unwrap().text, "\u{4e00}\u{3001}");
        assert_eq!(t.feed(3, 1).unwrap().text, "\u{FF08}\u{4e00}\u{FF09}");
        assert_eq!(t.feed(3, 2).unwrap().text, "1.");
        assert_eq!(t.feed(3, 2).unwrap().text, "2.");
        assert_eq!(t.feed(3, 1).unwrap().text, "\u{FF08}\u{4e8c}\u{FF09}");
        // 二级新项 → 紧邻三级重置（再进三级从 1 起）。
        assert_eq!(t.feed(3, 2).unwrap().text, "1.");
        // 一级新项：二级重置（ECMA-376 直接触发语义——lvlRestart
        // 缺省 1 = 紧邻上级新项时重置；非传递式全重置，引擎语义锁）。
        assert_eq!(t.feed(3, 0).unwrap().text, "\u{4e8c}\u{3001}");
        // 一级新项后直接进三级：三级的重置触发级是二级（未出现）
        // → 延续计数（3.）——引擎既有语义，与 #266 正文「下级全部
        // 重置」的简化表述有差，PR 内向 Lead 标注裁定。
        assert_eq!(t.feed(3, 2).unwrap().text, "2.");
        // 再进二级（新一项）→ 三级随之重置。
        assert_eq!(t.feed(3, 1).unwrap().text, "\u{FF08}\u{4e00}\u{FF09}");
        assert_eq!(t.feed(3, 2).unwrap().text, "1.");
    }

    /// #266：中文序号写→读全链往返（define_numbering(chinese)
    /// → numbered_list_item → read_docx →
    /// resolved_number——新建列表编号保持的中文面验收）。
    #[test]
    fn write_read_roundtrip_chinese_resolved() {
        use crate::docx::DocxWriter;
        let mut w = DocxWriter::new();
        w.define_numbering(NumberingDef {
            id: 301,
            levels: vec![
                LevelSpec {
                    ilvl: 0,
                    num_fmt: "chineseCounting".into(),
                    lvl_text: "%1\u{3001}".into(),
                    start: 1,
                    lvl_restart: None,
                    is_lgl: false,
                    suffix: SuffixKind::Nothing,
                },
                LevelSpec {
                    ilvl: 1,
                    num_fmt: "chineseLegalSimplified".into(),
                    lvl_text: "\u{FF08}%2\u{FF09}".into(),
                    start: 1,
                    lvl_restart: None,
                    is_lgl: false,
                    suffix: SuffixKind::Nothing,
                },
                LevelSpec {
                    ilvl: 2,
                    num_fmt: "decimal".into(),
                    lvl_text: "%3.".into(),
                    start: 1,
                    lvl_restart: None,
                    is_lgl: false,
                    suffix: SuffixKind::Nothing,
                },
            ],
        })
        .unwrap();
        // 文档序：一、（一）1. 2. （二）1. 二、——多级 + 级联重置。
        w.numbered_list_item(&[crate::docx::RunSpec::text("总则")], 301, 0)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("范围")], 301, 1)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("项一")], 301, 2)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("项二")], 301, 2)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("定义")], 301, 1)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("项")], 301, 2)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("术语")], 301, 0)
            .unwrap();
        let bytes = w.save_to_vec().unwrap();

        // 读回（产品全链——read_docx 而非仅定义面）。
        let doc = crate::docx::read_docx_from_bytes(&bytes).unwrap();
        let seq: Vec<String> = doc
            .blocks
            .iter()
            .filter_map(|b| match b {
                crate::docx::Block::Paragraph(p) => p.resolved_number.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(
            seq,
            vec![
                "\u{4e00}\u{3001}".to_string(),         // 一、
                "\u{FF08}\u{58f9}\u{FF09}".to_string(), // （壹）——legal 大写
                "1.".to_string(),
                "2.".to_string(),
                "\u{FF08}\u{8d30}\u{FF09}".to_string(), // （贰）
                "1.".to_string(),
                "\u{4e8c}\u{3001}".to_string(), // 二、
            ],
            "中文多级序号全链往返（写面定义 \u{2192} 读面 resolved_number）"
        );
    }

    #[test]
    fn parse_numbering_full_face() {
        let bytes = build_test_docx(&three_level_xml());
        let b = read_docx_numbering_from_bytes(&bytes).unwrap();
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        let abs = b.table.abstract_def(7).unwrap();
        assert_eq!(abs.multi_level_type.as_deref(), Some("hybridMultilevel"));
        assert_eq!(abs.levels.len(), 3);
        let l0 = &abs.levels[0];
        assert_eq!(l0.lvl_text, "%1.");
        assert_eq!(l0.lvl_restart, Some(1));
        assert_eq!(l0.suffix, SuffixKind::Tab);
        assert_eq!(l0.pstyle.as_deref(), Some("ListPara"));
        assert_eq!(
            l0.indent,
            Some(ParagraphIndent {
                left: Some(720),
                right: None,
                special: Some(ParagraphIndentSpecial::Hanging(360)),
            })
        );
        // isLgl 承载（level2）。
        assert!(abs.levels[2].is_lgl);
        assert_eq!(abs.levels[2].suffix, SuffixKind::Nothing);
        assert_eq!(abs.levels[2].start, 2);
        // num 桥 + override 双形态。
        let n3 = b.table.num_def(3).unwrap();
        assert_eq!(n3.abstract_id, 7);
        assert_eq!(n3.overrides.len(), 2);
        // effective（numId=3）：lvl1 start=5（startOverride）、lvl2 整级
        // 替换（upperRoman/[%1-%2-%3]/start 9/space）。
        let eff3 = b.table.effective_levels(3).unwrap();
        assert_eq!(eff3[1].start, 5);
        assert_eq!(eff3[1].num_fmt, "lowerLetter"); // startOverride 浅覆盖不换格式
        assert_eq!(eff3[2].num_fmt, "upperRoman");
        assert_eq!(eff3[2].lvl_text, "[%1-%2-%3]");
        assert_eq!(eff3[2].start, 9);
        assert_eq!(eff3[2].suffix, SuffixKind::Space);
        assert!(!eff3[2].is_lgl); // 整级替换后 isLgl 位来自替换级
                                  // numId=4 无 override → 原样。
        let eff4 = b.table.effective_levels(4).unwrap();
        assert_eq!(eff4[2].num_fmt, "lowerRoman");
        assert_eq!(eff4[2].start, 2);
        assert!(eff4[2].is_lgl);
    }

    #[test]
    fn tracker_cascade_and_restart() {
        let bytes = build_test_docx(&three_level_xml());
        let b = read_docx_numbering_from_bytes(&bytes).unwrap();
        // numId=3（lvl1 start=5 经 startOverride；lvl2 替换 upperRoman
        // [%1-%2-%3] start=9；lvlRestart lvl0=Some(1) 显式）。
        let mut tr = NumberingTracker::new(&b.table);
        // lvl0=1；lvl1 从 5 起（e）；lvl2 从 9 起（ix）。
        assert_eq!(tr.feed(3, 0).unwrap().text, "1.");
        assert_eq!(tr.feed(3, 0).unwrap().text, "2.");
        assert_eq!(tr.feed(3, 1).unwrap().text, "2.e."); // startOverride 5: e
        assert_eq!(tr.feed(3, 1).unwrap().text, "2.f.");
        assert_eq!(tr.feed(3, 2).unwrap().text, "[2-f-IX]"); // 替换级 upperRoman：start 9 → IX
        assert_eq!(tr.feed(3, 0).unwrap().text, "3."); // 上级新项
                                                       // lvlRestart 缺省 1：lvl1 重置回 start 5 → e；lvl2 替换级无
                                                       // lvlRestart 元素（缺省 1）→ 重置回 9 → ix。
        assert_eq!(tr.feed(3, 1).unwrap().text, "3.e.");
        assert_eq!(tr.feed(3, 2).unwrap().text, "[3-e-IX]");
        // numId=4（原始定义）：lvl1 lowerLetter 从 1=a；lvl2 lowerRoman
        // start=2=ii、isLgl 在——isLgl 影响的是 % 引用渲染（此 lvlText
        // 全 %n 无字面差异）；实例独立。
        // 跳级（首见即 ilvl1）：父级 cur=0 → "0.a."（Word 行为，登记）。
        assert_eq!(tr.feed(4, 1).unwrap().text, "0.a.");
        // isLgl（lvl2 原始定义）→ %1/%2/%3 全 decimal：0.1.2.
        // （第二钉定场景：isLgl 的渲染级语义——被引用级原生 numFmt
        // [lowerLetter/lowerRoman] 不参与）。
        assert_eq!(tr.feed(4, 2).unwrap().text, "0.1.2.");
        // 非列表段不打断：feed 中穿插（无 API 面——同 numId 继续）。
        assert_eq!(tr.feed(4, 0).unwrap().text, "1.");
        // feed(4,0) 已级联重置 lvl1（r 缺省 1）→ 回 start："a"。
        assert_eq!(tr.feed(4, 1).unwrap().text, "1.a.");
        // reset 显式。
        tr.reset(4);
        assert_eq!(tr.feed(4, 0).unwrap().text, "1.");
        // 悬空/超域。
        assert!(tr.feed(99, 0).is_none());
        assert!(tr.feed(3, 8).is_none());
    }

    #[test]
    fn tracker_lvl_restart_zero_and_two() {
        // lvlRestart=0（永不重置）与 =2（隔级）。
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="1">
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2."/><w:lvlRestart w:val="0"/></w:lvl>
<w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2.%3."/><w:lvlRestart w:val="2"/></w:lvl>
</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;
        let bytes = build_test_docx(std::str::from_utf8(xml).unwrap());
        let b = read_docx_numbering_from_bytes(&bytes).unwrap();
        let mut tr = NumberingTracker::new(&b.table);
        assert_eq!(tr.feed(1, 1).unwrap().text, "0.1."); // 跳级：父计数 0
        assert_eq!(tr.feed(1, 2).unwrap().text, "0.1.1.");
        assert_eq!(tr.feed(1, 0).unwrap().text, "1.");
        // lvl1 lvlRestart=0：不重置 → 2.（跨上级持续计数）。
        assert_eq!(tr.feed(1, 1).unwrap().text, "1.2.");
        // lvl2 lvlRestart=2：ilvl==0 新项时已重置过（上面 feed(1,0)）
        // → 现在从 1。
        assert_eq!(tr.feed(1, 2).unwrap().text, "1.2.1.");
        // 新的 ilvl0：lvl2 应重置（r=2，j=2，ilvl+r=0+2=2 ✓）。
        assert_eq!(tr.feed(1, 0).unwrap().text, "2.");
        assert_eq!(tr.feed(1, 2).unwrap().text, "2.2.1.");
        // 而 lvl1（r=0）在两次 ilvl0 后仍持续。
        assert_eq!(tr.feed(1, 1).unwrap().text, "2.3.");
    }

    #[test]
    fn numfmt_rendering_set() {
        // 渲染集：roman/letters 26 进制/zeroDecimal/ordinal/none/bullet。
        let xml = r#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="1">
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%1."/></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerRoman"/><w:lvlText w:val="%2."/></w:lvl>
<w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="upperLetter"/><w:lvlText w:val="%3."/></w:lvl>
<w:lvl w:ilvl="3"><w:start w:val="1"/><w:numFmt w:val="zeroDecimal"/><w:lvlText w:val="%4."/></w:lvl>
<w:lvl w:ilvl="4"><w:start w:val="1"/><w:numFmt w:val="ordinal"/><w:lvlText w:val="%5."/></w:lvl>
<w:lvl w:ilvl="5"><w:start w:val="1"/><w:numFmt w:val="none"/><w:lvlText w:val="%6."/></w:lvl>
<w:lvl w:ilvl="6"><w:start w:val="1"/><w:numFmt w:val="russianLower"/><w:lvlText w:val="%7."/></w:lvl>
<w:lvl w:ilvl="7"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:suff w:val="nothing"/></w:lvl>
</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;
        let bytes = build_test_docx(xml);
        let b = read_docx_numbering_from_bytes(&bytes).unwrap();
        let mut tr = NumberingTracker::new(&b.table);
        // 逐级推进到值 27/13 等：先全 1。
        assert_eq!(tr.feed(1, 0).unwrap().text, "I.");
        assert_eq!(tr.feed(1, 1).unwrap().text, "i.");
        assert_eq!(tr.feed(1, 2).unwrap().text, "A.");
        assert_eq!(tr.feed(1, 3).unwrap().text, "01."); // zeroDecimal 2 位
        assert_eq!(tr.feed(1, 4).unwrap().text, "1st.");
        assert_eq!(tr.feed(1, 5).unwrap().text, ""); // none → 空
        assert_eq!(tr.feed(1, 6).unwrap().text, "1."); // russianLower 降级 decimal
        assert_eq!(tr.feed(1, 7).unwrap().text, "•"); // bullet 字面
                                                      // 值钉定：lowerRoman 4=iv；upperLetter 27=AA；ordinal 11/12/13。
        let mut tr2 = NumberingTracker::new(&b.table);
        for _ in 0..3 {
            let _ = tr2.feed(1, 0);
        }
        assert_eq!(tr2.feed(1, 0).unwrap().text, "IV.");
        for _ in 0..26 {
            let _ = tr2.feed(1, 2);
        }
        assert_eq!(tr2.feed(1, 2).unwrap().text, "AA."); // 27
        for _ in 0..10 {
            let _ = tr2.feed(1, 4);
        }
        assert_eq!(tr2.feed(1, 4).unwrap().text, "11th.");
        assert_eq!(tr2.feed(1, 4).unwrap().text, "12th.");
        assert_eq!(tr2.feed(1, 4).unwrap().text, "13th.");
        assert_eq!(tr2.feed(1, 4).unwrap().text, "14th.");
        // zeroDecimal ≥100 原样（先推 99 次，第 100 项 = 100）。
        for _ in 0..99 {
            let _ = tr2.feed(1, 3);
        }
        assert_eq!(tr2.feed(1, 3).unwrap().text, "100.");
    }

    #[test]
    fn dangling_and_num_style_link() {
        // numStyleLink 在场 → effective None（二跳面外）。
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="1"><w:numStyleLink w:val="StyleX"/>
<w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="99"/></w:num>
</w:numbering>"#;
        let bytes = build_test_docx(std::str::from_utf8(xml).unwrap());
        let b = read_docx_numbering_from_bytes(&bytes).unwrap();
        assert!(b.table.effective_levels(1).is_none()); // numStyleLink
        assert!(b.table.effective_levels(2).is_none()); // abstract 悬空
        let mut tr = NumberingTracker::new(&b.table);
        assert!(tr.feed(1, 0).is_none());
        assert!(tr.feed(2, 0).is_none());
    }

    #[test]
    fn write_face_validation_matrix() {
        use crate::docx::DocxWriter;
        let mut w = DocxWriter::new();
        // id 撞内建/预置。
        for bad in [1usize, 100, 101] {
            let e = w
                .define_numbering(NumberingDef {
                    id: bad,
                    levels: vec![LevelSpec::decimal(0, "%1.")],
                })
                .unwrap_err();
            assert!(e.to_string().contains("collides"), "{e}");
        }
        // 空 levels。
        let e = w
            .define_numbering(NumberingDef {
                id: 200,
                levels: vec![],
            })
            .unwrap_err();
        assert!(e.to_string().contains("non-empty"));
        // ilvl 降序。
        let e = w
            .define_numbering(NumberingDef {
                id: 200,
                levels: vec![
                    LevelSpec::decimal(1, "%1.%2."),
                    LevelSpec::decimal(0, "%1."),
                ],
            })
            .unwrap_err();
        assert!(e.to_string().contains("ascending"));
        // numFmt 白名单。
        let e = w
            .define_numbering(NumberingDef {
                id: 200,
                levels: vec![LevelSpec {
                    num_fmt: "chicago".into(),
                    ..LevelSpec::decimal(0, "%1.")
                }],
            })
            .unwrap_err();
        assert!(e.to_string().contains("whitelist"));
        // start 0。
        let e = w
            .define_numbering(NumberingDef {
                id: 200,
                levels: vec![LevelSpec {
                    start: 0,
                    ..LevelSpec::decimal(0, "%1.")
                }],
            })
            .unwrap_err();
        assert!(e.to_string().contains(">= 1"));
        // lvlText 深引用（%3 在 ilvl 1）。
        let e = w
            .define_numbering(NumberingDef {
                id: 200,
                levels: vec![LevelSpec::decimal(0, "%1."), LevelSpec::decimal(1, "%3.")],
            })
            .unwrap_err();
        assert!(e.to_string().contains("deeper level"));
        // 合法定义后：重名 + 悬空引用。
        w.define_numbering(NumberingDef {
            id: 200,
            levels: vec![
                LevelSpec::decimal(0, "%1."),
                LevelSpec {
                    ilvl: 1,
                    num_fmt: "lowerLetter".into(),
                    lvl_text: "%1.%2.".into(),
                    start: 1,
                    lvl_restart: Some(0),
                    is_lgl: false,
                    suffix: SuffixKind::Tab,
                },
            ],
        })
        .unwrap();
        let e = w
            .define_numbering(NumberingDef {
                id: 200,
                levels: vec![LevelSpec::decimal(0, "%1.")],
            })
            .unwrap_err();
        assert!(e.to_string().contains("already defined"));
        let e = w
            .numbered_list_item(&[crate::docx::RunSpec::text("x")], 999, 0)
            .unwrap_err();
        assert!(e.to_string().contains("define_numbering first"));
        // 合法引用 + 超域 ilvl。
        w.numbered_list_item(&[crate::docx::RunSpec::text("x")], 200, 1)
            .unwrap();
        let e = w
            .numbered_list_item(&[crate::docx::RunSpec::text("x")], 200, 8)
            .unwrap_err();
        assert!(e.to_string().contains("defines levels up to"), "{e}");
    }

    #[test]
    fn write_read_roundtrip_full() {
        use crate::docx::DocxWriter;
        let mut w = DocxWriter::new();
        w.define_numbering(NumberingDef {
            id: 300,
            levels: vec![
                LevelSpec {
                    ilvl: 0,
                    num_fmt: "decimal".into(),
                    lvl_text: "%1.".into(),
                    start: 1,
                    lvl_restart: Some(1),
                    is_lgl: false,
                    suffix: SuffixKind::Tab,
                },
                LevelSpec {
                    ilvl: 1,
                    num_fmt: "lowerLetter".into(),
                    lvl_text: "%1)%2.".into(),
                    start: 3,
                    lvl_restart: Some(0),
                    is_lgl: true,
                    suffix: SuffixKind::Space,
                },
                LevelSpec {
                    ilvl: 2,
                    num_fmt: "upperRoman".into(),
                    lvl_text: "%1.%2.%3.".into(),
                    start: 1,
                    lvl_restart: None,
                    is_lgl: false,
                    suffix: SuffixKind::Nothing,
                },
            ],
        })
        .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("一")], 300, 0)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("一a")], 300, 1)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("一a.i")], 300, 2)
            .unwrap();
        w.numbered_list_item(&[crate::docx::RunSpec::text("二")], 300, 0)
            .unwrap();
        let bytes = w.save_to_vec().unwrap();

        // 读回：定义镜像 + tracker 序列（与写时期望序号一致）。
        let b = read_docx_numbering_from_bytes(&bytes).unwrap();
        let eff = b.table.effective_levels(300).unwrap();
        assert_eq!(eff.len(), 3);
        assert_eq!(eff[0].lvl_text, "%1.");
        assert_eq!(eff[1].lvl_text, "%1)%2.");
        assert_eq!(eff[1].start, 3);
        assert_eq!(eff[1].lvl_restart, Some(0));
        assert!(eff[1].is_lgl);
        assert_eq!(eff[1].suffix, SuffixKind::Space);
        assert_eq!(eff[2].num_fmt, "upperRoman");
        assert_eq!(eff[2].suffix, SuffixKind::Nothing);

        let mut tr = NumberingTracker::new(&b.table);
        assert_eq!(tr.feed(300, 0).unwrap().text, "1.");
        // lvl1 start=3 → c；isLgl 在（lvlText 无 % 引用交叉[只 %1%2]
        // ——isLgl 位承载验证在 eff[1].is_lgl）。
        // isLgl = true：`%2` 渲染恒 decimal（legal numbering——本断言
        // 即 isLgl 行为钉定：lvl1 numFmt=lowerLetter 且 start=3，isLgl
        // 在场时 %2 显示 "3" 而非 "c"）。
        assert_eq!(tr.feed(300, 1).unwrap().text, "1)3.");
        // lvl2 无 isLgl：%2 按被引用级（lvl1）原生 numFmt lowerLetter
        // → "c"（start=3 首项 = 3 = c）。
        assert_eq!(tr.feed(300, 2).unwrap().text, "1.c.I.");
        // lvl2 lvlRestart 缺省 1：上级(ilvl1)新项时重置——但 lvl1
        // lvlRestart=0 不影响 lvl2 的判定级（lvl2 的 r 缺省=1 → 由
        // ilvl1 新项重置）。
        assert_eq!(tr.feed(300, 0).unwrap().text, "2.");
        // lvl1 持续（r=0）→ d。
        assert_eq!(tr.feed(300, 1).unwrap().text, "2)4."); // isLgl + r=0 持续（3→4）

        // XML 面：numbering.xml 关键属性形。
        use std::io::Read as _;
        let mut za = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut numxml = String::new();
        za.by_name("word/numbering.xml")
            .unwrap()
            .read_to_string(&mut numxml)
            .unwrap();
        for needle in [
            "w:abstractNumId=\"300\"",
            "<w:lvlText w:val=\"%1)%2.\" />",
            "<w:start w:val=\"3\" />",
            "<w:numFmt w:val=\"upperRoman\" />",
            "<w:lvlRestart w:val=\"0\" />",
            "<w:isLgl />",
            "<w:suff w:val=\"space\" />",
            "<w:suff w:val=\"nothing\" />",
            "<w:pStyle w:val=\"ListPara\" />", // 预置应有 ListPara？无——写面不产
        ] {
            if needle.starts_with("<w:pStyle") {
                continue; // 写面无 pstyle（登记）——占位跳过
            }
            assert!(
                numxml.contains(needle),
                "numbering.xml missing {needle}\n{numxml}"
            );
        }
        let mut docxml = String::new();
        za.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut docxml)
            .unwrap();
        assert!(docxml.contains("<w:numId w:val=\"300\" />"));
        assert!(docxml.contains("<w:ilvl w:val=\"2\" />"));
    }
}
