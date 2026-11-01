//! #156：**标书 docx → mang-shell 端到端**（两系统首次真对接）。
//!
//! ## 为什么是这个测试
//!
//! docx 编辑四面貌（段落/表格/域码段/批注脚注）各自有测试，但一直是
//! **分开验证**的：office-io 用自己的夹具，标书系统（ai-bid-research）
//! 用 python 解析 XML——「标书系统生成的 docx 交给 mang-shell 打开
//! 编辑保存，格式是否保真」从未连起来跑过。本测试把两个系统接上。
//!
//! ## 夹具（黄金例进仓——缺失即 FAIL，不许 SKIP）
//!
//! `tests/fixtures/bid-e2e/07_export.docx` = ai-bid-research e2e 全链产物
//! `artifacts/e2e/e2e-20261101-145945/07_export.docx` 快照（174272 字节）：
//! bidcore mock 起草 + docx-rs 导出的**完整标书**——六表（项目概况/
//! 技术偏离/人员/设备/业绩/报价）+ 七格式件（投标函/法定代表人身份证明/
//! 授权委托书/保证金退还/资格声明/无违法记录声明/廉洁承诺）+ **sdt 包裹
//! 的 TOC 域**（docx-rs `TableOfContents` 的落盘形态）。
//!
//! 再生成（夹具漂移核对）：ai-bid-research 仓 `scripts/e2e_bid/
//! run_e2e_artifacts.sh` → `artifacts/e2e/latest/07_export.docx`。
//!
//! ## sdt 盲区（本测试钉定的真对接缺口）
//!
//! docx-rs 读模型把 `w:sdt` 解析为 `StructuredDataTag`，office-io 宽面
//! 读门**结构性丢弃**其内容（登记的模型级切片面外项）；引擎却按原文
//! `w:p` 逐段计数——宽面 609 ≠ 引擎 611，装载对账一度整体降级只读。
//! 修复 = 引擎段落旗标 [`mdc_office_io::docx_edit::ParagraphRef::in_sdt`]
//! + 装载门 sdt 盲区跳过（纯段落可编辑 / sdt 包表格降级只读）。
//! 本测试第一断言就是**可编辑装载不降级**。
//!
//! ## 三段验证
//!
//! - **A 字节对比**：非目标部件逐字节恒等（唯一目标部件 =
//!   `word/document.xml`——段落/单元格编辑的落账面；压缩方法同样不变）；
//! - **B 结构验证**：六表不少、七格式件不少、TOC 域五段保活（begin/
//!   instrText/separate/end + sdt 容器——域不是死文本）；
//! - **C LibreOffice 开档**（软门：soffice 缺失 → `ENV_MISSING` 证据行
//!   不判红；本机有 soffice——49 页基线 ±2 + 两个编辑标记可见）。
//!
//! 四面貌在标书真档上的形态（诚实登记，不粉饰）：
//!
//! | 面 | 结果 |
//! |---|---|
//! | 段落 | 真编辑（Heading1「第一章 招标公告」行尾追加——首 run 格式保持） |
//! | 表格单元格 | 真编辑（六表首格追加） |
//! | 域 | 正文唯一域 = TOC（跨段）→ typed 拒 = 保护语义在真档上的验证；页脚 PAGE/NUMPAGES 在 footer 部件（引擎 v1 坐标系只扫 document.xml——登记缺口） |
//! | 批注 | 标书生成器不产批注（comments.xml 0 条）→ 空集取证（登记缺口） |

use std::path::{Path, PathBuf};

use mang_shell::editor::docx_doc;
use mang_shell::editor::docx_edit_bridge::DocxPort;

/// 夹具路径（黄金例——进仓，缺失即 FAIL）。
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bid-e2e/07_export.docx")
}

/// 产物目录（人工用 Word/LibreOffice 验收原料）。
fn out_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/e2e/docx-bid");
    std::fs::create_dir_all(&dir).expect("建输出目录");
    dir
}

