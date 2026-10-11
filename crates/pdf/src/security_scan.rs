//! PDF-e — S2/S6 打开时安全扫描（feature `engine-lopdf`）。
//!
//! SECURITY_POLICY S2.4 承诺的 `open_scan()` 产品面：lopdf 对象图只读
//! 扫描 → [`OpenAssessment`]（S2 八族告警 + S6 表单探针 + S5 加密
//! 旗标）——**只检测告警，不阻断打开**（S2.4：结构化报告，用户自行
//! 决定去留；v0 无杀进程面）。产品面出口：
//! - [`open_scan`]：独立打开路径（L7 输入字节 + L0/L4 打开预算 + 扫描
//!   ——产出的 assessment 喂 [`crate::document::PdfSession::open`]）；
//! - [`crate::writer_lopdf::LopdfWriter::from_bytes`]：装载后扫描随
//!   写器携带（[`LopdfWriter::assessment`] 取出）——同款限额两检查。
//!
//! 检测语义（设计决策在档）：
//! - **对象图全扫**（非可达性走查）：`Document::objects` 逐对象 + 内联
//!   嵌套递归（引用不追——间接对象全部在顶层枚举）——**不可达的敌意
//!   对象同样可见**（检测面不依赖解析器可达性判定；对象序 =
//!   BTreeMap 键序确定性）；
//! - 八族一 dict **至多一 hit**（同族多触发条件合并）；报告序 = 族
//!   声明序 × 详情字典序 + 精确去重（确定性报告——同文档任何环境
//!   同一报告）；
//! - 路径走查仅 /AcroForm /Fields（S6 探针需要字段树语义——引用链
//!   seen-set 环防 + 深度护栏 [`Limits::object_depth_max`] 复用）；
//! - 加密档检测边界（诚实性，lopdf 装载语义）：**仍加密**（口令不可
//!   得）态 lopdf 不解析对象图（仅 /Encrypt 字典可见）——检测面不可
//!   用，`encrypted` 旗标仍置位（会话层 S5 据此拒写）；**曾加密**
//!   （空口令自动解密）态对象图明文可见——检测照常。

use std::collections::HashSet;
use std::time::Instant;

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::command::CommandError;
use crate::security::{
    DangerousFamily, DangerousHit, FormField, FormProbe, Limits, OpenAssessment,
};

/// S2.4 open_scan：打开路径完整扫描（L7 输入字节 ❶ → 装载 → S2 八族
/// 与 S6 表单探针与 S5 加密旗标 → L0/L4 打开预算 ❶）。成功 = 可直接
/// 喂 [`crate::document::PdfSession::open`] 的 assessment 参数（打开
/// 不被告警阻断——S2.4 只读语义）；超限 = typed 拒绝。
pub fn open_scan(pdf: &[u8], limits: &Limits) -> Result<OpenAssessment, CommandError> {
    // S3-L7 输入层（open 家族入口统一检查——与 from_bytes /
    // PdfiumEngine::open 同款，先于解析）。
    limits.check_open_input_bytes(pdf.len() as u64)?;
    let started = Instant::now();
    let doc = Document::load_mem(pdf)
        .map_err(|e| CommandError::engine(format!("lopdf 装载失败：{e}")))?;
    let assessment = scan_document(&doc, limits);
    // S3-L4 打开预算（装载 + 扫描整段计时）。
    limits.check_open_budget(started)?;
    Ok(assessment)
}

/// 纯对象图扫描（已装载文档——`LopdfWriter::from_bytes` 装载后复用；
/// 预算由调用方包裹计时，本函数无时限）。
pub fn scan_document(doc: &Document, limits: &Limits) -> OpenAssessment {
    let mut assessment = OpenAssessment {
        encrypted: doc.is_encrypted(),
        warnings: Vec::new(),
        form_probe: None,
    };
    // ① 对象图全扫（BTreeMap 键序——确定性枚举）。
    for (id, object) in &doc.objects {
        sweep_object(&mut assessment, *id, object, 0, limits);
    }
    // ② S6 表单探针（catalog /AcroForm /Fields 路径走查）。
    if let Ok(catalog) = doc.catalog() {
        if let Ok(acroform) = catalog.get(b"AcroForm") {
            assessment.form_probe = probe_form(doc, acroform, limits);
        }
    }
    // ③ 确定性报告：族声明序 × 详情字典序 + 精确去重（同对象同族的
    // 嵌套重复命中合并为一条）。
    assessment
        .warnings
        .sort_by(|a, b| (&a.family, &a.detail).cmp(&(&b.family, &b.detail)));
    assessment.warnings.dedup();
    assessment
}

