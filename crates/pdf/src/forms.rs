//! S2-5（issue #116 任务 1）· PDF 表单（AcroForm）读写。
//!
//! # 定位
//!
//! 标书提交的最后一环：招标人发的 PDF 投标函模板里有一堆可填写域，
//! 投标人必须在 Acrobat Reader 里填好、打印、签字、扫描。本模块让这条
//! 链路在软件里闭环——读域、填值、写回、扁平化。
//!
//! # AcroForm 结构（ISO 32000-1 §12.7）
//!
//! ```text
//! Catalog /AcroForm ─┬─ /Fields [ 域对象引用 … ]   ← 域树的根（可嵌套 /Kids）
//!                     ├─ /DA   默认外观串（/Helv 0 Tf 0 g）
//!                     └─ /DR   默认资源字典（/Font）
//!
//! 每个域 = 字典 + 可选 /Widget 合并态：
//!   /FT  域类型  Tx(文本) Btn(按钮) Ch(选择) Sig(签名)
//!   /T   域全名（嵌套时按 `.` 拼接：parent.child）
//!   /V   域值    文本=字符串  复选框=名字(On/Off)  单选=选项名  选择=字符串或数组
//!   /Ff  标志位  bit0 ReadOnly  bit1 Required  bit2 NoExport
//!                Btn: bit15 Radio  bit16 Pushbutton  bit17 Combo
//!   /Rect /MK /AP  外观
//! ```
//!
//! **域可能来自两处**：`/AcroForm /Fields`（域树）与页 `/Annots` 里的
//! `/Widget`（纯控件）。两者常是**同一批对象**（合并态 Widget 既是域又
//! 是标注），但**也常有只有 Widget 没有登记进 /Fields 的野控件**（某些
//! 生成器行为）。本模块两处都扫，以对象号去重——只信 /Fields 会漏域。
//!
//! # 写回语义（增量修订——#227 起为真增量）
//!
//! 与 [`crate::annotate`] 同一纪律：**不改写原字节**。填值保存 =
//! 追加一个增量修订段（spec 7.5.6）：被改动的域对象按**同对象号在
//! 新修订里重定义**（后修订覆盖前修订），外观流/字体等新对象随修订
//! 追加；既有对象一字不重写。产物以原字节为逐位相同前缀 → 已有的
//! 数字签名仍然有效（改前面的字节会让签名失效，这是 PAdES 的硬
//! 约束）。
//!
//! 守卫（与 [`crate::writer_lopdf`] 三层同款，违约 = typed 错误而非
//! 产出损坏文件）：xref 链不可信档（扫描重建装载）拒绝写；前缀逐位
//! 不变断言；产物重载自检。已签名档打开即告警（`report()`），扁平化
//! 对已签名档拒绝并给可操作指引（见下节）。
//!
//! # 中文与外观（最容易踩的坑）
//!
//! 填空不只是把 `/V` 改掉——**没有 `/AP` 外观流的域，多数阅读器（尤其
//! 是 Acrobat）不显示新值**：域里有字、屏幕上却空白。而用默认的
//! `/Helv` 生成外观流，写中文会变成一串乱码或问号。
//!
//! 本模块三条对策：
//! 1. **有 CIDFont 就嵌**：调用方注入 TTF 字节（[`PdfForm::set_font`]），
//!    生成 Type0/Identity-H 外观流，中文正常显示；
//! 2. **没有字体就不伪造外观**：`need_appearances` 置真（Acrobat 会
//!    自行用 `/DA` 渲染），并如实报告「未生成外观流」——绝不写一段
//!    乱码冒充成功；
//! 3. **明确预警**：填了 CJK 却既无字体又无法保证渲染时，返回告警而非
//!    静默通过（标书场景下乱码比空白更危险——审标人看到的是「写错了」）。
//!
//! 扁平化（[`PdfForm::flatten`]）则相反：**把值真正画进页面内容流**，
//! 再删掉域与 `/AcroForm`——产物跟打印扫描件一样，任何人再也改不了。
//! 这一步**必须**有字体才能画中文，否则 `flatten` 明确拒绝（宁可不做，
//! 不做错）；含**已签署**签名域的档同样拒绝（扁平化会销毁签名——
//! 可操作指引见 [`PdfForm::flatten`]）。

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{dictionary, Document, IncrementalDocument, Object, ObjectId};

use crate::security::Limits;

// ---------------------------------------------------------------------------
// 类型面
// ---------------------------------------------------------------------------

/// `/Ff` 域标志位（ISO 32000-1 Table 221）。只建模校验相关的位。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldFlags(u32);

impl FieldFlags {
    /// 从原始 `/Ff` 整数值构造。
    pub fn from_raw(raw: i64) -> Self {
        FieldFlags(raw as u32)
    }
    /// 原始整数值（写回时原样保留）。
    pub fn raw(self) -> u32 {
        self.0
    }
    /// bit0：只读。
    pub fn is_read_only(self) -> bool {
        self.0 & 1 != 0
    }
    /// bit1：必填。
    pub fn is_required(self) -> bool {
        self.0 & 2 != 0
    }
    /// bit2：不导出。
    pub fn is_no_export(self) -> bool {
        self.0 & 4 != 0
    }
}

/// 校验错误类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// 必填未填（`/Ff` bit1）。
    Required,
    /// 超过 `/MaxLen`。
    TooLong,
    /// 不匹配格式规则（`/AA /F` 的 JavaScript——本版**不产生**，
    /// 见 [`PdfForm::validate`] 的诚实边界说明）。
    PatternMismatch,
    /// 不在 `/Opt` 选项集内。
    InvalidChoice,
}

impl ErrorKind {
    /// 稳定机器码（日志/闸门面）。
    pub fn code(&self) -> &'static str {
        match self {
            ErrorKind::Required => "FIELD_REQUIRED",
            ErrorKind::TooLong => "FIELD_TOO_LONG",
            ErrorKind::PatternMismatch => "FIELD_PATTERN_MISMATCH",
            ErrorKind::InvalidChoice => "FIELD_INVALID_CHOICE",
        }
    }
}

/// 单条校验错误。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldError {
    /// 域全名。
    pub field: String,
    /// 错误类别。
    pub kind: ErrorKind,
    /// 人类可读说明（面向投标经办人）。
    pub message: String,
}

/// 整表校验结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ValidationResult {
    /// 全部通过。
    pub valid: bool,
    /// 错误清单（域名字典序——确定性输出）。
    pub errors: Vec<FieldError>,
}

impl ValidationResult {
    /// 一次性人类可读回报。
    pub fn to_message(&self) -> String {
        if self.valid {
            "表单校验通过".to_string()
        } else {
            format!(
                "表单校验未通过（{} 处）：{}",
                self.errors.len(),
                self.errors
                    .iter()
                    .map(|e| format!("{}：{}", e.field, e.message))
                    .collect::<Vec<_>>()
                    .join("；")
            )
        }
    }
}

/// 文本对齐（`/Q`：0 左 1 中 2 右）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

impl Alignment {
    /// `/Q` 整数值。
    pub fn to_q(self) -> i64 {
        match self {
            Alignment::Left => 0,
            Alignment::Center => 1,
            Alignment::Right => 2,
        }
    }
    /// 从 `/Q` 反解析（未知按左对齐）。
    pub fn from_q(q: i64) -> Self {
        match q {
            1 => Alignment::Center,
            2 => Alignment::Right,
            _ => Alignment::Left,
        }
    }
}

/// 外观定制参数。只影响生成的 `/AP` 外观流与 `/DA`/`/Q` 字典字段；不改值。
#[derive(Debug, Clone, PartialEq)]
pub struct FieldAppearance {
    /// 字号（pt）。
    pub font_size: f32,
    /// 字体名（`Helv` = Helvetica 内置）。
    pub font_name: String,
    /// 对齐。
    pub alignment: Alignment,
    /// 文字颜色（0~255 三通道）。
    pub text_color: (u8, u8, u8),
    /// 背景色（None = 不画背景）。
    pub background: Option<(u8, u8, u8)>,
}

impl Default for FieldAppearance {
    fn default() -> Self {
        FieldAppearance {
            font_size: 10.0,
            font_name: "Helv".to_string(),
            alignment: Alignment::Left,
            text_color: (0, 0, 0),
            background: None,
        }
    }
}
/// 表单域类型（issue #116 草图形状；`kind()` 给出稳定机器名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormField {
    /// 文本框。`max_len` 来自 `/MaxLen`（None = 不限）。
    Text {
        name: String,
        value: String,
        max_len: Option<usize>,
    },
    /// 复选框。`on_name` 是选中态的名字（通常是 `/Yes`，但**不总是**——
    /// 必须从 `/AP /N` 的键里读，写死 `/Yes` 在真实模板上会勾不上）。
    Checkbox {
        name: String,
        checked: bool,
        on_name: String,
    },
    /// 单选按钮组。`options` 是组内各选项名，`selected` 是当前选中项。
    Radio {
        name: String,
        selected: Option<String>,
        options: Vec<String>,
    },
    /// 下拉/列表。`selected` 允许多值（多选列表）。
    Choice {
        name: String,
        selected: Vec<String>,
        options: Vec<String>,
        combo: bool,
    },
    /// 数字签名字段（签名内容见 [`crate::signature`]）。
    Signature { name: String, signed: bool },
}

