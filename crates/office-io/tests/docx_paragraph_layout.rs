//! #309② 写面薄片——段落对齐（w:jc）+ 行距/段距（w:spacing）。
//!
//! 研究仓 word-layout 拆解的「pPr/rPr 精细控制」缺口（五结构缺口之二
//! ：「段落/Run 微调——写面缺精细控制」）最小切片：`w:jc` 四值 +
//! `w:spacing` 全字段（line/lineRule/before/after——复用读面
//! [`LineSpacingSpec`] 类型，[`PageGeometry`] 读写对称先例）。
//!
//! 验证面：
//! · **XML 面**——`<w:jc w:val>` 四值映射 + `<w:spacing>` 属性形
//!   （line/lineRule/before/after——docx-rs 序列化形逐属性断言）；
//! · **读回容器**——写产物 `read_docx` 可解析（对齐/行距读面模型
//!   承载未做——r4-t3 登记同族：读面 ParagraphData 无 jc/spacing 字段
//!   ，往返断言限 XML 面）；
//! · **V9 同族正值域**——`line < 0` 粘性拒（save 漏斗 typed）。

use mdc_office_io::docx::{
    read_docx_from_bytes, DocxWriter, LineRuleSpec, LineSpacingSpec, ParaAlign, RunSpec,
};

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

/// 四值对齐映射（w:jc——Justify → both 是 OOXML 两端对齐值）。
#[test]
fn paragraph_align_four_values() {
    for (align, val) in [
        (ParaAlign::Left, "left"),
        (ParaAlign::Center, "center"),
        (ParaAlign::Right, "right"),
        (ParaAlign::Justify, "both"),
    ] {
        let mut w = DocxWriter::new();
        w.paragraph_aligned(&[RunSpec::text("段")], align);
        let bytes = w.save_to_vec().unwrap();
        let xml = document_xml(&bytes);
        assert!(
            xml.contains(&format!(r#"<w:jc w:val="{val}" />"#)),
            "{align:?} 应发射 w:jc={val}: {}",
            &xml[xml.len().saturating_sub(300)..]
        );
        // 读回容器完好（对齐读面承载未做——容器级断言）。
        assert!(read_docx_from_bytes(&bytes).is_ok(), "{align:?} 读回可解析");
    }
}

/// 行距/段距全字段（w:spacing——multiple/exact 两构造器 + 段前后距）。
#[test]
fn paragraph_line_spacing_fields() {
    let mut w = DocxWriter::new();
    // 1.5 倍行距 = line 360 auto（multiple 构造器换算）。
    w.paragraph_spaced(&[RunSpec::text("一")], LineSpacingSpec::multiple(1.5));
    // 精确行距 24pt = 480 twips exact。
    w.paragraph_spaced(&[RunSpec::text("二")], LineSpacingSpec::exact_twips(480));
    // 裸结构全字段：line 240 atLeast + before/after 120/60。
    w.paragraph_layout(
        &[RunSpec::text("三")],
        Some(ParaAlign::Center),
        Some(LineSpacingSpec {
            line: Some(240),
            line_rule: Some(LineRuleSpec::AtLeast),
            before: Some(120),
            after: Some(60),
        }),
    );
    let bytes = w.save_to_vec().unwrap();
    let xml = document_xml(&bytes);
    assert!(
        xml.contains(r#"w:line="360""#) && xml.contains(r#"w:lineRule="auto""#),
        "1.5 倍行距（line=360 auto）: 未发射"
    );
    assert!(
        xml.contains(r#"w:line="480""#) && xml.contains(r#"w:lineRule="exact""#),
        "精确行距（line=480 exact）: 未发射"
    );
    assert!(
        xml.contains(r#"w:line="240""#)
            && xml.contains(r#"w:lineRule="atLeast""#)
            && xml.contains(r#"w:before="120""#)
            && xml.contains(r#"w:after="60""#),
        "全字段（line/lineRule/before/after）: 未发射"
    );
    // 对齐 + 行距同段共存（组合面）。
    assert!(xml.contains(r#"<w:jc w:val="center" />"#), "组合段对齐在场");
    assert!(read_docx_from_bytes(&bytes).is_ok(), "读回可解析");
}

/// None 面零发射——`paragraph_layout(runs, None, None)` 不发射任何
/// `w:jc`/`w:spacing`（最小面纪律：None = 零污染）。注：不比字节——
/// docx-rs 的 `w14:paraId` 是跨实例全局递增计数器，两实例产物恒差
/// 该属性（与版面无关——实证见 PR 描述）。
#[test]
fn paragraph_layout_none_face_zero_emission() {
    let runs = [RunSpec::text("同一段")];
    let mut w = DocxWriter::new();
    w.paragraph_layout(&runs, None, None);
    let mut w2 = DocxWriter::new();
    w2.paragraph(&runs);
    w2.paragraph_layout(&[RunSpec::text("另一段")], None, None);
    for (tag, bytes) in [
        ("单段", w.save_to_vec().unwrap()),
        ("混合段", w2.save_to_vec().unwrap()),
    ] {
        let xml = document_xml(&bytes);
        assert!(!xml.contains("<w:jc"), "{tag}: None 面不得发射 w:jc");
        assert!(
            !xml.contains("<w:spacing"),
            "{tag}: None 面不得发射 w:spacing"
        );
        assert!(read_docx_from_bytes(&bytes).is_ok(), "{tag}: 读回可解析");
    }
}

/// V9 同族正值域：line < 0 粘性拒（save 漏斗 typed Limit/InvalidInput）。
#[test]
fn paragraph_layout_negative_line_rejected() {
    let mut w = DocxWriter::new();
    w.paragraph_layout(
        &[RunSpec::text("负")],
        None,
        Some(LineSpacingSpec {
            line: Some(-240),
            line_rule: Some(LineRuleSpec::Exact),
            before: None,
            after: None,
        }),
    );
    let err = w.save_to_vec().unwrap_err();
    assert!(
        matches!(err, mdc_office_io::docx::DocxError::InvalidInput(ref m) if m.contains("negative")),
        "负行距须 typed 拒（实际 {err:?}）"
    );
}
