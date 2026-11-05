//! #133 任务 2：高级表格写面（`DocxWriter::table_advanced`）——
//! 合并（gridSpan/vMerge）/ 底纹（w:shd）/ 边框（w:tcBorders）/
//! 列宽（w:tblGrid + w:tcW）。
//!
//! 三层验证：
//!
//! 1. **roundtrip 语义对账**：`grid_span`/`v_merge` 发射 → 引擎序列化
//!    → 读面（[`TableData`] 栅格展开 + `merges` 矩形清单）——与
//!    docx_merged_cells.rs 的读面语义互为镜像（同语义两侧独立钉定）；
//! 2. **XML 字节断言**：底纹/边框/列宽是纯写面装饰（docx-rs 反序列化
//!    不回读——与 tblHeader 同登记口径），解 zip 断言 document.xml
//!    的属性 token（contains 断言与属性顺序无关）；
//! 3. **LibreOffice 开档代验**（Word 兼容代理——真 Word 验收登记
//!    Lead 真机）：`soffice --headless --convert-to pdf` 成功 +
//!    pdftotext 含表格内容。工具缺失打日志跳过不阻塞（循 golden.rs
//!    跨实现验证惯例——加分项不拦纯 Rust 环境；本沙箱实测有 soffice）。
//!
//! V10 粘性校验矩阵（save 漏斗 typed 拒 + 零产物——与 V7 同纪律）：
//! grid_span 0 / 底纹色非法 / 边框色非法 / 边框尺寸越域 / 格宽 0 /
//! 列宽 0 / 续格带文本 / 空表空行。

use std::io::Read as _;

use mdc_office_io::docx::{
    read_docx_from_bytes, Block, CellBorderEdge, CellBorderStyle, CellBorders, CellVMerge,
    DocxWriter, MergeRange, TableCellSpec, TableData, TableSpec,
};

fn tmp(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("mdc_io_tbl_{name}_{}.docx", std::process::id()));
    p.to_string_lossy().into_owned()
}

/// zip 单条目读取（docx_roundtrip.rs 同款——容器门不拦自有写面产物）。
fn zip_read_entry(path: &str, entry: &str) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let mut za = zip::ZipArchive::new(f).ok()?;
    let mut e = za.by_name(entry).ok()?;
    let mut s = String::new();
    e.read_to_string(&mut s).ok()?;
    Some(s)
}

fn body_tables(bytes: &[u8]) -> Vec<TableData> {
    let doc = read_docx_from_bytes(bytes).expect("read_docx_from_bytes");
    doc.blocks
        .into_iter()
        .filter_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn mr(r0: usize, c0: usize, r1: usize, c1: usize) -> MergeRange {
    MergeRange {
        start_row: r0,
        start_col: c0,
        end_row: r1,
        end_col: c1,
    }
}

fn owned(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| r.iter().map(|s| s.to_string()).collect())
        .collect()
}

fn edge(style: CellBorderStyle, size: u8, color: &str) -> CellBorderEdge {
    CellBorderEdge {
        style,
        size_eighths: size,
        color: color.to_string(),
    }
}

// ---------------------------------------------------------------------------
// ① 合并 roundtrip（读面语义镜像）
// ---------------------------------------------------------------------------