/// 极简 zip 条目扫描（与 docx_edit_e2e 同口径——独立于被测代码）。
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
        assert_eq!(&bytes[p..p + 4], &[0x50, 0x4b, 0x01, 0x02], "CD header");
        let method = u16::from_le_bytes([bytes[p + 10], bytes[p + 11]]);
        let csize = u32::from_le_bytes([bytes[p + 20], bytes[p + 21], bytes[p + 22], bytes[p + 23]])
            as usize;
        let name_len = u16::from_le_bytes([bytes[p + 28], bytes[p + 29]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[p + 30], bytes[p + 31]]) as usize;
        let comm_len = u16::from_le_bytes([bytes[p + 32], bytes[p + 33]]) as usize;
        let lho = u32::from_le_bytes([bytes[p + 42], bytes[p + 43], bytes[p + 44], bytes[p + 45]])
            as usize;
        let name = String::from_utf8_lossy(&bytes[p + 46..p + 46 + name_len]).into_owned();
        let lname_len = u16::from_le_bytes([bytes[lho + 26], bytes[lho + 27]]) as usize;
        let lextra_len = u16::from_le_bytes([bytes[lho + 28], bytes[lho + 29]]) as usize;
        let data_start = lho + 30 + lname_len + lextra_len;
        let data = &bytes[data_start..data_start + csize];
        let plain = if method == 0 {
            data.to_vec()
        } else {
            inflate_mini(data)
        };
        out.push((name, method, plain));
        p += 46 + name_len + extra_len + comm_len;
    }
    out
}

/// 迷你 inflate（storage/deflate——测试独立面）。
fn inflate_mini(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut decoder = flate2::read::DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    out
}

fn part_text(bytes: &[u8], name: &str) -> String {
    let entry = zip_entries(bytes)
        .into_iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| panic!("部件 {name} 在场"));
    String::from_utf8(entry.2).expect("UTF-8")
}

/// 七格式件标题（标书第七章「投标文件格式」的七件——验收 B 段的锚）。
const SEVEN_FORMS: [&str; 7] = [
    "投标函",
    "法定代表人身份证明",
    "授权委托书",
    "投标保证金退还",
    "资格声明",
    "违法记录",
    "廉洁承诺",
];

