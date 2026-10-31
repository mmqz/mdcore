//! 渲染黄金例生成器。
//!
//! 用法（仓库根）：
//!
//!     cargo run -p mdc-canvas --example gen_fixtures
//!
//! 产出 crates/canvas/tests/fixtures/canvas_render/ 下的 golden.svg 与
//! golden.png。夹具进仓，缺失即测试失败（见 tests/golden.rs）。
//!
//! 画布定义与测试**共用** tests/fixtures/golden_data.rs，避免两处分叉。

use std::fs;
use std::path::PathBuf;

include!("../tests/fixtures/golden_data.rs");

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("tests/fixtures/canvas_render");
    fs::create_dir_all(&out).expect("创建夹具目录");

    let canvas = golden_canvas();
    canvas.check().expect("黄金例画布必须合法");

    let svg = mdc_canvas::export_svg(&canvas);
    fs::write(out.join("golden.svg"), &svg).expect("写 golden.svg");

    let png = mdc_canvas::export_png(&canvas, GOLDEN_PNG_SIZE).expect("导出 PNG");
    fs::write(out.join("golden.png"), &png).expect("写 golden.png");

    println!("golden.svg  {} bytes", svg.len());
    println!("golden.png  {} bytes", png.len());
    println!("-> {}", out.display());
}