/// 纵合并（restart-continue）+ 横合并（gridSpan）同表 → 读回
/// 两条矩形 + 栅格展开（覆盖格/续格置空——内容居原点）。
#[test]
fn merges_roundtrip_vertical_and_horizontal() {
    let mut w = DocxWriter::new();
    w.table_advanced(TableSpec {
        rows: vec![
            vec![
                TableCellSpec {
                    text: "纵向".into(),
                    v_merge: CellVMerge::Restart,
                    ..Default::default()
                },
                TableCellSpec {
                    text: "横向".into(),
                    grid_span: 2,
                    ..Default::default()
                },
            ],
            vec![
                TableCellSpec {
                    text: String::new(),
                    v_merge: CellVMerge::Continue,
                    ..Default::default()
                },
                TableCellSpec::text("y"),
                TableCellSpec::text("z"),
            ],
        ],
        column_widths: vec![2000, 2000, 2000],
    });
    let tables = body_tables(&w.save_to_vec().expect("save"));
    assert_eq!(tables.len(), 1);
    // 顺序无关断言（读面 merges 为文档扫描序——循 word-merged 夹具
    // 测试先例：len + contains × 2）。
    assert_eq!(tables[0].merges.len(), 2, "纵 + 恰两处合并");
    assert!(tables[0].merges.contains(&mr(0, 0, 1, 0)), "纵 (0,0)-(1,0)");
    assert!(tables[0].merges.contains(&mr(0, 1, 0, 2)), "横 (0,1)-(0,2)");
    assert_eq!(
        tables[0].rows,
        owned(&[&["纵向", "横向", ""], &["", "y", "z"]])
    );
}

/// 块合并（vMerge + gridSpan 叠加）：单条矩形不拆两条（读面语义）。
#[test]
fn block_merge_roundtrip_single_rectangle() {
    let mut w = DocxWriter::new();
    w.table_advanced(TableSpec {
        rows: vec![
            vec![
                TableCellSpec {
                    text: "块".into(),
                    grid_span: 2,
                    v_merge: CellVMerge::Restart,
                    ..Default::default()
                },
                TableCellSpec::text("c"),
            ],
            vec![
                TableCellSpec {
                    text: String::new(),
                    grid_span: 2,
                    v_merge: CellVMerge::Continue,
                    ..Default::default()
                },
                TableCellSpec::text("f"),
            ],
        ],
        column_widths: vec![1500, 1500, 1500],
    });
    let tables = body_tables(&w.save_to_vec().expect("save"));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].merges, vec![mr(0, 0, 1, 1)], "块合并单矩形");
    assert_eq!(tables[0].rows, owned(&[&["块", "", "c"], &["", "", "f"]]));
}

/// 无 tblGrid（column_widths 空）：引擎行和兜底——读面同语义
/// （镜像 docx_merged_cells::missing_tbl_grid_falls_back_to_row_sums）。
#[test]
fn no_grid_falls_back_to_row_sums() {
    let mut w = DocxWriter::new();
    w.table_advanced(TableSpec {
        rows: vec![vec![
            TableCellSpec {
                text: "AB".into(),
                grid_span: 2,
                ..Default::default()
            },
            TableCellSpec::text("C"),
        ]],
        column_widths: Vec::new(),
    });
    let tables = body_tables(&w.save_to_vec().expect("save"));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].merges, vec![mr(0, 0, 0, 1)]);
    assert_eq!(tables[0].rows, owned(&[&["AB", "", "C"]]));
}

/// 等价性钉定：纯文本格 `table_advanced` ≡ 既有 `table`（同内容
/// 读回 TableData 全等——并存面不引入语义漂移）。
#[test]
fn plain_spec_equivalent_to_table() {
    let rows = [vec!["甲", "乙"], vec!["1", "2"]];
    let mut w1 = DocxWriter::new();
    w1.table(
        &rows
            .iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect::<Vec<_>>(),
    );
    let mut w2 = DocxWriter::new();
    w2.table_advanced(TableSpec {
        rows: rows
            .iter()
            .map(|r| r.iter().map(|s| TableCellSpec::text(*s)).collect())
            .collect(),
        column_widths: Vec::new(),
    });
    let t1 = body_tables(&w1.save_to_vec().expect("save"));
    let t2 = body_tables(&w2.save_to_vec().expect("save"));
    assert_eq!(t1, t2, "纯文本格两入口读回全等");
}

// ---------------------------------------------------------------------------
// ② XML 字节断言（纯写面装饰——docx-rs 不回读，字节钉定）
// ---------------------------------------------------------------------------

