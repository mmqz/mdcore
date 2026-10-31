//! S2-3（PDF-f3）· 链接跳转读取——Page /Annots 的 /Link → 目标页/URI
//! （feature `engine-lopdf`）。
//!
//! issue #103 任务 3 之一：**打开带链接的 PDF，能取出所有链接（页码 +
//! 目标）**。lopdf 对象图直读（与 [`crate::organize`] 无依赖——分支
//! 独立），页码解析 = 对象引用 → 文档序页号映射（`get_pages` 反查表）。
//!
//! 目标形态全覆盖（spec 12.3.2 / 12.5.6.5）：
//! - `/Dest` 直接目标：`[page /XYZ x y z]`（page = 引用 or 显式页号）；
//! - `/Dest` 命名目标：PDF 1.1 `/Dests` 字典 + `/Names //Dests` 名称树
//!   （树深 ≤ 16 防御）；
//! - `/A` 动作：`/GoTo`（`/D` 同上两形态）/ `/URI`（外链字符串）；
//! - `/GoToR`（远程文档跳转）/ `/Launch`：**不可解析为本档页号**——
//!   [`LinkTarget::Unresolved`] 登记（远程目标是合法形态，非错误面）。
//!
//! 宽容纪律（与 S2-2 读取面同款）：畸形条目（无 /Subtype、非 /Link、
//! 载荷缺字段）跳过并计数——`LinkReport` 全量登记，无静默。

use lopdf::{Document, Object, ObjectId};

use crate::annotation::Rect;
use crate::command::CommandError;
use crate::page::PageIndex;
use crate::security::Limits;

/// 链接目标（本档页号 or 外链 URI or 不可解析[远程/悬空]）。
#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    /// 本档内部跳转（0 基目标页号）。
    Page(PageIndex),
    /// 外部 URI（`/A /URI`——http/mailto 等）。
    Uri(String),
    /// 不可解析：远程 GoToR / Launch 动作、悬空命名目的地、畸形目标。
    /// **合法登记面**（远程目标是正常形态），非错误。
    Unresolved,
}

/// 链接条目（所在页 + 热区 + 目标）。
#[derive(Debug, Clone, PartialEq)]
pub struct LinkInfo {
    /// 所在页（0 基）。
    pub page: PageIndex,
    /// 点击热区（PDF 用户空间）。
    pub rect: Rect,
    /// 目标。
    pub target: LinkTarget,
}

/// 读取报告（宽容跳过计数——无静默）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkReport {
    /// 解析出的链接条数。
    pub links: usize,
    /// 其中本档内部跳转数。
    pub internal: usize,
    /// 其中外部 URI 数。
    pub uri: usize,
    /// 其中不可解析（远程/悬空/畸形目标）数。
    pub unresolved: usize,
    /// 跳过的非链接标注（/Annots 内其他子型——读取面不消费）。
    pub skipped_non_link: usize,
    /// 跳过的畸形条目（无 /Subtype / 非 /Link 但形态异常 / 载荷缺字段）。
    pub skipped_malformed: usize,
}

/// 读取 PDF 全部链接（页升序 + 页内 /Annots 序）。
///
/// 安全镜像打开面（S3-L7 → 解析 → S5 加密拒 → S3-L4 预算——与
/// [`crate::writer_lopdf::LopdfWriter::from_bytes_with_limits`] 同序）。
pub fn read_links(
    pdf: Vec<u8>,
    limits: Limits,
) -> Result<(Vec<LinkInfo>, LinkReport), CommandError> {
    limits.check_open_input_bytes(pdf.len() as u64)?;
    let started = crate::security::portable_now();
    let doc = Document::load_mem(&pdf)
        .map_err(|e| CommandError::engine(format!("lopdf 装载失败：{e}")))?;
    if doc.is_encrypted() || doc.was_encrypted() {
        return Err(CommandError::SecurityDeny {
            reason: crate::security::REASON_ENCRYPTED_WRITE,
        });
    }
    limits.check_open_budget(started)?;

    // 对象引用 → 文档序页号反查表（get_pages 值集）。
    let page_of: std::collections::HashMap<ObjectId, u32> = doc
        .get_pages()
        .iter()
        .map(|(num, id)| (*id, *num - 1)) // 1 基 → 0 基
        .collect();
    // 页对象 id 文档序列表。
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();

    let mut out = Vec::new();
    let mut report = LinkReport::default();
    for (page_idx, &page_id) in pages.iter().enumerate() {
        for entry in annot_entries(&doc, page_id) {
            let dict = match &entry {
                Object::Reference(id) => match doc.get_object(*id).map(|o| o.as_dict().ok()) {
                    Ok(Some(d)) => d,
                    _ => {
                        report.skipped_malformed += 1;
                        continue;
                    }
                },
                Object::Dictionary(d) => d,
                _ => {
                    report.skipped_malformed += 1;
                    continue;
                }
            };
            if !matches!(dict.get(b"Subtype"), Ok(Object::Name(n)) if n == b"Link") {
                // 非 /Link 标注（高亮/便签/控件…）——本模块不消费。
                report.skipped_non_link += 1;
                continue;
            }
            let rect = match parse_rect(dict) {
                Some(r) => r,
                None => {
                    report.skipped_malformed += 1;
                    continue;
                }
            };
            let target = parse_target(&doc, dict, &page_of);
            match &target {
                LinkTarget::Page(_) => report.internal += 1,
                LinkTarget::Uri(_) => report.uri += 1,
                LinkTarget::Unresolved => report.unresolved += 1,
            }
            out.push(LinkInfo {
                page: PageIndex(page_idx as u32),
                rect,
                target,
            });
            report.links += 1;
        }
    }
    Ok((out, report))
}