#[test]
fn bid_docx_open_edit_save_three_stage() {
    let src_path = fixture();
    let src = std::fs::read(&src_path).unwrap_or_else(|e| {
        panic!(
            "黄金例夹具缺失/不可读（{}）：{e}——不许 SKIP，请从 ai-bid-research e2e 再生成并入仓",
            src_path.display()
        )
    });
    let dir = out_dir();
    std::fs::write(dir.join("01-original.docx"), &src).expect("落盘原档");

    // ── ① 打开：可编辑装载（不降级 = sdt 盲区对账生效的第一断言）──
    let mut loaded = docx_doc::load_editable(&src_path).expect("装载");
    let bridge = loaded.bridge.as_ref().unwrap_or_else(|| {
        panic!(
            "标书 docx 必须可编辑装载（降级原因：{:?}）——sdt 盲区对账失效",
            loaded.payload.notice
        )
    });
    let port = loaded.port.as_mut().expect("可编辑装载必带引擎窄面");
    let paras = port.paragraph_count();
    let tables = port.table_cells().len();
    let sdt_blind = port.sdt_paragraphs().len();
    assert_eq!(
        paras, 611,
        "引擎段落数（黄金例钉定——夹具漂移需重生成并更新本断言）"
    );
    assert_eq!(tables, 6, "六表（黄金例钉定）");
    assert_eq!(sdt_blind, 2, "sdt 盲区 = TOC 域两段（黄金例钉定）");
    eprintln!("e2e-bid: open ok paragraphs={paras} tables={tables} sdt_blind={sdt_blind}");

    // ── ② 四面貌编辑 ─────────────────────────────────────────────
    // 段落/单元格：行模型改行 → compute → 引擎 set（与 GUI 保存同链：
    // Editor::save_docx 对同一 bridge/port 走 flush+save）。
    let lines: Vec<String> = loaded
        .payload
        .text
        .split('\n')
        .map(str::to_string)
        .collect();
    let blocks = loaded
        .payload
        .blocks
        .as_ref()
        .expect("可编辑装载必有行块表");
    let para_line = (0..lines.len())
        .find(|&i| {
            matches!(
                blocks.get(i),
                Some(mang_shell::editor::docx_edit_bridge::LineBlock::Para { .. })
            ) && !lines[i].trim().is_empty()
        })
        .expect("有非空正文段行");
    let cell_line = (0..lines.len())
        .find(|&i| {
            matches!(
                blocks.get(i),
                Some(mang_shell::editor::docx_edit_bridge::LineBlock::Cell { .. })
            ) && !lines[i].trim().is_empty()
        })
        .expect("有非空表格单元格行");
    let mut edited = lines.clone();
    edited[para_line] = format!("{}（端到端段落编辑）", edited[para_line]);
    edited[cell_line] = format!("{}（端到端单元格编辑）", edited[cell_line]);
    eprintln!(
        "e2e-bid: para-edit line={para_line} {:?}… | cell-edit line={cell_line} {:?}",
        &edited[para_line][..edited[para_line]
            .char_indices()
            .nth(16)
            .map(|(i, _)| i)
            .unwrap_or(edited[para_line].len())],
        &edited[cell_line][..edited[cell_line]
            .char_indices()
            .nth(12)
            .map(|(i, _)| i)
            .unwrap_or(edited[cell_line].len())],
    );
    // 域面：正文唯一域 = TOC（跨段）——typed 拒 = 保护语义验证。
    let fields = port.fields();
    assert_eq!(fields.len(), 1, "黄金例：正文唯一域 TOC");
    assert!(!fields[0].editable, "TOC 跨段域只读");
    let refused = port.set_field_value(fields[0].para, fields[0].field_idx, "999");
    let refused_msg = refused.err().unwrap_or_else(|| panic!("TOC 必须拒改"));
    assert!(
        refused_msg.contains("cross-paragraph") || refused_msg.contains("跨段"),
        "拒改原因必须是跨段保护语义：{refused_msg}"
    );
    eprintln!("e2e-bid: field-edit 拒（保护语义）：{refused_msg}");
    // 批注面：标书生成器不产批注——空集取证（登记缺口，非测试失败）。
    let comments = port.comments();
    eprintln!(
        "e2e-bid: comments n={}（标书生成器不产批注——登记缺口）",
        comments.len()
    );
    assert!(comments.is_empty(), "黄金例：comments.xml 0 条");

    // 回写计划：恰好两段（段落 + 单元格），其余原文。
    // 原文快照（编辑前取——原 port 之后会被 set_text 落账污染）。
    let before_texts: Vec<String> = (0..paras).map(|i| port.paragraph_text(i)).collect();
    let plan = bridge.compute(&edited);
    assert_eq!(plan.sets.len(), 2, "只写回两段：{:?}", plan.sets);
    for &(p, ref t) in &plan.sets {
        port.set_text(p, t).expect("引擎 set（段落/单元格）");
    }
    let saved = port.save().expect("引擎落账");
    std::fs::write(dir.join("02-edited.docx"), &saved).expect("落盘改后档");
    eprintln!(
        "e2e-bid: saved {}（{} 字节；原 {} 字节）",
        dir.join("02-edited.docx").display(),
        saved.len(),
        src.len()
    );

    // ── ③ 重开断言（编辑都在 / 非目标段原文 / 六表 / TOC 域保活）────
    let re = docx_doc::load_editable(&dir.join("02-edited.docx")).expect("改后档可编辑重开");
    let re_port = re.port.as_ref().expect("重开可编辑");
    let changed: Vec<usize> = plan.sets.iter().map(|(p, _)| *p).collect();
    for (i, before) in before_texts.iter().enumerate() {
        let after = re_port.paragraph_text(i);
        if changed.contains(&i) {
            assert!(after.contains("端到端"), "目标段 {i} 含编辑标记：{after}");
        } else {
            assert_eq!(&after, before, "非目标段 {i} 文本不变");
        }
    }
    assert_eq!(re_port.table_cells().len(), 6, "六表保持");
    let re_fields = re_port.fields();
    assert_eq!(re_fields.len(), 1, "TOC 域保活（重开仍是一条域）");
    assert!(!re_fields[0].editable, "TOC 仍跨段只读");

    // ── A 字节对比：非目标部件逐字节恒等（含压缩方法）──────────────
    let ea = zip_entries(&src);
    let eb = zip_entries(&saved);
    assert_eq!(
        ea.iter().map(|(n, _, _)| n.clone()).collect::<Vec<_>>(),
        eb.iter().map(|(n, _, _)| n.clone()).collect::<Vec<_>>(),
        "zip 条目集合不变（22 部件）"
    );
    let mut same = 0usize;
    for ((name, ma, da), (_, mb, db)) in ea.iter().zip(eb.iter()) {
        if name == "word/document.xml" {
            continue;
        }
        assert_eq!(ma, mb, "压缩方法不变：{name}");
        assert_eq!(da, db, "非目标部件逐字节不变：{name}");
        same += 1;
    }
    assert_eq!(same, ea.len() - 1, "唯一目标部件 = document.xml");
    eprintln!("e2e-bid: A 非目标部件 {same} 个逐字节恒等 ✓");

    // ── B 结构验证：六表 + 七件 + TOC 域五段保活 ──────────────────
    let doc_xml = part_text(&saved, "word/document.xml");
    assert_eq!(doc_xml.matches("<w:tbl>").count(), 6, "六表不少");
    for form in SEVEN_FORMS {
        assert!(doc_xml.contains(form), "格式件在场：{form}");
    }
    assert!(doc_xml.contains("<w:sdt>"), "sdt 容器保真（TOC 域宿主）");
    assert!(doc_xml.contains(r#"fldCharType="begin""#), "域 begin 保活");
    assert!(
        doc_xml.contains(r#"fldCharType="separate""#),
        "域 separate 保活"
    );
    assert!(doc_xml.contains(r#"fldCharType="end""#), "域 end 保活");
    assert!(
        doc_xml.contains(r#"TOC \o &quot;1-3&quot; \h \u"#) || doc_xml.contains("TOC \\o"),
        "TOC 指令保活（域不是死文本）"
    );
    eprintln!("e2e-bid: B 六表 + 七件 + TOC 域五段保活 ✓");

    // ── C LibreOffice 开档（软门：soffice 缺失 → ENV_MISSING 不判红）──
    let soffice = std::process::Command::new("soffice")
        .arg("--version")
        .output();
    if soffice.is_err() {
        eprintln!("e2e-bid: C ENV_MISSING（soffice 不在场——本机必有，CI 无则软门跳过）");
        return;
    }
    let pdf_dir = dir.join("03-lo");
    let _ = std::fs::remove_dir_all(&pdf_dir);
    std::fs::create_dir_all(&pdf_dir).expect("建 LO 输出目录");
    let conv = std::process::Command::new("soffice")
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(&pdf_dir)
        .arg(dir.join("02-edited.docx"))
        .output()
        .expect("soffice 转换");
    assert!(conv.status.success(), "soffice 退出码：{:?}", conv.status);
    let pdf = pdf_dir.join("02-edited.pdf");
    assert!(pdf.exists(), "PDF 产物在场");
    // 页数合理：bid 仓同链 soffice 基线 = 49 页（±2 容差——两处短追加）。
    let info = std::process::Command::new("pdfinfo")
        .arg(&pdf)
        .output()
        .expect("pdfinfo");
    let info_s = String::from_utf8_lossy(&info.stdout);
    let pages: usize = info_s
        .lines()
        .find(|l| l.starts_with("Pages:"))
        .and_then(|l| l.split_whitespace().last())
        .and_then(|n| n.parse().ok())
        .expect("页数解析");
    assert!(
        (47..=51).contains(&pages),
        "页数合理（基线 49 ±2）：{pages}"
    );
    // 编辑内容可见（pdftotext 提取——段落与单元格两个标记都在）。
    let txt = std::process::Command::new("pdftotext")
        .arg(&pdf)
        .arg("-")
        .output()
        .expect("pdftotext");
    let txt_s = String::from_utf8_lossy(&txt.stdout);
    assert!(
        txt_s.contains("（端到端段落编辑）"),
        "段落编辑标记在 PDF 可见"
    );
    assert!(
        txt_s.contains("（端到端单元格编辑）"),
        "单元格编辑标记在 PDF 可见"
    );
    eprintln!("e2e-bid: C LibreOffice {pages} 页 + 两编辑标记可见 ✓");
    eprintln!(
        "e2e-bid: 三段验证全过（A 字节 / B 结构 / C 开档）——产物目录 {}",
        dir.display()
    );
}
