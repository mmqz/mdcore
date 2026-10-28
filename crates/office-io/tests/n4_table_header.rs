//! N-4（#29）：表格首行跨页表头重复——`table_header_repeat` +
//! `mark_table_header_rows` zip 后处理注入。
//!
//! 背景：标书 100-300 张表多跨页，Word 惯例 = 表头行 `w:trPr/
//! tblHeader` 跨页重复。docx-rs 0.4.22 `TableRowProperty` 无此面
//! （版本钉死——P2-2 serde 通道锁 0.4.22），故 save 漏斗内 zip 后处理
//! 注入（锚点 = 写面发射形态实证：`<w:tbl>`/`<w:tr>` 无属性、首子
//! 恒 `<w:trPr />` 自闭合）。
//!
//! 验收对照：
//! - XML 面：首行 `<w:trPr><w:tblHeader /></w:trPr>`、次行不变；
//! - 字节稳定：纯 `table()` 消费方零变化（无 tblHeader 注入路径）；
//! - 混合表记账：文档序索引只标记目标表；
//! - 结构合法：注入产物可被引擎读面解析（round-trip）。

use mdc_office_io::docx::{docx_part_bytes, read_docx_from_bytes, DocxWriter};

fn document_xml(bytes: &[u8]) -> String {
    String::from_utf8(docx_part_bytes(bytes, "word/document.xml").expect("document.xml"))
        .expect("utf8")
}

#[test]
fn header_repeat_marks_first_row_only() {
    let mut w = DocxWriter::new();
    w.table_header_repeat(&[
        vec!["列甲".into(), "列乙".into()],
        vec!["1".into(), "2".into()],
        vec!["3".into(), "4".into()],
    ]);
    let bytes = w.save_to_vec().expect("save");
    let xml = document_xml(&bytes);
    assert_eq!(
        xml.matches("<w:trPr><w:tblHeader /></w:trPr>").count(),
        1,
        "恰首行打标"
    );
    // 首行打标：tblHeader 位于首行（文档序第一个 <w:tr>）。
    let first_tr = xml.find("<w:tr>").expect("first row");
    let mark = xml.find("<w:tblHeader />").expect("mark");
    assert!(mark > first_tr, "tblHeader 在首行内");
    // 次行 trPr 保持自闭合原形。
    assert!(
        xml.contains("<w:trPr />"),
        "未打标行保持 <w:trPr /> 自闭合原形"
    );
}

#[test]
fn plain_table_bytes_unchanged() {
    // 纯 table() 消费方：零 tblHeader（字节级兼容锚——既有产物不变）。
    let mut w = DocxWriter::new();
    w.table(&[vec!["甲".into(), "乙".into()], vec!["1".into(), "2".into()]]);
    let bytes = w.save_to_vec().expect("save");
    let xml = document_xml(&bytes);
    assert!(!xml.contains("tblHeader"), "纯 table() 路径零注入");
    assert_eq!(xml.matches("<w:trPr />").count(), 2);
}

#[test]
fn mixed_tables_document_order_indexing() {
    // table()（idx 0，不打标）→ table_header_repeat()（idx 1，打标）→
    // table()（idx 2，不打标）：文档序记账只中目标表。
    let mut w = DocxWriter::new();
    w.table(&[vec!["A0".into()], vec!["a0".into()]]);
    w.table_header_repeat(&[vec!["A1".into()], vec!["a1".into()]]);
    w.table(&[vec!["A2".into()], vec!["a2".into()]]);
    let bytes = w.save_to_vec().expect("save");
    let xml = document_xml(&bytes);
    assert_eq!(xml.matches("tblHeader").count(), 1, "仅第 2 表打标");
    // 打标位置在 A1 表首行（trPr 先于单元格文本——标记应落在
    // 表 0 尾内容 a0 之后、A1 文本之前）。
    let a0 = xml.find("a0").expect("a0");
    let a1 = xml.find("A1").expect("A1");
    let mark = xml.find("tblHeader").expect("mark");
    assert!(
        mark > a0 && mark < a1,
        "tblHeader 位于第 2 表首行 trPr（a0 < mark < A1）"
    );
}

#[test]
fn single_row_table_not_marked() {
    // 单行表无跨页面——防御性不打（Word 对孤立 tblHeader 行语义未定义）。
    let mut w = DocxWriter::new();
    w.table_header_repeat(&[vec!["仅表头".into(), "无数据".into()]]);
    let bytes = w.save_to_vec().expect("save");
    let xml = document_xml(&bytes);
    assert!(!xml.contains("tblHeader"), "单行表不打标");
}

#[test]
fn marked_document_roundtrips_through_reader() {
    // 结构合法性：注入产物可被引擎读面解析（docx-rs 容器 + XML 合法）。
    let mut w = DocxWriter::new();
    w.table_header_repeat(&[
        vec!["列甲".into(), "列乙".into()],
        vec!["1".into(), "2".into()],
    ]);
    let bytes = w.save_to_vec().expect("save");
    let doc = read_docx_from_bytes(&bytes).expect("读面解析注入产物");
    let tables: Vec<_> = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, mdc_office_io::docx::Block::Table(_)))
        .collect();
    assert_eq!(tables.len(), 1, "表结构保持");
    // 读面不回读 tblHeader（docx-rs 反序列化丢 trPr 扩展——登记的
    // round-trip 缺口，非断言失败项：此处钉定该边界，防未来悄悄变化）。
}

#[test]
fn non_document_parts_raw_copied() {
    // 非目标部件 raw_copy 原样：styles.xml 逐字节等（注入只动
    // word/document.xml）。
    let rows = vec![vec!["甲".into(), "乙".into()], vec!["1".into(), "2".into()]];
    let mut w_plain = DocxWriter::new();
    w_plain.table(&rows);
    let plain = w_plain.save_to_vec().expect("save");
    let mut w_marked = DocxWriter::new();
    w_marked.table_header_repeat(&rows);
    let marked = w_marked.save_to_vec().expect("save");
    for part in [
        "word/styles.xml",
        "[Content_Types].xml",
        "word/document.xml",
    ] {
        let a = docx_part_bytes(&plain, part);
        let b = docx_part_bytes(&marked, part);
        if part == "word/document.xml" {
            assert_ne!(a, b, "document.xml 注入后不同");
        } else {
            assert_eq!(a, b, "{part} raw_copy 逐字节保持");
        }
    }
}
