//! r27 — 块身份层不变量（#244，审计 P0-1 / F-1）。
//!
//! #244 修复 = 路线 A（meta v2 持久化先序 ID 表：save→open 身份稳定）
//! + 路线 C（register 撞号守卫：纵深防线，旧档/异常面响亮失败）
//! + F-12（detach 注销 + path_of 去 panic）。
//!
//! 本文件按 issue #244 验收矩阵逐条落测试锁：
//!
//! 1. **I1 唯一性**：随机操作序列（插入/删除/移动/split/merge/undo/redo/
//!    save+open）× N 轮，每步断言活树 BlockId 唯一 + id_index 一致
//!    （每块可解且指向自身槽位、注册表条目数 == 挂载节点数）；
//! 2. **I2 往返稳定**：三层嵌套文档 save→open 逐块（路径, ID）恒等；
//!    fuzz 中每次 save+open 步进同样断言 ID/内容/历史三面无损；
//! 3. **I3 跨会话 undo 安全**：改 A → 改 B → save → open → undo×2 /
//!    redo×2，唯一 + 内容精确复原；
//! 4. **I4/I5 撞号响亮拒绝**：物化 ID 撞活树 → typed `IdCollision`
//!    （树级 / apply_ops 事务级两路），失败零副作用 / 整体回滚；
//! 5. **F-12**：detach 注销子树、path_of 对未挂载节点返回 None（不 panic）、
//!    move 后注册表完好；
//! 6. **F-2 现状锁**（审计指认缺失的端到端后果演示）：无 expect 的
//!    ByAnchor 跨 sync_from_markdown 后解析到**被顶位的内容**并静默
//!    改写——真实修复（偏移平移映射 / 强制 expect）超出 #244 范围，
//!    此处锁现状供 R9 立项，防「修了 ID 就以为锚点也稳了」的误判；
//! 7. **meta 边界**：blocks 表数量失配 → 降级（先序重发 + 丢弃历史）；
//!    表内重复 / meta_format 非数字 → 显式 Reject；未来版本 → 忽略
//!    blocks 表（前向保护）；二次往返 blocks 表恒等。
//!
//! fuzz 目标面纪律：结构/文本操作限定在 markdown 往返**可表示**面
//! （顶层段落插入、顶层节点与列表项的删除/移动、单行块文本编辑、
//! 顶层段落 split/merge）——Table/ThematicBreak 的文本改写会破坏序列化
//! 表示（属 markdown 模型表达力边界，非身份层缺陷），不进 fuzz 面。

use std::sync::Arc;

use mdcore::anchor::TextTarget;
use mdcore::id::BlockId;
use mdcore::op::Op;
use mdcore::tree::{BlockKind, NodeId, TreeError};
use mdcore::Core;

// ---------------------------------------------------------------------------
// 公共工具
// ---------------------------------------------------------------------------

/// 步进时钟（每次 +10s）：各命令独立成条目，不受合并窗干扰。
fn step_clock() -> Arc<dyn Fn() -> u64 + Send + Sync> {
    let tick = std::sync::atomic::AtomicU64::new(1000);
    Arc::new(move || tick.fetch_add(10_000, std::sync::atomic::Ordering::SeqCst) + 10_000)
}

/// 挂载树先序（路径, 槽位）——跳过虚根。
fn mounted_paths(c: &Core) -> Vec<(Vec<usize>, NodeId)> {
    fn walk(c: &Core, nid: NodeId, prefix: Vec<usize>, out: &mut Vec<(Vec<usize>, NodeId)>) {
        out.push((prefix.clone(), nid));
        let n = c.tree.node(nid);
        for (i, &ch) in n.children.iter().enumerate() {
            let mut p = prefix.clone();
            p.push(i);
            walk(c, ch, p, out);
        }
    }
    let mut out = Vec::new();
    for (i, &t) in c.tree.top_level().iter().enumerate() {
        walk(c, t, vec![i], &mut out);
    }
    out
}

/// 先序 ID（跳过虚根）。
fn preorder_ids(c: &Core) -> Vec<u64> {
    mounted_paths(c)
        .iter()
        .map(|(_, slot)| c.tree.node(*slot).id.0)
        .collect()
}

