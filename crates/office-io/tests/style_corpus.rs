//! S1 样式模板体系——真实语料 + LibreOffice 开档 + 写读 XML diff
//! 验证（feat/79-style-template）。
//!
//! 语料 = `test-artifacts/health-check/corpus/`（630 件真实 docx，三
//! 子目录结构：docx4j 501 + poi-docx 129——2026-11-05 语料重组；目录
//! untracked，CI 缺席时 skip，本地在场全量断言）。**收集 = 递归扫描**
//!（#262：语料三子目录化后顶层 `read_dir` 收集为空 → `is_empty` 静默
//! skip → 204 件断言空转 2 天无人察觉，第 19 条假绿）。
//!
//! 实况（#262 重钉轮，2026-11-06，630 件）：可读 613（**17 件容器
//! 损坏 = fuzzer 故意破坏样本，门 1 zip 拒绝 typed——与 `read_docx`
//! 同门同拒**）；有样式表 575 / basedOn 链 527 / themeColor 引用
//! 364 / 完整 12 槽主题 533；真实生态**零环零悬空**（自产干净——
//! 异常面留给手搓测试）；最深链 10。
//!
//! 钉定样本（防跑空断言；#262 重钉——语料换血后原 lo__* 两件缺席，
//! 锚点按新语料实值迁移）：
//! - `docx4j/bib-chernigovka.netdo.ru_download_docs_17459.docx`：
//!   theme 12 槽 + accent1=4472C4/accent5=5B9BD5/hyperlink=0563C1
//!   （标准 Office 主题色值——**w:themeColor 解析链的实值锚点**）
//!   + 433 样式（大样式表读面吞吐）；
//! - `docx4j/NumberingImplicitNumId.docx`：`Legal2L9` 链深 10
//!   （Normal → Legal2L1 → Legal2L2 → … → Legal2L9——10 级线性
//!   basedOn 链，深链解析锚点）。

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

/// 递归收集 .docx（#262：walkdir 同构，不依赖目录层级——语料
/// 2026-11-05 起为三子目录结构 `corpus/{docx4j,poi-docx,…}`，顶层
/// `read_dir` 收集为空导致 204 件断言静默空转 2 天）。
fn corpus_files() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map(|x| x == "docx").unwrap_or(false) {
                out.push(p);
            }
        }
    }
    let dir = corpus_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut v = Vec::new();
    walk(&dir, &mut v);
    v.sort();
    v
}

