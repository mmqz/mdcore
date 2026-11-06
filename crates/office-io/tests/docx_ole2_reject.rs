//! docx_ole2_reject —— issue #234（R11·P2）：.doc 旧格式识别与错误归因。
//!
//! 真实语料形态（研究仓 real-corpus batch2 / ai-bid-research #270 仓外
//! 侧已验）：异源 .doc（OLE2 复合文档）改名 .docx 后，zip 读入口报
//! `container corrupt: Could not find EOCD`——误导用户以为文件损坏。
//! 引擎侧修法：读/编两入口按 OLE2 魔数（D0 CF 11 E0 A1 B1 1A E1）
//! 识别 → 专用错误 `ReadError::LegacyOfficeFormat` + 可操作指引
//! （另存为 .docx / soffice 转换）。
//!
//! 锁清单：
//! 1. `ole2_magic_yields_dedicated_error_with_guidance`——魔数载荷 →
//!    专用变体（非 EOCD 容器损坏）+ 指引文案（另存为/转换）；
//! 2. `short_input_not_hijacked`——短于魔数的输入走既有容器错误路径；
//! 3. `editor_entry_same_defense`——编辑面（DocxEditor::open）同陷阱
//!    同防御；
//! 4. `valid_docx_roundtrip_unaffected`——合法档零扰动（正控）。
//!
//! 边界（守 issue 口径）：全量 .doc 解析（OLE2 内部结构）不在本项；
//! .xls 改名 .xlsx 同构问题登记不修（issue 只点名 .doc——研究仓 #270
//! 侧同边界）。

use mdc_office_io::docx::{read_docx_from_bytes, DocxWriter};
use mdc_office_io::docx_edit::DocxEditor;

/// 合成 CFB 载荷：OLE2 魔数 + 头部填充（识别面只看前 8 字节——真实
/// .doc 语料的容器级验证在研究仓 #270 侧已做，引擎侧锁魔数面）。
fn ole2_bytes(len: usize) -> Vec<u8> {
    let mut b = vec![0u8; len.max(8)];
    b[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    b
}

#[test]
fn ole2_magic_yields_dedicated_error_with_guidance() {
    // 典型形态：魔数 + 512B 扇区头填充（.doc 改名 .docx）。
    for len in [8, 512, 539 * 1024] {
        let bytes = ole2_bytes(len);
        let err = read_docx_from_bytes(&bytes)
            .err()
            .unwrap_or_else(|| panic!("len={len} 的 OLE2 载荷应被拒"));
        let msg = err.to_string();
        assert!(
            msg.contains("legacy office format") && msg.contains("OLE2"),
            "专用错误面（len={len}）：{msg}"
        );
        // 可操作指引（issue 验收：不再报 EOCD 损坏，给转换路径）。
        assert!(
            msg.contains("另存为") && msg.contains("docx"),
            "指引文案（len={len}）：{msg}"
        );
        assert!(
            !msg.to_lowercase().contains("eocd"),
            "不得再误报 EOCD 容器损坏（len={len}）：{msg}"
        );
    }

    // typed 变体断言（DocxError::Container(ReadError::LegacyOfficeFormat)）。
    let err = read_docx_from_bytes(&ole2_bytes(512)).unwrap_err();
    let repr = format!("{err:?}");
    assert!(
        repr.contains("LegacyOfficeFormat"),
        "typed 变体（Debug 面）：{repr}"
    );
    // 公开常量与探测函数同源（wasm 错误码面消费）。
    assert!(mdc_office_io::io::is_ole2(&ole2_bytes(8)));
    assert_eq!(
        mdc_office_io::io::OLE2_CFB_MAGIC,
        [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
    );
}

#[test]
fn short_input_not_hijacked() {
    // 短于魔数的输入（含恰好 PK 前缀的垃圾）走既有容器错误路径——
    // 探测不劫持非 OLE2 形态。
    assert!(!mdc_office_io::io::is_ole2(&[]));
    assert!(!mdc_office_io::io::is_ole2(&[0xD0, 0xCF, 0x11]));
    assert!(!mdc_office_io::io::is_ole2(b"PK\x03\x04garbage"));
    // 垃圾字节（非 OLE2）仍是既有 ContainerCorrupt 语义。
    let err = read_docx_from_bytes(b"garbage bytes not a zip").unwrap_err();
    let repr = format!("{err:?}");
    assert!(
        repr.contains("ContainerCorrupt"),
        "非 OLE2 垃圾 = 既有容器损坏语义：{repr}"
    );
}

#[test]
fn editor_entry_same_defense() {
    // 编辑面同陷阱（DocxEditor::open → open_with_policy）同防御。
    let err = match DocxEditor::open(&ole2_bytes(512)) {
        Ok(_) => panic!("编辑面 OLE2 载荷应被拒"),
        Err(e) => e,
    };
    let repr = format!("{err:?}");
    assert!(
        repr.contains("LegacyOfficeFormat"),
        "编辑面 typed 变体：{repr}"
    );
    assert!(!err.to_string().to_lowercase().contains("eocd"));
}

#[test]
fn valid_docx_roundtrip_unaffected() {
    // 正控：合法 docx（写面生成）读回零扰动——魔数前置不吞正常路径。
    let mut w = DocxWriter::new();
    w.paragraph_text("正控段落。");
    let bytes = w.save_to_vec().unwrap();
    assert!(!mdc_office_io::io::is_ole2(&bytes), "zip 容器非 OLE2");
    let doc = read_docx_from_bytes(&bytes).unwrap();
    let _ = doc;
    let editor = DocxEditor::open(&bytes);
    assert!(editor.is_ok(), "编辑面正控：{:?}", editor.err());
}
