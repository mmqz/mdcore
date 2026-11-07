//! #260 稳定性专项②·实施批（B 路线）——DOCX 结构账本：真实语料逐件
//! 结构平价门。
//!
//! # 定位（维度②：读后结构完整）
//!
//! #260 盘点结论：DOCX 面「打开成功 ≠ 结构没丢」——health_check 只数
//! ok/err，P1-1（中段节静默丢）630 件全绿过数月即实证。本文件补齐
//! 缺口：**独立第二意见 ground truth**（quick-xml 原始 XML 直扫，不经
//! docx-rs / read_docx）↔ 读面模型（[`DocxDocument`]）逐件结构平价。
//! 语料 = `test-artifacts/health-check/corpus/` 全量 630 件真实 docx
//! （docx4j 501 + poi-docx 129，tracked——缺席 = 红，#262 纪律）。
//!
//! # 11 个平价面（首钉轮 2026-11-07：592 件 ok 全部 0 差异）
//!
//! | 面 | GT 语义（原始 XML） | 模型面 | 平价公式 |
//! |---|---|---|---|
//! | 节 | 有效 `w:sectPr`（排 sectPrChange 内/sdt 内；#348 修复后空串数值属性经读前归一化不再致丢） | `sections` | `max(1, GT)` |
//! | 表（含嵌套/文本框内） | `w:tbl` 全量 − body 级 sdt 内 + body 级 AC 内 | 递归 `Block::Table` | 相等 |
//! | 嵌套表 | `w:tc` 祖先的 `w:tbl` − sdt 内 | `TableData::nested` | 相等 |
//! | 文本框 | 最外层 `w:txbxContent`（排 Fallback）− sdt 内 − tc 内 − 组损失 | 递归 `Block::TextBox` | 相等 |
//! | 顶层段 | body 直接 `w:p` + body 级 AC 扁平段 | 顶层 `Block::Paragraph` | 相等 |
//! | 页眉页脚槽位 | 有效引用（排 odd 型/strict 文档）r:id 经 rels 解析且部件在场 | Σ 各节挂接槽位 | 相等 |
//! | 脚注条目 | footnotes.xml `w:footnote`（id≥1，排分隔符） | `footnotes` | 相等 |
//! | 尾注条目 | endnotes.xml `w:endnote`（同上） | `endnotes` | 相等 |
//! | 批注条目 | comments.xml `w:comment` | `comments` | 相等 |
//! | 脚注引用位 | body 直接段内 `w:footnoteReference`（排 txbx/Fallback） | Σ `footnote_refs` | 相等 |
//! | 修订插入 run | `w:ins` 内 `w:r`（排 tc/sdt/del/Fallback/move 容器；#343 修复后 hyperlink 容器计入） | Σ `revision_inserted` run | 相等 |
//!
//! # 已知损失清单（GT 公式扣除项——每条机制背书 + 可见性计数）
//!
//! 以下损失是**读面当前真实行为**（多数为 docx-rs 0.4.22 引擎限制，
//! 修复归后续 issue；本账本先钉定「损失可见 + 不再静默」）：
//!
//! 1. **body 级 sdt 内容整体丢**（P1-2 已登记）：`DocumentChild` 词汇表
//!    外跳过（docx.rs extract_blocks `_ => {}`）。语料 29 件（段/表/
//!    文本框三形态，可见性计数不静默）。
//! 2. **表内文本框丢（已修 #345——#318 D2 闭项）**：单元格内容扁平化
//!    为文本串（`TableData::rows`），`w:tc` 内 `w:txbxContent` 结构与
//!    内容双丢 → 文档级第二趟补提取（`scan_tc_textbox_regions`/
//!    `apply_tc_textboxes`——宿主表块跨度后发射 TextBox 块）。
//!    语料 10 件（可见性计数保留——修复前基线对账面；平价公式已
//!    重钉：`txbx_in_tc` 扣除项移除，tests/tr32_tc_textbox.rs 锚）。
//! 3. **组内文本框每组只留 1**：`wgp`（DrawingML 组）内多个 `wsp:txbx`
//!    引擎只解析一个。语料 2 件（60316 组 2 留 1、shapes-with-text
//!    组 2 留 1）。
//! 4. **AC 多 Choice 全展开**（内容重复缺陷）：`mc:AlternateContent`
//!    的全部 Choice + Fallback 段落都进块流（mc-fun 族 5 件——
//!    mc-block 单件 10 Choice + 1 Fallback = 11 段重复面）。钉定现
//!    行为防退化（p_in_ac 计入平价），缺陷登记待 MCE 选择面裁决。
//! 5. **`w:type="odd"` 引用丢（已修 #347——#318 D5 闭项）**：
//!    ECMA-376 合法值 default/first/even，Word 容忍旧值 odd；
//!    docx-rs 词汇表外丢弃 → 自有 sectPr 第二趟补挂
//!    （`parse_legacy_odd_refs`——odd 按 default 语义挂接 + Warning
//!    可见性）。语料 1 件（60293；可见性计数保留；平价公式已重钉：
//!    `refs_odd` 计入挂接面，tests/tr33_odd_header_ref.rs 锚）。
//! 6. **空串属性 sectPr 整体丢**：**#348（#318 D6）已修复**——
//!    `w:pgMar`/`w:pgSz` 数值属性空串经 docx_panic_guard 读前归一化
//!    为 `"0"`（Word 零值语义），sectPr 不再静默丢；GT 的
//!    `sect_pr_dropped_attr` 扣除项已移除（cr065 7 节全量重钉）。
//! 7. **strict OOXML 页眉页脚引用丢（已修 #349——#318 D7 闭项）**：
//!    purl.oclc.org 命名空间文档的引用——typed 引用在场而 docx-rs
//!    联接面按过渡期关系 Type 锁定不联内容 → 末节槽丢。
//!    修复 = `extract_headers_footers` 引用-在-内容-未联接分支的
//!    部件直解析恢复（mid_section_slots 同构）+ 第二趟防御纵深
//!    （`attach_unresolved_refs`——typed 未承载引用兑底）。语料 1
//!    件（strict-sample；可见性计数保留；平价公式已重钉：
//!    `refs_strict_dropped` 计入挂接面，tests/tr34_strict_refs.rs 锚）。

