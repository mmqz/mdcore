//! PDF-d — 标注面常开锁（会话 flush 语义 + 写抽象契约 + Acrobat 互通
//! 纯函数约定；零引擎依赖，默认构建可测）。
//!
//! 锁清单（每锁一条 `#[test]`，断言消息讲清**为什么**）：
//! 1. `writer_trait_contract_mock_stats`——[`AnnotationWriter`] 契约面
//!    （`Box<dyn>` 装箱调度）+ [`MockWriter`] 统计模型确定性
//!    （追加 = 8 + 32×n）——flush 断言的标定基础；
//! 2. `flush_writes_dirty_in_id_order_then_idempotent`——flush 逐条写
//!    （脏集升序 = 与 AnnotateList id 升序同源）+ 成功清脏 + 二次 flush
//!    零追加零新写（幂等：bytes_appended 最小化）；
//! 3. `dirty_set_precision_add_edit_noop`——脏集合精确性：flush 后
//!    新增 / 实际编辑 / 未动 / no-op 编辑四类分别处理（脏 = 内容面
//!    变更，非命令次数）；
//! 4. `flush_s5_encrypted_denied_before_idle_shortcircuit`——S5 加密态
//!    flush 拒（`SecurityDeny` **先于一切**：本锁以「零脏 + 加密 → 拒
//!    而非 Ok」区分 S5 门与零脏短路的次序——若无 S5 门在前，零脏必
//!    返回 Ok）；
//! 5. `flush_without_writer_dirty_errors_zero_dirty_ok`——无写器错误面：
//!    有脏 → `Engine`（typed，不 panic）；零脏 → 幂等 Ok（双 0 常数 =
//!    未落盘会话的诚实占位）；
//! 6. `flush_save_failure_retry_safe`——保存失败重试安全：dirty 原样
//!    保留（重试重写全量脏集 = 写器幂等合同——同 id 重写 = 原位替换）；
//! 7. `writer_requires_no_engine`——写器与引擎独立装配（无引擎会话可
//!    落盘标注：attach_writer 不要求 attach_engine）；
//! 8. `origin_author_codec_roundtrip`——/T origin 编码约定：Human 原文
//!    / Ai `" [ai]"` 后缀，可逆还原；后缀词面冻结；文档化歧义钉行为；
//! 9. `pdf_date_string_known_values`——/M 日期串纯函数（值时钟语义）：
//!    epoch 0 / 已知时刻 / 闰日 / 世纪非闰年（2100）+ 形状恒 `D:` +
//!    14 位数字 + `+00'00'`。

use std::sync::{Arc, Mutex};

use mdc_pdf::annotation::{
    AnnotId, Annotation, AnnotationDraft, AnnotationKind, AnnotationPatch, Origin, Point, Quad,
    Rect, Rgb,
};
use mdc_pdf::command::{Command, CommandError, Outcome, Source};
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageCount, PageGeometry, PageIndex, Rotation};
use mdc_pdf::security::{OpenAssessment, REASON_ENCRYPTED_WRITE};
use mdc_pdf::writer::{
    decode_author, encode_author, pdf_date_string, AnnotationWriter, IncrementalSave, MockWriter,
    ORIGIN_SUFFIX_AI,
};

// ---------------------------------------------------------------------------
// 装配
// ---------------------------------------------------------------------------

fn geometry() -> PageGeometry {
    PageGeometry::simple(612.0, 792.0, Rotation::Deg0)
}

/// 装配会话（page_count 页；时钟注入使时间戳确定）。
fn session(page_count: u32, encrypted: bool) -> PdfSession {
    let mut s = PdfSession::open(
        DocumentMeta {
            id: PdfDocumentId(1),
            file_name: "test.pdf".into(),
            page_count: PageCount(page_count),
        },
        OpenAssessment {
            encrypted,
            warnings: vec![],
            form_probe: None,
        },
        vec![geometry(); page_count as usize],
        OutlineTree::new(),
    )
    .expect("会话装配自洽");
    s.set_clock_ms(1_700_000_000_000);
    s
}

fn highlight_kind() -> AnnotationKind {
    let quad = |x0: f32, y0: f32, x1: f32, y1: f32| Quad {
        points: [
            Point { x: x0, y: y1 },
            Point { x: x1, y: y1 },
            Point { x: x0, y: y0 },
            Point { x: x1, y: y0 },
        ],
    };
    AnnotationKind::Highlight {
        quads: vec![quad(72.0, 700.0, 300.0, 716.0)],
        color: Rgb {
            r: 255,
            g: 255,
            b: 0,
        },
    }
}

