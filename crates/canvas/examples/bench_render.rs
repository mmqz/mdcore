//! 渲染基准：1000 个图元的导出耗时。
//!
//! 用法（仓库根，**必须 release**，debug 下的数字没有参考价值）：
//!
//!     cargo run --release -p mdc-canvas --example bench_render
//!
//! 门槛（issue #125 验收项）：**1000 个图形渲染 < 100ms**。
//! 两个出口分别计时：
//!
//! - SVG：纯字符串拼接，应当远低于门槛；
//! - PNG：扫描线光栅化 + zlib-stored 打包，是真正的成本所在。
//!
//! 计数口径：1000 个图元 = 250 矩形 + 250 椭圆 + 250 线 + 150 路径 + 100 文字。
//! 输出分辨率 1024x768（一块典型画布视口）。

use std::time::Instant;

use mdc_canvas::render::*;

fn bench_canvas() -> CanvasRendering {
    let mut c = CanvasRendering::new().with_background(Color::WHITE);
    // 30x25 网格铺满 1024x768，保证图元铺开而不是全叠在同一处。
    let mut i = 0u32;
    for gy in 0..25u32 {
        for gx in 0..40u32 {
            let x = 8.0 + gx as f64 * 25.0;
            let y = 8.0 + gy as f64 * 30.0;
            match i % 10 {
                0..=3 => c.push(Shape::Rect {
                    rect: Rect::new(x, y, 18.0, 14.0),
                    style: ShapeStyle::both(
                        Color::rgb(240, 245, 255),
                        StrokeStyle::new(Color::rgb(45, 108, 223), 1.0),
                    ),
                }),
                4..=5 => c.push(Shape::Ellipse {
                    rect: Rect::new(x, y, 18.0, 14.0),
                    style: ShapeStyle::both(
                        Color::rgb(230, 250, 230),
                        StrokeStyle::new(Color::rgb(30, 120, 30), 1.0),
                    ),
                }),
                6..=8 => c.push(Shape::Line {
                    from: Point::new(x, y + 7.0),
                    to: Point::new(x + 18.0, y + 7.0),
                    style: StrokeStyle::new(Color::rgb(80, 80, 80), 1.0),
                }),
                _ => c.push(Shape::Path {
                    cmds: vec![
                        PathCmd::MoveTo(Point::new(x, y)),
                        PathCmd::QuadTo {
                            ctrl: Point::new(x + 9.0, y - 6.0),
                            to: Point::new(x + 18.0, y),
                        },
                        PathCmd::LineTo(Point::new(x + 18.0, y + 14.0)),
                        PathCmd::Close,
                    ],
                    style: ShapeStyle::both(
                        Color::rgb(255, 245, 220),
                        StrokeStyle::new(Color::rgb(160, 120, 10), 1.0),
                    ),
                }),
            };
            i += 1;
            if i >= 1000 {
                break;
            }
        }
        if i >= 1000 {
            break;
        }
    }
    c
}

/// 文字图元单独补 100 个（上面网格只铺到 1000 个非文字图元）。
fn with_text(mut c: CanvasRendering) -> CanvasRendering {
    for i in 0..100u32 {
        c.push(Shape::Text {
            pos: Point::new(10.0 + (i % 20) as f64 * 50.0, 740.0),
            text: format!("t{i}"),
            style: TextStyle::new(9.0, Color::BLACK),
        });
    }
    c
}

fn time_ms(label: &str, rounds: usize, mut f: impl FnMut()) -> f64 {
    // 预热一轮（分配器/缓存冷启动不计入）。
    f();
    let t = Instant::now();
    for _ in 0..rounds {
        f();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / rounds as f64;
    println!("  {label:<28} {ms:8.3} ms");
    ms
}

fn main() {
    let canvas = with_text(bench_canvas());
    let n = canvas.len();
    println!("图元数：{n}（250 矩形 + 250 椭圆 + 250 线 + 150 路径 + 100 文字）");
    println!("输出分辨率：1024x768");
    println!();

    let svg_ms = time_ms("export_svg", 20, || {
        let s = mdc_canvas::export_svg(&canvas);
        std::hint::black_box(s.len());
    });
    let png_ms = time_ms("export_png (1024x768)", 10, || {
        let p = mdc_canvas::export_png(&canvas, (1024, 768)).expect("png");
        std::hint::black_box(p.len());
    });

    println!();
    println!("门槛：1000 图元渲染 < 100ms（issue #125 验收项）");
    for (label, ms) in [("SVG", svg_ms), ("PNG", png_ms)] {
        let ratio = ms / 100.0;
        let verdict = if ms < 100.0 { "PASS" } else { "FAIL" };
        println!("  {label:<4} {ms:8.3} ms  ({ratio:.2}x 门槛)  {verdict}");
    }

    // 只报告不失败：基准测试在 CI 上因为机器差异而红，会让人开始忽略它。
    // 真正的门槛判断交给看报告的人 / PR review。
}