//! 8. **修订面排除容器**（非损失——语义正确面）：`w:del` 内
//!    delText 不提取（#17）；moveFrom/moveTo 容器内 ins 不标记
//!    （Tika-792）。（历史：hyperlink 内嵌 `w:ins` 的 run 整体丢
//!    ——#318 D3 缺陷，#343 修复后从排除项移除，修订插入 run 面
//!    GT 公式同步收紧计入。）
//!
//! # 分类锚点（语料冻结态——重钉须显式）
//!
//! 630 件 = ok 592 + 容器拒绝 16（fuzzer 破坏件）+ 模型解析拒绝 22
//! + panic 0。panic 恒 0 是硬门（新 panic = 红）。
//!
//! # LO 实开门（维度④ 跨应用最小面）
//!
//! 五类各取 1 件钉定代表，本机 soffice `--headless --convert-to pdf`
//! 实开（转换成功 + 产物非平凡）。soffice 缺席（CI）→ **显式 SKIP
//! 横幅**（AGENTS.md 13.1——不静默绿，先例 style_corpus.rs）。

use std::collections::BTreeSet;
use std::io::Read as _;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use mdc_office_io::docx::{read_docx_from_bytes_with_options, Block, DocxDocument, DocxError};
use mdc_office_io::io::ReadOptions;

/// 语料根（tracked——两子目录与 health_check_docx.rs 先例同构）。
const CORPUS_ROOT: &str = "test-artifacts/health-check/corpus";
/// 语料件数锚（provenance 一致性锁：docx4j 501 + poi-docx 129）。
const CORPUS_ANCHOR: usize = 630;

// ---------------------------------------------------------------------------
// 语料收集（#262：递归收集 + 在场空立红）
// ---------------------------------------------------------------------------

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(CORPUS_ROOT)
}

fn corpus_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
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
    let mut v = Vec::new();
    walk(&dir, &mut v);
    v.sort();
    v
}

// ---------------------------------------------------------------------------
// GT 扫描器——quick-xml 独立第二意见（与仓内既有二趟同技术：local-name
// 匹配，命名空间无关——strict OOXML（purl.oclc.org）天然兼容）
// ---------------------------------------------------------------------------

/// 逐件 ground truth 计数（语义见模块文档 11 面表）。
#[derive(Default, Debug)]
struct Gt {
    sect_pr: usize,
    refs_resolved: usize,
    refs_odd: usize,
    refs_strict_dropped: usize,
    p_direct: usize,
    p_in_ac: usize,
    p_in_sdt: usize,
    tbl_total: usize,
    tbl_in_sdt: usize,
    tbl_nested: usize,
    tbl_nested_in_sdt: usize,
    tbl_in_ac: usize,
    txbx_outermost: usize,
    txbx_in_sdt: usize,
    txbx_in_tc: usize,
    txbx_in_group: usize,
    groups_with_txbx: usize,
    ins_runs: usize,
    footnote_ref_body: usize,
    footnote_entries: usize,
    endnote_entries: usize,
    comment_entries: usize,
}

/// 扫描上下文（栈式——进入子元素时克隆 + 按元素名置位）。
#[derive(Clone, Default)]
struct Ctx {
    in_tc: bool,
    in_bsdt: bool,
    in_change: bool,
    in_fallback: bool,
    in_ins: bool,
    in_del: bool,
    in_move: bool,
    in_ac: bool,
    in_wgp: bool,
    group_id: usize,
    txbx_depth: usize,
    body_scope: bool,
    is_body_ctx: bool,
    in_body_p: bool,
}

/// sectPr 流式解析状态（直子属性校验 + 引用采集）。
struct SectParse {
    /// 计入节序（非 sectPrChange/sdt 内）。
    counted: bool,
    /// 当前打开的子孙深度（0 = 直子层）。
    depth: usize,
    refs: Vec<(bool, String, String)>, // (is_footer, w:type, r:id)
}

fn attr_local(key: &[u8]) -> &[u8] {
    match key.iter().position(|&b| b == b':') {
        Some(i) => &key[i + 1..],
        None => key,
    }
}

fn attr_map(e: &quick_xml::events::BytesStart<'_>) -> Vec<(Vec<u8>, String)> {
    let mut out = Vec::new();
    for a in e.attributes() {
        let Ok(a) = a else { continue };
        out.push((
            attr_local(a.key.as_ref()).to_vec(),
            String::from_utf8_lossy(&a.value).into_owned(),
        ));
    }
    out
}

fn attr_get<'a>(attrs: &'a [(Vec<u8>, String)], name: &[u8]) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k.as_slice() == name)
        .map(|(_, v)| v.as_str())
}