/// I1 + 注册表一致性：活树 ID 唯一；每块 lookup_id 命中自身槽位；
/// id_index 条目数 == 挂载节点数（无泄漏 / 无缺失）。
/// 仅对「无已注册未挂载节点」的状态成立（deep_clone 产物除外，见其专属测试）。
fn assert_id_invariants(c: &Core, ctx: &str) {
    let nodes = mounted_paths(c);
    let mut seen = std::collections::BTreeSet::new();
    for (_, slot) in &nodes {
        let id = c.tree.node(*slot).id;
        assert!(
            seen.insert(id.0),
            "{ctx}: 活树出现重复 BlockId {id:?}（I1 破坏）"
        );
        assert_eq!(
            c.tree.lookup_id(id),
            Some(*slot),
            "{ctx}: id_index 指向不符（id={id:?}）"
        );
    }
    assert_eq!(
        c.tree.mem_stats().id_index_entries,
        nodes.len(),
        "{ctx}: id_index 条目数 != 挂载节点数（注册表泄漏/缺失）"
    );
}

fn md_of(c: &Core) -> String {
    mdcore::markdown::tree_to_markdown(&c.tree)
}

/// 确定性 LCG（种子固定——失败可复现，无 flake）。
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

// ---------------------------------------------------------------------------
// 1. I1：随机操作序列唯一性（验收 #1）
// ---------------------------------------------------------------------------

