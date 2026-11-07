//! pdfo_transform —— #304：设备↔用户空间变换的真实语料验收面。
//!
//! 夹具：`fixtures/rotated_cropped.pdf`（gen_rotated_cropped_fixture.py
//! 生成）——四页矩阵：Rotate 90/180/270/0 × CropBox 居中偏移/无/非对称
//! 偏移/零偏移全幅。每页用户空间 (100,100) 处有 6×6pt 实心黑方块标记。
//!
//! # 验证方法论（三层）
//!
//! 1. **渲染级 oracle（开发期，pypdfium2）**：真实渲染每页 @72/96dpi →
//!    连通域定位标记块像素中心 → 与变换公式比对。4 页 × 2 DPI 全部
//!    Δ ≤ 0.83px（整点化噪声内）——公式与真实渲染器一致的证据（PR 描述
//!    有完整输出）。本文件把 oracle 实测值固化为已知答案断言。
//! 2. **lopdf 真实语料**：测试自读夹具的 /MediaBox /CropBox /Rotate
//!    构造 [`PageTransform`]（不经手喂参数——引擎产几何的真实路径）。
//! 3. **纯数学面**：`transform.rs` 内嵌单元测试（全旋转 × 偏移 × DPI
//!    扫描往返）。
//!
//! 诚实边界：engine-pdfium 侧（FPDFPage_GetCropBox 接线）本机无兼容
//! libpdfium（pypdfium2 的库与 pdfium-render 0.9.4 绑定 dlsym 不兼容），
//! 只做了编译验证；渲染 oracle 用同源 pdfium（pypdfium2 绑定）完成，
//! 数学一致性已由此背书。

#![cfg(feature = "engine-lopdf")]

use std::path::PathBuf;

use mdc_pdf::annotation::Point;
use mdc_pdf::page::{PageBox, Rotation};
use mdc_pdf::transform::PageTransform;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// 用 lopdf 直读夹具页几何（MediaBox/CropBox/Rotate）→ PageTransform。
/// （引擎产几何的真实路径镜像——page_geometry() 的 lopdf 侧等价物。）
fn transform_of_page(n: usize) -> PageTransform {
    let bytes = std::fs::read(fixtures().join("rotated_cropped.pdf"))
        .expect("夹具 rotated_cropped.pdf 缺失");
    let doc = lopdf::Document::load_mem(&bytes).expect("夹具装载失败");
    let pages = doc.get_pages();
    let mut ids: Vec<_> = pages.values().collect();
    ids.sort();
    let page_id = *ids[n - 1];
    let page = doc.get_dictionary(page_id).expect("页字典");

    let read_box = |key: &[u8]| -> Option<PageBox> {
        let arr = page.get(key).ok()?.as_array().ok()?;
        let f = |i: usize| -> f32 {
            match arr.get(i) {
                Some(lopdf::Object::Integer(v)) => *v as f32,
                Some(lopdf::Object::Real(v)) => *v as f32,
                _ => 0.0,
            }
        };
        Some(PageBox {
            x0: f(0),
            y0: f(1),
            x1: f(2),
            y1: f(3),
        })
    };

    let media = read_box(b"MediaBox").expect("夹具必有 MediaBox");
    let crop = read_box(b"CropBox");
    let rotate = match page
        .get(b"Rotate")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0)
    {
        90 => Rotation::Deg90,
        180 => Rotation::Deg180,
        270 => Rotation::Deg270,
        _ => Rotation::Deg0,
    };
    PageTransform::new(media, crop, rotate).expect("夹具几何应合法")
}

/// 夹具参数矩阵（与生成脚本一致——测试对照用）。
const MATRIX: [(Rotation, Option<(f32, f32, f32, f32)>); 4] = [
    (Rotation::Deg90, Some((30.0, 40.0, 582.0, 752.0))),
    (Rotation::Deg180, None),
    (Rotation::Deg270, Some((100.0, 20.0, 400.0, 700.0))),
    (Rotation::Deg0, Some((0.0, 0.0, 612.0, 792.0))),
];

/// 标记点中心（用户空间）：方块 (100,100)-(106,106)。
const MARK: (f32, f32) = (103.0, 103.0);

// ------------------------------------------------ 1. 夹具几何直读自检

