//! pdfp_transform_engine —— #304：设备↔用户空间变换的引擎面语义锁
//! （feature `engine-pdfium`）。
//!
//! **环境合同同 pdfc_engine**（不静默 skip）：库定位走
//! [`PdfiumEngine::open`] 定位序；fixtures 走 env `MDC_PDF_TEST_FIXTURES`
//! → 编译期候选。任一缺失 = panic 打印候选——门禁可诊断。
//!
//! 夹具 `rotated_crop.pdf`（生成器 `gen_rotated_crop_fixture.py`）：
//! MediaBox [0,0,612,792] / CropBox [50,40,562,752]（⊂ MediaBox 非零
//! 偏移）/ /Rotate 90 / 红方块 @ 用户空间 [100,132,160,192]（中心
//! (130,162)）。
//!
//! **交叉验证链（真 pdfium，非纯数学自证）**：夹具的盒/旋转读取与
//! 红方块渲染落点已用 pypdfium2 原生 C API + 渲染逐项验证（见
//! PR/gates 记录）；四旋转公式 × 偏移 CropBox × 96 DPI 的渲染互证
//! 误差 ≤ 0.5pt（采样量化级）。本文件 = 同链路的 Rust 侧锁。
//!
//! 锁清单：
//! 1. `engine_page_transform_reads_real_boxes`——引擎读真实双盒+旋转
//!    （MediaBox/CropBox 原点全量——from_geometry 近似在偏移 CropBox
//!    文档上必错，此锁区分）；
//! 2. `engine_display_dims_match_rotation_normalized_crop`——显示域
//!    尺寸 = CropBox 旋转归一（与 FPDF_GetPageWidthF 同语义）；
//! 3. `engine_screen_user_roundtrip_real_transform`——真引擎变换上
//!    96 DPI 双向往返（验收 3/5）；
//! 4. `engine_rasterize_pixel_maps_to_user_anchor`——**端到端主锁**
//!    （验收 1/2/4）：rasterize 真位图 → 红方块像素中心 →
//!    screen_to_user → ≈ 用户真值 (130,162)（±1.5pt = 2px 采样级）；
//!    这正是 #302 标注 UI 的落点管线（点击像素 → 用户点 → QuadPoints）。

// feature 门（默认构建零引擎依赖——退化为空目标，跨环境可构建纪律）。
#![cfg(feature = "engine-pdfium")]

use std::path::{Path, PathBuf};

use mdc_pdf::command::CommandError;
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::page::{PageIndex, Rotation};
use mdc_pdf::render::{Rasterizer, RenderRequest};
use mdc_pdf::transform::PageTransform;

/// fixtures 定位环境变量（目录形态；与 pdfc_engine 同合同）。
const FIXTURES_ENV: &str = "MDC_PDF_TEST_FIXTURES";

fn fixture_candidates() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        manifest.join("fixtures"),
        manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/fixtures"),
    ]
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let mut tried: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var(FIXTURES_ENV) {
        dirs.push(PathBuf::from(dir));
    }
    dirs.extend(fixture_candidates());
    for dir in &dirs {
        let path = dir.join(name);
        match std::fs::read(&path) {
            Ok(bytes) => return bytes,
            Err(why) => tried.push(format!("{} → {why}", path.display())),
        }
    }
    panic!(
        "fixture {name} 定位失败（定位序：{FIXTURES_ENV} → 编译期候选）；已尝试：{}",
        tried.join("；")
    );
}

/// 打开夹具引擎（绑定失败 = panic 打印定位序——门禁可诊断，不静默 skip）。
fn open_fixture() -> PdfiumEngine {
    match PdfiumEngine::open(fixture_bytes("rotated_crop.pdf")) {
        Ok(engine) => engine,
        Err(CommandError::Engine(detail)) => {
            panic!("pdfium 绑定/打开失败（定位序：MDC_PDFIUM_LIB → 编译期候选 → 系统）：{detail}")
        }
        Err(other) => panic!("打开 rotated_crop.pdf 应成功或 Engine 错误，得 {other:?}"),
    }
}

/// 真实双盒 + 旋转读取（from_geometry 近似在此夹具上必错——判别面）。
#[test]
fn engine_page_transform_reads_real_boxes() {
    let engine = open_fixture();
    let t: PageTransform = engine
        .page_transform(PageIndex(0))
        .expect("page_transform 应成功");

    // MediaBox 全量（原点 + 尺寸）。
    let media = t.media();
    assert!(
        (media.x0() - 0.0).abs() < 1e-3
            && (media.y0() - 0.0).abs() < 1e-3
            && (media.x1() - 612.0).abs() < 1e-3
            && (media.y1() - 792.0).abs() < 1e-3,
        "MediaBox 应为 [0,0,612,792]（得 {media:?}）"
    );
    // CropBox 全量：偏移原点（50,40）——近似路径（from_geometry）会把
    // crop 读成 [0,0,612,792]，此断言即「真实双盒 vs 近似」的判别器。
    let crop = t.crop();
    assert!(
        (crop.x0() - 50.0).abs() < 1e-3
            && (crop.y0() - 40.0).abs() < 1e-3
            && (crop.x1() - 562.0).abs() < 1e-3
            && (crop.y1() - 752.0).abs() < 1e-3,
        "CropBox 应为 [50,40,562,752]（偏移原点——近似的判别面）：{crop:?}"
    );
    assert_eq!(t.rotate(), Rotation::Deg90, "/Rotate 90");

    // 对照：page_geometry（MediaBox 尺寸面）不因本条变化。
    let geo = engine.page_geometry(PageIndex(0)).expect("page_geometry");
    assert!((geo.width_pt - 612.0).abs() < 1e-3 && (geo.height_pt - 792.0).abs() < 1e-3);
}

