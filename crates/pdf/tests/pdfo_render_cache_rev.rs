//! PDF-o — #305：渲染缓存标注失效语义锁（常开：零引擎依赖，mock 可测）。
//!
//! 缺陷形态（issue 原话）：「渲染 `render_annotations(true)`——缓存位图
//! 含标注，flush 落盘新标注后缓存即陈旧，而 crate 无任何标注变更→缓存
//! 失效 API」。用户面后果：加标注 → 保存 → 屏幕还是旧位图 → 用户以为
//! 「加标注没生效」。
//!
//! 修法双轨（issue 倾向「A + B 都提供」）：
//! - **方案 A 键控**：[`RenderKey`]（page/width/height/annotation_rev）
//!   与 rev 源头（[`PdfSession::annotation_rev`]——flush 成功且实际
//!   追加字节时 +1）。rev 入键 → flush 后旧键自然 miss；
//! - **方案 B 显式失效**：[`PageCache`] 泛型面 `invalidate` /
//!   `invalidate_matching` + `PageCache<RenderKey, V>` PDF 感知粒度
//!   （`invalidate_page` / `invalidate_stale_revisions` / `invalidate_all`）
//!   ——键控负责「不再服役」，失效负责「预算即刻回收」（旧 rev 条目
//!   不失效则占预算直到 LRU 驱逐）。
//!
//! 锁清单（每锁一条 `#[test]`，断言消息讲清**为什么**）：
//! 1. `flush_bumps_rev_only_when_bytes_appended`——rev 语义：零脏幂等
//!    flush 不 bump（位图没变，缓存不得 miss——命中率面）；有脏 flush
//!    （追加 > 0）+1；再零脏 flush 不动。修复前无 rev 面，本锁即缺口；
//! 2. `flush_failure_keeps_rev`——保存失败不 bump（产物未变——失败
//!    flush 不该让缓存全体 miss）；
//! 3. `stale_rev_key_misses_after_flush`——验收 4 主锁：flush 前旧键
//!    命中；flush（rev+1）后**新键 miss + 旧键内容仍在位**（键控 =
//!    不服役不销毁，销毁走显式失效）；
//! 4. `re_render_after_flush_refills_and_hits`——端到端（mock 光栅化
//!    器计数）：flush → invalidate_stale_revisions 回收 → 重渲染**恰好
//!    一轮**（每页一次，不多不少）→ 新键命中 → 后续取零 rasterize
//!    （验收 1/2/3 的 mock 化：flush 后渲染不再是旧位图 + 命中率不降）；
//! 5. `invalidate_stale_keeps_current_rev_entries`——粒度锁：混合 rev
//!    条目下只摘旧的，当前 rev 全在位（跨 rev 共存的真实形态：flush
//!    后翻页回看未重渲染页）；
//! 6. `invalidate_page_is_scoped`——页级粒度：只摘该页条目，他页命中
//!    零扰动；
//! 7. `invalidate_all_returns_keys_lru_to_mru`——全量粒度 + 返回序
//!    （LRU→MRU，与 put 驱逐键集同款载荷——调用方按序作废关联资源）；
//! 8. `render_key_discriminates_all_factors`——键判别：页/宽/高/rev
//!    任一因子不同即不同键（语义合同「key 相等 ⟺ 光栅输出可复用」的
//!    因子完备性）；
//! 9. `generic_invalidate_returns_value_and_is_idempotent`——泛型面：
//!    单键失效返回值（调用方作废关联资源的载荷）、幂等（重复失效
//!    None 不报错）；
//! 10. `generic_invalidate_matching_keeps_budget_and_order`——泛型面
//!     谓词失效只摘命中、预算同步回收、返回序 = LRU→MRU；
//! 11. [feature `engine-lopdf`] `annotator_rev_matches_session_contract`
//!     ——[`PdfAnnotator::annotation_rev`] 与会话同款合同（save 成功且
//!     追加 > 0 时 +1，幂等 save 不 bump）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use mdc_pdf::cache::PageCache;
use mdc_pdf::command::{Command, CommandError, Outcome, Source};
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageCount, PageGeometry, PageIndex, Rotation};
use mdc_pdf::render::{Rasterizer, RenderKey, RenderRequest, RenderedPage};
use mdc_pdf::security::OpenAssessment;
use mdc_pdf::writer::{AnnotationWriter, IncrementalSave, MockWriter};

// ---------------------------------------------------------------------------
// 装配（与 pdfd_session_flush 同款纪律）
// ---------------------------------------------------------------------------