/// 400 步随机操作（插入/删除/移动/文本/split/merge/undo/redo/save+open），
/// 每步全量断言 I1 + 注册表一致；save+open 步进额外断言 I2（ID/内容/历史
/// 三面无损）。操作合法性由各 API 自行校验（失败 = 跳过该步，不变量断言
/// 不受影响）。
#[test]
fn fuzz_random_ops_keep_id_uniqueness() {
    let mut rng = Rng(0x0244_2026_1106);
    let mut c = Core::from_markdown_with_clock(
        "t0\n\n# H\n\n- L1\n  - L1a\n  - L1b\n\n| a | b |\n| 1 | 2 |\n\n> quote\n\n```rust\nfn x() {}\n```\n\ntail",
        step_clock(),
    );
    assert_id_invariants(&c, "init");

    for round in 0..400 {
        let nodes = mounted_paths(&c);
        let top_len = c.tree.top_level().len();
        let base = c.revision();
        let ctx = format!("round {round}");
        match rng.next() % 10 {
            0 | 1 => {
                // 插入顶层段落（index ∈ [0, top_len]）
                let idx = (rng.next() as usize) % (top_len + 1);
                let _ = c.insert_block(
                    base,
                    vec![],
                    idx,
                    BlockKind::Paragraph,
                    Default::default(),
                    format!("ins-{round}"),
                );
            }
            2 => {
                // 删除：顶层节点或列表项（删后保持 markdown 可表示）
                let cands: Vec<Vec<usize>> = nodes
                    .iter()
                    .filter(|(p, s)| p.len() == 1 || c.tree.node(*s).kind == BlockKind::ListItem)
                    .map(|(p, _)| p.clone())
                    .collect();
                if !cands.is_empty() {
                    let _ =
                        c.delete_block(base, cands[(rng.next() as usize) % cands.len()].clone());
                }
            }
            3 => {
                // 移动到顶层随机位（顶层节点或列表项）
                let cands: Vec<Vec<usize>> = nodes
                    .iter()
                    .filter(|(p, s)| p.len() == 1 || c.tree.node(*s).kind == BlockKind::ListItem)
                    .map(|(p, _)| p.clone())
                    .collect();
                if !cands.is_empty() {
                    let from = cands[(rng.next() as usize) % cands.len()].clone();
                    let idx = (rng.next() as usize) % (top_len + 1);
                    let _ = c.move_block(base, from, vec![], idx);
                }
            }
            4 | 5 => {
                // 文本替换：单行可表示块（段落/标题/列表项/代码块）；
                // 插入串非空 → 不产生空块。
                let cands: Vec<(Vec<usize>, NodeId)> = nodes
                    .iter()
                    .filter(|(_, s)| {
                        matches!(
                            c.tree.node(*s).kind,
                            BlockKind::Paragraph
                                | BlockKind::Heading
                                | BlockKind::ListItem
                                | BlockKind::CodeBlock
                        )
                    })
                    .map(|(p, s)| (p.clone(), *s))
                    .collect();
                if !cands.is_empty() {
                    let (path, slot) = cands[(rng.next() as usize) % cands.len()].clone();
                    let len = c.tree.node(slot).text.len_chars();
                    let start = (rng.next() as usize) % (len + 1);
                    let end = start + (rng.next() as usize) % (len + 1 - start);
                    let _ = c
                        .replace_text(TextTarget::ByPath { path, start, end }, format!("t{round}"));
                }
            }
            6 => {
                // split 顶层段落（len ≥ 2 且切点 1..len-1 → 两侧非空）
                let paras: Vec<(Vec<usize>, NodeId)> = nodes
                    .iter()
                    .filter(|(p, s)| p.len() == 1 && c.tree.node(*s).kind == BlockKind::Paragraph)
                    .map(|(p, s)| (p.clone(), *s))
                    .collect();
                if !paras.is_empty() {
                    let (path, slot) = paras[(rng.next() as usize) % paras.len()].clone();
                    let len = c.tree.node(slot).text.len_chars();
                    if len >= 2 {
                        let _ = c.split_block(base, path, len / 2);
                    }
                }
            }
            7 => {
                // merge 相邻顶层段落对
                let top: Vec<NodeId> = c.tree.top_level().to_vec();
                let mut cands = Vec::new();
                for i in 1..top.len() {
                    let a = c.tree.node(top[i - 1]);
                    let b = c.tree.node(top[i]);
                    if a.kind == BlockKind::Paragraph && b.kind == BlockKind::Paragraph {
                        cands.push(i);
                    }
                }
                if !cands.is_empty() {
                    let i = cands[(rng.next() as usize) % cands.len()];
                    let _ = c.merge_block(base, vec![i]);
                }
            }
            8 => {
                let _ = c.undo();
            }
            9 if rng.next().is_multiple_of(4) => {
                let _ = c.redo();
            }
            _ => {
                // save → open 往返（fuzz 内高频面：I2 + 内容 + 历史）
                let ids_before = preorder_ids(&c);
                let md_before = md_of(&c);
                let depth_before = c.history.undo_depth();
                let (md, meta) = c.save_document();
                let reopened = Core::open_document(&md, &meta).expect("往返 open 必须成功");
                assert_eq!(
                    preorder_ids(&reopened),
                    ids_before,
                    "{ctx}: I2 破坏——往返 ID 漂移"
                );
                assert_eq!(md_of(&reopened), md_before, "{ctx}: 往返内容漂移");
                assert_eq!(
                    reopened.history.undo_depth(),
                    depth_before,
                    "{ctx}: 往返历史丢失"
                );
                c = reopened;
            }
        }
        assert_id_invariants(&c, &ctx);
    }
}

// ---------------------------------------------------------------------------
// 2. I2：嵌套文档往返稳定（验收 #2）
// ---------------------------------------------------------------------------

/// 三层嵌套（列表 P > C1 > G1）+ 标题/段落混合：save → open → 逐块
/// （路径, ID）恒等——身份跟内容走，不跟位置走。
#[test]
fn nested_three_layer_roundtrip_ids_stable() {
    let md = "h\n\n- P\n  - C1\n    - G1\n\nt";
    let c = Core::from_markdown(md);
    let before: Vec<(Vec<usize>, u64)> = mounted_paths(&c)
        .into_iter()
        .map(|(p, s)| (p, c.tree.node(s).id.0))
        .collect();
    // 三层嵌套构型自检：h / P / C1 / G1 / t = 5 块，G1 路径深度 3。
    assert_eq!(before.len(), 5, "三层嵌套构型成立");
    assert_eq!(before[3], (vec![1, 0, 0], 4), "G1 在 P>C1 下第三层");

    let (md2, meta) = c.save_document();
    let reopened = Core::open_document(&md2, &meta).unwrap();
    assert_eq!(md_of(&reopened), md2, "往返内容稳定（规范化形）");
    let after: Vec<(Vec<usize>, u64)> = mounted_paths(&reopened)
        .into_iter()
        .map(|(p, s)| (p, reopened.tree.node(s).id.0))
        .collect();
    assert_eq!(after, before, "I2：save→open 逐块（路径, ID）恒等");
    assert_id_invariants(&reopened, "I2 reopen");
}