/// [Content_Types].xml → 主件路径（Override wordprocessingml.document.
/// main+xml；缺席回落 word/document.xml——HelloWordOnline 主件为
/// document22.xml 的实锚）。
fn main_part_name(za: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> String {
    let Ok(mut zf) = za.by_name("[Content_Types].xml") else {
        return "word/document.xml".into();
    };
    let mut s = String::new();
    if zf.read_to_string(&mut s).is_err() {
        return "word/document.xml".into();
    }
    let mut reader = quick_xml::Reader::from_str(&s);
    let mut buf = Vec::new();
    let mut mains: Vec<String> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                if e.local_name().as_ref() != b"Override" {
                    buf.clear();
                    continue;
                }
                let attrs = attr_map(&e);
                let ct = attr_get(&attrs, b"ContentType").unwrap_or("");
                if ct.contains("wordprocessingml.document.main+xml") {
                    if let Some(pn) = attr_get(&attrs, b"PartName") {
                        // 多 main Override（docx4j INCLUDETEXT chunk 形态
                        // ——phase0 语料实锚：chunk47.docx + document.xml
                        // 双声明）：优先常规 document.xml 主件（docx-rs
                        // 引擎同选——实测模型按 document.xml 解析）。
                        let p = pn.trim_start_matches('/').to_string();
                        if p == "word/document.xml" {
                            return p;
                        }
                        mains.push(p);
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    // 无常规主件 → 取首个 main Override（HelloWordOnline 实锚：
    // document22.xml）；全无 → 惯例名。
    mains.pop().unwrap_or_else(|| "word/document.xml".into())
}

/// rels 部件 → rId → Target。
fn parse_rels(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let attrs = attr_map(&e);
                    let id = attr_get(&attrs, b"Id").unwrap_or("");
                    let tgt = attr_get(&attrs, b"Target").unwrap_or("");
                    if !id.is_empty() {
                        out.push((id.to_string(), tgt.to_string()));
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    out
}

/// zip 内部件名正规化：相对目标按主件目录拼合，绝对目标去前导 /
/// （Bug60341 行内 xmlns:r + /word/footer.xml 绝对目标实锚）。
fn normalize_part(main_dir: &str, target: &str) -> String {
    let t = target.replace('\\', "/");
    let joined = if t.starts_with('/') {
        t.trim_start_matches('/').to_string()
    } else {
        format!("{main_dir}/{t}")
    };
    let mut out: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// 主件 document.xml 结构扫描（单趟栈式——语义见模块文档）。
fn scan_document(xml: &str, is_strict: bool) -> Option<Gt> {
    let mut gt = Gt::default();
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().allow_unmatched_ends = false;
    let mut buf = Vec::new();
    let mut stack: Vec<Ctx> = vec![Ctx::default()];
    let mut sects: Vec<SectParse> = Vec::new();
    let mut next_group = 0usize;
    let mut groups_seen: BTreeSet<usize> = BTreeSet::new();

    loop {
        let ev = reader.read_event_into(&mut buf);
        match ev {
            Ok(quick_xml::events::Event::Start(e)) => {
                let name = e.local_name().as_ref().to_vec();
                let top = stack.last().cloned().unwrap_or_default();
                // ---- sectPr 子树内：直子层属性校验 + 引用采集 ----
                if let Some(sp) = sects.last_mut() {
                    if sp.depth == 0 {
                        // 直子（pgSz/pgMar/headerReference/…）
                        let attrs = attr_map(&e);
                        if name.as_slice() == b"headerReference"
                            || name.as_slice() == b"footerReference"
                        {
                            let is_footer = name.as_slice() == b"footerReference";
                            let ty = attr_get(&attrs, b"type").unwrap_or("").to_string();
                            let rid = attr_get(&attrs, b"id").unwrap_or("").to_string();
                            sp.refs.push((is_footer, ty, rid));
                        }
                    }
                    sp.depth += 1;
                }
                // ---- 元素计数（top = 父上下文）----
                match name.as_slice() {
                    b"p" => {
                        let direct = top.body_scope
                            && !top.in_tc
                            && top.txbx_depth == 0
                            && !top.in_bsdt
                            && !top.in_ac;
                        let in_ac_para = top.in_ac
                            && top.body_scope
                            && !top.in_tc
                            && top.txbx_depth == 0
                            && !top.in_bsdt;
                        if direct {
                            gt.p_direct += 1;
                        } else if in_ac_para {
                            gt.p_in_ac += 1;
                        } else if top.in_bsdt && top.body_scope && !top.in_tc && top.txbx_depth == 0
                        {
                            gt.p_in_sdt += 1;
                        }
                        let mut c = top.clone();
                        // 脚注引用位标注域：body 直接段（非 sdt/AC 扁平段
                        // ——与 scan_note_references 的 XML 侧索引口径一致）。
                        c.in_body_p = direct;
                        // 段不是 body 元素——is_body_ctx 清（run 级 sdt
                        // 判定依赖此位，见上下文置位区注释）。
                        c.is_body_ctx = false;
                        stack.push(c);
                        buf.clear();
                        continue;
                    }
                    b"tbl" => {
                        gt.tbl_total += 1;
                        if top.in_bsdt {
                            gt.tbl_in_sdt += 1;
                            if top.in_tc {
                                gt.tbl_nested_in_sdt += 1;
                            }
                        } else if top.in_ac && top.body_scope && !top.in_tc && top.txbx_depth == 0 {
                            gt.tbl_in_ac += 1;
                        }
                        if top.in_tc {
                            gt.tbl_nested += 1;
                        }
                    }
                    b"txbxContent" => {
                        if !top.in_fallback && top.txbx_depth == 0 {
                            gt.txbx_outermost += 1;
                            if top.in_bsdt {
                                gt.txbx_in_sdt += 1;
                            } else if top.in_tc {
                                gt.txbx_in_tc += 1;
                            } else if top.in_wgp {
                                gt.txbx_in_group += 1;
                                groups_seen.insert(top.group_id);
                            }
                        }
                    }
                    b"sectPr" => {
                        // 进入 sectPr（计态在 sects 栈——上下文不变）。
                        sects.push(SectParse {
                            counted: !top.in_change && !top.in_bsdt,
                            depth: 0,
                            refs: Vec::new(),
                        });
                        stack.push(top);
                        buf.clear();
                        continue;
                    }
                    b"r" => {
                        if top.in_ins
                            && !top.in_change
                            && !top.in_tc
                            && !top.in_bsdt
                            && !top.in_del
                            && !top.in_move
                            && !top.in_fallback
                        {
                            gt.ins_runs += 1;
                        }
                    }
                    b"footnoteReference" => {
                        if top.in_body_p && !top.in_fallback && top.txbx_depth == 0 {
                            gt.footnote_ref_body += 1;
                        }
                    }
                    _ => {}
                }
                // ---- 上下文置位（进入子元素）----
                let mut c = top.clone();
                // is_body_ctx 只属于 body 元素自身的 ctx（克隆即清——
                // 子元素不继承：p > sdt 的 run 级 sdt 不算 body 级，
                // check34_34b 实锚：引擎提取 run 级 sdt 内修订 run）。
                c.is_body_ctx = false;
                match name.as_slice() {
                    b"body" => {
                        c.body_scope = true;
                        c.is_body_ctx = true;
                    }
                    b"tc" => {
                        c.in_tc = true;
                        c.body_scope = false;
                    }
                    b"txbxContent" => {
                        c.txbx_depth += 1;
                        c.body_scope = false;
                    }
                    b"sdt" | b"sdtContent" => {
                        if (top.is_body_ctx || top.in_bsdt) && !top.in_tc && top.txbx_depth == 0 {
                            c.in_bsdt = true;
                        }
                    }
                    b"sectPrChange" => c.in_change = true,
                    b"Fallback" => c.in_fallback = true,
                    b"AlternateContent" => {
                        if top.body_scope && !top.in_tc && top.txbx_depth == 0 && !top.in_bsdt {
                            c.in_ac = true;
                        }
                    }
                    b"ins" => c.in_ins = true,
                    b"del" => c.in_del = true,
                    b"moveFrom" | b"moveTo" | b"moveFromRangeStart" | b"moveToRangeStart" => {
                        c.in_move = true
                    }
                    b"wgp" | b"grpSp" => {
                        next_group += 1;
                        c.in_wgp = true;
                        c.group_id = next_group;
                    }
                    _ => {}
                }
                stack.push(c);
            }
            Ok(quick_xml::events::Event::Empty(e)) => {
                let name = e.local_name().as_ref().to_vec();
                let top = stack.last().cloned().unwrap_or_default();
                if let Some(sp) = sects.last_mut() {
                    if sp.depth == 0 {
                        let attrs = attr_map(&e);
                        if name.as_slice() == b"headerReference"
                            || name.as_slice() == b"footerReference"
                        {
                            let is_footer = name.as_slice() == b"footerReference";
                            let ty = attr_get(&attrs, b"type").unwrap_or("").to_string();
                            let rid = attr_get(&attrs, b"id").unwrap_or("").to_string();
                            sp.refs.push((is_footer, ty, rid));
                        }
                    }
                }
                match name.as_slice() {
                    b"p" => {
                        let direct = top.body_scope
                            && !top.in_tc
                            && top.txbx_depth == 0
                            && !top.in_bsdt
                            && !top.in_ac;
                        if direct {
                            gt.p_direct += 1;
                        } else if top.in_ac
                            && top.body_scope
                            && !top.in_tc
                            && top.txbx_depth == 0
                            && !top.in_bsdt
                        {
                            gt.p_in_ac += 1;
                        } else if top.in_bsdt && top.body_scope && !top.in_tc && top.txbx_depth == 0
                        {
                            gt.p_in_sdt += 1;
                        }
                    }
                    b"tbl" => {
                        gt.tbl_total += 1;
                        if top.in_bsdt {
                            gt.tbl_in_sdt += 1;
                            if top.in_tc {
                                gt.tbl_nested_in_sdt += 1;
                            }
                        } else if top.in_ac && top.body_scope && !top.in_tc && top.txbx_depth == 0 {
                            gt.tbl_in_ac += 1;
                        }
                        if top.in_tc {
                            gt.tbl_nested += 1;
                        }
                    }
                    b"txbxContent" => {
                        // 自闭合 txbxContent 无内容——读面不捕获（scan_
                        // vml_textbox_regions 同口径），GT 同样不计。
                    }
                    b"sectPr" => {
                        // 自闭合 sectPr：无直子——即时结算（#348 修复
                        // 后空串属性经读前归一化不再致丢）。
                        if !top.in_change && !top.in_bsdt {
                            gt.sect_pr += 1;
                        }
                    }
                    b"r" => {
                        if top.in_ins
                            && !top.in_change
                            && !top.in_tc
                            && !top.in_bsdt
                            && !top.in_del
                            && !top.in_move
                            && !top.in_fallback
                        {
                            gt.ins_runs += 1;
                        }
                    }
                    b"footnoteReference" => {
                        if top.in_body_p && !top.in_fallback && top.txbx_depth == 0 {
                            gt.footnote_ref_body += 1;
                        }
                    }
                    b"headerReference" | b"footerReference" => {
                        // 已在 sects 直子层采集（上方分支）；sect 外出现
                        // = 非法形态，忽略。
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                let name = e.local_name().as_ref().to_vec();
                if name.as_slice() == b"sectPr" {
                    if let Some(sp) = sects.pop() {
                        if sp.counted {
                            // #348 修复后：空串数值属性经读前归一化，
                            // sectPr 不再静默丢——全部计入节序。
                            {
                                gt.sect_pr += 1;
                                for (is_footer, ty, rid) in &sp.refs {
                                    let _ = is_footer;
                                    if ty == "odd" {
                                        // 已知损失 #5：Word 旧值 odd 引擎丢。
                                        gt.refs_odd += 1;
                                    } else {
                                        gt.refs_resolved += pending_ref_weight(rid);
                                    }
                                }
                            }
                        }
                    }
                    stack.pop();
                    buf.clear();
                    continue;
                }
                if let Some(sp) = sects.last_mut() {
                    sp.depth = sp.depth.saturating_sub(1);
                }
                stack.pop();
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                // 门 2+3 已过结构校验的件不应到这（模型 ok 而 GT 失败 =
                // 语义漂移，调用面计 gt_scan_fail 断言）。
                eprintln!(
                    "[gt-scan] quick-xml error @ {}: {e}",
                    reader.buffer_position()
                );
                return None;
            }
        }
        buf.clear();
    }
    gt.groups_with_txbx = groups_seen.len();
    // strict 文档引用全丢（已知损失 #7）——refs_resolved 归零转记。
    if is_strict {
        gt.refs_strict_dropped = gt.refs_resolved;
        gt.refs_resolved = 0;
    } else {
        // r:id 解析与部件在场校验由调用面完成（需 rels/zip 信息）——
        // 此处先按「引用条目在场」计，随后 reconcile。
    }
    // 说明：refs_resolved 此处 = 未解析条目数占位；调用面
    // scan_gt 以 rels 实际解析结果覆盖。
    Some(gt)
}

/// 占位（scan_document 内未解析 rid 的权重——恒 1，语义见调用面）。
fn pending_ref_weight(_rid: &str) -> usize {
    1
}

/// notes/comments 部件条目计数（footnotes=w:footnote / endnotes=
/// w:endnote / comments=w:comment；id≥1 且非分隔符型）。
fn scan_notes_part(xml: &str, element: &[u8]) -> usize {
    // 分隔符过滤 + id≥1 下限仅适用于脚注/尾注部件（w:footnote 的 -1/0
    // 分隔符惯例——读面 FootnoteData 同滤）；批注 w:comment 的 id 是
    // 普通标识（0 基合法），不过滤（读面 CommentData 全收）。
    let is_notes = element == b"footnote" || element == b"endnote";
    let mut n = 0usize;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                if e.local_name().as_ref() == element {
                    if is_notes {
                        let attrs = attr_map(&e);
                        let ty = attr_get(&attrs, b"type").unwrap_or("");
                        if ty == "separator" || ty == "continuationSeparator" {
                            buf.clear();
                            continue;
                        }
                        let id = attr_get(&attrs, b"id").unwrap_or("");
                        if let Ok(v) = id.parse::<i64>() {
                            if v < 1 {
                                buf.clear();
                                continue;
                            }
                        }
                    }
                    n += 1;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    n
}

/// 整包 GT 扫描：主件 + rels + notes/comments 部件。
fn scan_gt(bytes: &[u8]) -> Result<Gt, String> {
    let mut za = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let names: Vec<String> = za.file_names().map(|n| n.to_string()).collect();
    let main = main_part_name(&mut za);
    if !names.contains(&main) {
        return Err("main part missing".into());
    }
    // 真实语料存在非 UTF-8 字节件（敌意/老编码样本 2 件）——lossy
    // 转换（计数语义不受替换字符影响；读面同 tolerance）。
    let mut doc_bytes = Vec::new();
    za.by_name(&main)
        .map_err(|e| e.to_string())?
        .read_to_end(&mut doc_bytes)
        .map_err(|e| e.to_string())?;
    let doc_xml = String::from_utf8_lossy(&doc_bytes).into_owned();

    // strict 检测：根元素标签内声明 purl.oclc.org/ooxml（ISO Strict
    // 命名空间）。窗口 = 跳过 <?xml ...?> 声明后的首个 '>'（根标签
    // 收口——声明自身的 '?'>' 不是根标签边界）。
    let after_decl = doc_xml.find("?>").map(|i| i + 2).unwrap_or(0);
    let root_tag_end = doc_xml[after_decl..]
        .find('>')
        .map(|i| after_decl + i + 1)
        .unwrap_or(doc_xml.len().min(2500));
    let is_strict = doc_xml[..root_tag_end].contains("purl.oclc.org/ooxml");

    let mut gt = scan_document(&doc_xml, is_strict).ok_or("document scan failed")?;

    // 引用解析：rels rId → Target → 部件在场（strict 转记已置零——
    // strict 文档的 refs_resolved 在 scan_document 已转 strict_dropped）。
    let main_dir = main.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let rels_part = format!(
        "{}/_rels/{}.rels",
        main_dir,
        main.rsplit_once('/')
            .map(|(_, b)| b)
            .unwrap_or(main.as_str())
    );
    let mut resolved = 0usize;
    if !is_strict && gt.refs_resolved > 0 {
        let mut rels_bytes = Vec::new();
        let has_rels = za
            .by_name(&rels_part)
            .and_then(|mut f| {
                f.read_to_end(&mut rels_bytes)
                    .map(|_| ())
                    .map_err(|e| e.into())
            })
            .is_ok();
        let rels_xml = String::from_utf8_lossy(&rels_bytes).into_owned();
        let rels = if has_rels {
            parse_rels(&rels_xml)
        } else {
            Vec::new()
        };
        // 重新走一遍引用（scan_document 只记了条目数——此处需要 rid）。
        // 简化：直接重扫 sectPr 引用并逐条解析。
        let refs = collect_refs(&doc_xml);
        for (_is_footer, ty, rid) in refs {
            if ty == "odd" {
                continue; // 已知损失 #5：odd 型引擎丢——不占解析位
            }
            if let Some((_, tgt)) = rels.iter().find(|(id, _)| id == &rid) {
                let part = normalize_part(main_dir, tgt);
                if names.contains(&part) {
                    resolved += 1;
                }
            }
        }
    }
    if !is_strict {
        gt.refs_resolved = resolved;
    }

    // notes / comments 部件。
    for (part, element, field) in [
        ("word/footnotes.xml", "footnote", 0usize),
        ("word/endnotes.xml", "endnote", 1),
        ("word/comments.xml", "comment", 2),
    ] {
        if !names.contains(&part.to_string()) {
            continue;
        }
        let mut xml_bytes = Vec::new();
        if za
            .by_name(part)
            .and_then(|mut f| {
                f.read_to_end(&mut xml_bytes)
                    .map(|_| ())
                    .map_err(|e| e.into())
            })
            .is_err()
        {
            return Err(format!("{part} unreadable"));
        }
        let xml = String::from_utf8_lossy(&xml_bytes).into_owned();
        let n = scan_notes_part(&xml, element.as_bytes());
        match field {
            0 => gt.footnote_entries = n,
            1 => gt.endnote_entries = n,
            _ => gt.comment_entries = n,
        }
    }
    Ok(gt)
}

/// sectPr 引用直扫（rid 解析用——与 scan_document 同语义的轻量重扫）。
fn collect_refs(xml: &str) -> Vec<(bool, String, String)> {
    let mut out = Vec::new();
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut in_change = 0usize;
    let mut in_sdt = 0usize;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let name = e.local_name().as_ref().to_vec();
                match name.as_slice() {
                    b"sectPrChange" => in_change += 1,
                    b"sdt" | b"sdtContent" => in_sdt += 1,
                    b"headerReference" | b"footerReference" => {
                        if in_change == 0 && in_sdt == 0 {
                            let attrs = attr_map(&e);
                            let ty = attr_get(&attrs, b"type").unwrap_or("").to_string();
                            let rid = attr_get(&attrs, b"id").unwrap_or("").to_string();
                            out.push((name.as_slice() == b"footerReference", ty, rid));
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::End(e)) => match e.local_name().as_ref() {
                b"sectPrChange" => in_change = in_change.saturating_sub(1),
                b"sdt" | b"sdtContent" => in_sdt = in_sdt.saturating_sub(1),
                _ => {}
            },
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    out
}

// ---------------------------------------------------------------------------
// 模型计数（read_docx 读面）
// ---------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Mc {
    sections: usize,
    section_slots: usize,
    top_para: usize,
    all_table: usize,
    all_nested_table: usize,
    all_textbox: usize,
    footnotes: usize,
    endnotes: usize,
    comments: usize,
    footnote_refs: usize,
    ins_runs: usize,
}

fn walk_blocks(blocks: &[Block], top: bool, c: &mut Mc) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                if top {
                    c.top_para += 1;
                }
                c.ins_runs += p.runs.iter().filter(|r| r.revision_inserted).count();
                c.footnote_refs += p.footnote_refs.len();
            }
            Block::Table(t) => {
                c.all_table += 1;
                if t.nested {
                    c.all_nested_table += 1;
                }
            }
            Block::TextBox(tb) => {
                c.all_textbox += 1;
                walk_blocks(&tb.blocks, false, c);
            }
        }
    }
}

/// #318 D4 验收①：文本框文本采集（文档序，每框 = 内层段落拼接）。
fn textbox_texts(doc: &DocxDocument) -> Vec<String> {
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            if let Block::TextBox(tb) = b {
                let text: Vec<String> = tb
                    .blocks
                    .iter()
                    .filter_map(|b| {
                        if let Block::Paragraph(p) = b {
                            Some(p.text().to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                out.push(text.join(" "));
                walk(&tb.blocks, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out);
    out
}

fn model_counts(doc: &DocxDocument) -> Mc {
    let mut c = Mc::default();
    walk_blocks(&doc.blocks, true, &mut c);
    c.sections = doc.sections.len();
    for s in &doc.sections {
        c.section_slots += s.headers.len() + s.footers.len();
    }
    c.footnotes = doc.footnotes.len();
    c.endnotes = doc.endnotes.len();
    c.comments = doc.comments.len();
    c
}

// ---------------------------------------------------------------------------
// 主门：全语料逐件平价
// ---------------------------------------------------------------------------

/// 单件平价断言（返回违规清单——面名 + GT 期望 + 模型实值）。
fn parity_violations(file: &str, gt: &Gt, m: &Mc) -> Vec<String> {
    let mut v = Vec::new();
    let mut chk = |face: &str, expected: usize, got: usize| {
        if expected != got {
            v.push(format!("{file} [{face}] GT={expected} model={got}"));
        }
    };
    chk("sections", gt.sect_pr.max(1), m.sections);
    chk(
        "tables",
        gt.tbl_total - gt.tbl_in_sdt + gt.tbl_in_ac,
        m.all_table,
    );
    chk(
        "nested_tables",
        gt.tbl_nested - gt.tbl_nested_in_sdt,
        m.all_nested_table,
    );
    // #345（#318 D2）+ #346（#318 D4）：表内/组内文本框均已由文档级
    // 第二趟补提取——`txbx_in_tc` 与 group_loss（txbx_in_group −
    // groups_with_txbx）双扣除项移除（平价重钉：GT 最外层框数 = 模型
    // TextBox 块数 + sdt 损失族扣除；tests/tr32_tc_textbox.rs /
    // D4 补发射锚见 five_category_pinned_anchors）。
    chk(
        "textboxes",
        gt.txbx_outermost - gt.txbx_in_sdt,
        m.all_textbox,
    );
    chk("top_para", gt.p_direct + gt.p_in_ac, m.top_para);
    // #347（D5）+ #349（D7）：odd / strict 两损失面均已修——计入
    // 挂接面（strict 件的 refs_resolved 归零转记 refs_strict_dropped，
    // 恢复面在 extract_headers_footers 的部件直解析分支）。
    chk(
        "hf_slots",
        gt.refs_resolved + gt.refs_odd + gt.refs_strict_dropped,
        m.section_slots,
    );
    chk("footnote_entries", gt.footnote_entries, m.footnotes);
    chk("endnote_entries", gt.endnote_entries, m.endnotes);
    chk("comment_entries", gt.comment_entries, m.comments);
    chk("footnote_refs", gt.footnote_ref_body, m.footnote_refs);
    chk("ins_runs", gt.ins_runs, m.ins_runs);
    v
}

#[test]
fn full_corpus_structural_parity() {
    let files = corpus_files();
    // tracked 语料缺席/空转 = 红（#262 纪律）。
    assert!(
        corpus_dir().is_dir(),
        "tracked corpus missing: {}（checkout 破损？）",
        corpus_dir().display()
    );
    assert!(
        !files.is_empty(),
        "corpus 在场但递归收集到 0 件——目录结构变化，须重钉"
    );
    assert_eq!(files.len(), CORPUS_ANCHOR, "语料件数漂移（重钉须显式）");

    let (mut ok, mut reject_container, mut reject_parse) = (0usize, 0usize, 0usize);
    let mut panics: Vec<String> = Vec::new();
    let mut gt_fails: Vec<String> = Vec::new();
    let mut violations: Vec<String> = Vec::new();
    // 已知损失可见性聚合（不静默——汇总打印）。
    let (mut sdt_loss, mut txbx_tc_fixed, mut group_txbx_fixed, mut odd_ref_fixed) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut ac_flatten_files, mut strict_fixed) = (0usize, 0usize);
    let (
        mut nested_files,
        mut txbx_files,
        mut ins_files,
        mut hf_files,
        mut fn_files,
        mut cm_files,
        mut multi_sect,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);

    for p in &files {
        let rel = p
            .strip_prefix(corpus_dir())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        }));
        let doc = match outcome {
            Err(_) => {
                panics.push(rel);
                continue;
            }
            Ok(Err(e)) => {
                match e {
                    DocxError::Container(_) => reject_container += 1,
                    DocxError::Parse(_) => reject_parse += 1,
                    _ => reject_parse += 1,
                }
                continue;
            }
            Ok(Ok(d)) => d,
        };
        ok += 1;
        let gt = match scan_gt(&bytes) {
            Ok(g) => g,
            Err(e) => {
                gt_fails.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let m = model_counts(&doc);
        violations.extend(parity_violations(&rel, &gt, &m));

        // 类别与可见性聚合。
        if gt.tbl_nested > gt.tbl_nested_in_sdt {
            nested_files += 1;
        }
        if m.all_textbox + gt.txbx_in_sdt + gt.txbx_in_tc > 0 {
            txbx_files += 1;
        }
        if gt.ins_runs > 0 {
            ins_files += 1;
        }
        if gt.refs_resolved + gt.refs_odd + gt.refs_strict_dropped > 0 {
            hf_files += 1;
        }
        if gt.footnote_entries > 0 {
            fn_files += 1;
        }
        if gt.comment_entries > 0 {
            cm_files += 1;
        }
        if gt.sect_pr > 1 {
            multi_sect += 1;
        }
        if gt.p_in_sdt + gt.tbl_in_sdt + gt.txbx_in_sdt > 0 {
            sdt_loss += 1;
        }
        if gt.txbx_in_tc > 0 {
            txbx_tc_fixed += 1;
        }
        if gt.txbx_in_group > 0 {
            group_txbx_fixed += 1;
        }
        if gt.refs_odd > 0 {
            odd_ref_fixed += 1;
        }
        if gt.p_in_ac > 0 {
            ac_flatten_files += 1;
        }
        if gt.refs_strict_dropped > 0 {
            strict_fixed += 1;
        }
    }

    // ---- 分类锚点（语料冻结态精确锚——漂移 = 显式重钉）----
    assert!(panics.is_empty(), "新 panic（基线恒 0）: {panics:?}");
    assert_eq!(ok, 592, "ok 件数漂移（基线 592——语料或读面变化，须重钉）");
    assert_eq!(
        reject_container, 16,
        "容器拒绝件数漂移（基线 16——fuzzer 破坏样本）"
    );
    assert_eq!(reject_parse, 22, "模型解析拒绝件数漂移（基线 22）");
    assert!(
        gt_fails.is_empty(),
        "GT 扫描失败（模型 ok 而 XML 二趟失败——语义漂移须查）: {gt_fails:?}"
    );

    // ---- 11 面逐件平价（核心门）----
    assert!(
        violations.is_empty(),
        "结构平价违规 {} 条（前 30）：\n{}",
        violations.len(),
        violations
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );

    // ---- 五类覆盖下限（防跑空——2026-11-07 首钉轮实值）----
    assert!(nested_files >= 11, "嵌套表样本缩水: {nested_files}");
    assert!(txbx_files >= 32, "文本框样本缩水: {txbx_files}");
    assert!(ins_files >= 91, "修订插入样本缩水: {ins_files}");
    assert!(hf_files >= 72, "页眉页脚样本缩水: {hf_files}");
    assert!(fn_files >= 88, "脚注样本缩水: {fn_files}");
    assert!(cm_files >= 103, "批注样本缩水: {cm_files}");
    assert!(multi_sect >= 25, "多节样本缩水: {multi_sect}");

    // ---- 可见性汇总（已知损失不静默——#260 账本面）----
    eprintln!("[structural-ledger] 630 件：ok {ok} / 容器拒绝 {reject_container} / 解析拒绝 {reject_parse} / panic 0");
    eprintln!("[structural-ledger] 五类覆盖：嵌套表 {nested_files} / 文本框 {txbx_files} / 修订 {ins_files} / 页眉页脚 {hf_files} / 脚注 {fn_files} / 批注 {cm_files} / 多节 {multi_sect}");
    eprintln!("[structural-ledger] 已知损失（登记缺陷，非平价门内）：sdt 内容丢 {sdt_loss} 件 / AC 扁平重复 {ac_flatten_files} 件；表内文本框已修（#345 D2——第二趟补提取）{txbx_tc_fixed} 件 / 组内文本框已修（#346 D4——第二趟补发射）{group_txbx_fixed} 件 / odd 型引用已修（#347 D5——Default 语义容错挂接）{odd_ref_fixed} 件 / strict 引用已修（#349 D7——部件直解析恢复）{strict_fixed} 件");
}

// ---------------------------------------------------------------------------
// 五类钉定锚点（逐件具体值——语料换血须重钉；数值 = 2026-11-07 首钉轮实测）
// ---------------------------------------------------------------------------

fn read_one(rel: &str) -> DocxDocument {
    let path = corpus_dir().join(rel);
    assert!(path.exists(), "钉定语料缺席（语料换血？须重钉）: {rel}");
    let bytes = std::fs::read(&path).unwrap();
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn gt_one(rel: &str) -> Gt {
    let path = corpus_dir().join(rel);
    let bytes = std::fs::read(&path).unwrap();
    scan_gt(&bytes).unwrap_or_else(|e| panic!("{rel} GT: {e}"))
}

#[test]
fn five_category_pinned_anchors() {
    // ① 嵌套表格：60329（POI 真实件，4 层内表）+ bug65738（1 内表）。
    for (rel, nested, tbl) in [
        ("poi-docx/60329.docx", 4, 5),
        ("poi-docx/bug65738.docx", 1, 2),
    ] {
        let m = model_counts(&read_one(rel));
        assert_eq!(m.all_nested_table, nested, "{rel} 嵌套表数");
        assert_eq!(m.all_table, tbl, "{rel} 总表数");
    }

    // ② 修订记录：check18（4 个 ins run + 1 脚注引用位——修订+脚注交叉）
    //    + cr043-tracked（13 ins run + 3 尾注条目——表插入修订族）。
    let m = model_counts(&read_one(
        "docx4j/revisions_core-ts_check18_accept-destination.docx",
    ));
    assert_eq!(m.ins_runs, 4, "check18 ins run 数");
    assert_eq!(m.footnote_refs, 1, "check18 脚注引用位");
    let m = model_counts(&read_one("docx4j/revisions_cr043_cr043-tracked.docx"));
    assert_eq!(m.ins_runs, 13, "cr043-tracked ins run 数");
    assert_eq!(m.endnotes, 3, "cr043-tracked 尾注条目");

    // ③ 文本框：extraction-probe（11 框——#345（#318 D2）修复后表内框
    //    TBX25_IN_CELL 已补：第二趟补提取 + 平价重钉，基线 10 → 11）。
    let m = model_counts(&read_one("docx4j/text-extraction_extraction-probe.docx"));
    assert_eq!(
        m.all_textbox, 11,
        "extraction-probe 文本框数（D2 修复后含表内框）"
    );
    let g = gt_one("docx4j/text-extraction_extraction-probe.docx");
    assert_eq!(g.txbx_outermost, 11, "GT 原始最外层框数");
    assert_eq!(g.txbx_in_tc, 1, "表内框计数（D2 已修——GT 计数保留对账面）");

    // ④ 页眉页脚：60293（6 槽 = 6 引用全挂——#347（#318 D5）修复后
    //    odd 型引用补挂：5 → 6）+ headers_footers_sections_multiple
    //    （多节 7 槽全挂）。
    let m = model_counts(&read_one("poi-docx/60293.docx"));
    assert_eq!(
        m.section_slots, 6,
        "60293 挂接槽位数（D5 修复后含 odd 补挂）"
    );
    let g = gt_one("poi-docx/60293.docx");
    assert_eq!(
        g.refs_odd, 1,
        "60293 odd 型引用（D5 已修——GT 计数保留对账面）"
    );
    let m = model_counts(&read_one("docx4j/headers_footers_sections_multiple.docx"));
    assert_eq!(m.sections, 2, "多节件节数");
    assert_eq!(m.section_slots, 7, "多节件挂接槽位");

    // ⑤ 脚注/尾注/批注：bug65649（POI 真实件 6 脚注条目）+
    //    review-for-word（5 批注）。
    let m = model_counts(&read_one("poi-docx/bug65649.docx"));
    assert_eq!(m.footnotes, 6, "bug65649 脚注条目");
    let m = model_counts(&read_one(
        "docx4j/revisions_editor_review_review-for-word.docx",
    ));
    assert_eq!(m.comments, 5, "review-for-word 批注条目");
    // #343（#318 D3）修复重钉：hyperlink 内嵌 ins 的 run 计入修订插入
    // 面（「lease」一词——原先 GT 排除 + 模型静默丢 = 17，两侧同步
    // 收紧后 = 18，含 hyperlink 内 id=45 的 1 run）。
    let g = gt_one("docx4j/revisions_editor_review_review-for-word.docx");
    assert_eq!(
        m.ins_runs, 18,
        "review-for-word 修订插入 run（含 hyperlink 内）"
    );
    assert_eq!(
        g.ins_runs, 18,
        "review-for-word GT 同步计入 hyperlink 内 ins"
    );

    // ---- 结构面补充钉定 ----
    // 节：cr065（#348 重钉：7 sectPr 全量——历史 5 = 7 − 2 空串属性
    // 静默丢，读前归一化修复后不再丢；docx_panic_guard 实锚同钉）。
    let m = model_counts(&read_one("docx4j/cr065-decimals-probe.docx"));
    assert_eq!(m.sections, 7, "cr065 节数（空串属性修复后全量）");
    let g = gt_one("docx4j/cr065-decimals-probe.docx");
    assert_eq!(g.sect_pr, 7, "cr065 GT 有效节");
    // sdt 内容丢（P1-2）：Bug54771a 双表全在 sdt 内 → 模型 0 表。
    let m = model_counts(&read_one("poi-docx/Bug54771a.docx"));
    assert_eq!(m.all_table, 0, "Bug54771a sdt 内表全丢（P1-2 可见性）");
    let g = gt_one("poi-docx/Bug54771a.docx");
    assert_eq!(g.tbl_in_sdt, 2, "Bug54771a GT sdt 内表数");
    // 组内文本框（#318 D4 修复后全保留）：60316（组 2 框——模型面
    // 幸存末位 [Address…] + 第二趟补发首位 [Allison…]）+
    // shapes-with-text（散 3 框 + 组 2 框 = 5，幸存末位
    // "Where some contain text"，补发首位 "A group of shapes"）。
    let m = model_counts(&read_one("poi-docx/60316.docx"));
    assert_eq!(m.all_textbox, 2, "60316 组内框全保留（D4）");
    let g = gt_one("poi-docx/60316.docx");
    assert_eq!(g.txbx_in_group, 2, "60316 GT 组内框数");
    let m = model_counts(&read_one("poi-docx/shapes-with-text.docx"));
    assert_eq!(m.all_textbox, 5, "shapes-with-text 全框保留（散3+组2，D4）");
    let g = gt_one("poi-docx/shapes-with-text.docx");
    assert_eq!(g.txbx_in_group, 2, "shapes-with-text GT 组内框数");
    // D4 验收①：被丢成员具体文本在场（补发首位 + 幸存末位双断言）。
    let texts = textbox_texts(&read_one("poi-docx/60316.docx"));
    assert!(
        texts.iter().any(|t| t.contains("Allison")),
        "60316 补发成员（组首位）文本在场：{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Address 1")),
        "60316 幸存成员（组末位）文本在场：{texts:?}"
    );
    let texts = textbox_texts(&read_one("poi-docx/shapes-with-text.docx"));
    assert!(
        texts.iter().any(|t| t.contains("A group of shapes")),
        "shapes-with-text 补发成员（组首位）文本在场：{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Where some contain text")),
        "shapes-with-text 幸存成员（组末位）文本在场：{texts:?}"
    );
    // AC 扁平（内容重复缺陷钉定）：mc-block 2 直段 + 11 AC 段 = 13。
    let m = model_counts(&read_one("docx4j/mc-fun_mc-block.docx"));
    assert_eq!(m.top_para, 13, "mc-block AC 全展开段数（钉现行為）");
    let g = gt_one("docx4j/mc-fun_mc-block.docx");
    assert_eq!(g.p_direct, 2, "mc-block GT 直段");
    assert_eq!(g.p_in_ac, 11, "mc-block GT AC 内段");
    // strict OOXML：段落照常（63），引用恢复挂接（#349 D7 修复——
    // slots=1：typed 引用在、内容未联接的部件直解析恢复）。
    let m = model_counts(&read_one("docx4j/strict_strict-sample-docx.docx"));
    assert_eq!(m.top_para, 63, "strict-sample 段数（内容不受影响）");
    assert_eq!(
        m.section_slots, 1,
        "strict-sample 引用恢复挂接（D7 修复后：部件直解析恢复）"
    );
    // 非常规主件名：HelloWordOnline（document22.xml 经 [Content_Types]
    // 解析）。
    let m = model_counts(&read_one("docx4j/HelloWordOnline.docx"));
    assert_eq!(m.top_para, 1, "HelloWordOnline 主件解析段数");
    // 多 body 敌意件（MultipleBodyBug）：3 个 w:body 全提取。
    let m = model_counts(&read_one("poi-docx/MultipleBodyBug.docx"));
    assert_eq!(m.top_para, 3, "MultipleBodyBug 多 body 段数");
}

// ---------------------------------------------------------------------------
// LO 实开门（维度④ 最小面——五类各 1 件；soffice 缺席 → 显式 SKIP）
// ---------------------------------------------------------------------------

#[test]
fn lo_gate_five_category_representatives() {
    if std::env::var_os("CI").is_some() {
        eprintln!("[structural-ledger/lo-gate] SKIP: CI env — LibreOffice gate 未执行（本机有 soffice 时真跑）");
        return;
    }
    let soffice = std::process::Command::new("which")
        .arg("soffice")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| "soffice");
    let Some(soffice) = soffice else {
        eprintln!("[structural-ledger/lo-gate] SKIP: ENV_MISSING soffice（安装：apt install libreoffice —— AGENTS.md 13.1 口径，不静默绿）");
        return;
    };
    let representatives = [
        ("poi-docx/60329.docx", "嵌套表格"),
        (
            "docx4j/revisions_core-ts_check18_accept-destination.docx",
            "修订记录",
        ),
        ("docx4j/text-extraction_extraction-probe.docx", "文本框"),
        (
            "docx4j/headers_footers_sections_multiple.docx",
            "页眉页脚多节",
        ),
        ("poi-docx/bug65649.docx", "脚注"),
    ];
    let out_dir = std::env::temp_dir().join(format!("lo_gate_260_{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).unwrap();
    for (rel, cat) in representatives {
        let path = corpus_dir().join(rel);
        assert!(path.exists(), "LO 门钉定件缺席: {rel}");
        let out = std::process::Command::new(soffice)
            .arg("--headless")
            .arg("--convert-to")
            .arg("pdf")
            .arg("--outdir")
            .arg(&out_dir)
            .arg(&path)
            .output()
            .unwrap_or_else(|e| panic!("run soffice {rel}: {e}"));
        assert!(
            out.status.success(),
            "[{cat}] soffice 转换失败 {rel}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stem = path.file_stem().unwrap().to_string_lossy();
        let pdf = out_dir.join(format!("{stem}.pdf"));
        assert!(pdf.exists(), "[{cat}] PDF 产物缺席: {pdf:?}");
        let size = std::fs::metadata(&pdf).unwrap().len();
        assert!(size > 500, "[{cat}] PDF 疑似空产物（{size} B）: {rel}");
        eprintln!("[structural-ledger/lo-gate] [{cat}] {rel} → PDF {size} B ✓");
        let _ = std::fs::remove_file(&pdf);
    }
    let _ = std::fs::remove_dir(&out_dir);
}
