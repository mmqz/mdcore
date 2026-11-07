//! #309③ 域缓存写入——TOC 族 dirty 语义翻转（写面行为锁）。
//!
//! 现状（#309③ 前）：TOC 族三处发射 begin[dirty=true]（RunSpec
//! `field_toc` 五连 / `toc_entries` 域头 / `toc_entries` 空退化五连）
//! ——Word/LO 打开弹「是否更新域」提示，而缓存内容（目录项 + 占位
//! 页码 / 占位提示文本）本就在场：弹窗纯属打扰。
//!
//! #309③ 后：begin 不置 dirty——**打开即显缓存**（零弹窗），域结构
//! 五连完整保留（F9 重算生成真目录——页码占位 "1" 精确化）。
//!
//! 两验证组：
//! · **XML 面**——三发射路径的 dirty 缺席 + 缓存内容在场（结构锁）；
//! · **LO 实开**——本机 soffice（25.2）转 PDF 后 pdftotext 提取，
//!   目录项文本可见 = **缓存直显**实证（独立于 mdcore 读面的第二
//!   意见）。soffice/pdftotext 缺席 → 显式 SKIP 横幅（AGENTS.md 13.1
//!   口径——不静默绿；先例 docx_fields.rs lo_tools 同款）。
//!
//! 不动面（#309③ 范围裁决，认领评论在档）：PAGE/NUMPAGES 简单域维持
//! begin/instr/end 无缓存三连——域值是渲染期计算面（页码/总页数写时
//! 不可知），假缓存显示错误值比诚实空白更糟；footer 面 Word 布局期
//! 自动重算。PAGEREF 已是非 dirty + 缓存 "1"（#196 既有），本套件
//! 仅回归钉定。

use std::path::{Path, PathBuf};
use std::process::Command;

use mdc_office_io::docx::{read_docx_from_bytes, DocxWriter, RunSpec, TocEntry};

fn document_xml(bytes: &[u8]) -> String {
    let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    use std::io::Read as _;
    let mut v = Vec::new();
    ar.by_name("word/document.xml")
        .unwrap()
        .read_to_end(&mut v)
        .unwrap();
    String::from_utf8(v).unwrap()
}

// ---------------------------------------------------------------------------
// XML 面（常开——结构锁）
// ---------------------------------------------------------------------------

