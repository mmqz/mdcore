//! #141 任务 2（批注脚注编辑）：黄金例 + 真实样本 + LibreOffice 开档。
//!
//! 三验证组（用户硬性要求 A/B/C）：
//! · **A 黄金例**——`test-artifacts/F2/notes-golden.docx`（仓内夹具，
//!   `scripts/make_notes_fixture.py` 生成——缺失即 FAIL 不 SKIP）：
//!   实体脚注单段多 span / 双段 / 双批注含作者日期 / 分隔符惯例
//!   （-1/0 不入列）一夹具全覆盖；
//! · **B 真实 docx**——真 Word COM 件 `word-complex.docx`（真
//!   comments.xml：作者 zi ye，批注 COMMENT-NEED-CERT；footnotes.xml
//!   实测为分隔符空壳——真实语料 27 件扫描零实体脚注，实体脚注面
//!   以黄金例 + soffice 往返承载）+ `D2a-nonbody.docx`（tr19 件：
//!   无 footnotes.xml 部件——缺席面恒保真透传钉定）；
//! · **C LibreOffice 开档**——本机 soffice（25.2）；缺 soffice/
//!   pdftotext 时 SKIP（issue #89 口径）。脚注文本在 PDF 页底渲染
//!   （pdftotext 可见 = 编辑值被真实办公软件消费）；批注文本默认
//!   不进 PDF 导出（LibreOffice 口径），其验证面 = 转换成功
//!   （包结构合法 = Word 打开正常）+ A/B 组 reopen 断言。
//!
//! 行为登记（WIP 期设计，此处钉定）：多段脚注/批注编辑后段落壳
//! 保留（`paras` 不变——空段占行，Word 显示 = 新文本单段）；
//! `w:footnoteRef`/`w:annotationRef`/作者/日期零接触；无编辑部件
//! 逐字节透传。

use mdc_office_io::docx_edit::DocxEditor;
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

fn part_text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(part_bytes(bytes, name)).unwrap()
}

fn zip_archive(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<&[u8]>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap()
}

fn entry_names(bytes: &[u8]) -> Vec<String> {
    zip_archive(bytes)
        .file_names()
        .map(str::to_string)
        .collect()
}

/// soffice/pdftotext 环境门（缺→SKIP；issue #89 口径）。
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
    let work = std::env::temp_dir().join(format!("docx_notes_lo_{tag}_work"));
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
// A 黄金例（仓内夹具——实体脚注/批注全覆盖）
// ---------------------------------------------------------------------------

const GOLDEN: &str = "../../test-artifacts/F2/notes-golden.docx";

/// 夹具在场钉定（缺失 FAIL——不静默 SKIP）。
#[test]
fn golden_fixture_present() {
    let path = fixture(GOLDEN);
    assert!(
        path.exists(),
        "golden fixture missing: {} — regenerate via scripts/make_notes_fixture.py",
        path.display()
    );
}

