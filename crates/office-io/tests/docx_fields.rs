//! #140 任务 1（域码段编辑）：黄金例 + 真实样本 + LibreOffice 开档。
//!
//! 三验证组（用户硬性要求 A/B/C）：
//! · **A 黄金例**——`test-artifacts/F1/fields-golden.docx`（仓内夹具，
//!   `scripts/make_fields_fixture.py` 生成——缺失即 FAIL 不 SKIP）：
//!   PAGE 五段 / DATE 多 span / TOC 跨段 / 嵌套 PAGEREF / fldSimple /
//!   无 separate 退化域，六形态一夹具全覆盖；
//! · **B 真实 docx**——真 Word COM 件 `word-complex.docx`（PAGE 域，
//!   tr14 钉定形态）+ docx4j `loadAndSave.docx`（真 TOC 跨段 + 嵌套
//!   PAGEREF ×3 + TIME）+ `fields-fun`（DATE/IF 嵌套）；
//! · **C LibreOffice 开档**——本机 soffice（25.2）；缺 soffice/pdftotext
//!   时 SKIP（issue #89 口径——qpdf 先例，CI 无 LibreOffice 面）。
//!   **域活证伪法**：把 PAGE/DATE/TIME 显示值改写不可能值
//!   （ZZZPLACEMARKER 族）→ soffice 转 pdf → pdftotext **不含**该值
//!   （LibreOffice 重算域：PAGE→页码、DATE→当天、TIME→时刻）——
//!   若域被编辑成死文本，转换将照显 ZZZ 值。正向对照：普通段文本
//!   编辑「已编辑甲」必须照显（非域面），防转错原文件。
//!
//! 冒烟依据（LibreOffice 25.2 实测，/tmp 转换记录）：PAGE 域重算为
//! 页码、DATE 重算为当天、fldSimple TIME 重算为当前时刻；TOC/PAGEREF
//! 无 dirty 标记时用缓存（证伪只取重算族）。

use mdc_office_io::docx_edit::{DocxEditor, FieldKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_fixture(rel: &str) -> Vec<u8> {
    let path = fixture(rel);
    std::fs::read(&path)
        .unwrap_or_else(|e| panic!("fixture missing: {} ({e}) — FAIL not SKIP", path.display()))
}

fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut ar = zip_archive(bytes);
    let mut e = ar.by_name(name).unwrap();
    let mut v = vec![];
    std::io::Read::read_to_end(&mut e, &mut v).unwrap();
    v
}

fn zip_archive(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<&[u8]>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap()
}

fn document_xml(bytes: &[u8]) -> String {
    String::from_utf8(part_bytes(bytes, "word/document.xml")).unwrap()
}