/// RunSpec::field_toc 五连：dirty 缺席 + 占位缓存（F9 提示文本）在场。
#[test]
fn runspec_toc_no_dirty_cache_visible() {
    let mut w = DocxWriter::new();
    w.heading(1, "章").unwrap();
    w.paragraph(&[RunSpec::field_toc_heading_range(1, 2)]);
    let bytes = w.save_to_vec().unwrap();
    let xml = document_xml(&bytes);
    // docx-rs 恒显式写 w:dirty 属性（false 同样在场）——断言口径 =
    // dirty="true" 零出现（无任何域请求打开时更新）。
    assert_eq!(
        xml.matches(r#"w:dirty="true""#).count(),
        0,
        "#309③：TOC 域 begin 非 dirty（缓存直显无弹窗）"
    );
    assert!(
        xml.contains(r#"w:dirty="false""#),
        "dirty=false 显式在场（docx-rs 恒写——打开不更新域）"
    );
    assert!(xml.contains(r#"w:fldCharType="separate""#), "separate 在场");
    assert!(
        xml.contains("（目录域——打开后全选再按 F9 更新生成）"),
        "占位缓存文本在场（打开即显内容）"
    );
    assert!(
        xml.contains(r#"w:fldCharType="end""#),
        "end 在场（F9 可重算）"
    );
    // 读面往返：占位缓存保留（内容型域——tr14 同锚）。
    let doc = read_docx_from_bytes(&bytes).unwrap();
    let all_text: String = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            mdc_office_io::docx::Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    assert!(
        all_text.contains("目录域"),
        "读面占位缓存可见：{all_text:?}"
    );
}

/// toc_entries（静态目录项）：域头 dirty 缺席 + 目录项缓存（文本 +
/// 占位页码 "1"）在场 + 跨段域配平。
#[test]
fn toc_entries_no_dirty_entries_cached() {
    let mut w = DocxWriter::new();
    let entries = vec![
        TocEntry::new(1, "第一章 总体方案"),
        TocEntry::new(2, "1.1 分项说明"),
    ];
    let names = w.toc_entries(&entries, Some((1, 3)));
    assert_eq!(names.len(), 2);
    w.heading_bookmarked(1, "第一章 总体方案", &names[0])
        .unwrap();
    w.heading_bookmarked(2, "1.1 分项说明", &names[1]).unwrap();
    let bytes = w.save_to_vec().unwrap();
    let xml = document_xml(&bytes);
    assert_eq!(
        xml.matches(r#"w:dirty="true""#).count(),
        0,
        "#309③：全文档零 dirty=true（TOC + PAGEREF 族打开即显缓存）"
    );
    // 目录项缓存文本在场（hyperlink 内 run——Word 打开直接显示这两行）。
    assert!(xml.contains("第一章 总体方案"), "目录项缓存文本在场");
    assert!(xml.contains("1.1 分项说明"), "目录项缓存文本在场");
    // 跨段域配平：begin/separate/end = TOC(1) + PAGEREF(2)。
    assert_eq!(xml.matches(r#"w:fldCharType="begin""#).count(), 3);
    assert_eq!(xml.matches(r#"w:fldCharType="separate""#).count(), 3);
    assert_eq!(xml.matches(r#"w:fldCharType="end""#).count(), 3);
}

// ---------------------------------------------------------------------------
// LO 实开（环境门——soffice/pdftotext 缺席显式 SKIP）
// ---------------------------------------------------------------------------

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

/// LO 实开缓存直显：toc_entries 产物 → soffice → pdftotext →
/// 目录项文本可见（Word/LO 家族打开即显示缓存而非空白/弹窗——
/// 独立于 mdcore 读面的第二意见实证）。
#[test]
fn lo_opens_toc_cache_visible() {
    if std::env::var_os("CI").is_some() {
        eprintln!("[field-cache] SKIP: CI env — LO gate 未执行");
        return;
    }
    let (Some(soffice), Some(pdftotext)) = (which("soffice"), which("pdftotext")) else {
        eprintln!(
            "[field-cache] SKIP: ENV_MISSING soffice/pdftotext \
             （补装：apt install libreoffice poppler-utils —— AGENTS.md 13.1，不静默绿）"
        );
        return;
    };
    let mut w = DocxWriter::new();
    let entries = vec![
        TocEntry::new(1, "第一章 LO缓存直显锚"),
        TocEntry::new(2, "1.1 分项说明"),
    ];
    let names = w.toc_entries(&entries, Some((1, 3)));
    w.heading_bookmarked(1, "第一章 LO缓存直显锚", &names[0])
        .unwrap();
    w.heading_bookmarked(2, "1.1 分项说明", &names[1]).unwrap();
    w.paragraph_text("正文内容。");
    let bytes = w.save_to_vec().unwrap();

    let work = std::env::temp_dir().join(format!("field_cache_lo_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let docx = work.join("toc-cache.docx");
    std::fs::write(&docx, &bytes).unwrap();
    let out = Command::new(&soffice)
        .arg("--headless")
        .arg(format!(
            "-env:UserInstallation=file://{}",
            work.join("lo-profile").to_string_lossy()
        ))
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&work)
        .arg(&docx)
        .output()
        .expect("soffice spawn");
    assert!(
        out.status.success(),
        "soffice convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pdf = work.join("toc-cache.pdf");
    assert!(pdf.exists(), "pdf 未产出");
    let txt = Command::new(&pdftotext)
        .arg(&pdf)
        .arg("-")
        .output()
        .expect("pdftotext spawn");
    assert!(txt.status.success(), "pdftotext failed");
    let text = String::from_utf8_lossy(&txt.stdout).into_owned();
    // 缓存直显锚：目录项文本须在 PDF 可见（LO 渲染的是域缓存区——
    // headless 转换不弹更新提示、不重算 TOC）。pdftotext 会在中英
    // 边界插空格（实测「LO 缓存直显锚」）——空格归一后匹配。
    let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("LO缓存直显锚") && flat.contains("分项说明"),
        "LO 渲染未见目录项缓存（缓存直显失败？）：{}",
        &text[..text.len().min(400)]
    );
    let _ = std::fs::remove_dir_all(&work);
    let _ = Path::new(&work);
}