fn add_highlight(s: &mut PdfSession, page: PageIndex) -> AnnotId {
    let Outcome::AnnotAdded(a) = s
        .execute(Command::AnnotateAdd {
            draft: AnnotationDraft {
                page,
                kind: highlight_kind(),
                author: "tester".into(),
                origin: Origin::Human,
            },
            source: Source::View,
        })
        .expect("AnnotateAdd 成功")
    else {
        unreachable!("AnnotateAdd 载荷形状");
    };
    a.id
}

/// 会话标注库快照（观测面 = AnnotateList 命令——命令面即观测面）。
fn annots(s: &mut PdfSession) -> Vec<Annotation> {
    let Outcome::AnnotListed(list) = s
        .execute(Command::AnnotateList {
            filter: Default::default(),
            source: Source::System,
        })
        .expect("AnnotateList 成功")
    else {
        unreachable!("AnnotateList 载荷形状");
    };
    list
}

// ---------------------------------------------------------------------------
// 测试面观测写器：TapWriter（trait 是常开公共面——测试自带实现合法；
// MockWriter 走直接使用面（锁 1），会话内观测走本包装器）
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Tap {
    writes: Vec<(PageIndex, AnnotId)>,
    saves: usize,
    fail_next: bool,
    pending: usize,
    product: u64,
}

impl Tap {
    fn appended(&self, n: usize) -> u64 {
        if n == 0 {
            0
        } else {
            8 + 32 * n as u64
        }
    }
}

/// 观测写器：会话持有 Box 形态，测试经 Arc 保留观测柄。
struct TapWriter(Arc<Mutex<Tap>>);

impl TapWriter {
    fn new() -> (Self, Arc<Mutex<Tap>>) {
        let tap = Arc::new(Mutex::new(Tap::default()));
        (Self(tap.clone()), tap)
    }
}

impl AnnotationWriter for TapWriter {
    fn write_annotation(
        &mut self,
        page: PageIndex,
        annot: &Annotation,
    ) -> Result<(), CommandError> {
        let mut t = self.0.lock().unwrap();
        t.writes.push((page, annot.id));
        t.pending += 1;
        Ok(())
    }

    fn product_bytes(&self) -> Vec<u8> {
        Vec::new() // 观测面无产物字节——统计断言走 Tap 计数
    }

    fn save_incremental(&mut self) -> Result<IncrementalSave, CommandError> {
        let mut t = self.0.lock().unwrap();
        t.saves += 1;
        if t.fail_next {
            t.fail_next = false;
            return Err(CommandError::engine("tap 保存失败（注入）"));
        }
        let bytes_appended = t.appended(t.pending);
        let prefix_bytes = t.product;
        t.product += bytes_appended;
        t.pending = 0;
        Ok(IncrementalSave {
            bytes_appended,
            prefix_bytes,
        })
    }
}

// ---------------------------------------------------------------------------
// 1-2：写抽象契约 + flush 基本语义
// ---------------------------------------------------------------------------

#[test]
fn writer_trait_contract_mock_stats() {
    // 契约面标定：MockWriter 经 Box<dyn AnnotationWriter> 装箱调度（会话
    // 持有形态）+ 统计模型确定性（追加 = 8 + 32×n）——后续 flush 断言
    // 以本模型为标定基础；零待保存 → 零追加（与 LopdfWriter 同合同）。
    let mut writer: Box<dyn AnnotationWriter> = Box::new(MockWriter::new());
    let a = Annotation {
        id: AnnotId(7),
        page: PageIndex(0),
        kind: highlight_kind(),
        author: "tester".into(),
        origin: Origin::Human,
        created_ms: 1,
        modified_ms: 1,
    };
    writer
        .write_annotation(PageIndex(0), &a)
        .expect("write via trait");
    writer
        .write_annotation(PageIndex(0), &a)
        .expect("重复写（幂等合同：同 id 原位替换）");
    let stats = writer.save_incremental().expect("save via trait");
    assert_eq!(
        stats,
        IncrementalSave {
            bytes_appended: 8 + 32 * 2,
            prefix_bytes: 0,
        },
        "Mock 字节模型确定性：追加 = 8 + 32×待保存数，prefix 从 0 起算"
    );
    let idle = writer.save_incremental().expect("idle save");
    assert_eq!(idle.bytes_appended, 0, "零待保存保存不追加");
    assert_eq!(idle.prefix_bytes, 8 + 32 * 2, "prefix = 已累计产物长度");
}

