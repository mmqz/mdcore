//! #115：**端到端**证据链（真实文件进 → 真实文件出 → Word 可开）。
//!
//! 与 \`docx_edit_roundtrip.rs\` 的区别：那条链在内存里跑；这条链**落盘**，
//! 产出可直接用 Microsoft Word / LibreOffice 打开的文件——GUI 冒烟与人工
//! 验收的原料。
//!
//! 流程（完全复刻编辑器路径，只是不起 GUI）：
//! 1. \`docx_doc::load_editable\`（打开——与 Ctrl+O 同一函数）；
//! 2. 取行模型文本 → 按「用户在第二段里敲了字」改一行；
//! 3. 桥 \`compute\` → 引擎 \`set_paragraph_text\` → \`save_to_vec\`；
//! 4. 写盘到 \`target/e2e/docx-edit/*.docx\`；
//! 5. 重开断言：改过的段 = 新文，其余段 = 原文，非 document.xml 条目
//!    逐字节不变。
//!
//! 产出目录（含原档与改后档）供人工用 Word 打开比对：
//! \`\`\`text
//! cargo test --test docx_edit_e2e -- --nocapture
//! \`\`\`
use std::path::Path;

use mang_shell::editor::docx_doc;
use mang_shell::editor::docx_edit_bridge::DocxPort;
use mdc_office_io::docx_edit::DocxEditor;

fn e2e_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/e2e")
        .to_path_buf()
}

fn out_dir() -> std::path::PathBuf {
    let dir = e2e_root().join("docx-edit");
    std::fs::create_dir_all(&dir).expect("建输出目录");
    dir
}