// ---------------------------------------------------------------------------
// 3. I3：跨会话 undo/redo 链安全（验收 #3）
// ---------------------------------------------------------------------------

/// 改 A → 改 B → save → open → undo×2 → 唯一 + 内容回原始 → redo×2 →
/// 回链末。修复前该构型与 G4b 同族（重发 ID 与 inverses 物化 ID 撞号）。
#[test]
fn cross_session_undo_redo_chain_content_and_uniqueness() {
    let mut c = Core::from_markdown_with_clock("p1\n\np2\n\np3", step_clock());
    let original = md_of(&c);
    c.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 0,
            end: 0,
        },
        "改A".into(),
    )
    .unwrap();
    c.replace_text(
        TextTarget::ByPath {
            path: vec![1],
            start: 0,
            end: 0,
        },
        "改B".into(),
    )
    .unwrap();
    let edited = md_of(&c);
    assert_ne!(edited, original);

    let (md, meta) = c.save_document();
    let mut reopened = Core::open_document(&md, &meta).unwrap();
    assert_eq!(reopened.history.undo_depth(), 2, "历史栈跨档复原");

    assert!(reopened.undo().is_some(), "跨会话 undo 改B");
    assert!(reopened.undo().is_some(), "跨会话 undo 改A");
    assert_id_invariants(&reopened, "I3 undo×2");
    assert_eq!(md_of(&reopened), original, "undo×2 后内容回原始");

    assert!(reopened.redo().is_some(), "redo 改A");
    assert!(reopened.redo().is_some(), "redo 改B");
    assert_id_invariants(&reopened, "I3 redo×2");
    assert_eq!(md_of(&reopened), edited, "redo×2 后回链末");
}

// ---------------------------------------------------------------------------
// 4. I4/I5：撞号响亮拒绝（验收 #4）——树级 + 事务级两路
// ---------------------------------------------------------------------------

/// 树级：物化 ID 撞在册块 → typed `IdCollision`，失败零副作用
/// （不落槽、不覆盖原注册）。
#[test]
fn register_collision_rejected_typed_tree_level() {
    let mut tree = mdcore::tree::BlockTree::new();
    let a = tree.create_node(BlockKind::Paragraph, "a", Default::default(), Vec::new());
    let a_id = tree.node(a).id;
    assert_eq!(a_id, BlockId(1));

    let r = tree.create_node_with_id(
        BlockKind::Paragraph,
        "ghost",
        Default::default(),
        Vec::new(),
        a_id,
    );
    assert!(
        matches!(&r, Err(TreeError::IdCollision { id, .. }) if *id == a_id),
        "I4：typed IdCollision，got {r:?}"
    );
    assert_eq!(tree.lookup_id(a_id), Some(a), "原注册不被覆盖");
    assert_eq!(tree.node_count(), 1, "失败零副作用（未落槽）");
}

/// 事务级：apply_ops 携物化撞号 ID → 整体 Reject（OpError→Reject 链路
/// 可见 IdCollision），事务备份回滚，树与调用前一致。
#[test]
fn apply_ops_materialized_collision_rejects_and_rolls_back() {
    let mut c = Core::from_markdown("live1\n\nlive2");
    let before_ids = preorder_ids(&c);
    let before_md = md_of(&c);

    let op = Op::InsertBlock {
        parent: vec![],
        index: 0,
        kind: BlockKind::Paragraph,
        attrs: Default::default(),
        text: "ghost".into(),
        id: Some(2), // 撞活树第 2 块
    };
    let r = c.commit_transaction(c.revision(), vec![op]);
    let err = format!("{r:?}");
    assert!(r.is_err(), "I4：撞号必须 Reject");
    assert!(
        err.contains("IdCollision"),
        "I5：typed error 可见（got {err}）"
    );
    assert_eq!(preorder_ids(&c), before_ids, "事务回滚：ID 面零变更");
    assert_eq!(md_of(&c), before_md, "事务回滚：内容零变更");
    assert_id_invariants(&c, "rollback 后");
}