/// 单对象扫描：内联嵌套递归（数组/字典值——引用不追：间接对象都在
/// 顶层枚举）；深度护栏 = `object_depth_max`（病态内联嵌套不递归
/// 爆栈——护栏以下不检测）。面区分在档：文件面（load_mem）的深嵌套
/// 先被 lopdf 解析器嵌套预算截断（reader::MAX_NESTING_DEPTH = 100 <
/// 128，超深对象静默丢弃——探针实证），护栏只对内存构造面
/// （`scan_document` 直扫）可观测；判别锁见 pdfe_security 10 号。
fn sweep_object(
    assessment: &mut OpenAssessment,
    id: ObjectId,
    object: &Object,
    depth: u32,
    limits: &Limits,
) {
    if depth > limits.object_depth_max {
        return;
    }
    match object {
        Object::Dictionary(dict) => sweep_dict(assessment, id, dict, depth, limits),
        Object::Stream(stream) => sweep_dict(assessment, id, &stream.dict, depth, limits),
        Object::Array(items) => {
            for item in items {
                sweep_object(assessment, id, item, depth + 1, limits);
            }
        }
        _ => {}
    }
}

/// 字典族检测（八族，一 dict 每族至多一 hit——同族多触发条件合并）：
/// - 键在位族：`/OpenAction`、`/AA`（附加动作）、`/JavaScript`
///   （文档级 JS 名字树标记——NamesJavaScript 族）、`/EmbeddedFiles`
///   （嵌入文件名字树标记）、`/RichMedia*` 键族（设置/内容/激活字典）；
/// - 动作形态族（`/S` 名 + 载荷键双探测——畸形缺 `/S` 也可见）：
///   `/S /Launch`（LaunchAction）、`/S /URI` 或 `/URI` 键（UriAction）、
///   `/S /JavaScript` 或 `/JS` 键（JavaScript）；
/// - 流形态族：`/Type /EmbeddedFile`（嵌入文件流本体）。
fn sweep_dict(
    assessment: &mut OpenAssessment,
    id: ObjectId,
    dict: &Dictionary,
    depth: u32,
    limits: &Limits,
) {
    let site = format!("obj {} {}", id.0, id.1);
    if dict.get(b"OpenAction").is_ok() {
        push_hit(
            assessment,
            DangerousFamily::OpenAction,
            format!("{site}: /OpenAction"),
        );
    }
    if dict.get(b"AA").is_ok() {
        push_hit(
            assessment,
            DangerousFamily::AdditionalActions,
            format!("{site}: /AA"),
        );
    }
    let s_name = dict.get(b"S").and_then(Object::as_name).unwrap_or(b"");
    if s_name == b"Launch".as_slice() {
        push_hit(
            assessment,
            DangerousFamily::LaunchAction,
            format!("{site}: /S /Launch"),
        );
    }
    if s_name == b"URI".as_slice() || dict.get(b"URI").is_ok() {
        push_hit(
            assessment,
            DangerousFamily::UriAction,
            format!("{site}: /URI 动作"),
        );
    }
    if s_name == b"JavaScript".as_slice() || dict.get(b"JS").is_ok() {
        push_hit(
            assessment,
            DangerousFamily::JavaScript,
            format!("{site}: /JavaScript 动作"),
        );
    }
    // 文档级 JS 名字树（catalog /Names → /JavaScript 条目——键出现即
    // 族存在，与动作形态的 JavaScript 族分立告警）。
    if dict.get(b"JavaScript").is_ok() {
        push_hit(
            assessment,
            DangerousFamily::NamesJavaScript,
            format!("{site}: /Names /JavaScript 树"),
        );
    }
    let subtype = dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .unwrap_or(b"");
    let rich_media_key = dict.iter().any(|(key, _)| key.starts_with(b"RichMedia"));
    if subtype == b"RichMedia".as_slice() || rich_media_key {
        push_hit(
            assessment,
            DangerousFamily::RichMedia,
            format!("{site}: /RichMedia"),
        );
    }
    let type_name = dict.get(b"Type").and_then(Object::as_name).unwrap_or(b"");
    if type_name == b"EmbeddedFile".as_slice() || dict.get(b"EmbeddedFiles").is_ok() {
        push_hit(
            assessment,
            DangerousFamily::EmbeddedFiles,
            format!("{site}: /EmbeddedFile(s)"),
        );
    }
    // 内联嵌套（字典值/数组——引用不追）。
    for (_, value) in dict.iter() {
        sweep_object(assessment, id, value, depth + 1, limits);
    }
}

fn push_hit(assessment: &mut OpenAssessment, family: DangerousFamily, detail: String) {
    assessment.warnings.push(DangerousHit { family, detail });
}

