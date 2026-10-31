//! 图表写面（issue #113 任务 3）——行为锚点测试。
//!
//! 覆盖：四类图表 XML 面断言（chartN.xml 类型元素 + 范围引用 +
//! [Content_Types]/drawing/rels 关联链）+ 系列拆分约定（列/行两向）+
//! 变体选项（方向/堆积/平滑/孔径/连线）+ 校验拒绝矩阵 + LibreOffice
//! 渲染墨迹对照（环境门——soffice/pdftoppm/PIL 缺失 SKIP）。
//!
//! 「数据改了图表跟着变」的断言面：系列值/类别/名均为**范围引用**
//! （`<c:f>Sheet1!$B$2:$B$4</c:f>`——引用而非快照；`c:numCache` 只是
//! 打开期显示缓存，引用公式是权威）。

use std::io::Read as _;

use mdc_office_io::write::{CellValue, SheetWriter, WorkbookWriter};
use mdc_office_io::xlsx_chart::{BarDirection, ChartKind, ChartSpec};

fn read_zip_entry(bytes: &[u8], name: &str) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("zip open");
    let mut s = String::new();
    zip.by_name(name)
        .unwrap_or_else(|e| panic!("entry {name}: {e}"))
        .read_to_string(&mut s)
        .unwrap();
    s
}

fn zip_has(bytes: &[u8], name: &str) -> bool {
    let cursor = std::io::Cursor::new(bytes);
    let zip = zip::ZipArchive::new(cursor).expect("zip open");
    let found = zip.file_names().any(|n| n == name);
    found
}

/// 测试数据面（3 系列行 × 2 值列 + 表头/类别）：
/// A1=类别头，B1/C1=系列名；A2:A4=类别；B2:B4/C2:C4=值。
/// 与图表同借用会话写入（挂载需同一 SheetWriter）。
fn write_sample_data(s: &mut SheetWriter) {
    let rows: [[&str; 3]; 4] = [
        ["季度", "销售额", "目标"],
        ["Q1", "120", "100"],
        ["Q2", "150", "130"],
        ["Q3", "90", "110"],
    ];
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            let cell = if r == 0 || c == 0 {
                CellValue::Text((*v).into())
            } else {
                CellValue::Number(v.parse().unwrap())
            };
            s.write(r as u32, c as u16, cell, None).unwrap();
        }
    }
}

fn spec(kind: ChartKind) -> ChartSpec {
    ChartSpec {
        kind,
        title: "季度销售".into(),
        data_range: "Sheet1!A1:C4".into(),
        series_in_columns: true,
        position: (0, 4),
        size: (480 * 9_525, 288 * 9_525),
    }
}

// ---------------------------------------------------------------------------
// 1. 四类图表：XML 部件 + 关联链 + 范围引用
// ---------------------------------------------------------------------------

