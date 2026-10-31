//! 条件格式写面（issue #113 任务 2）——行为锚点测试。
//!
//! 覆盖：五种形态 XML 面断言（conditionalFormatting 元素/类型/阈值/
//! dxf 样式落盘）+ 范围解析通过/拒绝矩阵 + 校验拒绝（配对数/阈值数/
//! 颜色格式/数据条序）+ 多规则共存 + LibreOffice 渲染像素对照
//! （环境门——soffice 缺失 SKIP）。
//!
//! 「与 Excel 原生文件对照」登记：本环境无 Excel——对照面 =
//! rust_xlsxwriter 标准形态（其上游对照 Excel 测试）+ LO 渲染视觉
//! 验证（像素级断言命中格颜色）。

use std::io::Read as _;

use mdc_office_io::write::CellStyle;
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdc_office_io::xlsx_condfmt::{CompareOp, CondFormat, IconSetKind};

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

fn cell_style_yellow() -> CellStyle {
    CellStyle {
        fill_color: Some("#FFFF00".into()),
        bold: true,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// 五种形态 XML 面断言
// ---------------------------------------------------------------------------

#[test]
fn condfmt_databar_written() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    s.write(0, 0, CellValue::Number(10.0), None).unwrap();
    s.add_cond_format(
        "A1:A4",
        CondFormat::DataBar {
            min: 0.0,
            max: 100.0,
            color: "#638EC6".into(),
        },
    )
    .unwrap();
    let bytes = w.save_to_vec().unwrap();
    let sheet = read_zip_entry(&bytes, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("<conditionalFormatting sqref=\"A1:A4\">"),
        "范围落盘"
    );
    assert!(sheet.contains("<dataBar>"), "dataBar 类型");
    assert!(sheet.contains("type=\"num\" val=\"0\""), "min 数值阈值");
    assert!(sheet.contains("type=\"num\" val=\"100\""), "max 数值阈值");
    assert!(sheet.contains("FF638EC6"), "数据条颜色（ARGB）");
}

#[test]
fn condfmt_colorscale_2_and_3_written() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    s.write(0, 0, CellValue::Number(10.0), None).unwrap();
    s.add_cond_format(
        "A1:A4",
        CondFormat::ColorScale {
            colors: vec!["#F8696B".into(), "#5A8AC6".into()],
            thresholds: vec![0.0, 100.0],
        },
    )
    .unwrap();
    s.add_cond_format(
        "B1:B4",
        CondFormat::ColorScale {
            colors: vec!["#F8696B".into(), "#FFEB84".into(), "#63BE7B".into()],
            thresholds: vec![0.0, 50.0, 100.0],
        },
    )
    .unwrap();
    let bytes = w.save_to_vec().unwrap();
    let sheet = read_zip_entry(&bytes, "xl/worksheets/sheet1.xml");
    // 2 色色阶：两端 cfvo + 两色。
    assert!(sheet.contains("<colorScale>"), "colorScale 类型");
    assert!(
        sheet.contains("FFF8696B") && sheet.contains("FF5A8AC6"),
        "2 色端色"
    );
    // 3 色：中点阈值。
    assert!(sheet.contains("type=\"num\" val=\"50\""), "3 色中点阈值");
    assert!(
        sheet.contains("FFFFEB84") && sheet.contains("FF63BE7B"),
        "3 色"
    );
}

#[test]
fn condfmt_iconset_written() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    s.write(0, 0, CellValue::Number(10.0), None).unwrap();
    s.add_cond_format(
        "A1:A6",
        CondFormat::IconSet {
            kind: IconSetKind::ThreeTrafficLights,
            thresholds: vec![33.0, 67.0],
        },
    )
    .unwrap();
    // 四箭头：类型属性显式（三灯 = Excel 默认值，省略形态——探针钉定）。
    s.add_cond_format(
        "B1:B8",
        CondFormat::IconSet {
            kind: IconSetKind::FourArrows,
            thresholds: vec![25.0, 50.0, 75.0],
        },
    )
    .unwrap();
    let bytes = w.save_to_vec().unwrap();
    let sheet = read_zip_entry(&bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains("type=\"iconSet\""), "图标集规则类型");
    // 三灯 = 默认值省略形态；首档 percent 0 + 自定义数值阈值。
    assert!(
        sheet.contains("<iconSet><cfvo type=\"percent\" val=\"0\"/>"),
        "首档 percent 0（三灯默认省略形态）"
    );
    assert!(sheet.contains("type=\"num\" val=\"33\""), "阈值 33");
    assert!(sheet.contains("type=\"num\" val=\"67\""), "阈值 67");
    assert!(sheet.contains("iconSet=\"4Arrows\""), "四箭头类型属性显式");
}