/// 页 /Annots 数组内容（直接/间接两形态；缺省 = 空）。
fn annot_entries(doc: &Document, page_id: ObjectId) -> Vec<Object> {
    let Ok(Some(page)) = doc.get_object(page_id).map(|o| o.as_dict().ok()) else {
        return Vec::new();
    };
    match page.get(b"Annots") {
        Ok(Object::Array(arr)) => arr.clone(),
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// /Rect → Rect（4 数；反序 = None）。
fn parse_rect(dict: &lopdf::Dictionary) -> Option<Rect> {
    let arr = match dict.get(b"Rect") {
        Ok(Object::Array(a)) => a,
        _ => return None,
    };
    if arr.len() < 4 {
        return None;
    }
    let mut v = [0f32; 4];
    for (i, o) in arr.iter().take(4).enumerate() {
        v[i] = match o {
            Object::Integer(n) => *n as f32,
            Object::Real(r) => *r,
            _ => return None,
        };
    }
    Rect::new(v[0], v[1], v[2], v[3]).ok()
}

/// 链接目标解析（/Dest or /A；三动作面 GoTo/URI/GoToR-Launch）。
fn parse_target(
    doc: &Document,
    dict: &lopdf::Dictionary,
    page_of: &std::collections::HashMap<ObjectId, u32>,
) -> LinkTarget {
    // 形态 1：/Dest 直接目标。
    if let Ok(dest) = dict.get(b"Dest") {
        if let Some(t) = dest_to_target(doc, dest, page_of) {
            return t;
        }
    }
    // 形态 2：/A 动作。
    if let Ok(Object::Dictionary(action)) = dict.get(b"A") {
        let subtype = action.get(b"S");
        if let Ok(dest) = action.get(b"D") {
            if matches!(subtype, Ok(Object::Name(n)) if n == b"GoTo") {
                if let Some(t) = dest_to_target(doc, dest, page_of) {
                    return t;
                }
            }
        }
        if let Ok(Object::String(bytes, _)) = action.get(b"URI") {
            return LinkTarget::Uri(String::from_utf8_lossy(bytes).to_string());
        }
        // GoToR / Launch：远程面——合法形态，Unresolved 登记。
    }
    LinkTarget::Unresolved
}

/// 目标对象 → LinkTarget（数组/名称/字符串名称三形态）。
fn dest_to_target(
    doc: &Document,
    dest: &Object,
    page_of: &std::collections::HashMap<ObjectId, u32>,
) -> Option<LinkTarget> {
    match dest {
        Object::Array(arr) => match arr.first() {
            Some(Object::Reference(id)) => page_of.get(id).map(|&p| LinkTarget::Page(PageIndex(p))),
            // 显式页号形态（spec 12.3.2.2——0 基文档序）。
            Some(Object::Integer(n)) if *n >= 0 => Some(LinkTarget::Page(PageIndex(*n as u32))),
            _ => None,
        },
        Object::Name(name) | Object::String(name, _) => {
            // 命名目的地：命中 → 目标对象递归换算；未命中 → 悬空（合法
            // 登记面——外部生成器的常见形态，非错误）。
            match lookup_named_dest(doc, name) {
                Some(dest_obj) => dest_to_target(doc, &dest_obj, page_of),
                None => Some(LinkTarget::Unresolved),
            }
        }
        _ => None,
    }
}

/// 命名目的地解析：PDF 1.1 catalog `/Dests` 字典 + `/Names //Dests`
/// 名称树（spec 7.9.6；Kids 递归 ≤ 16，/Names 数组 [k1 v1 k2 v2 ..]）。
/// 命中 → 目标对象（再经 [`dest_to_target`] 换算页号）；未命中 →
/// 悬空命名目的地（`Unresolved` 登记面）。
fn lookup_named_dest(doc: &Document, name: &[u8]) -> Option<Object> {
    let catalog = doc.catalog().ok()?;
    // 旧式字典。
    if let Ok(Object::Reference(dests_id)) = catalog.get(b"Dests") {
        if let Ok(Object::Dictionary(dests)) = doc.get_object(*dests_id) {
            if let Ok(dest) = dests.get(name) {
                return Some(dest.clone());
            }
        }
    }
    // 新式名称树。
    if let Ok(Object::Reference(names_id)) = catalog.get(b"Names") {
        if let Ok(Object::Dictionary(names)) = doc.get_object(*names_id) {
            if let Ok(Object::Reference(dests_id)) = names.get(b"Dests") {
                if let Some(found) = name_tree_lookup(doc, *dests_id, name, 0) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// 名称树查找（/Names 对数组或 /Kids 递归；深度防御）。
fn name_tree_lookup(
    doc: &Document,
    node_id: ObjectId,
    name: &[u8],
    depth: usize,
) -> Option<Object> {
    if depth > 16 {
        return None;
    }
    let node = doc.get_object(node_id).ok()?.as_dict().ok()?;
    if let Ok(Object::Array(pairs)) = node.get(b"Names") {
        let mut i = 0;
        while i + 1 < pairs.len() {
            let hit = matches!(&pairs[i], Object::Name(k) | Object::String(k, _) if k == name);
            if hit {
                return Some(pairs[i + 1].clone());
            }
            i += 2;
        }
    }
    if let Ok(Object::Array(kids)) = node.get(b"Kids") {
        for kid in kids {
            if let Object::Reference(kid_id) = kid {
                if let Some(found) = name_tree_lookup(doc, *kid_id, name, depth + 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}