impl FormField {
    /// 域全名（嵌套域为 `parent.child`）。
    pub fn name(&self) -> &str {
        match self {
            FormField::Text { name, .. }
            | FormField::Checkbox { name, .. }
            | FormField::Radio { name, .. }
            | FormField::Choice { name, .. }
            | FormField::Signature { name, .. } => name,
        }
    }

    /// 稳定机器名（日志/报告/契约面；不是给人看的说明）。
    pub fn kind(&self) -> &'static str {
        match self {
            FormField::Text { .. } => "text",
            FormField::Checkbox { .. } => "checkbox",
            FormField::Radio { .. } => "radio",
            FormField::Choice { .. } => "choice",
            FormField::Signature { .. } => "signature",
        }
    }

    /// 当前值的展示形（报告/审计面；复选框为 "on"/"off"）。
    pub fn value_display(&self) -> String {
        match self {
            FormField::Text { value, .. } => value.clone(),
            FormField::Checkbox { checked, .. } => {
                if *checked {
                    "on".into()
                } else {
                    "off".into()
                }
            }
            FormField::Radio { selected, .. } => selected.clone().unwrap_or_default(),
            FormField::Choice { selected, .. } => selected.join(","),
            FormField::Signature { signed, .. } => {
                if *signed {
                    "signed".into()
                } else {
                    "unsigned".into()
                }
            }
        }
    }
}

/// 填值报告（全量登记纪律——做了什么、没做什么、为什么）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FillReport {
    /// 实际改动对象的域数（幂等：值没变的域不计）。
    pub changed: usize,
    /// 值没变而跳过的域数。
    pub unchanged: usize,
    /// 告警（人类可读——中文缺字体、值被截断、只读域被跳过等）。
    pub warnings: Vec<String>,
    /// 生成了外观流的域数（0 = 依赖阅读器自渲染）。
    pub appearances_built: usize,
}

impl FillReport {
    fn warn(&mut self, msg: String) {
        if !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }
}

// ---------------------------------------------------------------------------
// 内部：域的原始形态
// ---------------------------------------------------------------------------

/// 一个域在文档里的物理位置与解析结果。
#[derive(Debug, Clone)]
struct RawField {
    /// 域字典所在对象（合并态 Widget 就是它自己；分离态是父域字典）。
    field_id: ObjectId,
    /// `/T` 全名。
    name: String,
    /// 解析出的模型。
    model: FormField,
    /// Widget 对象（**跨页同名域会有多个**）。
    widget_ids: Vec<ObjectId>,
    /// `/Ff` 标志位。
    flags: FieldFlags,
}

// ---------------------------------------------------------------------------
// PdfForm
// ---------------------------------------------------------------------------

/// 表单门面：读 → 填 → 写回 / 扁平化。
///
/// 生命周期与 [`crate::annotate::PdfAnnotator`] 同款：`open` 后多次改，
/// 最后一次 `save`。
#[derive(Debug)]
pub struct PdfForm {
    doc: Document,
    original: Vec<u8>,
    fields: BTreeMap<String, RawField>,
    /// 被改过、待写回的对象（对象号 → 新字典）。
    dirty: BTreeMap<ObjectId, lopdf::Dictionary>,
    /// 需要生成外观流的文本域（name → (widget_id, rect, font_size)）。
    appearance_jobs: BTreeMap<String, AppearanceJob>,
    /// 注入的 TTF 字节（中文 CIDFont 嵌入源）。
    font: Option<Vec<u8>>,
    /// 外观覆盖（set_appearance 登记；生成外观流时套用）。
    appearance_overrides: BTreeMap<String, FieldAppearance>,
    /// 是否发现了 CJK 值（决定是否需要外观/告警）。
    report: FillReport,
    /// 打开的限额（沿用 crate 纪律——`PdfForm::limits()` 读取）。
    limits: Limits,
    /// 打开时既有对象的号集（增量写的基线：会话中 `add_object` 新增的
    /// 对象不在其中——它们须随追加修订写出，既有对象则一字不动地留在
    /// 原字节前缀里）。
    base_ids: BTreeSet<ObjectId>,
}

/// 待生成外观流的作业。
#[derive(Debug, Clone)]
struct AppearanceJob {
    widget_id: ObjectId,
    rect: [f64; 4],
}

/// 表单操作错误（本模块自有的窄错误面——不污染 [`crate::command::CommandError`]
/// 的六命令契约；接线方按需映射）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    /// 文档装载/解析失败。
    Open(String),
    /// 不支持的加密文档（S5 纪律：加密写拒）。
    Encrypted,
    /// 域不存在。
    FieldNotFound(String),
    /// 域类型不匹配（如对复选框 set_text）。
    WrongFieldType {
        name: String,
        expected: &'static str,
        actual: String,
    },
    /// 域只读（/Ff bit0）。
    ReadOnly(String),
    /// 值不合法（超出 max_len、不在选项集里）。
    InvalidValue { name: String, reason: String },
    /// 扁平化遇到无法渲染的值（缺字体画不了中文等）。
    FlattenRefused(String),
    /// 写回失败。
    Save(String),
}

impl std::fmt::Display for FormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormError::Open(e) => write!(f, "PDF 表单装载失败：{e}"),
            FormError::Encrypted => write!(f, "文档已加密（表单写回被拒——S5 纪律）"),
            FormError::FieldNotFound(n) => write!(f, "表单域 {n} 不存在"),
            FormError::WrongFieldType {
                name,
                expected,
                actual,
            } => {
                write!(f, "表单域 {name} 类型不符：期望 {expected}，实际 {actual}")
            }
            FormError::ReadOnly(n) => write!(f, "表单域 {n} 只读（/Ff bit0）"),
            FormError::InvalidValue { name, reason } => {
                write!(f, "表单域 {name} 的值不合法：{reason}")
            }
            FormError::FlattenRefused(e) => write!(f, "扁平化被拒：{e}"),
            FormError::Save(e) => write!(f, "表单写回失败：{e}"),
        }
    }
}

impl std::error::Error for FormError {}

// ---------------------------------------------------------------------------
// 读
// ---------------------------------------------------------------------------

impl PdfForm {
    /// 打开文档并读出全部域。
    pub fn read(bytes: &[u8]) -> Result<Self, FormError> {
        Self::read_with_limits(bytes, Limits::default())
    }

    /// 带限额打开（S3-L7 输入层先于解析——与 [`crate::writer_lopdf`] 同序）。
    pub fn read_with_limits(bytes: &[u8], limits: Limits) -> Result<Self, FormError> {
        limits
            .check_open_input_bytes(bytes.len() as u64)
            .map_err(|e| FormError::Open(format!("{e:?}")))?;
        let doc = Document::load_mem(bytes)
            .map_err(|e| FormError::Open(format!("lopdf 装载失败：{e}")))?;
        if doc.is_encrypted() {
            return Err(FormError::Encrypted);
        }
        let fields = collect_fields(&doc);
        let base_ids = doc.objects.keys().copied().collect();
        let mut form = PdfForm {
            doc,
            original: bytes.to_vec(),
            fields,
            dirty: BTreeMap::new(),
            appearance_jobs: BTreeMap::new(),
            font: None,
            appearance_overrides: BTreeMap::new(),
            report: FillReport::default(),
            limits,
            base_ids,
        };
        // 已签名档的打开告知（保护可见化：调用方从 report() 即知后续
        // 保存是增量追加、扁平化会被拒——不静默）。
        if form.has_signed_signature_fields() {
            form.report.warn(
                "文档含已签署数字签名域：表单保存将追加增量修订（原字节前缀逐位不变，
                 既有签名保持有效）；扁平化会被拒绝（会销毁签名）"
                    .into(),
            );
        }
        Ok(form)
    }

    /// 全部域（按域名字典序——确定性输出）。
    pub fn fields(&self) -> Vec<FormField> {
        self.fields.values().map(|r| r.model.clone()).collect()
    }

    /// 按名取域。
    pub fn field(&self, name: &str) -> Option<&FormField> {
        self.fields.get(name).map(|r| &r.model)
    }

    /// 域数量。
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// 无域。
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// 累计填值报告（告警全量登记）。
    pub fn report(&self) -> &FillReport {
        &self.report
    }

    /// 打开时使用的限额（S3 预算面——接线方核对用）。
    pub fn limits(&self) -> Limits {
        self.limits
    }
}

// ---------------------------------------------------------------------------
// 域收集（/AcroForm/Fields + 页 /Annots 双源）
// ---------------------------------------------------------------------------