#[test]
fn condfmt_cellrule_written_with_dxf() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    s.write(0, 0, CellValue::Number(10.0), None).unwrap();
    s.add_cond_format(
        "A1:A4",
        CondFormat::CellRule {
            op: CompareOp::Greater,
            value: 80.0,
            style: cell_style_yellow(),
        },
    )
    .unwrap();
    let bytes = w.save_to_vec().unwrap();
    let sheet = read_zip_entry(&bytes, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("<cfRule type=\"cellIs\"") && sheet.contains("operator=\"greaterThan\""),
        "cellIs + 运算符"
    );
    assert!(sheet.contains("<formula>80</formula>"), "比较值");
    // dxf 样式：sheet 的 dxfId 引用 styles.xml 的 dxfs 条目（黄色填充）。
    assert!(sheet.contains("dxfId="), "dxfId 引用");
    let styles = read_zip_entry(&bytes, "xl/styles.xml");
    assert!(styles.contains("<dxfs"), "dxfs 落盘");
    assert!(styles.contains("FFFFFF00"), "命中样式黄色填充");
}

#[test]
fn condfmt_duplicate_written() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    s.write(0, 0, CellValue::Number(10.0), None).unwrap();
    s.add_cond_format(
        "A1:A8",
        CondFormat::Duplicate {
            style: cell_style_yellow(),
        },
    )
    .unwrap();
    let bytes = w.save_to_vec().unwrap();
    let sheet = read_zip_entry(&bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains("type=\"duplicateValues\""), "重复值类型");
}

#[test]
fn condfmt_multiple_rules_coexist() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    for (i, v) in [10.0, 40.0, 70.0, 90.0].iter().enumerate() {
        s.write(i as u32, 0, CellValue::Number(*v), None).unwrap();
    }
    s.add_cond_format(
        "A1:A4",
        CondFormat::DataBar {
            min: 0.0,
            max: 100.0,
            color: "#638EC6".into(),
        },
    )
    .unwrap();
    s.add_cond_format(
        "A1:A4",
        CondFormat::CellRule {
            op: CompareOp::Greater,
            value: 80.0,
            style: cell_style_yellow(),
        },
    )
    .unwrap();
    let bytes = w.save_to_vec().unwrap();
    let sheet = read_zip_entry(&bytes, "xl/worksheets/sheet1.xml");
    let count = sheet.matches("<cfRule").count();
    assert_eq!(count, 2, "两条规则共存");
    // priority 递增（引擎分配——1 起）。
    assert!(sheet.contains("priority=\"1\"") && sheet.contains("priority=\"2\""));
}

// ---------------------------------------------------------------------------
// 范围解析矩阵
// ---------------------------------------------------------------------------

#[test]
fn condfmt_range_parse_matrix() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    s.write(0, 0, CellValue::Number(1.0), None).unwrap();

    // 通过面：区间 / 单格 / $ 前缀 / 多字母列（AA = col 26）。
    for range in ["A1:C10", "B2", "$A$1:$C$10", "AA1:AB2"] {
        s.add_cond_format(
            range,
            CondFormat::Duplicate {
                style: CellStyle::default(),
            },
        )
        .unwrap_or_else(|e| panic!("range {range} 应通过：{e}"));
    }
    let bytes = w.save_to_vec().unwrap();
    let sheet = read_zip_entry(&bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains("sqref=\"A1:C10\""), "A1:C10");
    assert!(sheet.contains("sqref=\"B2\""), "单格");
    assert!(sheet.contains("sqref=\"A1:C10\""), "$ 剥除");
    assert!(sheet.contains("sqref=\"AA1:AB2\""), "多字母列");
}

// ---------------------------------------------------------------------------
// 校验拒绝
// ---------------------------------------------------------------------------