/// 显示域尺寸 = CropBox 旋转归一（FPDF_GetPageWidthF 同语义）。
#[test]
fn engine_display_dims_match_rotation_normalized_crop() {
    let engine = open_fixture();
    let t = engine.page_transform(PageIndex(0)).unwrap();
    // CropBox 512×756... 不：[50,40,562,752] → 512 × 712；90° → 712 × 512。
    assert!(
        (t.display_width_pt() - 712.0).abs() < 1e-3,
        "90° 显示宽 = CropBox 高（712）：{}",
        t.display_width_pt()
    );
    assert!(
        (t.display_height_pt() - 512.0).abs() < 1e-3,
        "90° 显示高 = CropBox 宽（512）：{}",
        t.display_height_pt()
    );
}

/// 真引擎变换上的 96 DPI 双向往返（验收 3/5）。
#[test]
fn engine_screen_user_roundtrip_real_transform() {
    let engine = open_fixture();
    let t = engine.page_transform(PageIndex(0)).unwrap();
    let dpi = 96.0;
    for &(x, y) in &[
        (0.0f32, 0.0f32),
        (100.5, 63.25),
        (400.0, 256.0),
        (700.75, 500.5),
    ] {
        let u = t.screen_to_user(x, y, dpi).expect("合法 DPI");
        let (sx, sy) = t.user_to_screen(u, dpi).expect("合法 DPI");
        assert!(
            (sx - x).abs() < 1e-3 && (sy - y).abs() < 1e-3,
            "往返（{x},{y}）→ {u:?} → ({sx},{sy})"
        );
    }
}

/// 端到端主锁（验收 1/2/4）：rasterize 真位图 → 红方块像素中心 →
/// screen_to_user → ≈ 用户真值 (130, 162)。
///
/// 这条链 = #302 标注 UI 的完整落点管线在引擎面上的实证（pdfium 渲染
/// 域 ↔ 变换公式互证——公式若与 pdfium 显示行为有分歧，此处红）。
#[test]
fn engine_rasterize_pixel_maps_to_user_anchor() {
    let mut engine = open_fixture();
    let t = engine.page_transform(PageIndex(0)).unwrap();

    // 渲染 @ 96 DPI（宽 = 显示域 712pt → 712×96/72 ≈ 949.33 → 引擎钳整）。
    let width_px = (t.display_width_pt() * 96.0 / 72.0).round() as u32;
    let page = engine
        .rasterize(RenderRequest {
            page: PageIndex(0),
            width_px,
        })
        .expect("渲染应成功");
    assert_eq!(page.width_px, width_px, "渲染宽 = 请求宽");

    // BGRA 扫红方块（r>200, g<80, b<80——生成器画的纯红填充）。
    let mut xs: Vec<u32> = Vec::new();
    let mut ys: Vec<u32> = Vec::new();
    let stride = page.width_px as usize * 4;
    for y in 0..page.height_px {
        for x in 0..page.width_px {
            let i = y as usize * stride + x as usize * 4;
            let (b, g, r) = (page.bgra[i], page.bgra[i + 1], page.bgra[i + 2]);
            if r > 200 && g < 80 && b < 80 {
                xs.push(x);
                ys.push(y);
            }
        }
    }
    assert!(!xs.is_empty(), "渲染位图应含红方块（内容流画了它）");
    let px_c = (
        xs.iter().sum::<u32>() as f32 / xs.len() as f32,
        ys.iter().sum::<u32>() as f32 / ys.len() as f32,
    );

    // 像素中心 → 用户空间：应回到红方块的用户中心 (130, 162)。
    // 容差 1.5pt（2px @ 96DPI 采样 + 边缘抗锯齿）。
    let u = t.screen_to_user(px_c.0, px_c.1, 96.0).expect("合法 DPI");
    assert!(
        (u.x - 130.0).abs() < 1.5 && (u.y - 162.0).abs() < 1.5,
        "红方块像素中心 {px_c:?} → 用户 {u:?}，期望 (130, 162)±1.5pt"
    );

    // 正向对照：用户真值 → 公式像素 ≈ 实测像素中心（±2px）。
    let (sx, sy) = t
        .user_to_screen(mdc_pdf::annotation::Point { x: 130.0, y: 162.0 }, 96.0)
        .expect("合法 DPI");
    assert!(
        (sx - px_c.0).abs() < 2.0 && (sy - px_c.1).abs() < 2.0,
        "用户真值 → 公式像素 ({sx},{sy}) vs 实测 {px_c:?}"
    );
}