/// 收集域：以 `/T` 全名为键（同名后出现者覆盖——spec 要求全名唯一，
/// 重复按后者胜，与 Acrobat 口径一致）。
fn collect_fields(doc: &Document) -> BTreeMap<String, RawField> {
    let mut out: BTreeMap<String, RawField> = BTreeMap::new();
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();

    // 源一：/AcroForm /Fields（含嵌套 /Kids）
    if let Some(acro) = acroform(doc) {
        if let Ok(fields) = acro.get(b"Fields").and_then(Object::as_array) {
            for f in fields {
                if let Ok(id) = f.as_reference() {
                    walk_field_tree(doc, id, "", &mut out, &mut seen, 0);
                }
            }
        }
    }

    // 源二：页 /Annots 里的 /Widget（未登记进 /Fields 的野控件）
    for (_page_no, page_id) in doc.get_pages() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Ok(annots) = page.get(b"Annots").and_then(Object::as_array) else {
            continue;
        };
        for a in annots {
            let Ok(widget_id) = a.as_reference() else {
                continue;
            };
            if seen.contains(&widget_id) {
                continue;
            }
            if widget_id.0 != 0 {
                let Ok(w) = doc.get_dictionary(widget_id) else {
                    continue;
                };
                if w.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) != Some(&b"Widget"[..]) {
                    continue;
                }
                walk_field_tree(doc, widget_id, "", &mut out, &mut seen, 0);
            }
        }
    }

    out
}