#[test]
fn four_kinds_chart_parts_and_refs() {
    for (name, kind, chart_element) in [
        (
            "柱状",
            ChartKind::Bar {
                direction: BarDirection::Vertical,
                stacked: false,
            },
            "c:barChart",
        ),
        ("折线", ChartKind::Line { smooth: false }, "c:lineChart"),
        ("饼图", ChartKind::Pie { hole_size: None }, "c:pieChart"),
        ("散点", ChartKind::Scatter { lines: true }, "c:scatterChart"),
    ] {
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("Sheet1").unwrap();
            write_sample_data(&mut s);
            s.add_chart(spec(kind)).unwrap();
        }
        let bytes = w.save_to_vec().unwrap();

        // 部件存在：chart1.xml / drawing / rels 关联链。
        assert!(
            zip_has(&bytes, "xl/charts/chart1.xml"),
            "{name}: chart1.xml"
        );
        assert!(
            zip_has(&bytes, "xl/drawings/drawing1.xml"),
            "{name}: drawing1.xml"
        );
        let ct = read_zip_entry(&bytes, "[Content_Types].xml");
        assert!(
            ct.contains("drawingml.chart+xml"),
            "{name}: Content_Types 缺 chart 条目"
        );
        let sheet_rels = read_zip_entry(&bytes, "xl/worksheets/_rels/sheet1.xml.rels");
        assert!(
            sheet_rels.contains("drawings/drawing1.xml"),
            "{name}: sheet rels 未关联 drawing"
        );
        let drawing_rels = read_zip_entry(&bytes, "xl/drawings/_rels/drawing1.xml.rels");
        assert!(
            drawing_rels.contains("charts/chart1.xml"),
            "{name}: drawing rels 未关联 chart"
        );

        // 类型元素 + 标题 + 范围引用（引用而非快照——改数据图表跟随）。
        let chart = read_zip_entry(&bytes, "xl/charts/chart1.xml");
        assert!(
            chart.contains(&format!("<{chart_element}>")),
            "{name}: 类型元素"
        );
        assert!(chart.contains("季度销售"), "{name}: 标题");
        // 类别 = 首列数据面；系列 1 名/值 = B 列表头/数据面。
        assert!(
            chart.contains("<c:f>Sheet1!$A$2:$A$4</c:f>"),
            "{name}: 类别引用（实为 {}）",
            chart.lines().filter(|l| l.contains("<c:f>")).count()
        );
        assert!(
            chart.contains("<c:f>Sheet1!$B$1</c:f>"),
            "{name}: 系列名引用"
        );
        assert!(
            chart.contains("<c:f>Sheet1!$B$2:$B$4</c:f>"),
            "{name}: 系列值引用"
        );
        // 两值列 → 两系列。
        assert_eq!(chart.matches("<c:ser>").count(), 2, "{name}: 系列数");
    }
}

// ---------------------------------------------------------------------------
// 2. 系列拆分：列向 vs 行向（Excel 惯例——模块文档契约）
// ---------------------------------------------------------------------------

#[test]
fn series_split_rows_vs_columns() {
    let build = |in_columns: bool| -> Vec<u8> {
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("Sheet1").unwrap();
            write_sample_data(&mut s);
            s.add_chart(ChartSpec {
                series_in_columns: in_columns,
                ..spec(ChartKind::Line { smooth: false })
            })
            .unwrap();
        }
        w.save_to_vec().unwrap()
    };

    // 列向：2 系列（B/C 列），类别 A2:A4。
    let cols = read_zip_entry(&build(true), "xl/charts/chart1.xml");
    assert_eq!(cols.matches("<c:ser>").count(), 2, "列向系列数 = 值列数");
    assert!(
        cols.contains("<c:f>Sheet1!$C$2:$C$4</c:f>"),
        "列向第二系列值"
    );

    // 行向：3 系列（数据行 2/3/4），类别 = 首行数据面 B1:D1……本数据
    // 面（A1:C4）行向时类别 = B1:C1，系列值 = B2:C2 / B3:C3 / B4:C4。
    let rows = read_zip_entry(&build(false), "xl/charts/chart1.xml");
    assert_eq!(rows.matches("<c:ser>").count(), 3, "行向系列数 = 值行数");
    assert!(
        rows.contains("<c:f>Sheet1!$B$2:$C$2</c:f>"),
        "行向系列 1 值（行 2）"
    );
    assert!(
        rows.contains("<c:f>Sheet1!$B$4:$C$4</c:f>"),
        "行向系列 3 值（行 4）"
    );
    assert!(
        rows.contains("<c:f>Sheet1!$B$1:$C$1</c:f>"),
        "行向类别（首行数据面）"
    );
    assert!(
        rows.contains("<c:f>Sheet1!$A$2</c:f>"),
        "行向系列名（首列表头下）"
    );
}

// ---------------------------------------------------------------------------
// 3. 变体选项落盘（方向/堆积/平滑/孔径/散点形态）
// ---------------------------------------------------------------------------