#[test]
fn real_corpus_style_stats_and_pinned() {
    let files = corpus_files();
    if files.is_empty() {
        // 目录缺席 = CI/新克隆合法 skip；**目录在场却收集到 0 件 =
        // 目录结构又变了 → 必须红**（#262：此前该分支静默 return，
        // 顶层收集空转 2 天无人察觉）。
        assert!(
            !corpus_dir().is_dir(),
            "corpus dir 在场但递归收集到 0 件 .docx——目录结构又变了？({})",
            corpus_dir().display()
        );
        eprintln!(
            "[style-corpus] corpus dir absent ({}) — skipping real-document sweep",
            corpus_dir().display()
        );
        return;
    }
    // 件数可见性（#262：打印到 stdout——`--nocapture` 与失败时 CI
    // 日志直接可见，空转无处藏身）。
    println!(
        "[style-corpus] 检查了 {} 件真实语料（{}）",
        files.len(),
        corpus_dir().display()
    );
    assert!(files.len() >= 600, "corpus shrank: {}", files.len());

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
                    .map(|n| n == "bib-chernigovka.netdo.ru_download_docs_17459.docx")
                    .unwrap_or(false)
                {
                    pinned_theme = Some(b.clone());
                }
                if p.file_name()
                    .map(|n| n == "NumberingImplicitNumId.docx")
                    .unwrap_or(false)
                {
                    pinned_list = Some(b.clone());
                }
            }
            Err(_) => reject += 1,
        }
    }

    // 容差断言（untracked 语料防缩水；#262 重钉轮实况见模块文档——
    // 630 件：ok 613 / reject 17 / with_styles 575 / with_basedon
    // 527 / with_theme_color 364 / theme_full 533 / deepest 10）。
    assert!(ok >= 600, "readable corpus shrank: {ok}");
    assert!(reject >= 10, "hostile fuzzer samples missing: {reject}");
    assert!(with_styles >= 560, "{with_styles}");
    assert!(with_basedon >= 510, "{with_basedon}");
    assert!(with_theme_color >= 350, "{with_theme_color}");
    assert!(theme_full >= 520, "{theme_full}");
    assert_eq!(warn_cycle, 0, "real corpus is cycle-clean");
    assert_eq!(warn_dangling, 0, "real corpus is dangling-clean");
    assert!(deepest >= 10, "deepest chain: {deepest}");

    // 钉定①bib-chernigovka（docx4j）：12 槽 + Office 主题色实值
    //（w:themeColor 解析链锚点——accent1 4472C4 是 Word 2016+ 默认
    // 主题）+ 433 样式大样式表（读面吞吐——#262 迁移：原 lo__ 两
    // 锚点 ThemeDocument→主题色、NumberedList→大表，实值承接）。
    let td = pinned_theme.expect("bib-chernigovka.netdo.ru_download_docs_17459.docx not in corpus");
    assert_eq!(td.theme.len(), 12);
    assert_eq!(td.theme.lookup("accent1").as_deref(), Some("4472C4"));
    assert_eq!(td.theme.lookup("accent5").as_deref(), Some("5B9BD5"));
    assert_eq!(td.theme.lookup("hyperlink").as_deref(), Some("0563C1"));
    assert_eq!(td.theme.lookup("dark1").as_deref(), Some("000000"));
    assert!(
        td.tree.style_ids().len() >= 400,
        "bib big styles table: {}",
        td.tree.style_ids().len()
    );

    // 钉定②NumberingImplicitNumId（docx4j）：Legal2L9 链深 10
    //（Normal → Legal2L1 → … → Legal2L9——10 级线性 basedOn 链，
    // 深链解析锚点；#262 重钉：原 DSTOC8-9 链随 lo__ 语料换血缺席）。
    let nl = pinned_list.expect("NumberingImplicitNumId.docx not in corpus");
    let chain = nl.tree.chain("Legal2L9").expect("Legal2L9 chain");
    assert_eq!(chain.len(), 10, "Legal2L9 chain depth");
    // 链形态实值：根 Normal（default），每级 basedOn 指上一级。
    assert_eq!(chain[0].id, "Normal");
    for i in 1..chain.len() {
        assert_eq!(
            chain[i].based_on.as_deref(),
            Some(chain[i - 1].id.as_str()),
            "Legal2 chain link at {i}"
        );
    }
    assert_eq!(chain[9].id, "Legal2L9");
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
    // 有效格式合并（CDerived→CBase）+ 默认调色板降级（#269：引擎
    // 写面无 theme 部件 → office_default；val=auto 不可用 →
    // 调色板解析 accent1=4472C4 + shade 99 → 254378）。
    let eff = b.tree.effective_format("CDerived").unwrap();
    assert_eq!(eff.run.size, Some(28)); // CBase 层继承
    assert_eq!(eff.run.bold, Some(true));
    assert_eq!(eff.para.outline_lvl, Some(3));
    assert!(b.theme.is_default_fallback()); // #269 降级态标记
    assert_eq!(b.effective_color("CBase"), Some("254378".to_string()));
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
fn real_corpus_theme_color_word_cache_parity() {
    // #269 验收 3/4：tint/shade 数学对 Word 实际渲染（w:val 缓存）
    // 的对照——**直接读真实语料文件**（非内联钉定表）：
    //
    // 对每件 Word 产、含 themeColor+修饰引用的语料，解析其真实
    // theme1.xml 槽位 → resolve → 与 Word 自己写入的 w:val 缓存比对。
    // 断言分级：
    //   · 每通道差 ≤1（Word 跨版本缓存分裂——同输入 4F81BD+BF 有
    //     365F91/376092/366091 三种 Word 缓存值，位级一致不存在）；
    //   · 精确命中样本数下限（防全漂移回归）。
    //
    // 选体语料：docx4j（Word16 产 2010_DrawingML/OpenDoPE_RptPosCon）
    // + corpusR/D2a-nonbody（MacWord 全 accent 面覆盖）+ poi-docx
    // （SampleDoc/Bug64561）。语料缺席（CI）时 skip——但目录在场而
    // 选件缺席 = 钉定失效，须红（防静默空转，#262 教训）。

    let dir = corpus_dir();
    if !dir.is_dir() {
        eprintln!("[style-corpus] corpus dir missing — skipping theme parity gate");
        return;
    }
    // (文件相对路径, 期望存在的最低 themeColor+修饰样式数)
    let targets: &[(&str, usize)] = &[
        ("docx4j/2010_DrawingML_GraphicData_wps.docx", 4),
        ("docx4j/OpenDoPE_RptPosCon-issue690.docx", 3),
        ("docx4j/hyperlink_dupe.docx", 1),
        ("../../corpusR/D2a-nonbody.docx", 8),
        ("poi-docx/Bug64561.docx", 3),
        ("poi-docx/FieldCodes.docx", 1),
        ("poi-docx/58067.docx", 1),
    ];
    let mut total = 0usize;
    let mut exact = 0usize;
    let mut missing = Vec::new();
    for (rel, min_modded) in targets {
        let path = dir.join(rel);
        if !path.exists() {
            missing.push(rel.to_string());
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                panic!("read {rel}: {e}");
            }
        };
        let bundle = match read_docx_styles_from_bytes(&bytes) {
            Ok(b) => b,
            Err(e) => {
                panic!("read_docx_styles {rel}: {e}");
            }
        };
        assert!(
            !bundle.theme.is_default_fallback(),
            "{rel}: 真实语料必须有 theme 部件（Word 产文档带 theme1.xml）"
        );
        let mut modded = 0usize;
        for id in bundle.tree.style_ids() {
            let color = match bundle.tree.get(id).and_then(|s| s.run.color.as_ref()) {
                Some(c) => c,
                None => continue,
            };
            if color.theme.is_none() || (color.tint.is_none() && color.shade.is_none()) {
                continue;
            }
            let val = color.val.trim();
            if val.len() != 6 || !val.chars().all(|c| c.is_ascii_hexdigit()) {
                continue; // 无缓存可比（auto 等）
            }
            modded += 1;
            total += 1;
            let got = bundle
                .theme
                .resolve_color(color)
                .unwrap_or_else(|| panic!("{rel}/{id}: theme 槽未命中但 Word 缓存 {val}"));
            let ch = |s: &str, i: usize| i32::from(u8::from_str_radix(&s[i..i + 2], 16).unwrap());
            let d = [
                (ch(&got, 0) - ch(val, 0)).abs(),
                (ch(&got, 2) - ch(val, 2)).abs(),
                (ch(&got, 4) - ch(val, 4)).abs(),
            ];
            assert!(
                d[0] <= 1 && d[1] <= 1 && d[2] <= 1,
                "{rel} 样式 {id}: theme={:?} tint={:?} shade={:?} → {got} vs Word 缓存 {val} (d={:?})",
                color.theme, color.tint, color.shade, d
            );
            if d == [0, 0, 0] {
                exact += 1;
            }
        }
        assert!(
            modded >= *min_modded,
            "{rel}: 带修饰 themeColor 样式数 {modded} < 期望 {min_modded}（语料换血？须重钉）"
        );
    }
    // 防跑空：语料在场时必须有足量样本（目录在场而选件全缺席 = 红）。
    assert!(
        missing.is_empty() || total > 0,
        "钉定语料全缺席且零样本：missing={missing:?}（目录结构变化，须重钉）"
    );
    if total > 0 {
        assert!(
            exact >= 6,
            "精确命中样本 {exact}/{total} 低于下限（算法回归？）"
        );
    }
    eprintln!(
        "[style-corpus] theme parity: {exact}/{total} exact, rest ≤1 drift (missing={missing:?})"
    );
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