/// 极简 zip 条目扫描（与 roundtrip 测试同口径——独立于被测代码）。
fn zip_entries(bytes: &[u8]) -> Vec<(String, u16, Vec<u8>)> {
    let n = bytes.len();
    let mut eocd = None;
    let mut i = n.saturating_sub(22);
    loop {
        if bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            eocd = Some(i);
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    let eocd = eocd.expect("EOCD");
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
    let cd_off = u32::from_le_bytes([
        bytes[eocd + 16],
        bytes[eocd + 17],
        bytes[eocd + 18],
        bytes[eocd + 19],
    ]) as usize;
    let mut out = Vec::new();
    let mut p = cd_off;
    for _ in 0..count {
        let method = u16::from_le_bytes([bytes[p + 10], bytes[p + 11]]);
        let csize = u32::from_le_bytes([bytes[p + 20], bytes[p + 21], bytes[p + 22], bytes[p + 23]])
            as usize;
        let nlen = u16::from_le_bytes([bytes[p + 28], bytes[p + 29]]) as usize;
        let xlen = u16::from_le_bytes([bytes[p + 30], bytes[p + 31]]) as usize;
        let clen = u16::from_le_bytes([bytes[p + 32], bytes[p + 33]]) as usize;
        let lho = u32::from_le_bytes([bytes[p + 42], bytes[p + 43], bytes[p + 44], bytes[p + 45]])
            as usize;
        let name = String::from_utf8_lossy(&bytes[p + 46..p + 46 + nlen]).to_string();
        let lnlen = u16::from_le_bytes([bytes[lho + 26], bytes[lho + 27]]) as usize;
        let lxlen = u16::from_le_bytes([bytes[lho + 28], bytes[lho + 29]]) as usize;
        let data_start = lho + 30 + lnlen + lxlen;
        out.push((name, method, bytes[data_start..data_start + csize].to_vec()));
        p += 46 + nlen + xlen + clen;
    }
    out
}

#[test]
fn e2e_open_edit_save_reopen() {
    let dir = out_dir();
    // 夹具：GUI 烟雾同款（mang-shell/tests/fixtures/smoke.docx）。
    let src_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/smoke.docx");
    let src =
        std::fs::read(&src_path).expect("夹具在场（cargo test gen_smoke_docx -- --ignored 生成）");
    std::fs::write(dir.join("01-original.docx"), &src).expect("落盘原档");

    // ① 打开（与 Ctrl+O 同一函数）
    let loaded = docx_doc::load_editable(&src_path).expect("可编辑装载");
    let payload = &loaded.payload;
    let (Some(bridge0), Some(port)) = (&loaded.bridge, &loaded.port) else {
        panic!(
            "夹具应可编辑（降级原因：{:?}）——见 docs/docx-edit.md §7",
            payload.notice
        );
    };
    let line_count = payload.text.split('\n').count();
    eprintln!(
        "e2e: 打开 {} 行 / {} 段（可编辑 {} / 锁定 {}）",
        line_count,
        payload.props.paragraphs,
        payload.props.editable_paragraphs,
        payload.props.locked_paragraphs
    );

    // ② 编辑：找一段可编辑正文改一个字（模拟用户在编辑器里键入）。
    let mut lines: Vec<String> = payload.text.split('\n').map(|s| s.to_string()).collect();
    let target_line = lines
        .iter()
        .position(|l| l.starts_with("平台采用云原生微服务架构"))
        .expect("找得到可编辑正文段");
    let original = lines[target_line].clone();
    lines[target_line] = format!("{original}【改】");
    eprintln!(
        "e2e: 编辑第 {target_line} 行：{original} → {}",
        lines[target_line]
    );

    // ③ 回写计划 → 引擎落账
    let plan = bridge0.compute(&lines);
    assert_eq!(plan.sets.len(), 1, "只写回一段：{:?}", plan.sets);
    let mut ed = DocxEditor::open(&src).expect("引擎开");
    for (p, t) in &plan.sets {
        ed.set_paragraph_text(*p, t).expect("set");
    }
    let out = ed.save_to_vec().expect("引擎落账");
    let saved_path = dir.join("02-edited.docx");
    std::fs::write(&saved_path, &out).expect("落盘改后档");
    eprintln!(
        "e2e: 保存 → {}（{} 字节；原文 {} 字节）",
        saved_path.display(),
        out.len(),
        src.len()
    );

    // ④ 重开断言：目标段新文、其余段原文。
    let re = DocxEditor::open(&out).expect("改后档可重开");
    let a = DocxEditor::open(&src).expect("原档可开");
    assert_eq!(re.paragraphs().len(), a.paragraphs().len(), "段落数不变");
    let changed: Vec<usize> = plan.sets.iter().map(|(p, _)| *p).collect();
    for i in 0..a.paragraphs().len() {
        if changed.contains(&i) {
            assert!(re.paragraph_text(i).contains("【改】"), "目标段含新文");
        } else {
            assert_eq!(re.paragraph_text(i), a.paragraph_text(i), "段 {i} 文本不变");
        }
    }

    // ⑤ 非 document.xml 条目逐字节不变（压缩字节级）。
    let ea = zip_entries(&src);
    let eb = zip_entries(&out);
    assert_eq!(
        ea.iter().map(|(n, _, _)| n.clone()).collect::<Vec<_>>(),
        eb.iter().map(|(n, _, _)| n.clone()).collect::<Vec<_>>(),
        "条目集合不变"
    );
    let mut same = 0;
    for ((name, ma, da), (_, mb, db)) in ea.iter().zip(eb.iter()) {
        if name == "word/document.xml" {
            continue;
        }
        assert_eq!(ma, mb, "压缩方法不变：{name}");
        assert_eq!(da, db, "非目标部件逐字节不变：{name}");
        same += 1;
    }
    eprintln!("e2e: 非目标条目 {same} 个逐字节恒等");

    // ⑥ 产物清单（人工用 Word 打开 02-edited.docx 验收：文字变了、格式没坏）。
    eprintln!("e2e: 产物目录 {}", dir.display());
    let _ = port;
}

/// 第二档：**Word 原生另存**的 docx（\`target/e2e/edges/word-rich.docx\`）
/// ——真实生产形态（Word 的 \`document.xml\` 与 office-io 写面生成的骨架
/// 差异很大）。存在才跑；不存在即跳过（夹具由人工/脚本用 Word 造）。
#[test]
fn e2e_word_authored_file() {
    let dir = out_dir();
    let src_path = e2e_root().join("edges/word-rich.docx");
    if !src_path.is_file() {
        eprintln!("e2e: 跳过 Word 原生夹具（{} 不存在）", src_path.display());
        return;
    }
    let src = std::fs::read(&src_path).expect("读 Word 夹具");
    std::fs::copy(&src_path, dir.join("03-word-original.docx")).expect("落盘对照");

    let loaded = docx_doc::load_editable(&src_path).expect("可编辑装载");
    let payload = &loaded.payload;
    let (Some(bridge), Some(_port)) = (&loaded.bridge, &loaded.port) else {
        eprintln!(
            "e2e: Word 夹具降级只读（{:?}）——登记，不判失败",
            payload.notice
        );
        return;
    };
    eprintln!(
        "e2e: Word 夹具 {} 行 / {} 段（可编辑 {} / 锁定 {}）",
        payload.text.split('\n').count(),
        payload.props.paragraphs,
        payload.props.editable_paragraphs,
        payload.props.locked_paragraphs
    );
    let mut lines: Vec<String> = payload.text.split('\n').map(|s| s.to_string()).collect();
    // 找一段「纯文字」正文（不含标记/缩进装饰）追加标记。
    // 目标段：第一段非空、非清单的正文行（Word 夹具的「项目概述」标题也算
    // 可编辑——标题文字同样能改，样式保留）。
    let Some(line) = lines.iter().position(|l| !l.trim().is_empty()) else {
        eprintln!("e2e: 行模型无正文——{:?}", lines);
        return;
    };
    lines[line] = format!("{}【Word 改】", lines[line]);
    let plan = bridge.compute(&lines);
    assert_eq!(plan.sets.len(), 1, "只写回一段：{:?}", plan.sets);
    let mut ed = DocxEditor::open(&src).expect("引擎开（Word 原生档）");
    for (p, t) in &plan.sets {
        ed.set_paragraph_text(*p, t).expect("set（Word 原生档）");
    }
    let out = ed.save_to_vec().expect("落账");
    let saved = dir.join("04-word-edited.docx");
    std::fs::write(&saved, &out).expect("落盘");
    eprintln!(
        "e2e: Word 原生档保存 → {}（{} 字节）",
        saved.display(),
        out.len()
    );

    // 重开：目标段新文、其余段原样。
    let re = DocxEditor::open(&out).expect("重开");
    let a = DocxEditor::open(&src).expect("原档");
    let changed: Vec<usize> = plan.sets.iter().map(|(p, _)| *p).collect();
    for i in 0..a.paragraphs().len().min(re.paragraphs().len()) {
        if changed.contains(&i) {
            assert!(
                re.paragraph_text(i).contains("【Word 改】"),
                "段 {i} 新文在场"
            );
        } else {
            assert_eq!(re.paragraph_text(i), a.paragraph_text(i), "段 {i} 未动");
        }
    }
    // 非 document.xml 条目逐字节不变。
    let ea = zip_entries(&src);
    let eb = zip_entries(&out);
    for ((name, _, da), (_, _, db)) in ea.iter().zip(eb.iter()) {
        if name == "word/document.xml" {
            continue;
        }
        assert_eq!(da, db, "Word 原生档非目标部件逐字节不变：{name}");
    }
    eprintln!("e2e: Word 原生档产物 {}", saved.display());
}