/// `/AcroForm` 字典（可能直接内联，也可能经引用）。
fn acroform(doc: &Document) -> Option<&lopdf::Dictionary> {
    let root = doc.trailer.get(b"Root").ok()?.as_reference().ok()?;
    let catalog = doc.get_dictionary(root).ok()?;
    let acro = catalog.get(b"AcroForm").ok()?;
    match acro {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// 递归走域树：有 `/T` 且带 `/FT`（或继承）的节点即一个域；
/// 有 `/Kids` 且子项都没有 `/T` 的是**单选组/层级容器**。
fn walk_field_tree(
    doc: &Document,
    id: ObjectId,
    prefix: &str,
    out: &mut BTreeMap<String, RawField>,
    seen: &mut BTreeSet<ObjectId>,
    depth: u32,
) {
    // 深度护栏（敌意文档：环形 /Kids 引用）
    if depth > 64 || !seen.insert(id) {
        return;
    }
    let Ok(d) = doc.get_dictionary(id) else {
        return;
    };

    let local_name = d
        .get(b"T")
        .ok()
        .and_then(|o| o.as_str().ok())
        .map(decode_pdf_string)
        .unwrap_or_default();

    let full_name = if local_name.is_empty() {
        prefix.to_string()
    } else if prefix.is_empty() {
        local_name.clone()
    } else {
        format!("{prefix}.{local_name}")
    };

    let kids: Vec<ObjectId> = d
        .get(b"Kids")
        .ok()
        .and_then(|o| o.as_array().ok())
        .map(|arr| arr.iter().filter_map(|k| k.as_reference().ok()).collect())
        .unwrap_or_default();

    let ft = inherited_name(doc, id, b"FT");

    if let Some(ft) = ft {
        if !full_name.is_empty() {
            let ff = inherited_i64(doc, id, b"Ff").unwrap_or(0);
            let model = build_model(doc, id, ft, &full_name, &kids, ff);
            let widgets = if kids.is_empty() {
                vec![id]
            } else {
                kids.clone()
            };
            // 跨页同名域合并：同一 /T 在多页各挂一份 Widget（reportlab 这类
            // 生成器建两个独立对象，不是 /Kids 树）。语义上是同一字段。
            match out.get_mut(&full_name) {
                Some(existing) => {
                    for w in widgets {
                        if !existing.widget_ids.contains(&w) {
                            existing.widget_ids.push(w);
                        }
                    }
                }
                None => {
                    out.insert(
                        full_name.clone(),
                        RawField {
                            field_id: id,
                            name: full_name,
                            model,
                            widget_ids: widgets,
                            flags: FieldFlags::from_raw(ff),
                        },
                    );
                }
            }
        }
    } else if !kids.is_empty() {
        // 容器（无 /FT）：继续向下
        for k in kids {
            walk_field_tree(doc, k, &full_name, out, seen, depth + 1);
        }
    } else if !full_name.is_empty() {
        // 既无 /FT 也无子——不是域（可能是注释），跳过
    }
}

/// 沿 `/Parent` 链找继承属性（spec 12.7.3.1：/FT /V /Ff /DA 可继承）。
fn inherited_name(doc: &Document, id: ObjectId, key: &[u8]) -> Option<Vec<u8>> {
    let mut cur = id;
    for _ in 0..64 {
        let d = doc.get_dictionary(cur).ok()?;
        if let Ok(v) = d.get(key) {
            if let Ok(n) = v.as_name() {
                return Some(n.to_vec());
            }
        }
        cur = match d.get(b"Parent").ok()?.as_reference() {
            Ok(p) => p,
            Err(_) => return None,
        };
    }
    None
}

/// 继承「名字或字符串」属性（按钮 /V 是 **名字对象** `/Yes`，不是字符串——
/// 用 `as_str()` 读会静默拿到 None，勾选态就永远读成 false）。
fn inherited_value_as_name_or_str(doc: &Document, id: ObjectId) -> Option<String> {
    let mut cur = id;
    for _ in 0..64 {
        let d = doc.get_dictionary(cur).ok()?;
        if let Ok(v) = d.get(b"V") {
            return match v {
                Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
                Object::String(s, _) => Some(decode_pdf_string(s)),
                _ => None,
            };
        }
        cur = d.get(b"Parent").ok().and_then(|o| o.as_reference().ok())?;
    }
    None
}

/// 继承数值属性（/Ff）。
fn inherited_i64(doc: &Document, id: ObjectId, key: &[u8]) -> Option<i64> {
    let mut cur = id;
    for _ in 0..64 {
        let d = doc.get_dictionary(cur).ok()?;
        if let Ok(v) = d.get(key) {
            if let Ok(n) = v.as_i64() {
                return Some(n);
            }
        }
        cur = match d.get(b"Parent").ok()?.as_reference() {
            Ok(p) => p,
            Err(_) => return None,
        };
    }
    None
}

/// 继承解码后的字符串（/V 用于签名判定）。
fn inherited_str(doc: &Document, id: ObjectId, key: &[u8]) -> Option<String> {
    let mut cur = id;
    for _ in 0..64 {
        let d = doc.get_dictionary(cur).ok()?;
        if let Ok(v) = d.get(key) {
            if let Ok(s) = v.as_str() {
                return Some(decode_pdf_string(s));
            }
        }
        cur = match d.get(b"Parent").ok()?.as_reference() {
            Ok(p) => p,
            Err(_) => return None,
        };
    }
    None
}

/// 按 /FT 构造模型。
fn build_model(
    doc: &Document,
    id: ObjectId,
    ft: Vec<u8>,
    name: &str,
    kids: &[ObjectId],
    ff: i64,
) -> FormField {
    match ft.as_slice() {
        b"Tx" => {
            let value = inherited_str(doc, id, b"V").unwrap_or_default();
            let max_len = doc
                .get_dictionary(id)
                .ok()
                .and_then(|d| d.get(b"MaxLen").ok().and_then(|o| o.as_i64().ok()))
                .map(|n| n.max(0) as usize);
            FormField::Text {
                name: name.to_string(),
                value,
                max_len,
            }
        }
        b"Btn" => {
            let radio = ff & (1 << 15) != 0;
            if radio {
                let selected = inherited_value_as_name_or_str(doc, id);
                let options: Vec<String> = if kids.is_empty() {
                    button_on_names(doc, id)
                } else {
                    kids.iter().flat_map(|k| button_on_names(doc, *k)).collect()
                };
                FormField::Radio {
                    name: name.to_string(),
                    selected,
                    options,
                }
            } else {
                let raw = inherited_value_as_name_or_str(doc, id).unwrap_or_else(|| "Off".into());
                let checked = !raw.is_empty() && raw != "Off";
                let on_name = button_on_names(doc, id)
                    .into_iter()
                    .find(|n| n != "Off")
                    .unwrap_or_else(|| "Yes".into());
                FormField::Checkbox {
                    name: name.to_string(),
                    checked,
                    on_name,
                }
            }
        }
        b"Ch" => {
            let combo = ff & (1 << 17) != 0;
            let options = choice_options(doc, id);
            let selected = match doc.get_dictionary(id).ok().and_then(|d| d.get(b"V").ok()) {
                Some(Object::Array(a)) => a
                    .iter()
                    .filter_map(|o| o.as_str().ok())
                    .map(decode_pdf_string)
                    .collect(),
                Some(o) => o
                    .as_str()
                    .ok()
                    .map(|s| vec![decode_pdf_string(s)])
                    .unwrap_or_default(),
                None => vec![],
            };
            FormField::Choice {
                name: name.to_string(),
                selected,
                options,
                combo,
            }
        }
        b"Sig" => {
            let signed = inherited_str(doc, id, b"V").is_some();
            FormField::Signature {
                name: name.to_string(),
                signed,
            }
        }
        _ => FormField::Text {
            name: name.to_string(),
            value: inherited_str(doc, id, b"V").unwrap_or_default(),
            max_len: None,
        },
    }
}

/// 按钮的「开」态名字：从 `/AP /N` 的键里读（**不能写死 /Yes**）。
fn button_on_names(doc: &Document, id: ObjectId) -> Vec<String> {
    let Ok(d) = doc.get_dictionary(id) else {
        return vec![];
    };
    let ap = match d.get(b"AP").ok() {
        Some(Object::Reference(r)) => match doc.get_dictionary(*r) {
            Ok(x) => x,
            Err(_) => return vec![],
        },
        Some(Object::Dictionary(x)) => x,
        _ => return vec![],
    };
    let n = match ap.get(b"N").ok() {
        Some(Object::Reference(r)) => match doc.get_dictionary(*r) {
            Ok(x) => x,
            Err(_) => return vec![],
        },
        Some(Object::Dictionary(x)) => x,
        _ => return vec![],
    };
    n.iter().map(|(k, _)| decode_pdf_string(k)).collect()
}

/// 选择域的选项集（`/Opt` 可为字符串数组，或 [导出值, 显示名] 二元组）。
fn choice_options(doc: &Document, id: ObjectId) -> Vec<String> {
    let Ok(d) = doc.get_dictionary(id) else {
        return vec![];
    };
    let Ok(opt) = d.get(b"Opt").and_then(Object::as_array) else {
        return vec![];
    };
    opt.iter()
        .filter_map(|o| match o {
            Object::String(s, _) => Some(decode_pdf_string(s)),
            Object::Array(pair) => pair
                .first()
                .and_then(|f| f.as_str().ok())
                .map(decode_pdf_string),
            _ => None,
        })
        .collect()
}

/// 读字典里的「文本值」——**兼容字符串与名字两种形态**。
///
/// PDF 字典同一个键可能是 `(文本)` / `<hex>`（字符串）或 `/名字`（名字），
/// 用 `as_str()` 读名字只会静默拿到 `None`。本函数两种都认（名字去前导 `/`），
/// 是 crate 文档「字典三形态」纪律的落点——有它之后这类坑不可能再犯。
///
/// 用法：
/// ```ignore
/// let sub = dict_text(&sig_dict, b"SubFilter");   // "/ETSI.CAdES.detached" -> Some("ETSI.CAdES.detached")
/// let why = dict_text(&sig_dict, b"Reason");      // "(投标文件签署)"      -> Some("投标文件签署")
/// ```
pub fn dict_text(d: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
    match d.get(key).ok()? {
        Object::String(s, _) => Some(decode_pdf_string(s)),
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

/// PDF 文本串解码：UTF-16BE（带 BOM）/ UTF-8 / PDFDocEncoding（近似 Latin-1）。
pub(crate) fn decode_pdf_string(raw: &[u8]) -> String {
    if raw.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = raw[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&raw[3..]).into_owned()
    } else {
        raw.iter().map(|&b| b as char).collect()
    }
}
// ---------------------------------------------------------------------------
// 填
// ---------------------------------------------------------------------------

impl PdfForm {
    /// 注入字体字节（TTF）——中文外观流/CIDFont 嵌入的唯一来源。
    ///
    /// 不注入时：数字/英文照常生成 Helvetica 外观；中文走
    /// `NeedAppearances` 交阅读器渲染，并产生告警（见模块头）。
    pub fn set_font(&mut self, ttf: &[u8]) {
        self.font = Some(ttf.to_vec());
    }

    /// 填文本域。
    ///
    /// 值超 `/MaxLen` → [`FormError::InvalidValue`]（**不静默截断**：标书里
    /// 截断的金额/工期是废标级错误，宁可报错让人看见）。
    pub fn set_field(&mut self, name: &str, value: &str) -> Result<(), FormError> {
        let raw = self
            .fields
            .get(name)
            .cloned()
            .ok_or_else(|| FormError::FieldNotFound(name.to_string()))?;
        match &raw.model {
            FormField::Text { max_len, .. } => {
                if let Some(max) = max_len {
                    let bytes: usize = value
                        .chars()
                        .map(|c| if (c as u32) < 0x80 { 1 } else { 2 })
                        .sum();
                    if bytes > *max {
                        return Err(FormError::InvalidValue {
                            name: name.to_string(),
                            reason: format!("超出 /MaxLen 限制 {max} 字节（实填 {bytes} 字节）"),
                        });
                    }
                }
                self.check_writable(&raw)?;
                if raw.model.value_display() == value {
                    self.report.unchanged += 1;
                    return Ok(());
                }
                let encoded = encode_pdf_text(value);
                // 跨页同名域：值必须写进每个 Widget。
                self.write_field_dict(raw.field_id, b"V", encoded.clone());
                for wid in &raw.widget_ids {
                    self.write_field_dict(*wid, b"V", encoded.clone());
                }
                let max_len = *max_len;
                if let Some(m) = self.fields.get_mut(name) {
                    m.model = FormField::Text {
                        name: name.to_string(),
                        value: value.to_string(),
                        max_len,
                    };
                }
                self.note_cjk(name, value);
                self.queue_appearance(name, raw.field_id);
                for wid in &raw.widget_ids {
                    self.queue_appearance(name, *wid);
                }
                self.report.changed += 1;
                Ok(())
            }
            other => Err(FormError::WrongFieldType {
                name: name.to_string(),
                expected: "text",
                actual: other.kind().to_string(),
            }),
        }
    }

    /// 勾选/取消复选框。
    pub fn set_checkbox(&mut self, name: &str, checked: bool) -> Result<(), FormError> {
        let raw = self
            .fields
            .get(name)
            .cloned()
            .ok_or_else(|| FormError::FieldNotFound(name.to_string()))?;
        let FormField::Checkbox {
            checked: was,
            on_name,
            ..
        } = &raw.model
        else {
            return Err(FormError::WrongFieldType {
                name: name.to_string(),
                expected: "checkbox",
                actual: raw.model.kind().to_string(),
            });
        };
        if *was == checked {
            self.report.unchanged += 1;
            return Ok(());
        }
        self.check_writable(&raw)?;
        let on_name = on_name.clone();
        let val = if checked {
            Object::Name(on_name.as_bytes().to_vec())
        } else {
            Object::Name(b"Off".to_vec())
        };
        // 按钮的 /V 与 /AS（外观态）必须同步——只改 /V 阅读器仍显示原态
        self.write_field_dict(raw.field_id, b"V", val.clone());
        let widgets = raw.widget_ids.clone();
        for w in widgets {
            self.write_field_dict(w, b"AS", val.clone());
        }
        if let Some(m) = self.fields.get_mut(name) {
            m.model = FormField::Checkbox {
                name: name.to_string(),
                checked,
                on_name,
            };
        }
        self.report.changed += 1;
        Ok(())
    }

    /// 选单选组的一项。
    pub fn set_radio(&mut self, name: &str, option: &str) -> Result<(), FormError> {
        let raw = self
            .fields
            .get(name)
            .cloned()
            .ok_or_else(|| FormError::FieldNotFound(name.to_string()))?;
        let FormField::Radio { options, .. } = &raw.model else {
            return Err(FormError::WrongFieldType {
                name: name.to_string(),
                expected: "radio",
                actual: raw.model.kind().to_string(),
            });
        };
        if !options.iter().any(|o| o == option) {
            return Err(FormError::InvalidValue {
                name: name.to_string(),
                reason: format!("选项 {option} 不在 {options:?} 中"),
            });
        }
        self.check_writable(&raw)?;
        let val = Object::Name(option.as_bytes().to_vec());
        self.write_field_dict(raw.field_id, b"V", val.clone());
        let widgets = raw.widget_ids.clone();
        for w in widgets {
            let hit = button_on_names(&self.doc, w).iter().any(|n| n == option);
            let as_val = if hit {
                val.clone()
            } else {
                Object::Name(b"Off".to_vec())
            };
            self.write_field_dict(w, b"AS", as_val);
        }
        let options = options.clone();
        if let Some(m) = self.fields.get_mut(name) {
            m.model = FormField::Radio {
                name: name.to_string(),
                selected: Some(option.to_string()),
                options,
            };
        }
        self.report.changed += 1;
        Ok(())
    }

    /// 选下拉/列表项（多选列表可传多个）。
    pub fn set_choice(&mut self, name: &str, values: &[&str]) -> Result<(), FormError> {
        let raw = self
            .fields
            .get(name)
            .cloned()
            .ok_or_else(|| FormError::FieldNotFound(name.to_string()))?;
        let FormField::Choice { options, combo, .. } = &raw.model else {
            return Err(FormError::WrongFieldType {
                name: name.to_string(),
                expected: "choice",
                actual: raw.model.kind().to_string(),
            });
        };
        for v in values {
            if !options.iter().any(|o| o == v) {
                return Err(FormError::InvalidValue {
                    name: name.to_string(),
                    reason: format!("值 {v} 不在选项集 {options:?} 中"),
                });
            }
        }
        self.check_writable(&raw)?;
        let obj = if values.len() == 1 {
            encode_pdf_text(values[0])
        } else {
            Object::Array(values.iter().map(|v| encode_pdf_text(v)).collect())
        };
        self.write_field_dict(raw.field_id, b"V", obj);
        let (options, combo) = (options.clone(), *combo);
        if let Some(m) = self.fields.get_mut(name) {
            m.model = FormField::Choice {
                name: name.to_string(),
                selected: values.iter().map(|v| v.to_string()).collect(),
                options,
                combo,
            };
        }
        self.report.changed += 1;
        Ok(())
    }

    /// 只读检查（/Ff bit0）。
    fn check_writable(&mut self, raw: &RawField) -> Result<(), FormError> {
        if let Some(ff) = inherited_i64(&self.doc, raw.field_id, b"Ff") {
            if ff & 1 != 0 {
                return Err(FormError::ReadOnly(raw.name.clone()));
            }
        }
        Ok(())
    }

    /// 把某个键写进待写回字典（先取当前字典快照，再覆盖）。
    fn write_field_dict(&mut self, id: ObjectId, key: &[u8], val: Object) {
        let base = self.doc.get_dictionary(id).cloned().unwrap_or_default();
        let entry = self.dirty.entry(id).or_insert(base);
        entry.set(key.to_vec(), val);
    }

    /// 记录 CJK 值（决定告警与外观策略）。
    fn note_cjk(&mut self, name: &str, value: &str) {
        let has_cjk = value.chars().any(|c| (c as u32) > 0x7F);
        if !has_cjk || self.font.is_some() {
            return;
        }
        let msg = format!(
            "域 {name} 填入了非 ASCII 文本，但未注入字体（set_font）——已置 NeedAppearances 交阅读器渲染；导出扁平件前必须注入 CIDFont，否则中文丢失"
        );
        self.report.warn(msg);
    }

    /// 登记文本域的外观生成作业。
    fn queue_appearance(&mut self, name: &str, widget_id: ObjectId) {
        let rect = self
            .doc
            .get_dictionary(widget_id)
            .ok()
            .and_then(|d| {
                d.get(b"Rect")
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .map(|a| a.to_vec())
            })
            .map(|a| {
                let n = |i: usize| match a.get(i) {
                    Some(Object::Real(r)) => *r as f64,
                    Some(Object::Integer(i)) => *i as f64,
                    _ => 0.0,
                };
                [n(0), n(1), n(2), n(3)]
            });
        if let Some(rect) = rect {
            self.appearance_jobs
                .insert(name.to_string(), AppearanceJob { widget_id, rect });
        }
    }
}
// ---------------------------------------------------------------------------
// 写回（增量修订）与扁平化
// ---------------------------------------------------------------------------

impl PdfForm {
    /// 写回：**真增量修订**（spec 7.5.6）——原字节作前缀逐位保持，只把
    /// 改动的那批对象与新增对象（外观流/字体）追加为一个修订段。
    ///
    /// # 语义（与 [`crate::writer_lopdf`] 同一纪律，三层守卫）
    ///
    /// 1. **xref 链守卫**：档以扫描重建装载（`xref_start == 0`，常见于
    ///    行尾污染/真损坏）时拒绝写——追加修订无从回指 `/Prev`，产物
    ///    页树不可达（2026-10-31 事故防线，详见 `writer_lopdf.rs`）；
    /// 2. **前缀逐位不变断言**：产物必须以保存前字节为逐位相同前缀，
    ///    违约 = typed 错误而非产出损坏文件——已签名文档的 `/ByteRange`
    ///    覆盖段因此不变，既有数字签名保持有效（PAdES 硬约束）；
    /// 3. **产物自检**：重载后页树可达 + 全部脏对象经新修订可解析，
    ///    否则拒绝交付。
    ///
    /// 没改任何域时返回原字节（幂等，不产生空修订）。
    pub fn save(mut self) -> Result<Vec<u8>, FormError> {
        if self.dirty.is_empty() {
            return Ok(self.original);
        }
        self.build_appearances()?;
        // 守卫 1：xref 链可信（扫描重建档上做增量写 = 静默产毒，入口响亮拒绝）。
        if self.doc.xref_start == 0 {
            return Err(FormError::Save(
                "xref 链不可信：档以扫描重建装载（无已知 xref 起点），增量写产物将丢失 \
                 /Prev 链导致页树不可达。常见成因：二进制被行尾转换污染（如 Windows \
                 git autocrlf）或 xref 损坏。请重取未污染原件或修复源文件后重试"
                    .into(),
            ));
        }
        let original = std::mem::take(&mut self.original);
        let dirty = std::mem::take(&mut self.dirty);
        let doc = std::mem::replace(&mut self.doc, Document::with_version("1.5"));
        // 会话新增对象（外观流/CIDFont/新建 AcroForm）——原字节里没有，
        // 必须随本轮修订写出；既有对象一字不重写（它们已逐位活在原字节
        // 前缀里，重写既膨胀又是元数据噪声）。
        let new_objects: Vec<(ObjectId, Object)> = doc
            .objects
            .iter()
            .filter(|(id, _)| !self.base_ids.contains(*id))
            .map(|(id, obj)| (*id, obj.clone()))
            .collect();
        // 增量文档：prev = 原字节 + 解析态（trailer/xref_start/max_id 均为
        // 原档形态——save 路径不改它们）；new_document = 待追加修订。
        let mut inc = IncrementalDocument::create_from(original.clone(), doc);
        // 脏对象：同对象号在新修订里重定义（后修订覆盖前修订——增量更新
        // 语义），原修订里的旧定义对新阅读器自然失效。
        for (id, dict) in dirty {
            inc.new_document.set_object(id, Object::Dictionary(dict));
        }
        for (id, obj) in new_objects {
            inc.new_document.set_object(id, obj);
        }
        let mut out = Vec::new();
        inc.save_to(&mut out)
            .map_err(|e| FormError::Save(format!("lopdf 增量保存失败：{e}")))?;
        // 守卫 2：增量语义核心断言（违约不产出、不污染基线）。
        if !out.starts_with(&original) {
            return Err(FormError::Save(
                "增量前缀违约：lopdf 产物未逐位保持原字节前缀".into(),
            ));
        }
        // 守卫 3：产物自检——重载后页树可达 + 脏对象经新修订可解析（拒绝
        // 交付「写入成功但阅读器看不到」的静默产毒形态）。
        let reloaded = Document::load_mem(&out)
            .map_err(|e| FormError::Save(format!("增量产物重载失败：{e}")))?;
        if reloaded.get_pages().is_empty() {
            return Err(FormError::Save(
                "增量产物自检失败：重载后页树为空（产物 xref 链断裂，页对象经链不可达）\
                 ——拒绝交付结构损坏产物"
                    .into(),
            ));
        }
        Ok(out)
    }

    /// 外观流：有字体走 CIDFont；无字体则置 NeedAppearances 交阅读器。
    fn build_appearances(&mut self) -> Result<(), FormError> {
        if self.appearance_jobs.is_empty() {
            return Ok(());
        }
        if self.font.is_some() {
            let jobs = std::mem::take(&mut self.appearance_jobs);
            let built = self.appearance_text(jobs)?;
            self.report.appearances_built = built;
            if built > 0 {
                self.set_need_appearances(false);
            }
        } else {
            self.set_need_appearances(true);
            self.appearance_jobs.clear();
        }
        Ok(())
    }

    /// 文本域外观流（ASCII → Helvetica；含中文 → 注入的 TTF 走 Type0）。
    fn appearance_text(
        &mut self,
        jobs: BTreeMap<String, AppearanceJob>,
    ) -> Result<usize, FormError> {
        let font = self.font.clone();
        let mut built = 0usize;
        for (name, job) in jobs {
            let Some(model) = self.fields.get(&name).map(|r| r.model.clone()) else {
                continue;
            };
            let value = match &model {
                FormField::Text { value, .. } => value.clone(),
                FormField::Choice { selected, .. } => selected.join(","),
                _ => continue,
            };
            let styled = self.appearance_overrides.get(&name).cloned();
            let w = (job.rect[2] - job.rect[0]).abs().max(1.0);
            let h = (job.rect[3] - job.rect[1]).abs().max(1.0);
            let fs = match &styled {
                Some(a) => (a.font_size as f64).clamp(1.0, h.max(1.0)),
                None => (h * 0.62).clamp(4.0, 72.0),
            };
            let mut ap = match (&font, value.is_ascii()) {
                (Some(ttf), false) => cid_appearance(ttf, &value, w, h, fs)?,
                _ => {
                    let (align, color, bg) = match &styled {
                        Some(a) => (a.alignment, a.text_color, a.background),
                        None => (Alignment::Left, (0, 0, 0), None),
                    };
                    simple_appearance_styled(&value, w, h, fs, align, color, bg)
                }
            };
            ap.dict.set("BBox", bbox(w, h));
            let ap_id = self.doc.add_object(ap);
            let mut n = lopdf::Dictionary::new();
            n.set(b"Off".to_vec(), Object::Reference(ap_id));
            let mut apd = lopdf::Dictionary::new();
            apd.set(b"N".to_vec(), Object::Dictionary(n));
            self.write_field_dict(job.widget_id, b"AP", Object::Dictionary(apd));
            built += 1;
        }
        Ok(built)
    }

    /// `/AcroForm /NeedAppearances` 开关。
    fn set_need_appearances(&mut self, on: bool) {
        let Some(acro_id) = self.acroform_id() else {
            return;
        };
        self.write_field_dict(acro_id, b"NeedAppearances", Object::Boolean(on));
    }

    /// `/AcroForm` 的对象号（缺失时新建并挂到 Catalog）。
    fn acroform_id(&mut self) -> Option<ObjectId> {
        let root = self
            .doc
            .trailer
            .get(b"Root")
            .ok()
            .and_then(|o| o.as_reference().ok())?;
        let catalog = self.doc.get_dictionary(root).ok()?;
        match catalog.get(b"AcroForm").ok() {
            Some(Object::Reference(id)) => Some(*id),
            Some(Object::Dictionary(d)) => {
                let d = d.clone();
                let id = self.doc.add_object(d);
                self.write_field_dict(root, b"AcroForm", Object::Reference(id));
                Some(id)
            }
            _ => None,
        }
    }
}

/// 外观流的 BBox（原点 0,0 + 域宽高）。
fn bbox(w: f64, h: f64) -> Object {
    Object::Array(vec![
        Object::Real(0.0),
        Object::Real(0.0),
        Object::Real(w as f32),
        Object::Real(h as f32),
    ])
}

/// 文本 → PDF 字符串：ASCII 用字面串，其余 UTF-16BE + BOM（spec 7.9.2.2）。
pub(crate) fn encode_pdf_text(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(s.as_bytes().to_vec(), lopdf::StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(bytes, lopdf::StringFormat::Literal)
    }
}

/// 带样式的外观流（字号/对齐/颜色/背景）。
///
/// 对齐按**字符数近似**定位（ASCII 记 0.5 em、其余 1.0 em）——精确排版
/// 需要字体度量表（hmtx），[`crate::generate`] 已登记该限制（R2 面）；
/// 表单一格文字量小，近似足够。
fn simple_appearance_styled(
    value: &str,
    w: f64,
    h: f64,
    fs: f64,
    align: Alignment,
    text_color: (u8, u8, u8),
    background: Option<(u8, u8, u8)>,
) -> lopdf::Stream {
    let esc = value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let em_width: f64 = value
        .chars()
        .map(|c| if c.is_ascii() { 0.5 } else { 1.0 })
        .sum();
    let text_w = em_width * fs;
    let x = match align {
        Alignment::Left => 2.0,
        Alignment::Center => ((w - text_w) / 2.0).max(2.0),
        Alignment::Right => (w - text_w - 2.0).max(2.0),
    };
    let y = (h - fs) / 2.0;
    let (r, g, b) = text_color;
    let mut ops = String::new();
    if let Some((br, bg, bb)) = background {
        ops.push_str(&format!(
            "q {:.3} {:.3} {:.3} rg 0 0 {w:.2} {h:.2} re f Q\n",
            br as f32 / 255.0,
            bg as f32 / 255.0,
            bb as f32 / 255.0
        ));
    }
    ops.push_str(&format!(
        "q BT /Helv {fs} Tf {:.3} {:.3} {:.3} rg {x:.2} {y:.2} Td ({esc}) Tj ET Q",
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0
    ));
    let mut s = lopdf::Stream::new(dictionary! {}, ops.into_bytes());
    s.dict.set("Type", "XObject");
    s.dict.set("Subtype", "Form");
    s.dict.set("BBox", bbox(w, h));
    s.dict.set(
        "Resources",
        Object::Dictionary(dictionary! {
            "Font" => Object::Dictionary(dictionary! {
                "Helv" => Object::Dictionary(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                    "Encoding" => "WinAnsiEncoding",
                }),
            }),
        }),
    );
    s
}
/// CIDFont 外观流（Type0 / Identity-H）——中文域值的正确落点。
///
/// 与 [`crate::generate`] 同源：复用其 TTF 解析与 Type0 字体字典构造，
/// 只是把内容从「整页文本」换成「一个域框里的一行字」。
fn cid_appearance(
    ttf: &[u8],
    value: &str,
    w: f64,
    h: f64,
    fs: f64,
) -> Result<lopdf::Stream, FormError> {
    let mut hex = String::new();
    for ch in value.chars() {
        let gid = crate::generate::font_gid(ttf, ch).ok_or_else(|| {
            FormError::Save(format!(
                "字体缺少字形 {ch:?}（U+{:04X}）——换完整字体或改用可渲染文本",
                ch as u32
            ))
        })?;
        hex.push_str(&format!("{gid:04X}"));
    }
    let content = format!("q BT /C0 {fs} Tf 2 {} Td <{hex}> Tj ET Q", (h - fs) / 2.0);
    let mut s = lopdf::Stream::new(dictionary! {}, content.into_bytes());
    s.dict.set("Type", "XObject");
    s.dict.set("Subtype", "Form");
    s.dict.set("BBox", bbox(w, h));
    let font_obj = crate::generate::font_object(ttf)
        .ok_or_else(|| FormError::Save("字体对象构造失败（TTF 缺必需表）".into()))?;
    s.dict.set(
        "Resources",
        Object::Dictionary(dictionary! {
            "Font" => Object::Dictionary(dictionary! { "C0" => font_obj }),
        }),
    );
    Ok(s)
}
// ---------------------------------------------------------------------------
// 扁平化
// ---------------------------------------------------------------------------

impl PdfForm {
    /// 扁平化：把域值画进页面内容流 + 删除 `/AcroForm` 与各页 `/Widget`。
    ///
    /// 产物与打印扫描件等价——任何人再也改不了（投标常见要求）。这是
    /// **销毁性**操作，与 [`PdfForm::save`] 的增量路径（前缀逐位不变、
    /// 签名保持）语义相反，故本方法保持全量重写。
    ///
    /// **拒绝条件**（[`FormError::FlattenRefused`]，宁可不错也不做错）：
    /// - 文档含**已签署**的数字签名域（扁平化会删掉签名域，既有签名被
    ///   销毁——即使是增量实现，当前修订视图里的签名也不复存在。要保
    ///   签名的定稿请走 [`PdfForm::save`]；要不可编辑定稿请对**副本**
    ///   扁平化并接受签名失效，或扁平后重新签署）；
    /// - 有非 ASCII 值却没有字体（画出来是乱码，比不扁平化更危险）；
    /// - 字体缺字形。
    pub fn flatten(mut self) -> Result<Vec<u8>, FormError> {
        if self.has_signed_signature_fields() {
            return Err(FormError::FlattenRefused(
                "文档含已签署的数字签名域——扁平化会删除签名域，既有签名将被销毁，已拒绝。\
                 要保留签名的可交付件：用 save()（增量修订，原字节前缀逐位不变，签名仍有效）；\
                 要不可编辑的定稿：请先另存副本，对副本 flatten 并接受签名失效，或扁平后重新签署"
                    .into(),
            ));
        }
        let mut needs_font = false;
        for f in self.fields.values() {
            if let FormField::Text { value, .. } = &f.model {
                if !value.is_ascii() && !value.is_empty() {
                    needs_font = true;
                }
            }
        }
        if needs_font && self.font.is_none() {
            return Err(FormError::FlattenRefused(
                "存在中文等非 ASCII 域值，但未注入字体（set_font）——扁平化会画出乱码，已拒绝"
                    .into(),
            ));
        }

        let font_bytes = self.font.clone();

        let page_of: BTreeMap<ObjectId, u32> = self
            .doc
            .get_pages()
            .into_iter()
            .map(|(n, id)| (id, n))
            .collect();
        let mut by_page: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        let mut widget_ids: BTreeSet<ObjectId> = BTreeSet::new();

        // 全部 Widget 先登记（删除面用——不能只收集「有文本值」的域，
        // 否则复选框/空域/下拉的 Widget 会留在页 /Annots 里，扁平化后
        // 又被读面当野控件扫回来）
        for f in self.fields.values() {
            for wid in &f.widget_ids {
                widget_ids.insert(*wid);
            }
        }
        for f in self.fields.values() {
            let text = match &f.model {
                FormField::Text { value, .. } if !value.is_empty() => value.clone(),
                _ => continue,
            };
            for wid in &f.widget_ids {
                let Ok(wd) = self.doc.get_dictionary(*wid) else {
                    continue;
                };
                let Some(rect) = wd
                    .get(b"Rect")
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .map(|a| a.to_vec())
                else {
                    continue;
                };
                let n = |i: usize| match rect.get(i) {
                    Some(Object::Real(r)) => *r as f64,
                    Some(Object::Integer(i)) => *i as f64,
                    _ => 0.0,
                };
                let (x0, y0, _x1, y1) = (n(0), n(1), n(2), n(3));
                let h = (y1 - y0).abs().max(1.0);
                let fs = (h * 0.62).clamp(4.0, 72.0);
                let ty = y0 + (h - fs) * 0.3;
                let page_no = wd
                    .get(b"P")
                    .ok()
                    .and_then(|o| o.as_reference().ok())
                    .and_then(|p| page_of.get(&p).copied())
                    .or_else(|| find_page_with_annot(&self.doc, *wid));
                let Some(page_no) = page_no else { continue };
                let op = if text.is_ascii() {
                    let esc = text
                        .replace('\\', "\\\\")
                        .replace('(', "\\(")
                        .replace(')', "\\)");
                    format!("BT /Ffl {fs} Tf {x0} {ty} Td ({esc}) Tj ET")
                } else {
                    let ttf = font_bytes.as_deref().unwrap_or(&[]);
                    let mut hex = String::new();
                    for ch in text.chars() {
                        let gid = crate::generate::font_gid(ttf, ch).ok_or_else(|| {
                            FormError::FlattenRefused(format!(
                                "字体缺少字形 {ch:?}（U+{:04X}）——扁平化会丢字",
                                ch as u32
                            ))
                        })?;
                        hex.push_str(&format!("{gid:04X}"));
                    }
                    format!("BT /Ffl {fs} Tf {x0} {ty} Td <{hex}> Tj ET")
                };
                by_page.entry(page_no).or_default().push(op);
            }
        }

        for (page_no, ops) in &by_page {
            let Some((_, page_id)) = self.doc.get_pages().into_iter().find(|(n, _)| n == page_no)
            else {
                continue;
            };
            let stream_id = self.doc.add_object(lopdf::Stream::new(
                dictionary! {},
                ops.join("\n").into_bytes(),
            ));
            if let Ok(page) = self.doc.get_dictionary_mut(page_id) {
                let old = page.get(b"Contents").ok().cloned();
                match old {
                    Some(Object::Array(mut arr)) => {
                        arr.push(Object::Reference(stream_id));
                        page.set(b"Contents".to_vec(), Object::Array(arr));
                    }
                    Some(obj) => {
                        page.set(
                            b"Contents".to_vec(),
                            Object::Array(vec![obj, Object::Reference(stream_id)]),
                        );
                    }
                    None => {
                        page.set(b"Contents".to_vec(), Object::Reference(stream_id));
                    }
                }
            }
        }

        if let Some(ttf) = &font_bytes {
            if let Some(fo) = crate::generate::font_object(ttf) {
                for (_, page_id) in self.doc.get_pages() {
                    attach_font(&mut self.doc, page_id, Object::Dictionary(fo.clone()));
                }
            }
        } else if !by_page.is_empty() {
            let helv = Object::Dictionary(dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            });
            for (_, page_id) in self.doc.get_pages() {
                attach_font(&mut self.doc, page_id, helv.clone());
            }
        }

        let root = self
            .doc
            .trailer
            .get(b"Root")
            .ok()
            .and_then(|o| o.as_reference().ok());
        if let Some(root) = root {
            if let Ok(catalog) = self.doc.get_dictionary_mut(root) {
                catalog.remove(b"AcroForm");
            }
        }
        for (_, page_id) in self.doc.get_pages() {
            if let Ok(page) = self.doc.get_dictionary_mut(page_id) {
                let keep: Option<Vec<Object>> = page
                    .get(b"Annots")
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .map(|arr| {
                        arr.iter()
                            .filter(|a| {
                                !a.as_reference()
                                    .map(|r| widget_ids.contains(&r))
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect()
                    });
                match keep {
                    Some(k) if k.is_empty() => {
                        page.remove(b"Annots");
                    }
                    Some(k) => {
                        page.set(b"Annots".to_vec(), Object::Array(k));
                    }
                    None => {}
                }
            }
        }

        let mut doc = std::mem::replace(&mut self.doc, Document::with_version("1.5"));
        let mut out = Vec::new();
        doc.save_to(&mut out)
            .map_err(|e| FormError::Save(format!("扁平化保存失败：{e}")))?;
        Ok(out)
    }
} // ---------------------------------------------------------------------------
  // 任务 1：校验
  // ---------------------------------------------------------------------------

impl PdfForm {
    /// 整表校验（必填 / 超长 / 选项集）。
    ///
    /// # 校验范围（**诚实边界**）
    ///
    /// | 规则 | 来源 | 本版 |
    /// |---|---|---|
    /// | 必填 | `/Ff` bit1 | ✅ |
    /// | 超长 | `/MaxLen` | ✅ |
    /// | 选项集 | `/Ch /Opt` | ✅ |
    /// | 格式 | `/AA /F` 的 JavaScript | ⚠️ **不做** |
    ///
    /// 格式校验（`/AA /F`）是一段 **JavaScript**（`AFNumber_Format`、
    /// `AFDate_FormatEx` 等）。执行任意 JS 需要嵌入式解释器——本 crate
    /// 明确不接 JS（见 [`crate::engine`] 的 S1.1 纪律）。
    ///
    /// 所以 [`ErrorKind::PatternMismatch`] **在本版不会被本函数产生**；
    /// 它保留在枚举里是为了接口稳定性。**与其猜一个正则，不如如实说
    /// 「这一项我们没验」。**
    ///
    /// 只读域（`/Ff` bit0）不参与校验——它们本就不可填，报「必填未填」
    /// 只会误导（真实投标函里只读域通常是自动计算字段）。
    pub fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        for (name, raw) in &self.fields {
            if raw.flags.is_read_only() {
                continue;
            }
            match &raw.model {
                FormField::Text { value, max_len, .. } => {
                    if raw.flags.is_required() && value.trim().is_empty() {
                        errors.push(FieldError {
                            field: name.clone(),
                            kind: ErrorKind::Required,
                            message: "必填项未填写".into(),
                        });
                    }
                    if let Some(max) = max_len {
                        let used = pdf_text_len(value);
                        if used > *max {
                            errors.push(FieldError {
                                field: name.clone(),
                                kind: ErrorKind::TooLong,
                                message: format!("超出长度限制 {max}，当前 {used}"),
                            });
                        }
                    }
                }
                FormField::Choice {
                    selected, options, ..
                } => {
                    if raw.flags.is_required() && selected.is_empty() {
                        errors.push(FieldError {
                            field: name.clone(),
                            kind: ErrorKind::Required,
                            message: "必选项未选择".into(),
                        });
                    }
                    if !options.is_empty() {
                        for v in selected {
                            if !options.iter().any(|o| o == v) {
                                errors.push(FieldError {
                                    field: name.clone(),
                                    kind: ErrorKind::InvalidChoice,
                                    message: format!("选择项「{v}」不在可选范围内"),
                                });
                            }
                        }
                    }
                }
                // 复选框的「必填」在 PDF 里没有标准位（/Ff bit1 对 Btn 是
                // Reserved）。不猜语义，跳过。
                FormField::Checkbox { .. } => {}
                FormField::Radio { .. } => {}
                // 签名字段由 signature 模块管。
                FormField::Signature { .. } => {}
            }
        }
        errors.sort_by(|a, b| a.field.cmp(&b.field).then(a.kind.code().cmp(b.kind.code())));
        ValidationResult {
            valid: errors.is_empty(),
            errors,
        }
    }

    /// 是否存在**已签署**的数字签名域（`/FT /Sig` 且 `/V` 已挂签名字典
    /// ——直挂字典或间接引用两形态；未签署的占位签名域不算，删它不毁
    /// 任何签名）。
    ///
    /// 两处用途：[`PdfForm::flatten`] 的拒绝判据（扁平化删掉签名域 =
    /// 销毁既有签名）；调用方 UI 探针（对已签名档提示「保存走增量追加，
    /// 签名保持有效」）。
    pub fn has_signed_signature_fields(&self) -> bool {
        self.fields.values().any(|raw| {
            if !matches!(raw.model, FormField::Signature { .. }) {
                return false;
            }
            // /V 可能在域字典（分离态）或 Widget 字典（合并态）上——两处都看。
            std::iter::once(&raw.field_id)
                .chain(raw.widget_ids.iter())
                .any(|id| self.sig_value_present(*id))
        })
    }

    /// 单个对象上 `/V` 是否挂了签名字典（直挂 / 间接引用两形态）。
    fn sig_value_present(&self, id: ObjectId) -> bool {
        let Ok(d) = self.doc.get_dictionary(id) else {
            return false;
        };
        match d.get(b"V") {
            Ok(Object::Dictionary(_)) => true,
            Ok(Object::Reference(vid)) => {
                matches!(self.doc.get_object(*vid), Ok(Object::Dictionary(_)))
            }
            _ => false,
        }
    }

    /// 某域的标志位（校验/接线方可读）。
    pub fn flags(&self, name: &str) -> Option<FieldFlags> {
        self.fields.get(name).map(|r| r.flags)
    }

    /// **仅供测试/诊断**：绕过校验直接把原始值写进域字典。
    ///
    /// 用途：构造「文档里已经有非法值」的状态，验证 [`PdfForm::validate`]
    /// 的第二道防线（别的工具写进了超 `/MaxLen` 的值）。生产不要用。
    #[doc(hidden)]
    pub fn force_raw_value(&mut self, name: &str, value: &str) {
        let Some(raw) = self.fields.get(name).cloned() else {
            return;
        };
        let encoded = encode_pdf_text(value);
        self.write_field_dict(raw.field_id, b"V", encoded.clone());
        for wid in &raw.widget_ids {
            self.write_field_dict(*wid, b"V", encoded.clone());
        }
        if let Some(m) = self.fields.get_mut(name) {
            let max_len = match &m.model {
                FormField::Text { max_len, .. } => *max_len,
                _ => None,
            };
            m.model = FormField::Text {
                name: name.to_string(),
                value: value.to_string(),
                max_len,
            };
        }
    }
}

/// PDF 文本长度口径：`/MaxLen` 以**字节**计（UTF-16BE 时中文占 2 字节，
/// 非 BMP 字符占 4 字节——与 Acrobat 判定一致）。
fn pdf_text_len(s: &str) -> usize {
    s.chars()
        .map(|c| {
            if (c as u32) < 0x80 {
                1
            } else if (c as u32) <= 0xFFFF {
                2
            } else {
                4
            }
        })
        .sum()
}

// ---------------------------------------------------------------------------
// 任务 2：外观定制
// ---------------------------------------------------------------------------

impl PdfForm {
    /// 设置某域外观（字号/字体/对齐/颜色/背景）。
    ///
    /// 写三处：域 `/DA`+`/Q`、`/MK /BG` 背景、以及注入字体时登记外观重建。
    /// 非文本/选择域返回 [`FormError::WrongFieldType`]——复选框外观是图形
    /// 不是文本串，改字号没有意义。
    pub fn set_appearance(&mut self, field: &str, app: FieldAppearance) -> Result<(), FormError> {
        let raw = self
            .fields
            .get(field)
            .cloned()
            .ok_or_else(|| FormError::FieldNotFound(field.to_string()))?;
        if !matches!(raw.model, FormField::Text { .. } | FormField::Choice { .. }) {
            return Err(FormError::WrongFieldType {
                name: field.to_string(),
                expected: "text/choice",
                actual: raw.model.kind().to_string(),
            });
        }
        if !(app.font_size.is_finite() && app.font_size > 0.0 && app.font_size <= 300.0) {
            return Err(FormError::InvalidValue {
                name: field.to_string(),
                reason: format!("字号 {} 非法（应在 (0, 300]）", app.font_size),
            });
        }
        self.check_writable(&raw)?;

        let (r, g, b) = app.text_color;
        let da = format!(
            "/{} {} Tf {:.3} {:.3} {:.3} rg",
            app.font_name,
            app.font_size,
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0
        );
        self.write_field_dict(
            raw.field_id,
            b"DA",
            Object::String(da.into_bytes(), lopdf::StringFormat::Literal),
        );
        self.write_field_dict(raw.field_id, b"Q", Object::Integer(app.alignment.to_q()));

        if let Some((br, bg, bb)) = app.background {
            let colors = Object::Array(vec![
                Object::Real(br as f32 / 255.0),
                Object::Real(bg as f32 / 255.0),
                Object::Real(bb as f32 / 255.0),
            ]);
            let base = self
                .doc
                .get_dictionary(raw.field_id)
                .cloned()
                .unwrap_or_default();
            let mut mk = match base.get(b"MK").ok() {
                Some(Object::Dictionary(d)) => d.clone(),
                _ => lopdf::Dictionary::new(),
            };
            mk.set(b"BG".to_vec(), colors);
            self.write_field_dict(raw.field_id, b"MK", Object::Dictionary(mk));
        }

        let widgets = raw.widget_ids.clone();
        for wid in &widgets {
            self.write_field_dict(*wid, b"Q", Object::Integer(app.alignment.to_q()));
        }

        self.appearance_overrides.insert(field.to_string(), app);
        if self.font.is_some() {
            for wid in &widgets {
                self.queue_appearance(field, *wid);
            }
        }
        self.report.changed += 1;
        Ok(())
    }

    /// 读当前外观参数。
    ///
    /// 本会话 `set_appearance` 登记的值优先——它在 `dirty` 里还没落盘，
    /// 直接读 `self.doc` 会拿到旧字典（实测踩坑：读回字号仍是默认 10）。
    pub fn appearance(&self, field: &str) -> Option<FieldAppearance> {
        if let Some(app) = self.appearance_overrides.get(field) {
            return Some(app.clone());
        }
        let raw = self.fields.get(field)?;
        let base = self.doc.get_dictionary(raw.field_id).ok()?;
        let mut app = FieldAppearance::default();
        if let Some(da) = dict_text(base, b"DA") {
            let mut it = da.split_whitespace();
            if let Some(name) = it.next() {
                app.font_name = name.trim_start_matches('/').to_string();
            }
            if let Some(size) = it.next() {
                if let Ok(v) = size.parse::<f32>() {
                    app.font_size = v;
                }
            }
        }
        if let Ok(q) = base.get(b"Q").and_then(|o| o.as_i64()) {
            app.alignment = Alignment::from_q(q);
        }
        Some(app)
    }
}

/// 某 Widget 挂在哪个页（/Annots 反查）。
fn find_page_with_annot(doc: &Document, id: ObjectId) -> Option<u32> {
    doc.get_pages().into_iter().find_map(|(n, pid)| {
        let p = doc.get_dictionary(pid).ok()?;
        let annots = p.get(b"Annots").ok()?.as_array().ok()?;
        let hit = annots
            .iter()
            .any(|a| a.as_reference().map(|r| r == id).unwrap_or(false));
        hit.then_some(n)
    })
}

/// 给页资源字典挂 `/Font /Ffl`。
fn attach_font(doc: &mut Document, page_id: ObjectId, font: Object) {
    let Ok(page) = doc.get_dictionary_mut(page_id) else {
        return;
    };
    let mut res = match page.get(b"Resources").ok() {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => lopdf::Dictionary::new(),
    };
    let mut fonts = match res.get(b"Font").ok() {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => lopdf::Dictionary::new(),
    };
    if fonts.get(b"Ffl").is_err() {
        fonts.set(b"Ffl".to_vec(), font);
    }
    res.set(b"Font".to_vec(), Object::Dictionary(fonts));
    page.set(b"Resources".to_vec(), Object::Dictionary(res));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_text_reads_both_string_and_name() {
        // 这就是「字典三形态」的回归锁：两种形态都必须读到值
        let mut d = lopdf::Dictionary::new();
        d.set("NameForm", Object::Name(b"ETSI.CAdES.detached".to_vec()));
        d.set(
            "StringForm",
            Object::String(b"plain text".to_vec(), lopdf::StringFormat::Literal),
        );
        assert_eq!(
            dict_text(&d, b"NameForm").as_deref(),
            Some("ETSI.CAdES.detached"),
            "名字形态必须可读（as_str 在这里会拿到 None）"
        );
        assert_eq!(dict_text(&d, b"StringForm").as_deref(), Some("plain text"));
        assert_eq!(dict_text(&d, b"Missing"), None);
    }

    #[test]
    fn pdf_string_roundtrip_utf16() {
        let obj = encode_pdf_text("投标函");
        let Object::String(bytes, _) = obj else {
            panic!("应为字符串")
        };
        assert_eq!(&bytes[..2], &[0xFE, 0xFF], "非 ASCII 必须带 UTF-16BE BOM");
        assert_eq!(decode_pdf_string(&bytes), "投标函");
    }

    #[test]
    fn ascii_stays_literal() {
        let obj = encode_pdf_text("ZFCG-2026-001");
        let Object::String(bytes, _) = obj else {
            panic!()
        };
        assert_eq!(String::from_utf8(bytes).unwrap(), "ZFCG-2026-001");
    }
}