fn geometry() -> PageGeometry {
    // #304：PageGeometry 加 media_box/crop_box 后改用 simple() 构造器
    // （media_box = (0,0,w,h)，crop_box = None = 显示域取 MediaBox）
    PageGeometry::simple(612.0, 792.0, Rotation::Deg0)
}

fn session(page_count: u32) -> PdfSession {
    let mut s = PdfSession::open(
        DocumentMeta {
            id: PdfDocumentId(1),
            file_name: "test.pdf".into(),
            page_count: PageCount(page_count),
        },
        OpenAssessment {
            encrypted: false,
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

fn highlight_kind() -> mdc_pdf::annotation::AnnotationKind {
    use mdc_pdf::annotation::{AnnotationKind, Point, Quad, Rgb};
    let quad = Quad {
        points: [
            Point { x: 72.0, y: 716.0 },
            Point { x: 300.0, y: 716.0 },
            Point { x: 72.0, y: 700.0 },
            Point { x: 300.0, y: 700.0 },
        ],
    };
    AnnotationKind::Highlight {
        quads: vec![quad],
        color: Rgb {
            r: 255,
            g: 255,
            b: 0,
        },
    }
}

fn add_highlight(s: &mut PdfSession, page: PageIndex) {
    let Outcome::AnnotAdded(_) = s
        .execute(Command::AnnotateAdd {
            draft: mdc_pdf::annotation::AnnotationDraft {
                page,
                kind: highlight_kind(),
                author: "tester".into(),
                origin: mdc_pdf::annotation::Origin::Human,
            },
            source: Source::View,
        })
        .expect("AnnotateAdd 成功")
    else {
        unreachable!("AnnotateAdd 载荷形状")
    };
}

/// 计数光栅化器（mock）：每次 rasterize 计数 +1（调用面观测——
/// 命中率锁的判据：缓存命中 = 零 rasterize）。
struct CountingRaster {
    calls: Arc<AtomicUsize>,
}

impl Rasterizer for CountingRaster {
    fn rasterize(&mut self, _request: RenderRequest) -> Result<RenderedPage, CommandError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(RenderedPage {
            width_px: 640,
            height_px: 828,
            bgra: vec![0u8; 640 * 828 * 4],
        })
    }
}

// ---------------------------------------------------------------------------
// 1-2：rev 源头语义（方案 A 的键控源头）
// ---------------------------------------------------------------------------

#[test]
fn flush_bumps_rev_only_when_bytes_appended() {
    let mut s = session(3);
    s.attach_writer(Box::new(MockWriter::new()));

    // 初始 rev = 0。
    assert_eq!(s.annotation_rev(), 0, "打开即 0");

    // 零脏幂等 flush：零追加 → 不 bump（位图没变——缓存不该 miss）。
    let idle = s.flush_annotations().expect("零脏 flush");
    assert_eq!(idle.bytes_appended, 0);
    assert_eq!(
        s.annotation_rev(),
        0,
        "零追加幂等 flush 不推进 rev（命中率面：空转不得全体 miss）"
    );

    // 有脏 flush：MockWriter 追加 8+32×1 > 0 → +1。
    add_highlight(&mut s, PageIndex(0));
    let stats = s.flush_annotations().expect("有脏 flush");
    assert!(stats.bytes_appended > 0, "MockWriter 有脏必追加");
    assert_eq!(
        s.annotation_rev(),
        1,
        "产物字节真变了 → rev +1（渲染输出身份变更）"
    );

    // 清脏后再幂等 flush：不动。
    let idle2 = s.flush_annotations().expect("清脏后 flush");
    assert_eq!(idle2.bytes_appended, 0);
    assert_eq!(s.annotation_rev(), 1, "无内容变更不推进");

    // 二次有脏 flush：单调推进（rev 是身份序，不回退不复用）。
    add_highlight(&mut s, PageIndex(1));
    s.flush_annotations().expect("二次有脏 flush");
    assert_eq!(s.annotation_rev(), 2, "单调递增");
}

#[test]
fn flush_failure_keeps_rev() {
    // TapWriter（pdfd 同款）：观测柄留在测试手上的包装写器。
    struct TapWriter(Arc<std::sync::Mutex<Tap>>);
    #[derive(Default)]
    struct Tap {
        fail_next: bool,
        pending: usize,
    }
    impl AnnotationWriter for TapWriter {
        fn write_annotation(
            &mut self,
            _page: PageIndex,
            _annot: &mdc_pdf::annotation::Annotation,
        ) -> Result<(), CommandError> {
            self.0.lock().unwrap().pending += 1;
            Ok(())
        }
        fn save_incremental(&mut self) -> Result<IncrementalSave, CommandError> {
            let mut t = self.0.lock().unwrap();
            if t.fail_next {
                t.fail_next = false;
                return Err(CommandError::engine("mock 保存失败（注入）"));
            }
            let appended = if t.pending == 0 {
                0
            } else {
                8 + 32 * t.pending as u64
            };
            t.pending = 0;
            Ok(IncrementalSave {
                bytes_appended: appended,
                prefix_bytes: 0,
            })
        }
        fn product_bytes(&self) -> Vec<u8> {
            Vec::new()
        }
    }

    let tap = Arc::new(std::sync::Mutex::new(Tap::default()));
    let mut s = session(2);
    s.attach_writer(Box::new(TapWriter(tap.clone())));
    add_highlight(&mut s, PageIndex(0));

    // 注入失败：flush 报错 → rev 不动（产物未变——失败 flush 不得让
    // 缓存全体 miss，否则「保存失败」表现为「页面闪烁重渲染」）。
    tap.lock().unwrap().fail_next = true;
    let err = s.flush_annotations().expect_err("注入失败必传播");
    assert!(matches!(err, CommandError::Engine(_)));
    assert_eq!(s.annotation_rev(), 0, "失败 flush 不推进 rev");

    // 重试成功 → +1（dirty 保留合同的可观测面）。
    s.flush_annotations().expect("重试成功");
    assert_eq!(s.annotation_rev(), 1, "重试成功才推进");
}

// ---------------------------------------------------------------------------
// 3-4：验收主锁（键控失效端到端）
// ---------------------------------------------------------------------------

#[test]
fn stale_rev_key_misses_after_flush() {
    let mut s = session(2);
    s.attach_writer(Box::new(MockWriter::new()));
    let mut cache: PageCache<RenderKey, u32> = PageCache::new(1024);

    // 渲染产物以 (page, w, h, rev=0) 入缓存。
    let key_v0 = RenderKey::new(0, 640, 828, 0);
    cache.put(key_v0, 111, 4);
    assert_eq!(cache.get(&key_v0), Some(&111), "flush 前旧键命中");

    // flush（rev 0 → 1）。
    add_highlight(&mut s, PageIndex(0));
    s.flush_annotations().expect("flush");
    assert_eq!(s.annotation_rev(), 1);

    // 新键 miss（= 触发重渲染——用户看到新标注的机制面）；
    // 旧键内容仍在位（键控 = 不服役不销毁；销毁走显式失效回收预算）。
    let key_v1 = RenderKey::new(0, 640, 828, 1);
    assert!(
        cache.get(&key_v1).is_none(),
        "flush 后新键必 miss——这是「重渲染看到新标注」的根机制"
    );
    assert!(
        cache.contains(&key_v0),
        "键控只让旧键不被取用，条目仍在位（回收走 invalidate_stale_revisions）"
    );
}

#[test]
fn re_render_after_flush_refills_and_hits() {
    let mut s = session(3);
    s.attach_writer(Box::new(MockWriter::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut raster = CountingRaster {
        calls: calls.clone(),
    };
    let mut cache: PageCache<RenderKey, RenderedPage> = PageCache::new(64 * 1024 * 1024);

    // 初渲染：3 页 @ rev 0（每页恰好一次 rasterize）。
    let render_all =
        |raster: &mut CountingRaster, cache: &mut PageCache<RenderKey, RenderedPage>, rev: u64| {
            for page in 0..3u32 {
                let key = RenderKey::new(page, 640, 828, rev);
                if cache.get(&key).is_none() {
                    let out = raster
                        .rasterize(RenderRequest {
                            page: PageIndex(page),
                            width_px: 640,
                        })
                        .expect("mock 渲染必成");
                    cache.put(key, out, 640 * 828 * 4);
                }
            }
        };
    render_all(&mut raster, &mut cache, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 3, "首轮 3 次 rasterize");

    // 稳态：同 rev 重复取 = 全命中（零 rasterize——验收 3 命中率面）。
    render_all(&mut raster, &mut cache, 0);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "同 rev 重复渲染零 rasterize（缓存命中语义不因键控受损）"
    );

    // flush（rev → 1）+ 陈旧回收 + 重渲染：恰一轮（3 次）。
    add_highlight(&mut s, PageIndex(1));
    s.flush_annotations().expect("flush");
    let rev = s.annotation_rev();
    assert_eq!(rev, 1);
    let evicted = cache.invalidate_stale_revisions(rev);
    assert_eq!(
        evicted.len(),
        3,
        "旧 rev 条目全部即刻回收（预算不空等到 LRU 驱逐）"
    );
    assert!(evicted.iter().all(|k| k.annotation_rev == 0));

    render_all(&mut raster, &mut cache, rev);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        6,
        "flush 后恰一轮重光栅（3 页各一次——不多不少，验收 1/2 的 mock 化）"
    );

    // 新稳态：再取全命中。
    render_all(&mut raster, &mut cache, rev);
    assert_eq!(calls.load(Ordering::SeqCst), 6, "新 rev 稳态零 rasterize");
}

// ---------------------------------------------------------------------------
// 5-7：失效粒度（方案 B）
// ---------------------------------------------------------------------------

#[test]
fn invalidate_stale_keeps_current_rev_entries() {
    let mut cache: PageCache<RenderKey, u32> = PageCache::new(4096);
    // 混合 rev 共存（真实形态：flush 后只回看了部分页——旧 rev 条目
    // 与新 rev 条目同时在位）。
    cache.put(RenderKey::new(0, 640, 828, 0), 100, 4); // 旧 rev
    cache.put(RenderKey::new(1, 640, 828, 0), 101, 4); // 旧 rev
    cache.put(RenderKey::new(0, 640, 828, 1), 200, 4); // 当前 rev
    cache.put(RenderKey::new(1, 640, 828, 1), 201, 4); // 当前 rev

    let dropped = cache.invalidate_stale_revisions(1);
    assert_eq!(dropped.len(), 2, "只摘旧 rev");
    assert!(dropped.iter().all(|k| k.annotation_rev == 0));

    // 当前 rev 全在位（跨 rev 视图回看场景的命中保持）。
    assert_eq!(cache.get(&RenderKey::new(0, 640, 828, 1)), Some(&200));
    assert_eq!(cache.get(&RenderKey::new(1, 640, 828, 1)), Some(&201));
    assert!(!cache.contains(&RenderKey::new(0, 640, 828, 0)));
    assert!(!cache.contains(&RenderKey::new(1, 640, 828, 0)));
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.bytes(), 8, "预算同步回收（bytes = 存活条目和）");
}

#[test]
fn invalidate_page_is_scoped() {
    let mut cache: PageCache<RenderKey, u32> = PageCache::new(4096);
    cache.put(RenderKey::new(0, 640, 828, 0), 100, 4);
    cache.put(RenderKey::new(0, 512, 662, 0), 101, 4); // 同页异宽也在页级影响域内
    cache.put(RenderKey::new(1, 640, 828, 0), 110, 4);
    cache.put(RenderKey::new(2, 640, 828, 0), 120, 4);

    let dropped = cache.invalidate_page(0);
    assert_eq!(dropped.len(), 2, "页 0 的全部条目（含异宽度）");
    assert!(dropped.iter().all(|k| k.page == 0));

    // 他页零扰动（页级失效不影响别的页的命中）。
    assert_eq!(cache.get(&RenderKey::new(1, 640, 828, 0)), Some(&110));
    assert_eq!(cache.get(&RenderKey::new(2, 640, 828, 0)), Some(&120));
    assert_eq!(cache.len(), 2);
}

#[test]
fn invalidate_all_returns_keys_lru_to_mru() {
    let mut cache: PageCache<RenderKey, u32> = PageCache::new(4096);
    let a = RenderKey::new(0, 640, 828, 0);
    let b = RenderKey::new(1, 640, 828, 0);
    let c = RenderKey::new(2, 640, 828, 0);
    cache.put(a, 1, 4);
    cache.put(b, 2, 4);
    cache.put(c, 3, 4);
    // get(a) 提权 → LRU→MRU = b, c, a。
    let _ = cache.get(&a);

    let dropped = cache.invalidate_all();
    assert_eq!(dropped.len(), 3);
    assert_eq!(
        dropped,
        vec![b, c, a],
        "返回序 = LRU→MRU（与 put 驱逐键集同款载荷——调用方按序作废关联资源）"
    );
    assert!(cache.is_empty());
    assert_eq!(cache.bytes(), 0);
}

// ---------------------------------------------------------------------------
// 8：键判别完备性
// ---------------------------------------------------------------------------

#[test]
fn render_key_discriminates_all_factors() {
    let base = RenderKey::new(0, 640, 828, 0);
    assert_eq!(base, RenderKey::new(0, 640, 828, 0), "全同 = 相等");
    assert_ne!(base, RenderKey::new(1, 640, 828, 0), "页不同");
    assert_ne!(base, RenderKey::new(0, 512, 828, 0), "宽不同");
    assert_ne!(base, RenderKey::new(0, 640, 662, 0), "高不同");
    assert_ne!(base, RenderKey::new(0, 640, 828, 1), "rev 不同");

    // from_outcome：请求页 + 产物尺寸 + rev 三源合成（渲染回执落账面）。
    let out = RenderedPage {
        width_px: 640,
        height_px: 828,
        bgra: Vec::new(),
    };
    assert_eq!(
        RenderKey::from_outcome(
            &RenderRequest {
                page: PageIndex(0),
                width_px: 640,
            },
            &out,
            7,
        ),
        RenderKey::new(0, 640, 828, 7),
        "from_outcome = new(请求页, 产物宽, 产物高, rev)"
    );
}

// ---------------------------------------------------------------------------
// 9-10：泛型失效面（任意 K——不限于 RenderKey）
// ---------------------------------------------------------------------------

#[test]
fn generic_invalidate_returns_value_and_is_idempotent() {
    let mut cache: PageCache<(u32, u32), u32> = PageCache::new(1024);
    cache.put((0, 640), 111, 4);

    let value = cache.invalidate(&(0, 640));
    assert_eq!(
        value,
        Some(111),
        "单键失效返回值（调用方作废关联资源的载荷——所有权移交）"
    );
    assert!(!cache.contains(&(0, 640)));
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.bytes(), 0, "成本同步回收");

    // 幂等：重复失效不报错（竞态下的重复作废是合法调用）。
    assert_eq!(cache.invalidate(&(0, 640)), None);
}