#[test]
fn fixture_geometry_reads_back_as_matrix() {
    for (i, (want_rot, want_crop)) in MATRIX.iter().enumerate() {
        let t = transform_of_page(i + 1);
        assert_eq!(t.rotate(), *want_rot, "page {}", i + 1);
        let media = t.media();
        assert_eq!(
            (media.x0, media.y0, media.x1, media.y1),
            (0.0, 0.0, 612.0, 792.0)
        );
        match want_crop {
            Some(c) => {
                let got = t.crop();
                assert!(
                    (got.x0 - c.0).abs() < 1e-3
                        && (got.y0 - c.1).abs() < 1e-3
                        && (got.x1 - c.2).abs() < 1e-3
                        && (got.y1 - c.3).abs() < 1e-3,
                    "page {} crop {:?} ≠ {:?}",
                    i + 1,
                    got,
                    c
                );
            }
            None => {
                // 无 CropBox → 显示域 = MediaBox 兜底
                assert_eq!(t.crop(), media, "page {} 应回落 MediaBox", i + 1);
            }
        }
    }
}

// ------------------------------------------------ 2. 显示域尺寸（渲染 oracle 背书）

/// 显示域尺寸 = CropBox 基 + 旋转（与 pdfium FPDF_GetPageWidth/Height
/// 同口径；pypdfium2 的 page.get_size() 交叉核对过——PR 描述有记录）。
#[test]
fn display_size_matches_rendered_page_size() {
    // (页号, 期望宽, 期望高)
    let expect = [
        (1usize, 712.0f32, 552.0f32), // rot90 crop 552×712 → 712×552
        (2, 612.0, 792.0),            // rot180 无 crop
        (3, 680.0, 300.0),            // rot270 crop 300×680 → 680×300
        (4, 612.0, 792.0),            // rot0 全幅 crop
    ];
    for (n, w, h) in expect {
        let t = transform_of_page(n);
        let s = t.display_size();
        assert!(
            (s.width - w).abs() < 1e-3 && (s.height - h).abs() < 1e-3,
            "page {n}: display {}×{} ≠ 期望 {w}×{h}",
            s.width,
            s.height
        );
    }
}

// ------------------------------------------------ 3. 标记点落点（渲染级已知答案）

/// 标记点 (103,103) 的屏幕位置——**渲染级 oracle 实测值**固化：
/// pypdfium2 渲染 @72dpi 连通域定位的黑块像素中心（PR 描述有完整
/// 对照表，最大偏差 0.83px）。
#[test]
fn mark_user_to_screen_matches_rendered_pixel_positions() {
    // (页号, dpi, oracle 实测像素中心, 公式值)
    let cases: &[(usize, f32, (f32, f32), (f32, f32))] = &[
        (1, 72.0, (62.5, 72.5), (63.0, 73.0)),
        (1, 96.0, (84.0, 96.5), (84.0, 97.333)),
        (2, 72.0, (508.5, 102.5), (509.0, 103.0)),
        (2, 96.0, (678.5, 136.5), (678.667, 137.333)),
        (3, 72.0, (596.5, 296.5), (597.0, 297.0)),
        (3, 96.0, (796.0, 395.5), (796.0, 396.0)),
        (4, 72.0, (102.5, 688.5), (103.0, 689.0)),
        (4, 96.0, (136.5, 918.5), (137.333, 918.667)),
    ];
    for (n, dpi, oracle, formula) in cases {
        let t = transform_of_page(*n);
        let (sx, sy) = t.user_to_screen(
            Point {
                x: MARK.0,
                y: MARK.1,
            },
            *dpi,
        );
        // ① 与公式值一致（Rust 实现自身）
        assert!(
            (sx - formula.0).abs() < 0.01 && (sy - formula.1).abs() < 0.01,
            "page {n} @{dpi}: Rust 公式 ({sx:.2},{sy:.2}) ≠ 推导值 {formula:?}"
        );
        // ② 与渲染实测一致（整点化噪声 ≤ 1.5px——oracle 层面的
        //    「点击位置 → 标注落点正确」验收）
        assert!(
            (sx - oracle.0).abs() <= 1.5 && (sy - oracle.1).abs() <= 1.5,
            "page {n} @{dpi}: 公式 ({sx:.2},{sy:.2}) vs 渲染实测 {oracle:?} 偏差超 1.5px"
        );
    }
}