/// soffice/pdftotext 环境门（缺→SKIP；本机与真实验证已跑通）。
fn lo_tools() -> Option<(PathBuf, PathBuf)> {
    let soffice = ["soffice", "/usr/bin/soffice"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .or_else(|| which("soffice"));
    let pdftotext = which("pdftotext");
    match (soffice, pdftotext) {
        (Some(s), Some(p)) => Some((s, p)),
        _ => None,
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()?;
    if out.status.success() {
        Some(PathBuf::from(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ))
    } else {
        None
    }
}

/// soffice → pdf → pdftotext 文本（独立 profile 防并发锁；转换失败即 panic）。
fn soffice_pdf_text(docx: &Path, tag: &str) -> String {
    let (soffice, pdftotext) = lo_tools().expect("caller checked");
    // 独立工作目录（_work 后缀——不与调用方 out 目录撞名：
    // remove_dir_all 第一步会清目录，撞名会删掉调用方刚写的文件[实施发现]）。
    let work = std::env::temp_dir().join(format!("docx_fields_lo_{tag}_work"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    std::fs::copy(docx, work.join(docx.file_name().unwrap())).unwrap();
    let profile = work.join("lo-profile");
    let out = Command::new(&soffice)
        .arg("--headless")
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.to_string_lossy()
        ))
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&work)
        .arg(docx)
        .output()
        .expect("soffice spawn");
    assert!(out.status.success(), "soffice convert failed: {out:?}");
    let pdf = work.join(docx.with_extension("pdf").file_name().unwrap());
    assert!(pdf.exists(), "pdf not produced: {}", pdf.display());
    let txt = Command::new(&pdftotext)
        .arg(&pdf)
        .arg("-")
        .output()
        .expect("pdftotext spawn");
    assert!(txt.status.success(), "pdftotext failed");
    String::from_utf8_lossy(&txt.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// A 黄金例（仓内夹具——六形态一夹具）
// ---------------------------------------------------------------------------

const GOLDEN: &str = "../../test-artifacts/F1/fields-golden.docx";

/// 夹具在场钉定（缺失 FAIL——不静默 SKIP）。
#[test]
fn golden_fixture_present() {
    let path = fixture(GOLDEN);
    assert!(
        path.exists(),
        "golden fixture missing: {} — regenerate via scripts/make_fields_fixture.py",
        path.display()
    );
}

/// 黄金例 fields() 全形态：6 域（PAGE/DATE/TOC 跨段/嵌套 PAGEREF/
/// fldSimple TIME/退化 REF）跨 9 段。
#[test]
fn golden_fields_all_shapes() {
    let bytes = read_fixture(GOLDEN);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    // 段落序（body 扁平）：0=PAGE 1=DATE 2=TOC开 3=TOC条目+PAGEREF
    // 4=TOC闭 5=普通 6=fldSimple 7=退化REF 8=普通。
    assert_eq!(ed.paragraphs().len(), 9, "九段");
    let fs = ed.fields();
    assert_eq!(fs.len(), 6, "六域: {fs:?}");
    // 段0 PAGE。
    assert_eq!(fs[0].para, 0);
    assert_eq!(fs[0].kind, FieldKind::Page);
    assert_eq!(fs[0].display_value, "3");
    assert!(fs[0].editable);
    // 段1 DATE 三 span 拼接。
    assert_eq!(fs[1].para, 1);
    assert_eq!(fs[1].kind, FieldKind::Date);
    assert_eq!(fs[1].display_value, "2020-06-30");
    assert!(fs[1].editable);
    // 段2 TOC：跨段（begin 段2 / end 段4）只读；结果区含段3/4 文本。
    assert_eq!(fs[2].para, 2);
    assert_eq!(fs[2].kind, FieldKind::Toc);
    assert!(!fs[2].same_para);
    assert!(!fs[2].editable);
    assert!(fs[2].display_value.contains("第一章 概述 "));
    assert!(fs[2].display_value.contains("第二章 方案 "));
    // 段3 嵌套 PAGEREF：子域 same_para → 可编辑（规则一致性；
    // nested 只记在栈顶 TOC——刷新覆盖是 Word 语义，结构保持是本面承诺）。
    assert_eq!(fs[3].para, 3);
    assert_eq!(fs[3].kind, FieldKind::Custom("PAGEREF _Toc1 \\h".into()));
    assert!(fs[3].same_para);
    assert!(fs[3].editable);
    assert_eq!(fs[3].display_value, "1");
    // 段6 fldSimple TIME。
    assert_eq!(fs[4].para, 6);
    assert_eq!(fs[4].kind, FieldKind::Time);
    assert_eq!(fs[4].display_value, "09:30");
    assert!(fs[4].editable);
    // 段7 退化 REF：只读（无 separate——不发明结构）。
    assert_eq!(fs[5].para, 7);
    assert_eq!(fs[5].kind, FieldKind::Ref("_Ref123456789".into()));
    assert_eq!(fs[5].display_value, "");
    assert!(!fs[5].editable);
    // TOC 域段（2/4）与域段（0/1/3/6/7）整段替换全拒（既有防线钉定）。
    for i in [0usize, 1, 2, 3, 4, 6, 7] {
        assert!(
            ed.set_paragraph_text(i, "x").is_err(),
            "段{i} 拒整段替换（域结构不可冲掉）"
        );
    }
    // 普通段（5/8）可正常编辑（回归对照）。
    assert!(ed.set_paragraph_text(5, "x").is_ok());
}

/// 黄金例编辑链：PAGE/DATE/TIME(fldSimple)/PAGEREF(嵌套子域) 四域编辑
/// + 普通段文本编辑，同 save——域结构字节守恒 + reopen 新值。
#[test]
fn golden_edit_chain_keeps_structure() {
    let bytes = read_fixture(GOLDEN);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    ed.set_field_value(0, 0, "7").unwrap();
    ed.set_field_value(1, 0, "2026-11-01").unwrap();
    ed.set_field_value(6, 0, "23:59").unwrap();
    ed.set_field_value(3, 0, "14").unwrap(); // 嵌套子域 PAGEREF
    ed.set_paragraph_text(5, "已编辑甲").unwrap();
    let saved = ed.save_to_vec().unwrap();
    let xml = document_xml(&saved);
    // 域结构字节守恒（6 域原样——活域非死文本）。
    assert_eq!(xml.matches(r#"w:fldCharType="begin""#).count(), 5);
    assert_eq!(xml.matches(r#"w:fldCharType="separate""#).count(), 4);
    assert_eq!(xml.matches(r#"w:fldCharType="end""#).count(), 5);
    assert_eq!(xml.matches("<w:fldSimple").count(), 1);
    for want in [
        " PAGE \\* MERGEFORMAT ",
        " DATE \\@ \"yyyy-MM-dd\" ",
        " TOC \\o \"1-3\" \\h \\z \\u ",
        " PAGEREF _Toc1 \\h ",
        " TIME \\* MERGEFORMAT ",
        " REF _Ref123456789 \\h ",
    ] {
        assert!(xml.contains(want), "指令原样 {want:?}: {xml}");
    }
    // 新值在场；旧值消失。
    assert!(xml.contains(r#">7</w:t>"#));
    assert!(xml.contains(r#">2026-11-01</w:t>"#));
    assert!(xml.contains(r#">23:59</w:t>"#));
    assert!(xml.contains(r#">14</w:t>"#));
    assert!(xml.contains("已编辑甲"));
    assert!(!xml.contains(">3</w:t>"));
    assert!(!xml.contains(">2020</w:t>"));
    assert!(!xml.contains(">09:30</w:t>"));
    assert!(!xml.contains(">1</w:t>"));
    assert!(!xml.contains("普通段落甲"));
    // TOC 缓存条目字面未动（跨段只读域零接触）。
    assert!(xml.contains("第一章 概述 "));
    assert!(xml.contains("第二章 方案 "));
    // reopen：六域仍全识别（结构未破坏 = Word 打开仍活域）+ 新值。
    let ed2 = DocxEditor::open(&saved).unwrap();
    let fs2 = ed2.fields();
    assert_eq!(fs2.len(), 6, "六域仍在（结构未破坏）: {fs2:?}");
    assert_eq!(fs2[0].display_value, "7");
    assert_eq!(fs2[1].display_value, "2026-11-01");
    assert_eq!(
        fs2[2].display_value, "第一章 概述 14第二章 方案 ",
        "TOC 跨段结果区重拼（含 PAGEREF 子域新值 14 + 段4 字面）"
    );
    assert_eq!(fs2[3].display_value, "14");
    assert_eq!(fs2[4].display_value, "23:59");
    assert_eq!(ed2.paragraph_text(5), "已编辑甲");
}

// ---------------------------------------------------------------------------
// B 真实 docx（真 Word COM / docx4j）
// ---------------------------------------------------------------------------

const TR2: &str = "../../test-artifacts/TR2/fixtures/word-complex.docx";
const LOADSAVE: &str = "../../test-artifacts/health-check/corpus/docx4j/loadAndSave.docx";
const FIELDSFUN: &str =
    "../../test-artifacts/health-check/corpus/docx4j/fields_cr067-fields-fun_one-file-many-readings.docx";

/// 真 Word COM 件（tr14 钉定 h3 内嵌 PAGE 域）：列出 + 编辑 + 结构守恒。
#[test]
fn real_word_complex_page_field_edit() {
    let bytes = read_fixture(TR2);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    // tr14：word-complex 的 PAGE 域嵌 h3 标题段（"1.1 表格" 前缀"1"剥离根因）。
    let fs = ed.fields();
    let page = fs
        .iter()
        .find(|f| f.kind == FieldKind::Page)
        .expect("真 Word 件 PAGE 域在场");
    assert_eq!(page.display_value, "1");
    assert!(page.editable, "同段五段结构——可编辑");
    assert!(page.same_para);
    let (para, idx) = (page.para, page.field_idx);
    // 有域段落拒整段替换（防结构冲掉）。
    assert!(ed.set_paragraph_text(para, "x").is_err());
    // 编辑显示值。
    ed.set_field_value(para, idx, "9").unwrap();
    let saved = ed.save_to_vec().unwrap();
    let xml = document_xml(&saved);
    // 域五段结构原样（tr14 形态：instr PAGE \* MERGEFORMAT）。
    assert!(xml.contains(r#"w:fldCharType="begin""#));
    assert!(xml.contains(" PAGE \\* MERGEFORMAT "));
    assert!(xml.contains(r#"w:fldCharType="separate""#));
    assert!(xml.contains(r#"w:fldCharType="end""#));
    assert!(xml.contains(r#">9</w:t>"#));
    assert!(!xml.contains(">1</w:t>"));
    // reopen。
    let ed2 = DocxEditor::open(&saved).unwrap();
    let page2 = ed2
        .fields()
        .iter()
        .find(|f| f.kind == FieldKind::Page)
        .cloned()
        .expect("reopen 后 PAGE 域仍识别（活域）");
    assert_eq!(page2.display_value, "9");
    assert!(page2.editable, "reopen 后仍可编辑（幂等链）");
}

/// docx4j loadAndSave：真 TOC 跨段 + 嵌套 PAGEREF×3 + TIME——只读面钉定。
#[test]
fn real_docx4j_toc_nested_readonly() {
    let bytes = read_fixture(LOADSAVE);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    let fs = ed.fields();
    // TOC 域（跨段——begin/end 不同段）只读。
    let toc = fs
        .iter()
        .find(|f| f.kind == FieldKind::Toc)
        .expect("TOC 域");
    assert!(!toc.same_para, "TOC begin/end 跨段: {toc:?}");
    assert!(!toc.editable);
    assert!(ed.set_field_value(toc.para, toc.field_idx, "x").is_err());
    // TIME 域可编辑（同段）。
    let time = fs.iter().find(|f| f.kind == FieldKind::Time).expect("TIME");
    assert!(time.editable, "{time:?}");
    // 嵌套 PAGEREF 子域：可编辑（nested 只记 TOC 栈顶）。
    let pr = fs
        .iter()
        .find(|f| matches!(&f.kind, FieldKind::Custom(c) if c.starts_with("PAGEREF")))
        .expect("PAGEREF 子域");
    assert!(pr.editable, "嵌套子域 same_para: {pr:?}");
    // 域计数（TOC1 + PAGEREF3 + TIME1 = 5 域——扫描钉定）。
    assert_eq!(fs.len(), 5, "五域: {fs:?}");
}

/// docx4j fields-fun：DATE + IF 嵌套（真实嵌套域判定）。
#[test]
fn real_docx4j_if_nested() {
    let bytes = read_fixture(FIELDSFUN);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    let fs = ed.fields();
    let date = fs.iter().find(|f| f.kind == FieldKind::Date).expect("DATE");
    assert!(date.editable, "顶层 DATE 同段: {date:?}");
    // IF 域（Custom("IF …")）在场。
    assert!(
        fs.iter()
            .any(|f| matches!(&f.kind, FieldKind::Custom(c) if c.starts_with("IF"))),
        "IF 域在场: {fs:?}"
    );
    // 全量域段整段替换全拒（域段落结构不可冲掉——验收第 4 条真实件钉定）。
    for f in &fs {
        assert!(
            ed.set_paragraph_text(f.para, "x").is_err(),
            "段 {} 拒整段替换（field_code）",
            f.para
        );
    }
}

// ---------------------------------------------------------------------------
// C LibreOffice 开档 + 域活证伪（soffice 缺失 SKIP——issue #89 口径）
// ---------------------------------------------------------------------------

/// 域活证伪：PAGE/DATE/TIME 显示值改不可能值 → soffice 转 pdf
/// → pdftotext **不含** ZZZ（LibreOffice 重算域）——若编辑把域变成
/// 死文本，转换将照显 ZZZ。正向对照：普通段「已编辑甲」照显
/// （非域面 + 防转错原文件）。
#[test]
fn soffice_open_and_field_alive_falsification() {
    let Some(_tools) = lo_tools() else {
        eprintln!("SKIP: soffice/pdftotext not on PATH (issue #89 口径)");
        return;
    };
    let bytes = read_fixture(GOLDEN);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    // 重算族（冒烟钉定 LibreOffice 25.2 行为）：PAGE/DATE/fldSimple TIME。
    ed.set_field_value(0, 0, "ZZZPAGE").unwrap();
    ed.set_field_value(1, 0, "ZZZDATE").unwrap();
    ed.set_field_value(6, 0, "ZZZTIME").unwrap();
    // 对照面：普通段文本（非域——死值照显）。
    ed.set_paragraph_text(5, "已编辑甲").unwrap();
    let out = std::env::temp_dir().join("docx_fields_lo_out");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let docx = out.join("edited.docx");
    std::fs::write(&docx, ed.save_to_vec().unwrap()).unwrap();
    // LibreOffice 开档（转换成功 = 开档正常）。
    let text = soffice_pdf_text(&docx, "alive");
    // 证伪断言：ZZZ 族全不在（域活——LibreOffice 重算覆盖）。
    assert!(
        !text.contains("ZZZPAGE"),
        "PAGE 域活（重算非死文本）: {text}"
    );
    assert!(!text.contains("ZZZDATE"), "DATE 域活: {text}");
    assert!(!text.contains("ZZZTIME"), "TIME 域活: {text}");
    // 正向对照：编辑确实生效（转换的是编辑后文件）。
    assert!(text.contains("已编辑甲"), "普通段文本编辑照显: {text}");
    assert!(!text.contains("普通段落甲"), "原文本已改掉");
    // PAGE 重算值在场（当前页码 1——冒烟同款）。
    let first_page_num = text
        .lines()
        .find(|l| l.trim() == "1")
        .expect("PAGE 重算页码在场");
    assert_eq!(first_page_num.trim(), "1");
}

/// 真实件（word-complex）编辑后 LibreOffice 开档：域结构合法、PAGE 重算。
#[test]
fn soffice_real_word_complex_after_edit() {
    let Some(_tools) = lo_tools() else {
        eprintln!("SKIP: soffice/pdftotext not on PATH (issue #89 口径)");
        return;
    };
    let bytes = read_fixture(TR2);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    let page = ed
        .fields()
        .iter()
        .find(|f| f.kind == FieldKind::Page)
        .cloned()
        .expect("PAGE");
    ed.set_field_value(page.para, page.field_idx, "ZZZPAGE")
        .unwrap();
    let out = std::env::temp_dir().join("docx_fields_lo_real");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let docx = out.join("word-complex.docx");
    std::fs::write(&docx, ed.save_to_vec().unwrap()).unwrap();
    let text = soffice_pdf_text(&docx, "real");
    assert!(!text.contains("ZZZPAGE"), "真 Word 件 PAGE 域活: {text}");
    // 真件原有文本仍在（未编辑区域零接触——soffice 全文渲染）。
    assert!(text.contains("表格"), "原文档主体文本仍在: {text}");
}
