//! S1 样式模板体系——真实语料 + LibreOffice 开档 + 写读 XML diff
//! 验证（feat/79-style-template）。
//!
//! 语料 = `test-artifacts/health-check/corpus/`（204 件真实 docx：Apache
//! POI test-data 130 + LibreOffice core 主题相关 73 + 仓库夹具 1——
//! 2026-10-31 经 GitHub 公开仓收集；目录 untracked，CI 缺席时测试
//! 空跑通过[1 test/pass]，本地在场全量断言）。
//!
//! 实况（2026-10-31 首轮，204 件）：可读 187（**17 件容器损坏 =
//! POI fuzzer 故意破坏样本，门 1 zip 拒绝 typed——与 `read_docx`
//! 同门同拒**）；有样式表 183 / docDefaults 151 / `w:default` 位
//! 169 / basedOn 链 137 / themeColor 引用 37 / 完整 12 槽主题 171；
//! 真实生态**零环零悬空**（Word 自产干净——异常面留给手搓测试）；
//! 最深链 10（NumberedList.docx，322 个样式）。
//!
//! 钉定样本（防跑空断言）：
//! - `lo__ThemeDocument.docx`（LibreOffice desktop/qa/data）：theme
//!   12 槽 + accent1=4472C4/accent5=5B9BD5/hyperlink=0563C1（标准
//!   Office 主题色值——**w:themeColor 解析链的实值锚点**）；
//! - `lo__NumberedList.docx`（LibreOffice ooxmlexport）：322 样式 +
//!   最深链 10（DSTOC8-9 → DSTOC7-9 → … → DSTOC1-9 → Heading9 →
//!   Normal；v2 重校：本批 Heading6-9 basedOn 直接 Normal[LO 导出
//!   风格]，chain("Heading9") 实深 2）。

use mdc_office_io::docx::{DocxWriter, RunSpec};
use mdc_office_io::style::{
    read_docx_styles_from_bytes, StyleColor, StyleDef, StyleRunProps, StyleType,
};

/// 语料目录（环境变量可覆盖；默认 test-artifacts/health-check/corpus）。
fn corpus_dir() -> std::path::PathBuf {
    std::env::var("STYLE_CORPUS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            // 2 级上溯 = 仓根（crates/office-io → 仓库）；与
            // external_corpus.rs 的 ../../test-artifacts 先例同构。
            // 3 级上溯会越出仓根（parent-of-repo），任何 clone 布局
            // 下都找不到语料——v2 rebase 时修正。
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test-artifacts/health-check/corpus")
        })
}

fn corpus_files() -> Vec<std::path::PathBuf> {
    let dir = corpus_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "docx").unwrap_or(false))
        .collect();
    v.sort();
    v
}