// ---------------------------------------------------------------------------
// 5. F-12：detach 注销 + path_of 不 panic（验收 #5）
// ---------------------------------------------------------------------------

/// detach → 子树整树注销（lookup 失联、path_of 返回 None 而非 assert
/// panic）；重挂载注册恢复；move 后注册表完好（回归）。
#[test]
fn detach_unregisters_and_path_of_is_safe() {
    let mut c = Core::from_markdown("a\n\n- P\n  - C1\n  - C2\n\nb");
    // 目标：顶层列表项 P（审计 F-12 的 pub tree 误用可达构型）
    let (p_slot, p_id, c1_id) = {
        let p_slot = c.tree.resolve(&[1]).expect("P at [1]");
        let p_id = c.tree.node(p_slot).id;
        let c1_id = c.tree.node(c.tree.node(p_slot).children[0]).id;
        (p_slot, p_id, c1_id)
    };

    // 摘除 P 子树
    c.tree.detach(p_slot).expect("detach");
    assert_eq!(c.tree.lookup_id(p_id), None, "F-12：detach 注销子树根");
    assert_eq!(c.tree.lookup_id(c1_id), None, "F-12：注销递归整棵子树");
    assert_eq!(
        c.tree.path_of(p_slot),
        None,
        "F-12：path_of 对已摘除节点返回 None（修复前 assert panic）"
    );
    assert_eq!(
        c.tree.path_of(c.tree.node(p_slot).children[0]),
        None,
        "F-12：子树成员 path_of 同样安全"
    );

    // 重挂载：注册对称恢复（insert_child 侧）
    let root = c.tree.root();
    let top_len = c.tree.top_level().len();
    c.tree.insert_child(root, top_len, p_slot).expect("remount");
    assert_eq!(c.tree.lookup_id(p_id), Some(p_slot), "重挂载恢复注册");
    assert!(c.tree.lookup_id(c1_id).is_some(), "子树整树恢复");
    assert_eq!(c.tree.path_of(p_slot).expect("重挂载后路径可解").len(), 1);

    // move（detach+insert 组合路径）注册表完好回归
    let base = c.revision();
    let p_path_now = c.tree.path_of(p_slot).expect("P 当前路径");
    c.move_block(base, p_path_now, vec![], 0).expect("move");
    assert_id_invariants(&c, "move 后");
}

/// deep_clone 产出「已注册未挂载」子树根：path_of → None（不 panic）——
/// 审计 F-12 的第二个可达面（spike §8.2 copy 路径）。注意：deep_clone
/// 产物在挂载前**有意**留在注册表（宿主可先按新 ID 寻址再挂载），故本
/// 用例不做「注册表条目数 == 挂载节点数」全量断言，只钉 F-12 面。
#[test]
fn deep_clone_unmounted_root_path_of_safe() {
    let mut c = Core::from_markdown("- P\n  - C1");
    let src = c.tree.top_level()[0];
    let data = c.tree.collect_subtree(src);
    let fresh = c.tree.deep_clone(&data);
    let fresh_id = c.tree.node(fresh).id;
    assert_ne!(fresh_id, data.id, "copy 新身份（§8.2）");
    assert_eq!(
        c.tree.lookup_id(fresh_id),
        Some(fresh),
        "未挂载根仍在册（宿主可寻址），但——"
    );
    assert_eq!(
        c.tree.path_of(fresh),
        None,
        "F-12：未挂载根 path_of → None（修复前 assert panic）"
    );
    // 挂载树本体不变量不受未挂载产物影响
    let mounted = mounted_paths(&c);
    let mut seen = std::collections::BTreeSet::new();
    for (_, slot) in &mounted {
        assert!(seen.insert(c.tree.node(*slot).id.0), "挂载树 I1");
    }
}

// ---------------------------------------------------------------------------
// 6. F-2：无 expect 的 ByAnchor 跨 sync 静默改错内容——现状锁
// ---------------------------------------------------------------------------