#[test]
fn flush_writes_dirty_in_id_order_then_idempotent() {
    // flush 逐条写（脏集升序——BTreeSet 键序，与 AnnotateList 的 id
    // 升序纪律同源）+ 成功清脏 + 二次 flush 零追加零新写。
    let (tap_writer, tap) = TapWriter::new();
    let mut s = session(3, false);
    s.attach_writer(Box::new(tap_writer));
    let a1 = add_highlight(&mut s, PageIndex(0));
    let a2 = add_highlight(&mut s, PageIndex(1));
    let stats = s.flush_annotations().expect("首次 flush");
    assert_eq!(stats.bytes_appended, 8 + 32 * 2, "两条脏标注全部落盘");
    assert_eq!(
        tap.lock().unwrap().writes,
        vec![(PageIndex(0), a1), (PageIndex(1), a2)],
        "写入序 = id 升序（确定性——同库同序）+ 页号随标注走"
    );
    // 幂等：二次 flush 零脏 → 不追加字节、零新写入。
    let before = tap.lock().unwrap().writes.len();
    let idle = s.flush_annotations().expect("二次 flush（幂等）");
    assert_eq!(
        idle.bytes_appended, 0,
        "零脏 flush 不追加（bytes_appended 最小化）"
    );
    assert_eq!(
        idle.prefix_bytes,
        stats.prefix_bytes + stats.bytes_appended,
        "prefix 基线推进到已落盘产物长度"
    );
    assert_eq!(
        tap.lock().unwrap().writes.len(),
        before,
        "幂等 flush 零新写入"
    );
}

// ---------------------------------------------------------------------------
// 3-6：脏集合精确性 + S5 先于一切 + 无写器面 + 重试安全
// ---------------------------------------------------------------------------

#[test]
fn dirty_set_precision_add_edit_noop() {
    // 脏集合精确性：首轮 flush（A/B 落盘）后——新增 C 入脏 / 实际编辑
    // A 重标脏 / 未动的 B 不脏 / no-op 编辑 B 不脏（脏 = 内容面变更，
    // 非命令次数——与 modified_ms 不推进同语义）。观测 = 二轮 flush 的
    // 写入恰 {A, C}：误脏 B 则出现第三条写入。
    let (tap_writer, tap) = TapWriter::new();
    let mut s = session(3, false);
    s.attach_writer(Box::new(tap_writer));
    let a = add_highlight(&mut s, PageIndex(0));
    let b = add_highlight(&mut s, PageIndex(0));
    s.flush_annotations().expect("首轮 flush 清脏");
    tap.lock().unwrap().writes.clear(); // 观测窗口重置：只看二轮

    // 实际编辑 A（Highlight 可变面 = color）。
    s.execute(Command::AnnotateEdit {
        id: a,
        patch: AnnotationPatch {
            color: Some(Rgb { r: 1, g: 2, b: 3 }),
            ..Default::default()
        },
        source: Source::View,
    })
    .expect("实际编辑 A");
    // no-op 编辑 B（同值 color——内容无变更，不脏）。
    let b_color = match &annots(&mut s).iter().find(|x| x.id == b).unwrap().kind {
        AnnotationKind::Highlight { color, .. } => *color,
        _ => unreachable!("B 是 Highlight"),
    };
    s.execute(Command::AnnotateEdit {
        id: b,
        patch: AnnotationPatch {
            color: Some(b_color),
            ..Default::default()
        },
        source: Source::View,
    })
    .expect("no-op 编辑 B");
    // 新增 C（入脏）。
    let c = add_highlight(&mut s, PageIndex(1));

    let stats = s.flush_annotations().expect("二轮 flush");
    assert_eq!(
        stats.bytes_appended,
        8 + 32 * 2,
        "脏集恰 = A(实际编辑)+C(新增)——B 未动与 no-op 编辑不入脏"
    );
    assert_eq!(
        tap.lock().unwrap().writes,
        vec![(PageIndex(0), a), (PageIndex(1), c)],
        "二轮写入恰 {{A, C}}（升序；多一条即脏集合不精确）"
    );
}

