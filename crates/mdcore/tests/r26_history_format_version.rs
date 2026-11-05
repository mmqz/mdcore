//! r26 — 持久化 History 格式版本闸门（#221 补充修复，承 #220）。
//!
//! #220 修复了新建条目的 inverses 存储序（组间逆序、组内保序），但
//! undo 历史随文档持久化（`save_document` → meta.history →
//! `open_document` → `set_history_json`）——#220 修复前写下的旧档携带
//! 整体摊平反转形态的错序 inverses，升级后打开若照常恢复栈，用户一次
//! undo 就会执行错序重建（非叶删除条目：先插子后插父 → BadPath 硬
//! 失败/整栈卡死，或顶替路径静默错挂）。
//!
//! 本组回归锁定 `History::from_json` 版本闸门（`HISTORY_FORMAT_VERSION`）：
//!
//! 1. 旧档（缺 format_version / 显式 v1 / 未知未来版本）打开：正文、
//!    document_id、revision 完整复原；旧 undo/redo 栈被降级丢弃，
//!    undo()/redo() 不执行旧错序 inverses（树不被污染）；
//! 2. 当前格式 save/open 往返：undo/redo 栈无损，非叶删除条目（#220
//!    场景）跨存档边界 undo/redo 精确工作（文本逐字节 + 物化 ID 复原）；
//! 3. 版本字段钉定：新档必带 format_version；降级后的档再存即升当前版；
//!    降级档可继续编辑并撤销（丢的是历史，不是文档）；
//! 4. 「格式旧」与「格式坏」分开：JSON 畸形仍显式 Reject（降级不吞
//!    真正的损坏）。
//!
//! 旧档制作手法：用当前代码构造正确序栈（F3 非叶删除），对栈顶条目
//! 的 inverses 数组整体 reverse 并删除 format_version 字段。对单事务
//! 单 RemoveBlock（逆组 = [ins(P), ins(C1), ins(C2)]，摊平后整体
//! reverse = [ins(C2), ins(C1), ins(P)]）与 #220 修复前代码的产出
//! **逐位一致**——测试材料是真实旧档形态，不是抽象模拟。

use std::sync::Arc;

use mdcore::anchor::TextTarget;
use mdcore::document::DocumentId;
use mdcore::history::HISTORY_FORMAT_VERSION;
use mdcore::tree::{BlockKind, NodePath};
use mdcore::Core;

// ---------------------------------------------------------------------------
// 公共工具（同 r25 手法）
// ---------------------------------------------------------------------------