/// 审计 02-core-state F-2 指认：「没有一个测试演示『锚点跨 sync 后指向
/// 错误内容』的端到端后果」。本测试补上该演示并**锁为现状**（非语义
/// 承诺）：AI 持 (block=2, offset=0)（针对 B 铸造，无 expect）→ 用户
/// sync_from_markdown 全量重同步（中段删除 B）→ diff_forest 按位对齐
/// 使 C 的内容写进 ID 2 的块 → 无 expect 的 ByAnchor 写入**静默改错
/// 内容**（写进 C，而 B 已不存在）。
///
/// 这是「位置 ≠ 身份」族问题（与 P0-1 同根）——#244 路线 A 修的是
/// save→open 的身份稳定性；sync 的按位对齐是**既定设计规则**（r17
/// 已锁），锚点侧的真实修复（偏移平移映射 / 强制 AI 锚点携 expect）
/// 属 R9 / API 演进范围。现状锁确保该后果面始终在档可见。
#[test]
fn anchor_across_sync_silently_retargets_lock() {
    let mut c = Core::from_markdown("A\n\nB\n\nC");
    let b_id = c.tree.node(c.tree.resolve(&[1]).unwrap()).id;
    assert_eq!(b_id, BlockId(2));

    // 铸锚时刻的语义：block=2 即 B。锚点写入 B（start 0 插入）。
    c.replace_text(
        TextTarget::ByAnchor {
            block: b_id,
            start: 0,
            end: 0,
            expect: None,
        },
        "X".into(),
    )
    .unwrap();
    assert_eq!(md_of(&c), "A\nXB\nC", "铸锚后写入 B（正确目标）");

    // 全量重同步（中段删除 B）：diff_forest 按位对齐——[A,XB,C] vs [A,C]
    // → C 的文本写进路径 [1]（ID 2 的块），尾部删除原 [2]。
    c.sync_from_markdown("A\n\nC").unwrap();
    let slot2 = c.tree.lookup_id(BlockId(2)).expect("ID 2 仍在册");
    assert_eq!(
        c.tree.node(slot2).text.to_string(),
        "C",
        "现状锁：sync 按位对齐——C 的内容落进 ID 2（B 的旧身份）"
    );

    // 端到端后果：AI 再用同一锚点写入（无 expect）→ 静默改写 C 的内容。
    c.replace_text(
        TextTarget::ByAnchor {
            block: b_id,
            start: 0,
            end: 0,
            expect: None,
        },
        "Y".into(),
    )
    .unwrap();
    assert_eq!(
        md_of(&c),
        "A\nYC",
        "现状锁（非语义承诺）：无 expect 的 ByAnchor 跨 sync 静默改写被顶位\
         的内容（Y 写进了 C，而非不存在的 B）——F-2，待 R9 偏移平移映射"
    );

    // 对照分支：expect 保护（既有缓解）——同锚点带 expect=B → 响亮拒绝。
    let r = c.replace_text(
        TextTarget::ByAnchor {
            block: b_id,
            start: 0,
            end: 0,
            expect: Some("B".into()),
        },
        "Z".into(),
    );
    assert!(r.is_err(), "expect 不匹配 → 拒绝（乐观并发保护，§11.3）");
}

// ---------------------------------------------------------------------------
// 7. meta 边界：blocks 表三路（恢复 / 降级 / 拒收）+ 前向保护
// ---------------------------------------------------------------------------

/// 数量失配（正文被外部改动）：降级开档——先序重发 + 丢弃历史（旧结构
/// 路径/物化 ID 不可信，照常恢复会静默写错位置）。
#[test]
fn blocks_count_mismatch_degrades_open() {
    let mut c = Core::from_markdown_with_clock("a\n\nb\n\nc", step_clock());
    c.insert_block(
        c.revision(),
        vec![],
        1,
        BlockKind::Paragraph,
        Default::default(),
        "mid".to_string(),
    )
    .unwrap();
    assert_eq!(c.history.undo_depth(), 1);

    let (md, meta) = c.save_document();
    let mut v: serde_json::Value = serde_json::from_str(&meta).unwrap();
    v["blocks"].as_array_mut().unwrap().pop(); // 表尾部少一个 → 数量失配
    let opened =
        Core::open_document(&md, &v.to_string()).expect("数量失配 = 降级开档，不拒收（正文优先）");
    assert_eq!(preorder_ids(&opened), vec![1, 2, 3, 4], "先序重发");
    assert_eq!(
        opened.history.undo_depth(),
        0,
        "历史丢弃（旧结构路径不可信）"
    );
    assert_id_invariants(&opened, "降级开档后");
}