#[test]
fn flush_s5_encrypted_denied_before_idle_shortcircuit() {
    // S5 先于一切：加密态 flush 拒（SecurityDeny + 稳定理由码）。次序
    // 证明 = 「零脏 + 加密 → 拒而非 Ok」：加密会话的 AnnotateAdd/Edit
    // 本身已被 S5 拒（脏集不可达非空），若无 S5 门在最前，零脏路径必
    // 返回 Ok——本断言即门与短路的判别器。写器在位（Tap）时写路径
    // 零接触：本锁后段以「未收到任何写入」钉死。
    let (tap_writer, tap) = TapWriter::new();
    let mut s = session(3, true); // encrypted = true
    s.attach_writer(Box::new(tap_writer));
    let err = s
        .flush_annotations()
        .expect_err("加密态 flush 必拒（含零脏态）");
    assert_eq!(
        err,
        CommandError::SecurityDeny {
            reason: REASON_ENCRYPTED_WRITE
        },
        "S5 拒绝 + 稳定理由码（Ok 即 = 门禁失效/次序漂移）"
    );
    assert!(
        tap.lock().unwrap().writes.is_empty(),
        "写路径零接触（拒绝先于写器调度）"
    );
    assert_eq!(tap.lock().unwrap().saves, 0, "保存路径零接触");
}

#[test]
fn flush_without_writer_dirty_errors_zero_dirty_ok() {
    // 无写器：有脏 → Engine（typed 错误，不 panic，消息指向装配出口）；
    // 零脏 → 幂等 Ok（双 0 常数 = 未落盘会话的诚实占位，非伪造长度）。
    let mut s = session(3, false);
    add_highlight(&mut s, PageIndex(0));
    match s.flush_annotations() {
        Err(CommandError::Engine(detail)) => {
            assert!(
                detail.contains("attach_writer"),
                "错误消息指向装配出口：{detail}"
            );
        }
        other => panic!("有脏 + 无写器应 Engine 错误，得 {other:?}"),
    }
    let mut idle = session(3, false);
    let stats = idle.flush_annotations().expect("零脏 + 无写器 → 幂等 Ok");
    assert_eq!(
        stats,
        IncrementalSave {
            bytes_appended: 0,
            prefix_bytes: 0,
        },
        "零脏 + 无写器 = 无产物面（双 0 诚实占位）"
    );
}

#[test]
fn flush_save_failure_retry_safe() {
    // 保存失败重试安全：失败 → dirty 原样保留。可观测判据 = 重试
    // flush 产生**非零追加**且写入数 = 全量脏集重写（若失败时误清脏，
    // 重试为幂等空转 → bytes_appended 0——本断言即判别器）。
    let (tap_writer, tap) = TapWriter::new();
    let mut s = session(3, false);
    s.attach_writer(Box::new(tap_writer));
    add_highlight(&mut s, PageIndex(0));
    tap.lock().unwrap().fail_next = true;
    let err = s.flush_annotations().expect_err("注入的保存失败必传播");
    assert!(
        matches!(err, CommandError::Engine(_)),
        "保存失败 = typed Engine 错误（得 {err:?}）"
    );
    assert_eq!(
        tap.lock().unwrap().saves,
        1,
        "失败也计入保存次数（含失败次）"
    );
    // dirty 保留 → 重试重写全量（Tap 计数 = 首写 + 重写共 2 次写入；
    // 真写器同构：失败修订里的对象留位，重写 = 同 id 原位替换）。
    let stats = s.flush_annotations().expect("重试 flush 成功");
    assert_eq!(
        stats.bytes_appended,
        8 + 32 * 2,
        "重试非幂等空转 = dirty 在失败时被保留（写器幂等合同：同 id 重写 = 原位替换）"
    );
    assert_eq!(
        tap.lock().unwrap().writes.len(),
        2,
        "两轮各写 1 次（首写 + 重写）"
    );
    // 成功后清脏 → 三次 flush 零追加。
    let idle = s.flush_annotations().expect("清脏后 flush");
    assert_eq!(idle.bytes_appended, 0, "成功清脏后的幂等 flush");
}

// ---------------------------------------------------------------------------
// 7：写器与引擎独立装配
// ---------------------------------------------------------------------------