#[test]
fn condfmt_validation_rejects() {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("d").unwrap();
    s.write(0, 0, CellValue::Number(1.0), None).unwrap();

    // 范围拒绝面：非法记法 / 反序 / 越域列 / 三段。
    for range in ["1A:C10", "A10:A1", "A1:ZZZ1", "A1:B2:C3"] {
        let r = s.add_cond_format(
            range,
            CondFormat::Duplicate {
                style: CellStyle::default(),
            },
        );
        assert!(r.is_err(), "range {range} 应拒绝");
    }

    // 色阶配对数：4 色（>3）拒；色/阈值长度不齐拒。
    for cf in [
        CondFormat::ColorScale {
            colors: vec!["#000000".into(); 4],
            thresholds: vec![0.0, 25.0, 50.0, 100.0],
        },
        CondFormat::ColorScale {
            colors: vec!["#000000".into(), "#FFFFFF".into()],
            thresholds: vec![0.0],
        },
    ] {
        assert!(s.add_cond_format("A1:A4", cf).is_err(), "色阶配对校验应拒");
    }

    // 图标阈值数：Three* 期望 2，给 3 拒。
    assert!(s
        .add_cond_format(
            "A1:A4",
            CondFormat::IconSet {
                kind: IconSetKind::ThreeTrafficLights,
                thresholds: vec![1.0, 2.0, 3.0],
            },
        )
        .is_err());

    // 颜色格式：非 #RRGGBB 拒。
    for bad in ["638EC6", "#638EC", "#GGGGGG", ""] {
        assert!(
            s.add_cond_format(
                "A1:A4",
                CondFormat::DataBar {
                    min: 0.0,
                    max: 1.0,
                    color: bad.into(),
                },
            )
            .is_err(),
            "颜色 {bad:?} 应拒"
        );
    }

    // 数据条 min > max 拒。
    assert!(s
        .add_cond_format(
            "A1:A4",
            CondFormat::DataBar {
                min: 10.0,
                max: 5.0,
                color: "#638EC6".into(),
            },
        )
        .is_err());
}

// ---------------------------------------------------------------------------
// LibreOffice 渲染像素对照（环境门——「渲染成图对比」；**对照式断言**：
// 无条件格式对照簿黄像素 = 0，有条件格式簿 > 0——形态无关的生效性
// 证明，避免「单格占整页比例」这类绝对阈值脆断言）
// ---------------------------------------------------------------------------

#[test]
fn condfmt_lo_render_pixel_check() {
    let probe = std::process::Command::new("soffice")
        .arg("--version")
        .output();
    if probe.is_err() {
        eprintln!("[condfmt] soffice 缺失 — SKIP（环境门）");
        return;
    }

    let build = |with_cf: bool| -> f64 {
        // A 列 4 值；CellRule > 80 黄色填充——A4（90）命中，A1（10）不命中。
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet("data").unwrap();
        for (i, v) in [10.0, 40.0, 70.0, 90.0].iter().enumerate() {
            s.write(i as u32, 0, CellValue::Number(*v), None).unwrap();
        }
        if with_cf {
            s.add_cond_format(
                "A1:A4",
                CondFormat::CellRule {
                    op: CompareOp::Greater,
                    value: 80.0,
                    style: cell_style_yellow(),
                },
            )
            .unwrap();
        }
        let bytes = w.save_to_vec().unwrap();

        // LO → pdf → png → 黄像素占比。
        let out_dir =
            std::env::temp_dir().join(format!("mdc-condfmt-lo-{}-{with_cf}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).unwrap();
        let src = out_dir.join("cf.xlsx");
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
        let pdf = out_dir.join("cf.pdf");
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
        assert!(page.exists(), "png 产物缺失：{}", page.display());
        let ratio = yellow_pixel_ratio(&page);
        let _ = std::fs::remove_dir_all(&out_dir);
        ratio
    };

    let without = build(false);
    let with = build(true);
    assert_eq!(without, 0.0, "对照簿（无条件格式）黄像素应为零");
    assert!(
        with > 0.0005,
        "条件格式簿黄像素 {with:.5} 应显著 > 0——LO 渲染命中格（A4=90 > 80）"
    );
}

/// 黄色系像素占比（R/G 高、B 低——LO 黄填充渲染面）。
fn yellow_pixel_ratio(png: &std::path::Path) -> f64 {
    // 简单 PNG 像素读：借 Python PIL（Rust 侧无 png 解码依赖——测试
    // 面不引三方依赖纪律）。
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(format!(
            "from PIL import Image; import sys; \
             im = Image.open({png:?}).convert('RGB'); \
             px = list(im.getdata()); n = len(px); \
             y = sum(1 for r,g,b in px if r>200 and g>200 and b<120); \
             print(y/n)"
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