#[test]
fn generic_invalidate_matching_keeps_budget_and_order() {
    let mut cache: PageCache<(u32, u32), u32> = PageCache::new(8192);
    // 3 个偶数键 + 2 个奇数键；访问 a 制造 LRU 序（a 提权到 MRU）。
    cache.put((0, 0), 0, 8);
    cache.put((1, 0), 1, 8);
    cache.put((2, 0), 2, 8);
    cache.put((3, 0), 3, 16);
    cache.put((4, 0), 4, 16);
    let _ = cache.get(&(0, 0)); // LRU→MRU: (1,0) (2,0) (3,0) (4,0) (0,0)

    let dropped = cache.invalidate_matching(|k| k.0 % 2 == 0);
    assert_eq!(
        dropped,
        vec![(2, 0), (4, 0), (0, 0)],
        "只摘偶数键，序 = LRU→MRU（提权后的 a 排最后）"
    );
    assert_eq!(cache.len(), 2, "奇数键原序在位");
    assert_eq!(cache.bytes(), 24, "预算 = 存活条目成本和（8 + 16）");
    assert_eq!(cache.get(&(1, 0)), Some(&1));
    assert_eq!(cache.get(&(3, 0)), Some(&3));
}

// ---------------------------------------------------------------------------
// 11：PdfAnnotator rev（feature 面——写器实装在 engine-lopdf）
// ---------------------------------------------------------------------------