/// 表内重复 ID：畸形 meta → 显式 Reject（不得静默开档）。
#[test]
fn blocks_table_duplicate_rejects() {
    let c = Core::from_markdown("a\n\nb");
    let (md, meta) = c.save_document();
    let mut v: serde_json::Value = serde_json::from_str(&meta).unwrap();
    v["blocks"] = serde_json::json!([1, 1, 2]);
    assert!(
        Core::open_document(&md, &v.to_string()).is_err(),
        "blocks 表重复 ID = 畸形 meta，显式 Reject"
    );
}

/// meta_format 存在但非数字：畸形 meta → 显式 Reject。
#[test]
fn meta_format_non_numeric_rejects() {
    let c = Core::from_markdown("a");
    let (md, meta) = c.save_document();
    let mut v: serde_json::Value = serde_json::from_str(&meta).unwrap();
    v["meta_format"] = serde_json::json!("2");
    assert!(
        Core::open_document(&md, &v.to_string()).is_err(),
        "meta_format 非数字 = 畸形 meta，显式 Reject"
    );
}

/// 未来 meta 版本：blocks 表语义未知 → 不解读（先序重发，撞号面由守卫
/// 拦截）；其余字段照常（history 自身版本闸门独立放行）——前向保护。
#[test]
fn future_meta_format_ignores_blocks_keeps_history() {
    let mut c = Core::from_markdown_with_clock("a\n\nb", step_clock());
    c.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 0,
            end: 0,
        },
        "x".into(),
    )
    .unwrap();
    let (md, meta) = c.save_document();
    let mut v: serde_json::Value = serde_json::from_str(&meta).unwrap();
    v["meta_format"] = serde_json::json!(99); // 未来版本
    let opened = Core::open_document(&md, &v.to_string()).unwrap();
    assert_eq!(
        preorder_ids(&opened),
        vec![1, 2],
        "未来版本：blocks 不解读，先序重发"
    );
    assert_eq!(
        opened.history.undo_depth(),
        1,
        "history 自身 v2 闸门照常放行（与 meta 版本独立）"
    );
}

/// 二次往返：blocks 表恒等（save → open → save 不漂移）。
#[test]
fn double_roundtrip_blocks_table_stable() {
    let mut c = Core::from_markdown_with_clock("a\n\n- P\n  - C1", step_clock());
    c.insert_block(
        c.revision(),
        vec![],
        0,
        BlockKind::Paragraph,
        Default::default(),
        "top".to_string(),
    )
    .unwrap();
    let (md1, meta1) = c.save_document();
    let reopened = Core::open_document(&md1, &meta1).unwrap();
    let (md2, meta2) = reopened.save_document();
    let v1: serde_json::Value = serde_json::from_str(&meta1).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&meta2).unwrap();
    assert_eq!(v1["blocks"], v2["blocks"], "二次往返 blocks 表恒等");
    assert_eq!(md1, md2, "二次往返正文恒等");
    assert_eq!(
        v1["blocks"],
        serde_json::json!([4, 1, 2, 3]),
        "blocks = 活树先序（top=4, a=1, P=2, C1=3）"
    );
}

/// 常规编辑管线回归：身份层修复后事件流/日志/undo 行为不变，I1 每步成立。
#[test]
fn regular_edit_pipeline_keeps_invariants() {
    let mut c = Core::from_markdown_with_clock("one\n\ntwo", step_clock());
    for i in 0..10 {
        let base = c.revision();
        let _ = c.replace_text(
            TextTarget::ByPath {
                path: vec![i % 2],
                start: 0,
                end: 0,
            },
            format!("n{i}"),
        );
        let _ = c.insert_block(
            base,
            vec![],
            0,
            BlockKind::Paragraph,
            Default::default(),
            format!("blk{i}"),
        );
        if i % 3 == 0 {
            let _ = c.undo();
        }
        c.take_events(); // 事件面消费
        assert_id_invariants(&c, &format!("pipeline step {i}"));
    }
}