/// 底纹 + 边框 + 格宽 + 列宽的 document.xml token 断言。
#[test]
fn shading_borders_widths_xml_tokens() {
    let p = tmp("xml_tokens");
    let mut w = DocxWriter::new();
    w.table_advanced(TableSpec {
        rows: vec![vec![
            TableCellSpec {
                text: "装饰格".into(),
                background: Some("#4f8ef7".into()),
                borders: Some(CellBorders {
                    top: Some(edge(CellBorderStyle::Single, 4, "auto")),
                    left: Some(edge(CellBorderStyle::Double, 8, "#cc66cc")),
                    bottom: Some(edge(CellBorderStyle::Nil, 1, "auto")),
                    right: Some(edge(CellBorderStyle::Thick, 24, "auto")),
                }),
                width: Some(2500),
                ..Default::default()
            },
            TableCellSpec::text("素格"),
        ]],
        column_widths: vec![2500, 3500],
    });
    w.save_to_path(&p).expect("save");

    let xml = zip_read_entry(&p, "word/document.xml").expect("document.xml 在位");
    // 底纹：w:val="clear" + 大写无前缀 fill（OOXML ST_HexColor 形）。
    assert!(
        xml.contains("w:fill=\"4F8EF7\""),
        "底纹 fill 大写无前缀：{}",
        &xml[xml.find("<w:tbl>").unwrap_or(0)..]
    );
    assert!(xml.contains("w:shd"), "底纹元素在位");
    // 列宽：tblGrid + gridCol。
    assert!(xml.contains("w:tblGrid"), "tblGrid 在位");
    assert!(
        xml.contains("w:w=\"2500\"") && xml.contains("w:w=\"3500\""),
        "gridCol 列宽"
    );
    // 格宽：tcW dxa。
    assert!(xml.contains("w:tcW"), "格宽在位");
    // 边框：tcBorders 四边 + 线型 + 尺寸 + 色。
    assert!(xml.contains("w:tcBorders"), "边框容器在位");
    assert!(xml.contains("w:val=\"double\""), "双线");
    assert!(xml.contains("w:val=\"thick\""), "粗线");
    assert!(xml.contains("w:sz=\"8\""), "边框尺寸 1 磅（8/8）");
    assert!(
        xml.contains("w:color=\"CC66CC\"") || xml.contains("w:color=\"cc66cc\""),
        "边框色大写形"
    );
    // 合并面不误发（本表无合并）：gridSpan 缺席。
    assert!(!xml.contains("w:gridSpan"), "grid_span=1 不发 gridSpan");
    let _ = std::fs::remove_file(&p);
}

/// 合并 XML：gridSpan/vMerge 精确 token。
#[test]
fn merge_xml_tokens() {
    let p = tmp("merge_tokens");
    let mut w = DocxWriter::new();
    w.table_advanced(TableSpec {
        rows: vec![
            vec![
                TableCellSpec {
                    text: "A".into(),
                    v_merge: CellVMerge::Restart,
                    ..Default::default()
                },
                TableCellSpec {
                    text: "BC".into(),
                    grid_span: 2,
                    ..Default::default()
                },
            ],
            vec![
                TableCellSpec {
                    text: String::new(),
                    v_merge: CellVMerge::Continue,
                    ..Default::default()
                },
                TableCellSpec::text("y"),
                TableCellSpec::text("z"),
            ],
        ],
        column_widths: vec![1000, 1000, 1000],
    });
    w.save_to_path(&p).expect("save");
    let xml = zip_read_entry(&p, "word/document.xml").expect("document.xml 在位");
    assert!(xml.contains("w:gridSpan w:val=\"2\""), "横向合并跨 2 列");
    assert!(
        xml.contains("w:vMerge w:val=\"restart\"") && xml.contains("w:vMerge w:val=\"continue\""),
        "纵向合并 restart + continue"
    );
    let _ = std::fs::remove_file(&p);
}

// ---------------------------------------------------------------------------
// ③ V10 粘性校验矩阵（save 漏斗 typed 拒 + 零产物）
// ---------------------------------------------------------------------------

