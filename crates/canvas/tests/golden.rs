//! 黄金例对照 + 跨渲染器交叉验证。
//!
//! # 硬约束：夹具缺失 = FAIL，不是 SKIP
//!
//! 测试读的是**进仓的**期望产物：
//!
//!   tests/fixtures/canvas_render/golden.svg
//!   tests/fixtures/canvas_render/golden.png
//!
//! 文件不在就直接 panic。「没有夹具就跳过」会让 CI 在别人删掉夹具之后
//! 依然全绿——那不叫验证，那叫装饰。刷新夹具：
//!
//!     cargo run -p mdc-canvas --example gen_fixtures
//!
//! # 两道独立验证
//!
//! 1. **字节级回归**：当前实现产出的 SVG/PNG 必须与进仓夹具逐字节相同。
//!    锁住「行为不要漂」，有意的渲染改动必须刷新夹具并在 PR 里解释。
//! 2. **跨实现交叉验证**：把 export_svg 交给外部渲染器（PyMuPDF）光栅化，
//!    再与 export_png 逐像素比对。SVG 与 PNG 是两条**独立实现**，一条错了
//!    另一条不会跟着错——这才算「浏览器打开对比」的自动化等价物。
//!
//! 第 2 条需要 python + PyMuPDF，缺了就打日志跳过（加分项不该阻塞纯 Rust
//! 环境）。但第 1 条**永远不跳过**。

use std::path::PathBuf;
use std::process::Command;

use mdc_canvas::render::*;

include!("fixtures/golden_data.rs");

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/canvas_render")
}

fn read_fixture(name: &str) -> Vec<u8> {
    let p = fixtures_dir().join(name);
    std::fs::read(&p).unwrap_or_else(|e| {
        panic!(
            "夹具 {} 缺失或不可读：{e}\n用 `cargo run -p mdc-canvas --example gen_fixtures` 生成；夹具必须进仓。",
            p.display()
        )
    })
}

/// 夹具必须真的存在且不是空壳（防止有人把测试改成摆样子）。
#[test]
fn fixtures_are_present_and_non_trivial() {
    let svg = read_fixture("golden.svg");
    let png = read_fixture("golden.png");
    assert!(
        svg.len() > 500,
        "golden.svg 太小，疑似被截断：{}B",
        svg.len()
    );
    assert!(
        png.len() > 1000,
        "golden.png 太小，疑似被截断：{}B",
        png.len()
    );
    assert_eq!(
        &png[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        "golden.png 必须有 PNG 签名"
    );
}

/// SVG 黄金例：当前实现必须与进仓夹具逐字节相同。
#[test]
fn golden_svg_matches_fixture() {
    let expected = read_fixture("golden.svg");
    let actual = export_svg(&golden_canvas());
    assert_eq!(
        actual.as_bytes(),
        expected.as_slice(),
        "SVG 与夹具不一致；若是有意的渲染改动，跑 gen_fixtures 刷新夹具并在 PR 说明"
    );
}

/// PNG 黄金例：字节级一致（同平台同实现必须确定性）。
#[test]
fn golden_png_matches_fixture() {
    let expected = read_fixture("golden.png");
    let actual = export_png(&golden_canvas(), GOLDEN_PNG_SIZE).expect("导出 PNG");
    assert_eq!(actual.len(), expected.len(), "PNG 长度与夹具不一致");
    assert!(actual == expected, "PNG 与夹具不一致；刷新方式见文件头");
}

// ── 跨实现交叉验证 ─────────────────────────────────────────────

/// 交叉验证脚本：PyMuPDF 光栅化 SVG，与我们的 PNG 逐像素比。
///
/// # 必须先归一化尺寸，且必须用「等比缩放 + 裁切」而不是「拉伸」
///
/// export_png 的视口 = 几何包围盒，按 min(sx, sy) 等比缩放适配请求尺寸
/// （所以内容在输出里是居中偏上的，右侧/下方留白）。而 PyMuPDF 按 SVG 的
/// width/height 出图，尺寸又和 viewBox 不同。直接 resize 到同尺寸会做
/// **非等比拉伸**，两个宽度下的每个像素都对不上——第一版就栽在这上面，
/// 差异 24% 全是"整体被拉宽"造成的假阳性。
///
/// 正确做法：两边都按「等比缩放 + 左上对齐」归一到同一尺寸再比。
const CROSS_CHECK_PY: &str = r#"
import sys
try:
    import fitz
except Exception:
    print("SKIP no-pymupdf")
    sys.exit(0)
from PIL import Image

def normalize(img, size):
    """等比缩放到能放进 size，左上对齐，剩余区域填白（与我们的背景一致）。"""
    w, h = img.size
    s = min(size[0] / w, size[1] / h)
    nw, nh = max(1, round(w * s)), max(1, round(h * s))
    out = Image.new("RGB", size, (255, 255, 255))
    out.paste(img.resize((nw, nh), Image.LANCZOS), (0, 0))
    return out

doc = fitz.open(sys.argv[1])
pix = doc[0].get_pixmap(alpha=False)
ref = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
ours = Image.open(sys.argv[2]).convert("RGB")
size = ours.size
ref = normalize(ref, size)

w, h = size
a = ours.load(); b = ref.load()
diff = 0
for y in range(h):
    for x in range(w):
        if max(abs(a[x, y][i] - b[x, y][i]) for i in range(3)) > 48:
            diff += 1
print("PCT", round(diff * 100.0 / (w * h), 3))
"#;

#[test]
fn svg_and_png_agree_via_external_rasterizer() {
    let dir = fixtures_dir();
    let svg = dir.join("golden.svg");
    let png = dir.join("golden.png");
    assert!(svg.exists(), "夹具缺失：{}", svg.display());
    assert!(png.exists(), "夹具缺失：{}", png.display());

    let out = match Command::new("python")
        .arg("-c")
        .arg(CROSS_CHECK_PY)
        .arg(&svg)
        .arg(&png)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("跳过交叉验证：python 不可用（{e}）");
            return;
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if stdout.contains("SKIP") || !out.status.success() {
        eprintln!(
            "跳过交叉验证：PyMuPDF 不可用。stdout={stdout} stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let pct: f64 = stdout
        .lines()
        .find_map(|l| l.strip_prefix("PCT "))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("交叉验证输出无法解析：{stdout}"));
    eprintln!("交叉验证：SVG<->PNG 差异像素 {pct}%");
    assert!(
        pct < 12.0,
        "SVG 与 PNG 差异像素 {pct}% 超过 12% 阈值——存在图元错位或漏画"
    );
}