#[test]
fn real_corpus_style_stats_and_pinned() {
    let files = corpus_files();
    if files.is_empty() {
        eprintln!(
            "[style-corpus] corpus dir absent ({}) — skipping real-document sweep",
            corpus_dir().display()
        );
        return;
    }
    assert!(files.len() >= 200, "corpus shrank: {}", files.len());

    let (mut ok, mut reject) = (0usize, 0usize);
    let (mut with_styles, mut with_basedon, mut with_theme_color, mut theme_full) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut warn_dangling, mut warn_cycle, mut deepest) = (0usize, 0usize, 0usize);
    let mut pinned_theme: Option<mdc_office_io::style::StyleBundle> = None;
    let mut pinned_list: Option<mdc_office_io::style::StyleBundle> = None;

    for p in &files {
        let bytes = std::fs::read(p).expect("read corpus file");
        match read_docx_styles_from_bytes(&bytes) {
            Ok(b) => {
                ok += 1;
                if !b.tree.style_ids().is_empty() {
                    with_styles += 1;
                }
                let has_chain = b
                    .tree
                    .style_ids()
                    .iter()
                    .any(|id| b.tree.get(id).and_then(|s| s.based_on.as_ref()).is_some());
                if has_chain {
                    with_basedon += 1;
                }
                let has_tc = b.tree.style_ids().iter().any(|id| {
                    b.tree
                        .get(id)
                        .and_then(|s| s.run.color.as_ref())
                        .and_then(|c| c.theme.as_ref())
                        .is_some()
                });
                if has_tc {
                    with_theme_color += 1;
                }
                if b.theme.len() >= 12 {
                    theme_full += 1;
                }
                for w in &b.warnings {
                    if w.detail.contains("dangling") {
                        warn_dangling += 1;
                    }
                    if w.detail.contains("cycles") {
                        warn_cycle += 1;
                    }
                }
                for id in b.tree.style_ids() {
                    if let Some(depth) = b.tree.chain(id).map(|c| c.len()) {
                        deepest = deepest.max(depth);
                    }
                }
                if p.file_name()
                    .map(|n| n == "lo__ThemeDocument.docx")
                    .unwrap_or(false)
                {
                    pinned_theme = Some(b.clone());
                }
                if p.file_name()
                    .map(|n| n == "lo__NumberedList.docx")
                    .unwrap_or(false)
                {
                    pinned_list = Some(b.clone());
                }
            }
            Err(_) => reject += 1,
        }
    }

    // 容差断言（untracked 语料防缩水；首轮实况见模块文档）。
    assert!(ok >= 180, "readable corpus shrank: {ok}");
    assert!(reject >= 10, "hostile fuzzer samples missing: {reject}");
    assert!(with_styles >= 175, "{with_styles}");
    assert!(with_basedon >= 120, "{with_basedon}");
    assert!(with_theme_color >= 30, "{with_theme_color}");
    assert!(theme_full >= 160, "{theme_full}");
    assert_eq!(warn_cycle, 0, "real corpus is cycle-clean");
    assert_eq!(warn_dangling, 0, "real corpus is dangling-clean");
    assert!(deepest >= 10, "deepest chain: {deepest}");

    // 钉定①ThemeDocument：12 槽 + Office 主题色实值（w:themeColor
    // 解析链锚点——accent1 4472C4 是 Word 2016+ 默认主题）。
    let td = pinned_theme.expect("lo__ThemeDocument.docx not in corpus");
    assert_eq!(td.theme.len(), 12);
    assert_eq!(td.theme.lookup("accent1").as_deref(), Some("4472C4"));
    assert_eq!(td.theme.lookup("accent5").as_deref(), Some("5B9BD5"));
    assert_eq!(td.theme.lookup("hyperlink").as_deref(), Some("0563C1"));
    assert_eq!(td.theme.lookup("dark1").as_deref(), Some("000000"));

    // 钉定②NumberedList：322 样式 + Heading 链深 10 + basedOn 在场
    //（Word 全量样式表——读面吞吐面）。
    let nl = pinned_list.expect("lo__NumberedList.docx not in corpus");
    assert!(
        nl.tree.style_ids().len() >= 300,
        "{}",
        nl.tree.style_ids().len()
    );
    let h1 = nl.tree.get("Heading1").expect("Heading1 style");
    assert!(h1.based_on.is_some(), "Heading1 basedOn chain");
    // v2 重校（rebase 到 main 82f8ec2e 后语料实测）：本批该文件
    // Heading6-9 basedOn 直接 Normal（LO 导出风格），chain(
    // "Heading9") 实深 2；真实最深链 = DSTOC8-9 → DSTOC7-9 → … →
    // DSTOC1-9 → Heading9 → Normal（10 节点，穿过 Heading9 落
    // Normal）——首轮「链深 10」即此链，钉定按实值迁移。
    let chain = nl.tree.chain("DSTOC8-9").expect("DSTOC8-9 chain");
    assert_eq!(chain.len(), 10, "DSTOC8-9 chain depth");
}