/// 反向：渲染像素位置 → 用户空间 ≈ 标记点中心（点击命中标记块）。
#[test]
fn rendered_pixel_back_to_user_lands_on_mark() {
    // oracle 实测像素中心（72dpi）→ screen_to_user → 应落在标记块
    // (100,100)-(106,106) 内
    let cases: &[(usize, (f32, f32))] = &[
        (1, (62.5, 72.5)),
        (2, (508.5, 102.5)),
        (3, (596.5, 296.5)),
        (4, (102.5, 688.5)),
    ];
    for (n, (px, py)) in cases {
        let t = transform_of_page(*n);
        let p = t.screen_to_user(*px, *py, 72.0);
        assert!(
            (100.0..=106.5).contains(&p.x) && (100.0..=106.5).contains(&p.y),
            "page {n}: 像素 ({px},{py}) → 用户 {p:?} 应落在标记块 (100,100)-(106,106) 内"
        );
    }
}

// ------------------------------------------------ 4. 往返（真实夹具 × DPI 扫描）

/// 验收 5（往返）：真实夹具四页 × DPI 72/96/150/300，
/// screen ↔ user 双向往返误差 < 0.01px（远严于 issue 的 1 点要求）。
#[test]
fn roundtrip_on_real_fixture_all_dpis() {
    for n in 1..=4 {
        let t = transform_of_page(n);
        let size = t.display_size();
        for dpi in [72.0f32, 96.0, 150.0, 300.0] {
            let (w, h) = (size.width * dpi / 72.0, size.height * dpi / 72.0);
            for (i, j) in (0..=4).flat_map(|i| (0..=4).map(move |j| (i, j))) {
                let (sx, sy) = (w * i as f32 / 4.0, h * j as f32 / 4.0);
                let p = t.screen_to_user(sx, sy, dpi);
                let (bx, by) = t.user_to_screen(p, dpi);
                assert!(
                    (bx - sx).abs() < 0.01 && (by - sy).abs() < 0.01,
                    "page {n} @{dpi}dpi: ({sx},{sy}) → {p:?} → ({bx},{by})"
                );
            }
        }
    }
}

/// 屏幕选择矩形 → Quad：真实夹具上框选中央区域 → quad 轴对齐 +
/// 角序正确 + 尺寸 = CropBox 中央 50% + 中心点双向命中。
#[test]
fn screen_selection_covers_center_on_all_pages() {
    for n in 1..=4 {
        let t = transform_of_page(n);
        let size = t.display_size();
        let dpi = 96.0;
        let (w, h) = (size.width * dpi / 72.0, size.height * dpi / 72.0);
        let quad = t.screen_rect_to_quad(w * 0.25, h * 0.25, w * 0.75, h * 0.75, dpi);
        let [tl, tr, bl, br] = quad.points;
        // 轴对齐 + 角序 TL,TR,BL,BR（用户空间取向）
        assert!(tl.y == tr.y && bl.y == br.y, "page {n} 上下边不水平");
        assert!(tl.x == bl.x && tr.x == br.x, "page {n} 左右边不垂直");
        assert!(tl.y > bl.y && tl.x < tr.x, "page {n} 角序非 TL,TR,BL,BR");
        // 尺寸 = CropBox 中央 50%（旋转不变的度量）
        let expect_w = t.crop().width() * 0.5;
        let expect_h = t.crop().height() * 0.5;
        assert!(
            ((tr.x - tl.x) - expect_w).abs() < 0.01 && ((tl.y - bl.y) - expect_h).abs() < 0.01,
            "page {n}: quad 尺寸 {:?} ≠ crop 中央 50% ({expect_w}×{expect_h})",
            quad.points
        );
        // 中心点双向命中：屏幕中心映射落在 quad 内
        let p = t.screen_to_user(w * 0.5, h * 0.5, dpi);
        assert!(
            p.x >= tl.x && p.x <= tr.x && p.y >= bl.y && p.y <= tl.y,
            "page {n}: 屏幕中心 → 用户 {p:?} 应落在 quad {:?} 内",
            quad.points
        );
    }
}