/// S6 表单探针：/AcroForm → /Fields 字段树枚举（被动可读面——字段
/// 名/现值只读，永不初始化交互 formfill）。/AcroForm 在位且解引用为
/// dict 即 Some（无 /Fields = 空表单：count 0）；**畸形 /AcroForm
/// （非 dict 形态 / 间接引用解析失败或成环）归 None——与无表单不可
/// 区分**（诚实边界：被动探针不做形态诊断，畸形 = 不可枚举面按良性
/// 缺省同判，T2-review ❸）；字段树游走 = 引用链 seen-set 环防 + 深度
/// 护栏。
fn probe_form(doc: &Document, acroform: &Object, limits: &Limits) -> Option<FormProbe> {
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let dict = deref_dict(doc, acroform, &mut seen)?;
    let mut probe = FormProbe {
        field_count: 0,
        fields: Vec::new(),
    };
    if let Ok(fields) = dict.get(b"Fields") {
        walk_field_container(doc, fields, "", &mut probe, &mut seen, 0, limits);
    }
    // 确定性：按全名排序 + 同名去重（同名字段在畸形文档可重复出现）。
    probe.fields.sort_by(|a, b| a.name.cmp(&b.name));
    probe.fields.dedup_by(|a, b| a.name == b.name);
    Some(probe)
}

/// 字段容器游走（/Fields 与 /Kids 的数组形态——数组项 = 字段节点）。
fn walk_field_container(
    doc: &Document,
    object: &Object,
    prefix: &str,
    probe: &mut FormProbe,
    seen: &mut HashSet<ObjectId>,
    depth: u32,
    limits: &Limits,
) {
    if depth > limits.object_depth_max {
        return;
    }
    let items: &Vec<Object> = match object {
        Object::Array(items) => items,
        Object::Reference(id) => match doc.objects.get(id) {
            Some(Object::Array(items)) => items,
            _ => return,
        },
        _ => return,
    };
    for item in items {
        walk_field_node(doc, item, prefix, probe, seen, depth + 1, limits);
    }
}

/// 单字段节点：/T 部分名（全名 = 前缀 `.` 连接）+ /FT 在位判定字段
/// （纯 widget 无 /FT 且无 /T = 挂载件非字段，不计数）+ /V 现值 +
/// /Kids 子树递归。
fn walk_field_node(
    doc: &Document,
    object: &Object,
    prefix: &str,
    probe: &mut FormProbe,
    seen: &mut HashSet<ObjectId>,
    depth: u32,
    limits: &Limits,
) {
    if depth > limits.object_depth_max {
        return;
    }
    let Some(dict) = deref_dict(doc, object, seen) else {
        return;
    };
    let partial = match dict.get(b"T") {
        Ok(object) => text_of(object),
        Err(_) => String::new(),
    };
    let full = if prefix.is_empty() {
        partial
    } else {
        format!("{prefix}.{partial}")
    };
    if dict.get(b"FT").is_ok() || dict.get(b"T").is_ok() {
        probe.field_count += 1;
        if probe.fields.len() < limits.search_max_hits_cap as usize {
            let value = dict.get(b"V").ok().and_then(text_value);
            probe.fields.push(FormField {
                name: full.clone(),
                value,
            });
        }
    }
    if let Ok(kids) = dict.get(b"Kids") {
        walk_field_container(doc, kids, &full, probe, seen, depth + 1, limits);
    }
}

/// 解引用到字典（Reference → objects 表查 + seen 环防[重复/环引用 =
/// None，节点至多游走一次——共享子树无指数放大]；直接字典原样；其余
/// 形态 None）。
fn deref_dict<'a>(
    doc: &'a Document,
    object: &'a Object,
    seen: &mut HashSet<ObjectId>,
) -> Option<&'a Dictionary> {
    match object {
        Object::Dictionary(dict) => Some(dict),
        Object::Reference(id) => {
            if !seen.insert(*id) {
                return None; // 环/重复引用——字段树游走终止面
            }
            match doc.objects.get(id) {
                Some(Object::Dictionary(dict)) => Some(dict),
                _ => None,
            }
        }
        _ => None,
    }
}

/// PDF 文本串 → String（/T 部分名等可选形态：非文本串/缺失 → 空串）。
fn text_of(object: &Object) -> String {
    match object {
        Object::String(bytes, _) => decode_pdf_text(bytes),
        Object::Name(name) => String::from_utf8_lossy(name).into_owned(),
        _ => String::new(),
    }
}

/// /V 现值（探针只读面）：文本串（UTF-16BE/PDFDocEncoding 近似）或
/// 名字形态可读；数值/布尔等非文本形态 None（不造词）。
fn text_value(object: &Object) -> Option<String> {
    match object {
        Object::String(bytes, _) => Some(decode_pdf_text(bytes)),
        Object::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
        _ => None,
    }
}

/// PDF 文本串解码（探针可读面）：UTF-16BE BOM → UTF-16 解码（代理对
/// 拼合，lossy 替换非法单元）；否则 PDFDocEncoding 近似——ASCII 段
/// 精确，高位按 latin-1 映射（探针语义的损失式回退，在档）。
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let mut units = Vec::with_capacity(bytes.len() / 2);
        let mut i = 2;
        while i + 1 < bytes.len() {
            units.push(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
            i += 2;
        }
        return String::from_utf16_lossy(&units);
    }
    bytes.iter().map(|&byte| byte as char).collect()
}