/// 8 类违规逐一钉定：save 以 `DocxError::InvalidInput` 拒（消息含
/// V10 标记）且无产物字节；首错粘性（后续 save 恒拒）。
#[test]
fn v10_matrix_sticky_rejects() {
    let bad_bg = || {
        let mut w = DocxWriter::new();
        w.table_advanced(TableSpec {
            rows: vec![vec![TableCellSpec {
                text: "x".into(),
                background: Some("purple".into()),
                ..Default::default()
            }]],
            column_widths: Vec::new(),
        });
        w
    };
    let cases: Vec<(&str, DocxWriter)> = vec![
        ("grid_span 0", {
            let mut w = DocxWriter::new();
            w.table_advanced(TableSpec {
                rows: vec![vec![TableCellSpec {
                    text: "x".into(),
                    grid_span: 0,
                    ..Default::default()
                }]],
                column_widths: Vec::new(),
            });
            w
        }),
        ("底纹色非法", bad_bg()),
        ("边框色非法", {
            let mut w = DocxWriter::new();
            w.table_advanced(TableSpec {
                rows: vec![vec![TableCellSpec {
                    text: "x".into(),
                    borders: Some(CellBorders {
                        top: Some(edge(CellBorderStyle::Single, 4, "红色")),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]],
                column_widths: Vec::new(),
            });
            w
        }),
        ("边框尺寸越域", {
            let mut w = DocxWriter::new();
            w.table_advanced(TableSpec {
                rows: vec![vec![TableCellSpec {
                    text: "x".into(),
                    borders: Some(CellBorders {
                        left: Some(edge(CellBorderStyle::Single, 0, "auto")),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]],
                column_widths: Vec::new(),
            });
            w
        }),
        ("格宽 0", {
            let mut w = DocxWriter::new();
            w.table_advanced(TableSpec {
                rows: vec![vec![TableCellSpec {
                    text: "x".into(),
                    width: Some(0),
                    ..Default::default()
                }]],
                column_widths: Vec::new(),
            });
            w
        }),
        ("列宽 0", {
            let mut w = DocxWriter::new();
            w.table_advanced(TableSpec {
                rows: vec![vec![TableCellSpec::text("x")]],
                column_widths: vec![0, 1000],
            });
            w
        }),
        ("续格带文本", {
            let mut w = DocxWriter::new();
            w.table_advanced(TableSpec {
                rows: vec![vec![TableCellSpec {
                    text: "会被静默丢的内容".into(),
                    v_merge: CellVMerge::Continue,
                    ..Default::default()
                }]],
                column_widths: Vec::new(),
            });
            w
        }),
        ("空行", {
            let mut w = DocxWriter::new();
            w.table_advanced(TableSpec {
                rows: vec![Vec::new()],
                column_widths: Vec::new(),
            });
            w
        }),
    ];
    for (name, mut w) in cases {
        let err = w.save_to_vec().expect_err(&format!("{name}：save 须拒"));
        let msg = match &err {
            mdc_office_io::docx::DocxError::InvalidInput(m) => m.clone(),
            other => panic!("{name}：应为 InvalidInput（实得 {other:?}）"),
        };
        assert!(msg.starts_with("V10:"), "{name}：消息含 V10 标记（{msg}）");
        // 粘性：修复不可行（发射已入引擎文档）——二次 save 恒拒同错。
        let again = w.save_to_vec();
        assert!(again.is_err(), "{name}：粘性恒拒");
    }
}

/// 合法域边界恰通过（1/96 端点 + auto 色 + 大小写色）。
#[test]
fn v10_boundary_accepts() {
    let mut w = DocxWriter::new();
    w.table_advanced(TableSpec {
        rows: vec![vec![
            TableCellSpec {
                text: "a".into(),
                background: Some("#AbCdEf".into()),
                borders: Some(CellBorders {
                    top: Some(edge(CellBorderStyle::Dotted, 1, "#010203")),
                    bottom: Some(edge(CellBorderStyle::Dashed, 96, "auto")),
                    ..Default::default()
                }),
                width: Some(1),
                ..Default::default()
            },
            TableCellSpec {
                text: "b".into(),
                grid_span: 3,
                ..Default::default()
            },
        ]],
        column_widths: vec![1, 1000, 1000, 1000],
    });
    let bytes = w.save_to_vec().expect("边界值全合法");
    assert!(!bytes.is_empty());
}

// ---------------------------------------------------------------------------
// ④ LibreOffice 开档代验（Word 兼容代理——真 Word 验收登记 Lead 真机）
// ---------------------------------------------------------------------------

/// 完整标书表（合并 + 底纹 + 边框 + 列宽）→ soffice 转 PDF 成功 +
/// pdftotext 含表格内容。工具缺失打日志跳过（不阻塞纯 Rust 环境——
/// golden.rs 跨实现验证同惯例）。
#[test]
fn libreoffice_opens_full_bid_table() {
    let soffice = std::process::Command::new("which")
        .arg("soffice")
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    if !soffice {
        eprintln!("libreoffice 代验跳过：无 soffice（Word 兼容代理证据需 Lead 真机/带 LO 环境）");
        return;
    }

    let p = tmp("lo_bid");
    let mut w = DocxWriter::new();
    w.heading(1, "报价汇总表").unwrap();
    w.table_advanced(TableSpec {
        rows: vec![
            vec![
                TableCellSpec {
                    text: "项目".into(),
                    v_merge: CellVMerge::Restart,
                    background: Some("#D9E2F3".into()),
                    borders: Some(CellBorders {
                        top: Some(CellBorderEdge::single()),
                        left: Some(CellBorderEdge::single()),
                        bottom: Some(CellBorderEdge::single()),
                        right: Some(CellBorderEdge::single()),
                    }),
                    ..Default::default()
                },
                TableCellSpec {
                    text: "报价（万元）".into(),
                    grid_span: 2,
                    background: Some("#D9E2F3".into()),
                    borders: Some(CellBorders {
                        top: Some(edge(CellBorderStyle::Double, 8, "auto")),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            vec![
                TableCellSpec {
                    text: String::new(),
                    v_merge: CellVMerge::Continue,
                    ..Default::default()
                },
                TableCellSpec::text("一期"),
                TableCellSpec::text("120"),
            ],
        ],
        column_widths: vec![3000, 2500, 2500],
    });
    w.save_to_path(&p).expect("save");

    let outdir = std::env::temp_dir().join(format!("mdc_io_tbl_lo_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outdir);
    std::fs::create_dir_all(&outdir).unwrap();
    let conv = std::process::Command::new("soffice")
        .args([
            "--headless",
            "--convert-to",
            "pdf",
            "--outdir",
            outdir.to_str().unwrap(),
            &p,
        ])
        .output()
        .expect("soffice 进程");
    assert!(
        conv.status.success(),
        "LibreOffice 转换失败：{}",
        String::from_utf8_lossy(&conv.stderr)
    );
    // soffice 输出名 = 输入 stem（tmp() 带 pid 后缀）。
    let stem = std::path::Path::new(&p)
        .file_stem()
        .unwrap()
        .to_string_lossy();
    let pdf = outdir.join(format!("{stem}.pdf"));
    let pdf_bytes =
        std::fs::read(&pdf).unwrap_or_else(|e| panic!("PDF 产物缺失（{}）：{e}", pdf.display()));
    assert!(pdf_bytes.len() > 1000, "PDF 非空壳：{}B", pdf_bytes.len());
    assert_eq!(&pdf_bytes[..5], b"%PDF-", "PDF 魔数");

    // 内容代验：pdftotext 提取表格文本。
    let txt = std::process::Command::new("pdftotext")
        .arg(&pdf)
        .arg("-")
        .output()
        .expect("pdftotext 进程");
    let text = String::from_utf8_lossy(&txt.stdout);
    for marker in ["报价汇总表", "项目", "报价（万元）", "一期", "120"] {
        assert!(
            text.replace('　', "").contains(marker),
            "PDF 文本应含 {marker}（提取文本：{text}）"
        );
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_dir_all(&outdir);
}