#[test]
fn write_read_xml_diff_define_style() {
    // 写面产物 styles.xml ↔ 读回逐字段 diff（读写镜像——define_style
    // 的 20 个语义位经 save → read_docx_styles 还原）。
    let mut w = DocxWriter::new();
    w.define_style(StyleDef {
        name: Some("Custom Base".into()),
        run: StyleRunProps {
            size: Some(28),
            color: Some(StyleColor {
                val: "auto".into(),
                theme: Some("accent1".into()),
                shade: Some("99".into()),
                tint: None,
            }),
            ..Default::default()
        },
        ..StyleDef::new("CBase", StyleType::Paragraph)
    })
    .unwrap();
    w.define_style(StyleDef {
        based_on: Some("CBase".into()),
        next: Some("Normal".into()),
        link: Some("CBaseChar".into()),
        q_format: true,
        run: StyleRunProps {
            bold: Some(true),
            italic: Some(false),
            strike: None,
            underline: Some("single".into()),
            ..Default::default()
        },
        para: mdc_office_io::style::StyleParaProps {
            outline_lvl: Some(3),
            alignment: Some("center".into()),
            indent: Some(mdc_office_io::docx::ParagraphIndent {
                left: Some(1440),
                right: None,
                special: None,
            }),
        },
        ..StyleDef::new("CDerived", StyleType::Paragraph)
    })
    .unwrap();
    w.define_style(StyleDef {
        type_: StyleType::Character,
        based_on: Some("Normal".into()),
        link: Some("CDerived".into()),
        run: StyleRunProps {
            fonts: Some(mdc_office_io::style::StyleFont {
                ascii: Some("Aptos".into()),
                hi_ansi: Some("Aptos".into()),
                east_asia: None,
                cs: Some("Aptos".into()),
            }),
            ..Default::default()
        },
        ..StyleDef::new("CBaseChar", StyleType::Character)
    })
    .unwrap();
    w.styled_paragraph(&[RunSpec::text("hello style")], "CDerived")
        .unwrap();
    let bytes = w.save_to_vec().unwrap();

    // 读回 diff。
    let b = read_docx_styles_from_bytes(&bytes).unwrap();
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    let base = b.tree.get("CBase").unwrap();
    assert_eq!(base.name.as_deref(), Some("Custom Base"));
    assert_eq!(base.run.size, Some(28));
    assert_eq!(
        base.run.color,
        Some(StyleColor {
            val: "auto".into(),
            theme: Some("accent1".into()),
            shade: Some("99".into()),
            tint: None,
        })
    );
    let d = b.tree.get("CDerived").unwrap();
    assert_eq!(d.based_on.as_deref(), Some("CBase"));
    assert_eq!(d.next.as_deref(), Some("Normal"));
    assert_eq!(d.link.as_deref(), Some("CBaseChar"));
    assert!(d.q_format);
    assert_eq!(d.run.bold, Some(true));
    assert_eq!(d.run.italic, Some(false));
    assert_eq!(d.run.underline.as_deref(), Some("single"));
    assert_eq!(d.para.outline_lvl, Some(3));
    assert_eq!(d.para.alignment.as_deref(), Some("center"));
    assert_eq!(d.para.indent.as_ref().unwrap().left, Some(1440));
    let ch = b.tree.get("CBaseChar").unwrap();
    assert_eq!(
        ch.run.fonts,
        Some(mdc_office_io::style::StyleFont {
            ascii: Some("Aptos".into()),
            hi_ansi: Some("Aptos".into()),
            east_asia: None,
            cs: Some("Aptos".into()),
        })
    );
    // 有效格式合并（CDerived→CBase）+ 主题空表回退。
    let eff = b.tree.effective_format("CDerived").unwrap();
    assert_eq!(eff.run.size, Some(28)); // CBase 层继承
    assert_eq!(eff.run.bold, Some(true));
    assert_eq!(eff.para.outline_lvl, Some(3));
    assert_eq!(b.effective_color("CBase"), None); // 无 theme 部件
    assert_eq!(
        b.tree.get("CBase").unwrap().run.color.as_ref().unwrap().val,
        "auto"
    );

    // XML 层 diff：styles.xml 的关键属性形（w:basedOn/w:themeColor/
    // w:themeShade/w:outlineLvl/w:jc 原样在场）。
    use std::io::Read as _;
    let mut za = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
    let mut styles_xml = String::new();
    za.by_name("word/styles.xml")
        .unwrap()
        .read_to_string(&mut styles_xml)
        .unwrap();
    for needle in [
        "<w:basedOn w:val=\"CBase\" />",
        "<w:next w:val=\"Normal\" />",
        "<w:link w:val=\"CBaseChar\" />",
        "w:themeColor=\"accent1\"",
        "w:themeShade=\"99\"",
        "<w:outlineLvl w:val=\"3\" />",
        "<w:jc w:val=\"center\" />",
        "<w:b />",
        "<w:i w:val=\"false\" />",
        "<w:u w:val=\"single\" />",
    ] {
        assert!(styles_xml.contains(needle), "styles.xml missing {needle}");
    }
    let mut doc_xml = String::new();
    za.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut doc_xml)
        .unwrap();
    assert!(doc_xml.contains("<w:pStyle w:val=\"CDerived\" />"));
}

#[test]
fn libreoffice_opens_define_style_output() {
    // LibreOffice 开档门：define_style 产物（basedOn 链 + 主题色引用 +
    // pStyle 引用段）→ soffice --headless --convert-to pdf 零错误。
    // soffice 缺席（CI）时 skip。
    if std::env::var_os("CI").is_some() {
        eprintln!("[style-corpus] CI env — skipping LibreOffice gate");
        return;
    }
    let soffice = std::process::Command::new("which")
        .arg("soffice")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| "soffice");
    let soffice = match soffice {
        Some(s) => s,
        None => {
            eprintln!("[style-corpus] soffice not in PATH — skipping LibreOffice gate");
            return;
        }
    };
    let mut w = DocxWriter::new();
    w.define_style(StyleDef {
        based_on: Some("Normal".into()),
        run: StyleRunProps {
            color: Some(StyleColor {
                val: "2E74B5".into(),
                theme: Some("accent1".into()),
                shade: Some("99".into()),
                tint: None,
            }),
            size: Some(26),
            ..Default::default()
        },
        ..StyleDef::new("LoGate", StyleType::Paragraph)
    })
    .unwrap();
    w.styled_paragraph(&[RunSpec::text("LibreOffice gate")], "LoGate")
        .unwrap();
    let dir = std::env::temp_dir().join(format!("lo_gate_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let docx = dir.join("gate.docx");
    w.save_to_path(docx.to_str().unwrap()).unwrap();

    let out = std::process::Command::new(soffice)
        .arg("--headless")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&dir)
        .arg(&docx)
        .output()
        .expect("run soffice");
    assert!(
        out.status.success(),
        "soffice failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pdf = dir.join("gate.pdf");
    assert!(pdf.exists(), "pdf output missing");
    let meta = std::fs::metadata(&pdf).unwrap();
    assert!(meta.len() > 500, "pdf suspiciously small: {}", meta.len());
    let _ = std::fs::remove_dir_all(&dir);
}