#[cfg(feature = "engine-lopdf")]
#[test]
fn annotator_rev_matches_session_contract() {
    use mdc_pdf::annotate::PdfAnnotator;
    use mdc_pdf::annotation::{AnnotationKind, Border, Origin, Rect, Rgb};
    use std::path::PathBuf;

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("small.pdf");
    let bytes = std::fs::read(&fixture).expect("fixture small.pdf 在仓内随包");

    let mut annotator = PdfAnnotator::open(bytes).expect("打开");
    assert_eq!(annotator.annotation_rev(), 0, "打开即 0");

    // 幂等 save（零待保存）：零追加 → 不 bump。
    let idle = annotator.save().expect("幂等 save");
    assert_eq!(idle.bytes_appended, 0);
    assert_eq!(annotator.annotation_rev(), 0, "零追加不推进");

    // add → save：追加 > 0 → +1。
    annotator
        .add(
            PageIndex(0),
            AnnotationKind::Rectangle {
                rect: Rect::new(72.0, 600.0, 300.0, 700.0).expect("合法矩形"),
                border: Border {
                    color: Rgb {
                        r: 220,
                        g: 20,
                        b: 60,
                    },
                    width: 2.5,
                },
            },
            "tester",
            Origin::Human,
        )
        .expect("add");
    let stats = annotator.save().expect("有内容 save");
    assert!(stats.bytes_appended > 0, "真写器有内容必追加");
    assert_eq!(annotator.annotation_rev(), 1, "产物变了 → +1");

    // 再幂等 save：不动。
    annotator.save().expect("再幂等 save");
    assert_eq!(annotator.annotation_rev(), 1, "无内容变更不推进");
}
