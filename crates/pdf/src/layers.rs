//! S2-3（PDF-f3）· 图层（OCG）读取与默认可见性开关（feature
//! `engine-lopdf`）。
//!
//! issue #103 任务 3 之三：**图层（OCG）开关**。PDF 图层 = Optional
//! Content Group（spec 8.11）：catalog `/OCProperties` → `/OCGs` 组数组
//! + `/D` 默认配置（`/ON` `/OFF` 可见性列表）。
//!
//! # 实现边界（如实登记）
//!
//! **pdfium-render 0.9.4 绑定面无 OCG 函数**（`FPDFDoc_GetOCG*`/
//! `FPDF_SetOCGState` 均未封装——绑定层仅暴露 pdfium-render 自用函数
//! 集）——**渲染时即时开关不可达**。本模块走 **lopdf 持久化路径**：
//! 读取图层清单 + 修改 `/D //ON` `/OFF` 默认可见性 + 全量保存——
//! 重载后 pdfium/Acrobat 按新默认渲染（图层开关的持久语义，与
//! Acrobat 图层面板的「默认状态」等价；即时切换属渲染引擎升级面，
//! 升级 pdfium-render 或自建绑定后接 [`crate::engine_pdfium`]）。
//!
//! 手术纪律：仅克隆编辑 `/OCProperties` → `/D` 的 `/ON` `/OFF` 两键
//! （其余配置 `/Order` `/BaseState` 等零触碰）；`max_id` 保存前收口
//! （S2-1 PR #105 实施发现——lopdf 写出器从 max_id 派生 /Size，结构
//! 手术必使其陈旧）。

use lopdf::{Document, Object, ObjectId};

use crate::command::CommandError;
use crate::security::Limits;

/// 图层条目。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerInfo {
    /// 图层名（`/Name`；缺省回退 `(OCG n)` 形式名）。
    pub name: String,
    /// 当前默认可见性（在 `/D /ON` 列表 = true；`/OFF` = false；
    /// 两列表都不在 = `/BaseState` 面外——按可见处理[spec 缺省 /ON]）。
    pub default_visible: bool,
}

/// 读取报告。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayerReport {
    /// 解析出的图层数。
    pub layers: usize,
    /// `/OCProperties` 缺失（无图层文档——合法形态，非错误）。
    pub no_ocg: bool,
    /// 无名图层计数（回退命名面登记）。
    pub unnamed: usize,
}

/// 读取图层清单（/OCGs 声明序）。
///
/// 安全镜像打开面（S3-L7 → 解析 → S5 → S3-L4——与其他读取面同序）。
pub fn read_layers(
    pdf: Vec<u8>,
    limits: Limits,
) -> Result<(Vec<LayerInfo>, LayerReport), CommandError> {
    let doc = load(&pdf, &limits)?;
    let mut report = LayerReport::default();
    let Some((ocgs, on_set, off_set)) = ocg_view(&doc) else {
        report.no_ocg = true;
        return Ok((Vec::new(), report));
    };
    let mut layers = Vec::with_capacity(ocgs.len());
    for (i, ocg_id) in ocgs.iter().enumerate() {
        let dict = doc.get_object(*ocg_id).ok().and_then(|o| o.as_dict().ok());
        let name = dict
            .and_then(|d| match d.get(b"Name") {
                Ok(Object::String(bytes, _)) => Some(crate::security_scan::decode_pdf_text(bytes)),
                _ => None,
            })
            .unwrap_or_else(|| {
                report.unnamed += 1;
                format!("(OCG {})", i + 1)
            });
        let default_visible = if on_set.contains(ocg_id) {
            true
        } else if off_set.contains(ocg_id) {
            false
        } else {
            true // 两列表缺席 = 缺省可见（spec 8.11.4.4 /BaseState 默认 ON）
        };
        layers.push(LayerInfo {
            name,
            default_visible,
        });
    }
    report.layers = layers.len();
    Ok((layers, report))
}

/// 切换图层默认可见性（持久化——修改 /D /ON//OFF 后全量保存返回新字节）。
///
/// `layer` = [`read_layers`] 返回序（0 基）；`visible` = 新默认态。
/// 幂等：目标态与现态一致时仍产出（结构重排面 = 全量保存语义，
/// 与 organize.rs save_to_vec 同族——不承诺字节不动）。
pub fn set_layer_default_visible(
    pdf: Vec<u8>,
    limits: Limits,
    layer: usize,
    visible: bool,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = load(&pdf, &limits)?;
    let Some((ocgs, on_set, off_set)) = ocg_view(&doc) else {
        return Err(CommandError::engine(
            "无 /OCProperties（无图层文档——开关无对象）",
        ));
    };
    let Some(&ocg_id) = ocgs.get(layer) else {
        return Err(CommandError::engine(format!(
            "图层序号越界：{layer}（共 {} 层）",
            ocgs.len()
        )));
    };
    // 目标集合（含与不含的语义由两列表之差承载——不在此处重读现态，
    // 直接落目标态：visible → 入 ON 出 OFF；invisible → 反之）。
    let _ = off_set;
    let new_on: Vec<ObjectId> = if visible {
        let mut s: std::collections::BTreeSet<ObjectId> = on_set;
        s.insert(ocg_id);
        s.into_iter().collect()
    } else {
        on_set.into_iter().filter(|id| *id != ocg_id).collect()
    };
    let new_off: Vec<ObjectId> = if visible {
        // 出 OFF。
        off_set.into_iter().filter(|id| *id != ocg_id).collect()
    } else {
        let mut s = off_set;
        s.insert(ocg_id);
        s.into_iter().collect()
    };
    write_default_state(&mut doc, new_on, new_off)?;
    save_full(&mut doc)
}

// ---------------------------------------------------------------------------
// 内部
// ---------------------------------------------------------------------------