#[test]
fn writer_requires_no_engine() {
    // attach_writer 不要求 attach_engine（无引擎会话可落盘标注——阅读
    // 与落盘两个能力面互不为前提）。
    let mut s = session(2, false);
    assert!(!s.engine_attached(), "测试前提：无引擎会话");
    s.attach_writer(Box::new(MockWriter::new()));
    assert!(s.writer_attached(), "写器在位观测");
    add_highlight(&mut s, PageIndex(0));
    let stats = s.flush_annotations().expect("无引擎会话 flush 照常");
    assert!(stats.bytes_appended > 0, "写器不依赖引擎在位");
}

// ---------------------------------------------------------------------------
// 8-9：Acrobat 互通纯函数约定（/T origin 编码 + /M 日期串）
// ---------------------------------------------------------------------------

#[test]
fn origin_author_codec_roundtrip() {
    // /T origin 编码约定（PDF-d 定稿）：Human = 原文；Ai = " [ai]" 后缀
    // （可逆且 Acrobat 作者栏可读）。已知歧义在档并钉为文档化行为：
    // Human 名字面恰以 " [ai]" 结尾时解码侧判 Ai——解码仅用于自家回读
    // 对账，写侧权威在模型（库内 origin 字段才是权威）。
    assert_eq!(ORIGIN_SUFFIX_AI, " [ai]", "后缀词面冻结（协议面锚）");
    assert_eq!(encode_author("alice", Origin::Human), "alice");
    assert_eq!(encode_author("bob", Origin::Ai), "bob [ai]");
    assert_eq!(encode_author("中文作者", Origin::Ai), "中文作者 [ai]");
    // 可逆还原。
    assert_eq!(decode_author("alice"), ("alice", Origin::Human));
    assert_eq!(decode_author("bob [ai]"), ("bob", Origin::Ai));
    assert_eq!(decode_author("中文作者 [ai]"), ("中文作者", Origin::Ai));
    // 空作者面。
    assert_eq!(encode_author("", Origin::Ai), " [ai]");
    assert_eq!(decode_author(" [ai]"), ("", Origin::Ai));
    // 文档化歧义行为钉死（在档不修）。
    assert_eq!(decode_author("eve [ai]"), ("eve", Origin::Ai));
    // Rect 类型面借用（本文件装配使用，防 unused-import 误删）。
    let _ = Rect::new(0.0, 0.0, 1.0, 1.0).unwrap();
}

#[test]
fn pdf_date_string_known_values() {
    // /M 日期串纯函数（值时钟语义——同值时钟任何环境同一字符串）：
    // 格式 `D:YYYYMMDDHHmmSS+00'00'`（UTC 固定偏移，无系统时区依赖）。
    assert_eq!(
        pdf_date_string(0),
        "D:19700101000000+00'00'",
        "epoch 0 = UTC 起点"
    );
    // 2023-11-14T22:13:20Z（1_700_000_000 s——与探针同一校准值）。
    assert_eq!(
        pdf_date_string(1_700_000_000_000),
        "D:20231114221320+00'00'",
        "已知时刻"
    );
    // 闰日：2024-02-29（1_709_164_800 s = 54 年 + 31 + 28 天）。
    assert_eq!(
        pdf_date_string(1_709_164_800_000),
        "D:20240229000000+00'00'",
        "闰日正确（4 年规则）"
    );
    // 世纪非闰年：2100-01-01（4_102_444_800 s——2100 被 100 整除不被
    // 400 整除，不闰）。
    assert_eq!(
        pdf_date_string(4_102_444_800_000),
        "D:21000101000000+00'00'",
        "世纪年非闰（2100 不闰；2104 才闰）"
    );
    // 形状恒定：D: + 14 位 ASCII 数字 + "+00'00'"（总长 21）。
    for ms in [0u64, 1, 999, 1_700_000_000_123, u32::MAX as u64 * 1000] {
        let s = pdf_date_string(ms);
        assert_eq!(s.len(), 23, "形状总长恒 23（{ms} ms → {s}）");
        let body = s
            .strip_prefix("D:")
            .and_then(|b| b.strip_suffix("+00'00'"))
            .unwrap_or_else(|| panic!("形状拆解失败：{s}"));
        assert!(
            body.len() == 14 && body.bytes().all(|c| c.is_ascii_digit()),
            "14 位纯数字时刻面（{ms} ms → {s}）"
        );
    }
    // 毫秒截断（非舍入）：123ms 与 0ms 同秒——值时钟语义，无隐藏进位。
    assert_eq!(
        pdf_date_string(1_700_000_000_123),
        pdf_date_string(1_700_000_000_000),
        "毫秒面截断"
    );
}