#[test]
fn variant_options_written() {
    let chart_of = |kind: ChartKind| -> String {
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("Sheet1").unwrap();
            write_sample_data(&mut s);
            s.add_chart(spec(kind)).unwrap();
        }
        read_zip_entry(&w.save_to_vec().unwrap(), "xl/charts/chart1.xml")
    };

    let vertical = chart_of(ChartKind::Bar {
        direction: BarDirection::Vertical,
        stacked: false,
    });
    assert!(vertical.contains("<c:barDir val=\"col\"/>"), "竖向 = col");
    assert!(
        vertical.contains("<c:grouping val=\"clustered\"/>"),
        "默认并列"
    );

    let horizontal = chart_of(ChartKind::Bar {
        direction: BarDirection::Horizontal,
        stacked: true,
    });
    assert!(horizontal.contains("<c:barDir val=\"bar\"/>"), "横向 = bar");
    assert!(horizontal.contains("<c:grouping val=\"stacked\"/>"), "堆积");

    let smooth = chart_of(ChartKind::Line { smooth: true });
    assert!(
        smooth.contains("<c:smooth val=\"1\"/>"),
        "平滑曲线（逐系列）"
    );

    let doughnut = chart_of(ChartKind::Pie {
        hole_size: Some(50.0),
    });
    assert!(doughnut.contains("<c:doughnutChart>"), "孔径 Some = 环形");
    assert!(doughnut.contains("<c:holeSize val=\"50\"/>"), "孔径值");

    let pie = chart_of(ChartKind::Pie { hole_size: None });
    assert!(pie.contains("<c:pieChart>"), "孔径 None = 实心饼");
    assert!(!pie.contains("c:doughnutChart"), "实心饼无环形元素");

    let scatter_lines = chart_of(ChartKind::Scatter { lines: true });
    assert!(
        scatter_lines.contains("<c:scatterStyle val=\"lineMarker\"/>"),
        "连线形态"
    );

    let scatter_dots = chart_of(ChartKind::Scatter { lines: false });
    assert!(scatter_dots.contains("<c:scatterChart>"), "仅点形态");
}

// ---------------------------------------------------------------------------
// 4. 校验拒绝矩阵（typed，不触引擎）
// ---------------------------------------------------------------------------

#[test]
fn validation_typed_rejections() {
    let base = ChartKind::Bar {
        direction: BarDirection::Vertical,
        stacked: false,
    };
    let bad_specs: Vec<(&str, ChartSpec)> = vec![
        (
            "范围缺表名",
            ChartSpec {
                data_range: "A1:C4".into(),
                ..spec(base.clone())
            },
        ),
        (
            "半引用",
            ChartSpec {
                data_range: "Sheet1!A1:".into(),
                ..spec(base.clone())
            },
        ),
        (
            "单列（无值面）",
            ChartSpec {
                data_range: "Sheet1!A1:A4".into(),
                ..spec(base.clone())
            },
        ),
        (
            "单行（无数据面）",
            ChartSpec {
                data_range: "Sheet1!A1:C1".into(),
                ..spec(base.clone())
            },
        ),
        (
            "行越界",
            ChartSpec {
                data_range: "Sheet1!A1:C1048577".into(),
                ..spec(base.clone())
            },
        ),
        (
            "0 EMU 宽",
            ChartSpec {
                size: (0, 288 * 9_525),
                ..spec(base.clone())
            },
        ),
        (
            "孔径越界",
            ChartSpec {
                kind: ChartKind::Pie {
                    hole_size: Some(95.0),
                },
                ..spec(base.clone())
            },
        ),
        (
            "孔径零",
            ChartSpec {
                kind: ChartKind::Pie {
                    hole_size: Some(0.0),
                },
                ..spec(base.clone())
            },
        ),
        (
            "位置列越界",
            ChartSpec {
                position: (0, 99_999),
                ..spec(base.clone())
            },
        ),
    ];
    let mut w = WorkbookWriter::new();
    let bytes = {
        let mut s = w.add_sheet("Sheet1").unwrap();
        write_sample_data(&mut s);
        for (name, sp) in bad_specs {
            assert!(
                matches!(
                    s.add_chart(sp),
                    Err(mdc_office_io::write::WriteError::InvalidInput(_))
                ),
                "{name} 应 typed 拒绝"
            );
        }
        // s 此后不再使用——NLL 借用结束，无需显式 drop。
        w.save_to_vec().unwrap()
    };
    // 全拒绝后写器仍可正常收尾（无半态污染）。
    assert!(
        !zip_has(&bytes, "xl/charts/chart1.xml"),
        "全拒绝 = 无图表部件"
    );
}