/// 先序 dump：每块一行（路径, 类型, 文本）。块 ID 不进比较——save 面
/// 是 markdown（ID 不落纸），跨 save/open 只有 inverses 重建的子树
/// 携带物化原 ID，其余块 ID 由重开侧 markdown 解析现场分配（既有
/// 设计）；ID 复原断言单独钉（见各用例）。
fn shape(core: &Core) -> Vec<(NodePath, BlockKind, String)> {
    fn walk(core: &Core, path: &mut Vec<usize>, out: &mut Vec<(NodePath, BlockKind, String)>) {
        let id = core.tree.resolve(path).expect("valid path");
        let n = core.tree.node(id);
        out.push((path.clone(), n.kind, n.text.to_string()));
        let n_children = n.children.len();
        for i in 0..n_children {
            path.push(i);
            walk(core, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    for i in 0..core.tree.top_level().len() {
        let mut path = vec![i];
        walk(core, &mut path, &mut out);
    }
    out
}

fn md_of(core: &Core) -> String {
    mdcore::markdown::tree_to_markdown(&core.tree)
}

/// 步进时钟（每次 +10s）：各命令独立成条目，不受合并窗干扰。
fn step_clock() -> Arc<dyn Fn() -> u64 + Send + Sync> {
    let tick = std::sync::atomic::AtomicU64::new(1000);
    Arc::new(move || tick.fetch_add(10_000, std::sync::atomic::Ordering::SeqCst) + 10_000)
}

// ---------------------------------------------------------------------------
// 旧档材料
// ---------------------------------------------------------------------------

/// 「#220 修复前」形态的档：F3（"A\n- P\n  - C1\n  - C2\nB"）非叶删除
/// [1] 后保存。`version`：None = 删版本字段（真实旧档形态）；
/// Some(v) = 显式写 v（旧版本号/未来版本号变体）。返回
/// (markdown 正文, meta, document_id, 删除后 revision)。
fn legacy_meta(version: Option<u32>) -> (String, String, DocumentId, u64) {
    let mut core = Core::from_markdown_with_clock("A\n- P\n  - C1\n  - C2\nB", step_clock());
    assert_eq!(md_of(&core), "A\n- P\n  - C1\n  - C2\nB", "F3 初始形状自检");
    let base = core.revision();
    core.delete_block(base, vec![1]).unwrap();
    assert_eq!(md_of(&core), "A\nB", "非叶删除后正文");
    assert_eq!(core.history.undo_depth(), 1, "单事务单条目");
    core.set_document_id(None).unwrap(); // 注入时钟现场生成
    let id = core.document_id().expect("document_id set");
    let rev = core.revision();

    // 测试材料自检：栈顶条目逆组长度 3（P/C1/C2 先序重建），且为当前
    // 格式（带版本字段）——随后才降格为旧档形态。
    let (md, meta) = core.save_document();
    let v: serde_json::Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(
        v["history"]["format_version"],
        serde_json::json!(HISTORY_FORMAT_VERSION),
        "自检：当前代码存档必带版本字段"
    );
    let inv = v["history"]["undo"][0]["inverses"]
        .as_array()
        .expect("inverses array");
    assert_eq!(inv.len(), 3, "自检：F3 非叶删除逆组长度 3");

    // 降格为旧档：删版本字段 + inverses 整体反转（= 修复前代码产出）。
    let mut v = v;
    let obj = v["history"].as_object_mut().unwrap();
    match version {
        None => {
            obj.remove("format_version");
        }
        Some(ver) => {
            obj.insert("format_version".into(), serde_json::json!(ver));
        }
    }
    let mut inverses = v["history"]["undo"][0]["inverses"]
        .as_array()
        .unwrap()
        .clone();
    inverses.reverse();
    v["history"]["undo"][0]["inverses"] = serde_json::Value::Array(inverses);
    (md, v.to_string(), id, rev)
}

/// 降级断言包：正文 / document_id / revision 完整复原；旧栈丢弃且
/// undo/redo 不执行（revision 不动、正文不动——错序 inverses 未触树）。
/// 参照元组来自制作函数（同一份档，避免随机 document_id 错配）。
fn assert_degraded_open(md: &str, meta_json: &str, id: DocumentId, rev: u64) {
    let mut core = Core::open_document(md, meta_json)
        .expect("旧档必须能开：降级不拒收（正文/身份/revision 保留）");
    assert_eq!(md_of(&core), md, "正文完整复原");
    assert_eq!(core.document_id(), Some(id), "document_id 完整复原");
    assert_eq!(core.revision(), rev, "revision 完整复原");
    assert_eq!(core.history.undo_depth(), 0, "旧 undo 栈被丢弃");
    assert_eq!(core.history.redo_depth(), 0, "旧 redo 栈被丢弃");
    assert!(!core.history.can_undo(), "can_undo = false");
    assert!(!core.history.can_redo(), "can_redo = false");
    assert!(core.undo().is_none(), "旧错序 inverses 不可执行");
    assert!(core.redo().is_none(), "旧 redo 不可执行");
    assert_eq!(md_of(&core), md, "undo 尝试后正文不动（树未被污染）");
    assert_eq!(core.revision(), rev, "undo 尝试后 revision 不动（未 bump）");
}

// ---------------------------------------------------------------------------
// 1. 旧档降级（Owner 审核要求：缺版本或旧版本 → 保留正文/身份/revision，
//    清空旧 undo/redo 栈，不执行旧错序 inverses）
// ---------------------------------------------------------------------------

/// 缺 format_version 字段（#220 修复前的真实旧档形态：当时序列化根本
/// 没有此字段，serde default = 0）。
#[test]
fn legacy_meta_without_version_opens_intact_undo_discarded() {
    let (md, meta, id, rev) = legacy_meta(None);
    assert_degraded_open(&md, &meta, id, rev);
}

/// 显式旧版本号 v1：与缺字段同途降级（版本闸门只认当前版本）。
#[test]
fn explicit_v1_history_degrades() {
    let (md, meta, id, rev) = legacy_meta(Some(1));
    assert_degraded_open(&md, &meta, id, rev);
}

/// 未知未来版本（v3+）：语义未知，同样不执行——前向保护（宁可弃栈
/// 不赌树；降级语义与旧档一致）。
#[test]
fn unknown_future_version_degrades() {
    let (md, meta, id, rev) = legacy_meta(Some(HISTORY_FORMAT_VERSION + 1));
    assert_degraded_open(&md, &meta, id, rev);
}

// ---------------------------------------------------------------------------
// 2. 当前格式往返（Owner 审核要求：新版本 History save/open 后正常
//    支持 undo/redo）
// ---------------------------------------------------------------------------

/// 编辑链「文本 → 非叶删除 → 文本」（#220 场景：中间条目的 inverses
/// 为长度 3 逆组）save/open 往返后：栈深无损；undo×3 / redo×3 精确
/// 复原（文本逐字节 + 重建子树物化原 ID）。
#[test]
fn current_format_roundtrip_undo_redo_across_save_open() {
    let mut core = Core::from_markdown_with_clock("A\n- P\n  - C1\n  - C2\nB", step_clock());
    let pre_shape = shape(&core);
    let pre_md = md_of(&core);
    // 编辑前抓 P 的身份 BlockId：undo 重建必须携带物化原 ID（跨 JSON 往返）。
    // （resolve 给槽位 NodeId，身份在 Node.id——与 r25 dump 同手法。）
    let pre_p_id = {
        let slot = core.tree.resolve(&[1]).expect("P at [1]");
        core.tree.node(slot).id.0
    };
    core.set_document_id(None).unwrap();

    // 编辑 1（文本，独立条目）
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 1,
            end: 1,
        },
        " v2".into(),
    )
    .unwrap();
    // 子树删除（结构，独立条目，逆组长度 3）
    let base = core.revision();
    core.delete_block(base, vec![1]).unwrap();
    // 编辑 2：B 已移位到 [1]
    core.replace_text(
        TextTarget::ByPath {
            path: vec![1],
            start: 1,
            end: 1,
        },
        "!".into(),
    )
    .unwrap();
    let final_md = md_of(&core);
    assert_eq!(final_md, "A v2\nB!", "链末正文");
    assert_eq!(core.history.undo_depth(), 3, "三条独立条目");
    let final_rev = core.revision();

    // save → open：栈无损
    let (md, meta) = core.save_document();
    let mut reopened = Core::open_document(&md, &meta).unwrap();
    assert_eq!(reopened.history.undo_depth(), 3, "undo 栈跨存档无损");
    assert_eq!(reopened.history.redo_depth(), 0, "redo 栈为空");
    assert_eq!(reopened.document_id(), core.document_id(), "身份无损");
    assert_eq!(reopened.revision(), final_rev, "revision 无损");

    // undo×3：全部成功，回初始形状 + 文本逐字节 + P 物化原 ID
    for i in 1..=3 {
        assert!(
            reopened.undo().is_some(),
            "第 {i} 次 undo 必须成功（跨存档边界；现状若恢复旧错序栈则第 2 次必坏）"
        );
    }
    assert_eq!(shape(&reopened), pre_shape, "三次 undo 后结构+文本复原");
    assert_eq!(
        md_of(&reopened),
        pre_md,
        "三次 undo 后 to_markdown 逐字节复原"
    );
    let restored_p_id = {
        let slot = reopened.tree.resolve(&[1]).expect("P restored at [1]");
        reopened.tree.node(slot).id.0
    };
    assert_eq!(
        restored_p_id, pre_p_id,
        "重建子树携带物化原 ID（ID 随 history JSON 往返）"
    );

    // redo×3：回链末
    for i in 1..=3 {
        assert!(reopened.redo().is_some(), "第 {i} 次 redo 必须成功");
    }
    assert_eq!(md_of(&reopened), final_md, "三次 redo 后回链末正文");
    assert_eq!(
        reopened.revision(),
        final_rev + 6,
        "undo×3 + redo×3 各 bump 一次"
    );
}

// ---------------------------------------------------------------------------
// 3. 版本字段钉定 + 降级档继续可用
// ---------------------------------------------------------------------------

/// 新档（含空栈）必带当前版本字段：序列化无条件写出（skip 会让新档
/// 被自己读回时降级——空栈丢得起，但格式标记必须在）。
#[test]
fn saved_meta_carries_current_format_version() {
    let core = Core::from_markdown("A");
    let (_, meta) = core.save_document();
    let v: serde_json::Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(
        v["history"]["format_version"],
        serde_json::json!(HISTORY_FORMAT_VERSION),
        "save_document 产物必带版本字段"
    );
    let h: serde_json::Value = serde_json::from_str(&core.history_json()).unwrap();
    assert_eq!(
        h["format_version"],
        serde_json::json!(HISTORY_FORMAT_VERSION),
        "history_json 直出同带"
    );
}

/// 降级不是死档：旧档打开（丢历史）后继续编辑入栈、undo 正常工作、
/// 再存档即升当前版本（undo:[] + format_version=2——升级路径单向收敛）。
#[test]
fn degraded_core_keeps_editing_and_resaves_as_current_version() {
    let (md, legacy_meta, _, _) = legacy_meta(None);
    let mut core = Core::open_document(&md, &legacy_meta).unwrap();
    assert_eq!(core.history.undo_depth(), 0, "旧栈已丢");

    // 降级后第一击键：正常入栈（新会话，无残留合并）
    core.replace_text(
        TextTarget::ByPath {
            path: vec![1],
            start: 1,
            end: 1,
        },
        "?".into(),
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), 1, "降级后新编辑入栈");
    let edited = md_of(&core);
    assert_eq!(edited, "A\nB?");

    // undo 正常回退
    assert!(core.undo().is_some(), "降级后的新条目 undo 正常");
    assert_eq!(md_of(&core), md, "undo 回到开档正文");

    // 再存档：升当前版本
    let (_, meta2) = core.save_document();
    let v: serde_json::Value = serde_json::from_str(&meta2).unwrap();
    assert_eq!(
        v["history"]["format_version"],
        serde_json::json!(HISTORY_FORMAT_VERSION),
        "降级档再存即升当前版本"
    );
    // 且新档 reopen：栈正常恢复（条目在 redo 栈——undo 过）
    let reopened = Core::open_document(&md, &meta2).unwrap();
    assert_eq!(reopened.history.redo_depth(), 1, "v2 档栈正常往返");
}

// ---------------------------------------------------------------------------
// 4. 「格式旧」≠「格式坏」：畸形 JSON 仍显式 Reject
// ---------------------------------------------------------------------------

/// 版本闸门只对**可解析**的旧格式降级；JSON 本身畸形（结构类型坏）
/// 仍然显式失败——open_document 的「宁可开不了档，不静默错配」语义
/// 不因降级路径放松。
#[test]
fn malformed_history_json_still_rejects() {
    let bad_hist = r#"{"document_id":null,"revision":3,"history":{"undo":"not-an-array"}}"#;
    assert!(
        Core::open_document("A", bad_hist).is_err(),
        "畸形 history（undo 非数组）必须显式 Reject，不得降级吞掉"
    );
    let bad_hist2 = r#"{"document_id":null,"revision":3,"history":[1,2,3]}"#;
    assert!(
        Core::open_document("A", bad_hist2).is_err(),
        "history 非对象（数组）必须显式 Reject"
    );
}