/// 黄金例列出面 + 编辑链：四面（脚注×2 + 批注×2 + 正文段）一次 save
/// 全中；结构零接触（footnoteRef/annotationRef/属性/分隔符）+ reopen
/// 新值 + 幂等链。
#[test]
fn golden_notes_list_and_edit_chain() {
    let bytes = read_fixture(GOLDEN);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    // 段落序：0=正文一（fn1+cm701 锚）1=正文二（fn2+cm702 锚）2=普通段。
    assert_eq!(ed.paragraphs().len(), 3);
    // 脚注两面：分隔符（-1/0）不入列。
    let fns = ed.footnotes();
    assert_eq!(fns.len(), 2, "两实体脚注: {fns:?}");
    assert_eq!(
        (
            fns[0].id,
            fns[0].text.as_str(),
            fns[0].anchor_para,
            fns[0].paras
        ),
        (1, "脚注一：机房建设标准", Some(0), 1),
        "单段双 span 拼接 + 锚段0: {fns:?}"
    );
    assert_eq!(
        (
            fns[1].id,
            fns[1].text.as_str(),
            fns[1].anchor_para,
            fns[1].paras
        ),
        (2, "脚注二第一段脚注二第二段", Some(1), 2),
        "双段拼接 + 锚段1: {fns:?}"
    );
    // 批注两面：作者/日期透传。
    let cms = ed.comments();
    assert_eq!(cms.len(), 2, "{cms:?}");
    assert_eq!(
        (
            cms[0].id,
            cms[0].author.as_str(),
            cms[0].date.as_deref(),
            cms[0].text.as_str(),
            cms[0].anchor_para
        ),
        (
            701,
            "示例作者",
            Some("2026-11-01T09:00:00Z"),
            "批注甲：口径差异待澄清",
            Some(0)
        ),
        "{cms:?}"
    );
    assert_eq!(
        (
            cms[1].id,
            cms[1].author.as_str(),
            cms[1].date.as_deref(),
            cms[1].text.as_str(),
            cms[1].anchor_para,
            cms[1].paras
        ),
        (
            702,
            "评审专家",
            Some("2026-11-01T10:00:00Z"),
            "批注乙第一段批注乙第二段",
            Some(1),
            2
        ),
        "{cms:?}"
    );
    // 编辑链：两脚注 + 两批注 + 一正文段，一次 save。
    ed.set_footnote_text(1, "黄金例新脚注一").unwrap();
    ed.set_footnote_text(2, "黄金例新脚注二").unwrap();
    ed.set_comment_text(701, "黄金例新批注甲").unwrap();
    ed.set_comment_text(702, "黄金例新批注乙").unwrap();
    ed.set_paragraph_text(2, "已编辑甲").unwrap();
    let saved = ed.save_to_vec().unwrap();
    // footnotes.xml：首 w:t 换新值、余 w:t 移除、壳与引用零接触。
    let fx = part_text(&saved, "word/footnotes.xml");
    assert!(
        fx.contains(r#"<w:t xml:space="preserve">黄金例新脚注一</w:t>"#),
        "{fx}"
    );
    assert!(
        fx.contains(r#"<w:t xml:space="preserve">黄金例新脚注二</w:t>"#),
        "{fx}"
    );
    assert!(
        !fx.contains("脚注一：机房") && !fx.contains("建设标准"),
        "fn1 旧值消失: {fx}"
    );
    assert!(
        !fx.contains("脚注二第一段") && !fx.contains("脚注二第二段"),
        "fn2 旧值消失: {fx}"
    );
    assert!(fx.contains("<w:footnoteRef/>"), "页底编号零接触: {fx}");
    assert!(
        fx.contains("<w:p><w:r></w:r></w:p>"),
        "fn2 残余空段壳: {fx}"
    );
    assert!(
        fx.contains(r#"<w:footnote w:type="separator" w:id="-1">"#)
            && fx.contains(r#"<w:footnote w:type="continuationSeparator" w:id="0">"#),
        "分隔符条目原样: {fx}"
    );
    // comments.xml：作者/日期/initials 属性零接触 + 新值。
    let cm = part_text(&saved, "word/comments.xml");
    assert!(cm.contains(r#"w:author="示例作者""#), "作者零接触: {cm}");
    assert!(
        cm.contains(r#"w:date="2026-11-01T09:00:00Z""#),
        "日期零接触: {cm}"
    );
    assert!(
        cm.contains(r#"w:author="评审专家""#) && cm.contains(r#"w:initials="评""#),
        "{cm}"
    );
    assert!(
        cm.contains(r#"<w:t xml:space="preserve">黄金例新批注甲</w:t>"#),
        "{cm}"
    );
    assert!(
        cm.contains(r#"<w:t xml:space="preserve">黄金例新批注乙</w:t>"#),
        "{cm}"
    );
    assert!(
        !cm.contains("批注甲：口径差异待澄清") && !cm.contains("批注乙第一段"),
        "{cm}"
    );
    assert!(cm.contains("<w:annotationRef/>"), "批注锚引用零接触: {cm}");
    // document.xml：正文段编辑落地；未编辑段零接触。
    let doc = part_text(&saved, "word/document.xml");
    assert!(
        doc.contains(r#"<w:t xml:space="preserve">已编辑甲</w:t>"#),
        "{doc}"
    );
    assert!(!doc.contains("普通段落"), "旧正文消失: {doc}");
    assert!(
        doc.contains("正文第一段") && doc.contains("正文第二段"),
        "未编辑段零接触: {doc}"
    );
    assert!(
        doc.contains(r#"<w:footnoteReference w:id="1"/>"#),
        "正文脚注锚零接触: {doc}"
    );
    // reopen：显示面新值 + 壳段数不变 + 幂等链。
    let ed2 = DocxEditor::open(&saved).unwrap();
    let fns2 = ed2.footnotes();
    assert_eq!(fns2[0].text, "黄金例新脚注一");
    assert_eq!(fns2[1].text, "黄金例新脚注二");
    assert_eq!(fns2[1].paras, 2, "双段壳保留");
    let cms2 = ed2.comments();
    assert_eq!(cms2[0].text, "黄金例新批注甲");
    assert_eq!(cms2[0].author, "示例作者", "作者 reopen 仍在");
    assert_eq!(cms2[1].text, "黄金例新批注乙");
    assert_eq!(cms2[1].paras, 2);
    assert_eq!(ed2.paragraph_text(2), "已编辑甲");
    assert_eq!(
        ed2.save_to_vec().unwrap(),
        saved,
        "reopen 再 save 同字节（幂等链）"
    );
}

// ---------------------------------------------------------------------------
// B 真实 docx（真 Word COM / tr19）
// ---------------------------------------------------------------------------

const TR2: &str = "../../test-artifacts/TR2/fixtures/word-complex.docx";
const D2A: &str = "../../test-artifacts/corpusR/D2a-nonbody.docx";

/// 真 Word COM 件：真 comments.xml（作者 zi ye）编辑 + footnotes
/// 分隔符空壳（真实语料实测零实体脚注——27 件扫描）+ 未编辑部件
/// 逐字节透传。
#[test]
fn real_word_complex_comment_edit() {
    let bytes = read_fixture(TR2);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    // 真实件批注：id 1 / 作者 zi ye / COMMENT-NEED-CERT。
    let cms = ed.comments();
    assert_eq!(cms.len(), 1, "真 Word 批注: {cms:?}");
    assert_eq!(cms[0].author, "zi ye");
    assert_eq!(cms[0].text, "COMMENT-NEED-CERT");
    assert!(cms[0].anchor_para.is_some(), "正文 commentRangeStart 在场");
    // 真实件脚注：分隔符空壳 → 空集（不发明实体）。
    assert!(ed.footnotes().is_empty(), "真实语料零实体脚注（空壳透传）");
    // 编辑批注（真 Word 部件手术）。
    ed.set_comment_text(1, "已编辑批注乙").unwrap();
    let saved = ed.save_to_vec().unwrap();
    let cm = part_text(&saved, "word/comments.xml");
    assert!(
        cm.contains(r#"<w:t xml:space="preserve">已编辑批注乙</w:t>"#),
        "{cm}"
    );
    assert!(!cm.contains("COMMENT-NEED-CERT"), "旧批注消失: {cm}");
    assert!(cm.contains(r#"w:author="zi ye""#), "作者零接触: {cm}");
    assert!(
        cm.contains(r#"w:pStyle w:val="a8""#),
        "批注样式零接触: {cm}"
    );
    // 未编辑部件逐字节透传（footnotes 空壳 + 正文无编辑）。
    assert_eq!(
        part_text(&saved, "word/footnotes.xml"),
        part_text(&bytes, "word/footnotes.xml"),
        "footnotes.xml 逐字节透传"
    );
    assert_eq!(
        part_text(&saved, "word/document.xml"),
        part_text(&bytes, "word/document.xml"),
        "document.xml 逐字节透传"
    );
    // reopen：新值 + 幂等链。
    let ed2 = DocxEditor::open(&saved).unwrap();
    assert_eq!(ed2.comments()[0].text, "已编辑批注乙");
    assert_eq!(ed2.comments()[0].author, "zi ye");
    assert!(ed2.footnotes().is_empty());
    assert_eq!(ed2.save_to_vec().unwrap(), saved, "reopen 再 save 同字节");
}

/// tr19 件 D2a-nonbody：无 footnotes.xml 部件——缺席面空集 + save
/// 不发明部件（namelist 不变）+ 批注 701 编辑。
#[test]
fn real_d2a_absent_part_transparency() {
    let bytes = read_fixture(D2A);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    // 部件缺席：脚注空集 + typed 拒（不发明部件）。
    assert!(ed.footnotes().is_empty());
    let e = ed.set_footnote_text(1, "x").unwrap_err();
    assert!(e.to_string().contains("part absent"), "{e}");
    // tr19 批注 701 在场。
    let cms = ed.comments();
    assert_eq!(cms.len(), 1, "{cms:?}");
    assert_eq!(cms[0].id, 701);
    assert_eq!(cms[0].author, "示例作者");
    assert!(cms[0].text.contains("口径差异"));
    // 编辑 + save：不发明 footnotes 部件。
    ed.set_comment_text(701, "D2a 新批注值").unwrap();
    let saved = ed.save_to_vec().unwrap();
    assert_eq!(entry_names(&saved), entry_names(&bytes), "部件清单不变");
    assert!(!entry_names(&saved)
        .iter()
        .any(|n| n == "word/footnotes.xml"));
    let cm = part_text(&saved, "word/comments.xml");
    assert!(
        cm.contains(r#"<w:t xml:space="preserve">D2a 新批注值</w:t>"#),
        "{cm}"
    );
    // reopen。
    let ed2 = DocxEditor::open(&saved).unwrap();
    assert_eq!(ed2.comments()[0].text, "D2a 新批注值");
    assert!(ed2.footnotes().is_empty());
}

// ---------------------------------------------------------------------------
// C LibreOffice 开档（soffice 缺失 SKIP——issue #89 口径）
// ---------------------------------------------------------------------------

/// 黄金例编辑后 soffice 开档：脚注文本在 PDF 页底渲染（pdftotext
/// 可见——编辑值被真实办公软件消费）；旧值消失；正文编辑正向
/// 对照（防转错原文件）；批注面 = 转换成功（包结构合法）。
#[test]
fn soffice_open_edited_golden_notes() {
    let Some(_tools) = lo_tools() else {
        eprintln!("SKIP: soffice/pdftotext not on PATH (issue #89 口径)");
        return;
    };
    let bytes = read_fixture(GOLDEN);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    ed.set_footnote_text(1, "新脚注一渲染值").unwrap();
    ed.set_footnote_text(2, "新脚注二渲染值").unwrap();
    ed.set_comment_text(701, "新批注甲渲染值").unwrap();
    // 对照面：正文段文本（非 notes 面——死值照显）。
    ed.set_paragraph_text(2, "已编辑甲").unwrap();
    let out = std::env::temp_dir().join("docx_notes_lo_out");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let docx = out.join("edited.docx");
    std::fs::write(&docx, ed.save_to_vec().unwrap()).unwrap();
    // LibreOffice 开档（转换成功 = 包结构合法——含 comments.xml 手术）。
    let text = soffice_pdf_text(&docx, "golden");
    // 脚注文本页底渲染：新值在场、旧值消失。
    assert!(
        text.contains("新脚注一渲染值"),
        "脚注1 新值页底渲染: {text}"
    );
    assert!(
        text.contains("新脚注二渲染值"),
        "脚注2 新值页底渲染: {text}"
    );
    assert!(!text.contains("脚注一：机房"), "脚注1 旧值消失: {text}");
    assert!(!text.contains("脚注二第一段"), "脚注2 旧值消失: {text}");
    // 正向对照：正文编辑照显（非 notes 面 + 防转错原文件）。
    assert!(text.contains("已编辑甲"), "正文编辑照显: {text}");
    assert!(!text.contains("普通段落"), "原正文已改掉: {text}");
    // 未编辑正文零接触。
    assert!(text.contains("正文第一段"), "未编辑区照显: {text}");
}

/// 真实件（word-complex）批注编辑后 LibreOffice 开档：包结构合法
/// （转换成功）+ 原文档主体文本仍在（soffice 全文渲染）。
#[test]
fn soffice_real_word_complex_after_comment_edit() {
    let Some(_tools) = lo_tools() else {
        eprintln!("SKIP: soffice/pdftotext not on PATH (issue #89 口径)");
        return;
    };
    let bytes = read_fixture(TR2);
    let mut ed = DocxEditor::open(&bytes).unwrap();
    ed.set_comment_text(1, "已编辑批注乙").unwrap();
    let out = std::env::temp_dir().join("docx_notes_lo_real");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let docx = out.join("word-complex.docx");
    std::fs::write(&docx, ed.save_to_vec().unwrap()).unwrap();
    let text = soffice_pdf_text(&docx, "real");
    // 转换成功即本断言到达（包结构合法）；批注文本默认不进 PDF
    // 导出（LibreOffice 口径——渲染验证由 A/B 组 reopen 承载）。
    assert!(text.contains("表格"), "原文档主体文本仍在: {text}");
}