// ---------------------------------------------------------------------------
// 5. LO 渲染墨迹对照（环境门；图表区墨迹显著多于对照簿）
// ---------------------------------------------------------------------------

#[test]
fn lo_render_chart_ink_visible() {
    if std::process::Command::new("soffice")
        .arg("--version")
        .output()
        .is_err()
        || std::process::Command::new("pdftoppm")
            .arg("-v")
            .output()
            .is_err()
    {
        eprintln!("SKIP: soffice/pdftoppm 缺失");
        return;
    }

    let build = |with_chart: bool| -> (usize, f64) {
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("Sheet1").unwrap();
            write_sample_data(&mut s);
            if with_chart {
                s.add_chart(spec(ChartKind::Bar {
                    direction: BarDirection::Vertical,
                    stacked: false,
                }))
                .unwrap();
            }
        }
        let bytes = w.save_to_vec().unwrap();
        let out_dir =
            std::env::temp_dir().join(format!("mdc-chart-lo-{}-{with_chart}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).unwrap();
        let src = out_dir.join("c.xlsx");
        std::fs::write(&src, &bytes).unwrap();
        let status = std::process::Command::new("soffice")
            .args([
                "--headless",
                &format!(
                    "-env:UserInstallation=file:///{}",
                    out_dir.join("lo-profile").display()
                ),
                "--convert-to",
                "pdf",
                "--outdir",
                out_dir.to_str().unwrap(),
                src.to_str().unwrap(),
            ])
            .output()
            .expect("soffice 调用失败");
        assert!(status.status.success(), "soffice pdf 转换失败");
        let pdf = out_dir.join("c.pdf");
        assert!(pdf.exists(), "pdf 产物缺失");
        let png_status = std::process::Command::new("pdftoppm")
            .args([
                "-png",
                "-r",
                "100",
                pdf.to_str().unwrap(),
                out_dir.join("page").to_str().unwrap(),
            ])
            .output()
            .expect("pdftoppm 调用失败");
        assert!(png_status.status.success(), "pdftoppm 失败");
        let page = out_dir.join("page-1.png");
        assert!(page.exists(), "png 产物缺失");
        let ratio = nonwhite_pixel_ratio(&page);
        let _ = std::fs::remove_dir_all(&out_dir);
        (bytes.len(), ratio)
    };

    let (len_without, without) = build(false);
    let (len_with, with) = build(true);
    eprintln!(
        "墨迹占比：对照={without:.4} 图表={with:.4}；体积：对照={len_without}B 图表={len_with}B"
    );
    assert!(
        with > without + 0.02,
        "图表簿墨迹 {with:.4} 应显著高于对照 {without:.4}——LO 渲染出图体"
    );
    // 体积健全性：图部件（chart+drawing+rels 三件）增量应有限。
    assert!(
        len_with > len_without && len_with - len_without < 100 * 1024,
        "图表部件增量 {}B 应在 (0, 100KB) 区间",
        len_with as i64 - len_without as i64
    );
}

/// 非白像素占比（任一通道 < 240——网格线/文本/图体皆计入）。
fn nonwhite_pixel_ratio(png: &std::path::Path) -> f64 {
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(format!(
            "from PIL import Image; \
             im = Image.open({png:?}).convert('RGB'); \
             px = list(im.getdata()); n = len(px); \
             nw = sum(1 for r,g,b in px if r<240 or g<240 or b<240); \
             print(nw/n)"
        ))
        .output()
        .expect("python3 调用失败");
    assert!(
        out.status.success(),
        "PIL 像素统计失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("像素占比解析")
}