/// 装载（安全镜像：S3-L7 → 解析 → S5 → S3-L4）。
fn load(pdf: &[u8], limits: &Limits) -> Result<Document, CommandError> {
    limits.check_open_input_bytes(pdf.len() as u64)?;
    let started = crate::security::portable_now();
    let doc = Document::load_mem(pdf)
        .map_err(|e| CommandError::engine(format!("lopdf 装载失败：{e}")))?;
    if doc.is_encrypted() || doc.was_encrypted() {
        return Err(CommandError::SecurityDeny {
            reason: crate::security::REASON_ENCRYPTED_WRITE,
        });
    }
    limits.check_open_budget(started)?;
    Ok(doc)
}

/// OCG 视图：(组 id 声明序, ON 集, OFF 集)。无 /OCProperties → None。
fn ocg_view(
    doc: &Document,
) -> Option<(
    Vec<ObjectId>,
    std::collections::BTreeSet<ObjectId>,
    std::collections::BTreeSet<ObjectId>,
)> {
    let props = oc_properties_dict(doc)?;
    // /OCGs 组数组（引用序）。
    let ocgs: Vec<ObjectId> = match props.get(b"OCGs") {
        Ok(Object::Array(arr)) => arr
            .iter()
            .filter_map(|o| match o {
                Object::Reference(id) => Some(*id),
                _ => None,
            })
            .collect(),
        _ => return Some((Vec::new(), Default::default(), Default::default())),
    };
    // /D 默认配置的 /ON /OFF 两列表。
    let (on, off) = match props.get(b"D") {
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .map(on_off_sets),
        Ok(Object::Dictionary(d)) => Some(on_off_sets(d)),
        _ => None,
    }
    .unwrap_or_default();
    Some((ocgs, on, off))
}

/// /D 字典的 /ON /OFF 两集合。
fn on_off_sets(
    d: &lopdf::Dictionary,
) -> (
    std::collections::BTreeSet<ObjectId>,
    std::collections::BTreeSet<ObjectId>,
) {
    let set = |key: &[u8]| -> std::collections::BTreeSet<ObjectId> {
        match d.get(key) {
            Ok(Object::Array(arr)) => arr
                .iter()
                .filter_map(|o| match o {
                    Object::Reference(id) => Some(*id),
                    _ => None,
                })
                .collect(),
            Ok(Object::Reference(_)) => std::collections::BTreeSet::new(), // 间接数组罕见形态——空集回退
            _ => std::collections::BTreeSet::new(),
        }
    };
    (set(b"ON"), set(b"OFF"))
}

/// catalog /OCProperties 字典（间接引用/直接字典两形态）。
fn oc_properties_dict(doc: &Document) -> Option<lopdf::Dictionary> {
    let catalog = doc.catalog().ok()?;
    match catalog.get(b"OCProperties") {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned(),
        _ => None,
    }
}

/// 写回 /D 的 /ON /OFF（克隆 /OCProperties + /D 进对象图——原字典若为
/// 直接形态则替换；间接引用形态同对象号重定义[全量保存语义下二者等价]）。
fn write_default_state(
    doc: &mut Document,
    new_on: Vec<ObjectId>,
    new_off: Vec<ObjectId>,
) -> Result<(), CommandError> {
    // 现有 /D（直接或间接）定位后重建。
    let catalog_id = match doc.trailer.get(b"Root") {
        Ok(Object::Reference(id)) => *id,
        _ => return Err(CommandError::engine("catalog 定位失败")),
    };
    let mut props = oc_properties_dict(doc)
        .ok_or_else(|| CommandError::engine("无 /OCProperties（调用面前置校验已拒——不可达）"))?;
    // 现有 /D 保留其余键（/Order /BaseState /Intent 等——外科手术：
    // 只改 /ON /OFF）。
    let mut d = match props.get(b"D") {
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default(),
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => lopdf::Dictionary::default(),
    };
    let to_arr =
        |ids: Vec<ObjectId>| Object::Array(ids.into_iter().map(Object::Reference).collect());
    d.set("ON", to_arr(new_on));
    d.set("OFF", to_arr(new_off));
    let d_id = doc.add_object(Object::Dictionary(d));
    props.set("D", Object::Reference(d_id));
    // /OCProperties 挂回（直接字典形态——保存语义等价且免对象号管理）。
    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|e| CommandError::engine(format!("catalog 读取失败：{e}")))?
        .as_dict_mut()
        .map_err(|e| CommandError::engine(format!("catalog 非字典：{e}")))?;
    catalog.set("OCProperties", Object::Dictionary(props));
    Ok(())
}

/// 全量保存（max_id 收口——S2-1 实施发现：lopdf 写出器从 max_id 派生
/// trailer /Size，对象图手术后必陈旧）。
fn save_full(doc: &mut Document) -> Result<Vec<u8>, CommandError> {
    if let Some((&(true_max, _), _)) = doc.objects.iter().next_back() {
        doc.max_id = true_max;
    }
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::engine(format!("lopdf 保存失败：{e}")))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// 模块测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_semantics() {
        // /D /ON //OFF 与缺省可见语义钉定（spec 8.11.4.4）。
        // （结构面由集成测试 pdff3_render_ext 覆盖——手搓 OCG 夹具。）
        let mut d = lopdf::Dictionary::new();
        d.set("ON", Object::Array(vec![Object::Reference((1, 0))]));
        d.set("OFF", Object::Array(vec![Object::Reference((2, 0))]));
        let (on, off) = on_off_sets(&d);
        assert!(on.contains(&(1, 0)));
        assert!(off.contains(&(2, 0)));
        assert!(!on.contains(&(2, 0)));
        // 两列表都缺席 → 缺省可见（read_layers 的分支语义）。
    }
}
