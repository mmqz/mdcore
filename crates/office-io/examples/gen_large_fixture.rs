//! R-1 大文件夹具生成器（#22 任务池 · test-artifacts/large-file）。
//!
//! 经引擎自有写面生成导入边界夹具（写面即产线——顺带在 50MB/100MB
//! 量级压测本 crate 写路径；零外部工具依赖）：
//!
//! - docx：`target_bytes` 量级的变体文本段落文档（每段带序号——
//!   反共享去重/反压缩，模拟真实标书文本密度）；
//! - docx-real（N-6 路线 C · #29）：**结构真实**的标书形文档——密度
//!   对齐 Lead 判定标准（表格数/页 ≥ 0.3、标题数/页 ≥ 1.0、嵌套 ≥ 3
//!   级、页数 ≥ 1330），生成后**自读回验**（引擎读面重导入计块——写
//!   读双面互证，指标 JSON 打印）；
//! - xlsx：恒 1,000,000 格（写面上限 [`crate::write::MAX_CELLS`]——
//!   T3 探针 100 万格实测面）× 变体文本，流式表（constant_memory
//!   行序前向纪律）；总字节数取决于文本长度——`cells` 行数参数控制。
//!
//! 用法（仓库根）：
//! ```text
//! cargo run -p mdc-office-io --example gen_large_fixture -- docx out.docx 50000000
//! cargo run -p mdc-office-io --example gen_large_fixture -- docx-real out.docx 1330
//! cargo run -p mdc-office-io --example gen_large_fixture -- xlsx out.xlsx 250 250
//! ```
//!
//! 产物不入库（.gitignore：test-artifacts/large-file/*.docx|xlsx|pdf）。
//!
//! # docx-real 写面能力边界（诚实登记——路线 C v1）
//!
//! 判定标准三项无法经现有写面表达（写面扩展后补齐，登记非阻塞）：
//! 1. **合并单元格**（~10% 表格含纵横合并）——`DocxWriter::table` 仅
//!    纯文本栅格（xlsx 侧有 `merge()`，docx 侧无对等面）；
//! 2. **分节符**（每 50-80 页一节不同页面设置）——写面单文档级
//!    sectPr，六槽位页眉页脚亦文档级（无奇偶分节差异）；
//! 3. **交叉引用域**（表号/图号 REF）——无 `field_ref` 写构造器（读
//!    面 REF 在登记的 todo!() 面）。

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (kind, out) = match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("docx"), Some(p)) => ("docx", p.clone()),
        (Some("docx-real"), Some(p)) => ("docx-real", p.clone()),
        (Some("xlsx"), Some(p)) => ("xlsx", p.clone()),
        _ => {
            eprintln!("usage: gen_large_fixture <docx|docx-real|xlsx> <out-path> [param]");
            eprintln!("  docx param = 目标字节量（默认 50_000_000 ≈ 50MB）");
            eprintln!("  docx-real param = 目标页数（默认 1330——N-6 判定线下限）");
            eprintln!("  xlsx param = 每表行数（默认 250；列恒 250 → 每表 62,500 格 × 16 表 = 1M 格上限）");
            std::process::exit(2);
        }
    };
    let t0 = Instant::now();
    let bytes = match kind {
        "docx" => {
            let target: u64 = args
                .get(3)
                .and_then(|s| s.parse().ok())
                .unwrap_or(50_000_000);
            gen_docx(target)
        }
        "docx-real" => {
            let pages: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1330);
            gen_docx_real(pages)
        }
        _ => {
            let rows: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(250);
            gen_xlsx(rows)
        }
    };
    let gen_ms = t0.elapsed().as_millis();
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("write {out}: {e}"));
    println!(
        "kind={kind} path={out} bytes={} gen_ms={gen_ms}",
        bytes.len()
    );
}

/// 变体段落文本（序号前缀反压缩/反去重 + 固定语料池拼接——模拟标书
/// 文本密度；UTF-8 CJK 混合）。
fn para_text(i: usize) -> String {
    let pool = [
        "本项目为智能文档平台建设，涵盖需求解析、协同编辑与格式保真三大子系统。",
        "投标人应具备同类项目实施经验，并提供近三年内不少于两个案例证明材料。",
        "系统须支持国产化环境部署，包括信创操作系统与主流数据库适配能力。",
        "服务响应时间要求：工作日 9:00-18:00 电话支持，2 小时内远程响应。",
        "验收标准以本技术要求书逐项核对为准，任何偏离项须在投标文件中明示。",
    ];
    let mut s = format!("第{i}段：");
    for k in 0..3 {
        s.push_str(pool[(i + k) % pool.len()]);
        s.push('。');
    }
    s
}

fn gen_docx(target_bytes: u64) -> Vec<u8> {
    use mdc_office_io::docx::{DocxWriter, RunSpec};
    let mut w = DocxWriter::new();
    w.heading(1, "大文件边界测试文档（R-1 夹具）")
        .expect("heading");
    // 估算（实测校准：每段 ≈ 426B zip 后——350B UTF-8 文本 + 标签
    // 开销，DEFLATE ≈ 40%；91.6k 段 → 39.1MB 实测 → 段数 = target/426）。
    let est_paras = (target_bytes as f64 / 426.0) as usize;
    for i in 0..est_paras {
        w.paragraph(&[RunSpec::text(para_text(i))]);
    }
    w.save_to_vec().expect("save")
}

/// N-6 路线 C：结构真实标书形文档（#29）。
///
/// 密度模型（确定性——固定种子 LCG，同参逐字节可重现）：
/// - 页数按**保守行数模型**折算：A4 12pt ≈ 36 行/页；单元当量 ≈
///   标题 2 行 + 段落 4.5×4 行 + 表摊 2.3 行 + 列表摊 0.6 行 +
///   脚注摊 0.1 行 ≈ 23 行 → 约 1.57 单元/页；**取 2.0 单元/页
///   （余量 27%——页数判定线下限宁可超出不可不足）**；
/// - 每页单元 1 个标题（章内 H1/H2/H3 轮换——标题数/页 ≈ 1.0）；
/// - 每 3 个页单元 1 张表（4-9 行 × 3-7 列——表数/页 ≈ 0.33 ≥ 0.3）；
/// - 每 10 个页单元 1 组嵌套列表（4 级——满足嵌套 ≥ 3）；
/// - 每 20 个页单元 1 个脚注（RunSpec::footnote）；
/// - 文档级六槽位页眉页脚 + 开头 TOC 域；
/// - 生成后自读回验（read_docx_from_bytes 重导入计块）打印指标 JSON。
fn gen_docx_real(target_pages: usize) -> Vec<u8> {
    use mdc_office_io::docx::{DocxWriter, RunSpec};

    // 确定性 LCG（xorshift64*——与 xlsx 面同族，固定种子）。
    let mut state: u64 = 0x5A17_C0DE;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    let mut w = DocxWriter::new();
    // 六槽位（文档级——写面边界见模块文档登记）。
    w.header(&[RunSpec::text("XX 项目招标文件（N-6 结构真实夹具）——页眉")]);
    w.first_header(&[RunSpec::text("XX 项目招标文件——首页页眉")]);
    w.even_header(&[RunSpec::text("XX 项目招标文件——偶数页页眉")]);
    w.footer(&[RunSpec::text("页脚：招标文件编号 MDC-N6-2026")]);
    w.first_footer(&[RunSpec::text("首页页脚：仅限投标用途")]);
    w.even_footer(&[RunSpec::text("偶数页页脚：第 N 页（域占位）")]);
    // 目录域（W-1 同款 field_toc——收录 1-3 级）。
    w.heading(1, "目录").expect("heading");
    w.paragraph(&[RunSpec::field_toc_heading_range(1, 3)]);
    w.heading(1, "第一章 投标邀请书").expect("heading");

    // 保守行数模型：2.0 单元/页（见函数文档——宁可超页不可不足）。
    const UNITS_PER_PAGE: f64 = 2.0;
    let total_units = (target_pages as f64 * UNITS_PER_PAGE) as usize;
    let mut emitted_units = 0usize;
    let mut chapter = 1usize;
    let mut h2_in_chapter = 0usize;
    let mut seq = 0usize; // 全局序号（反压缩）

    let pool = [
        "投标人应对招标文件作出实质性响应，任何负偏离项将按评标办法酌情扣分。",
        "本工程技术要求包含设备参数、施工工艺与验收标准三部分内容。",
        "项目管理机构应配备项目经理、技术负责人、安全员与质量员各一名。",
        "工期要求自开工令下达之日起计算，延误一日按合同价款万分之三计违约金。",
        "质量保证期自竣工验收合格之日起不少于二十四个月，缺陷责任期内免费修复。",
        "投标人近三年财务状况良好，无重大违法记录，未被列入失信被执行人名单。",
    ];

    while emitted_units < total_units {
        let u = emitted_units;
        // 标题轮换：每页单元一个（章 → 节 → 小节 → 小节 → 节 …）。
        if u.is_multiple_of(7) {
            chapter += 1;
            h2_in_chapter = 0;
            w.heading(1, &format!("第{chapter}章 技术标准与要求（第{u}单元）"))
                .expect("h1");
        } else if u % 7 == 2 || u % 7 == 5 {
            h2_in_chapter += 1;
            w.heading(2, &format!("第{chapter}-{h2_in_chapter}节 施工组织与部署"))
                .expect("h2");
        } else {
            w.heading(
                3,
                &format!(
                    "{chapter}.{h2_in_chapter}.{} 技术细节与验收要求",
                    u % 97 + 1
                ),
            )
            .expect("h3");
        }
        // 段落 3-6 段（变体文本）。
        let paras = 3 + (next() % 4) as usize;
        for _ in 0..paras {
            let i = seq;
            seq += 1;
            let mut s = format!("（{i}）");
            for k in 0..3 {
                s.push_str(pool[(i.wrapping_add(k)) % pool.len()]);
            }
            w.paragraph(&[RunSpec::text(&s)]);
        }
        // 每 3 单元一张表。
        if u.is_multiple_of(3) {
            let rows = 4 + (next() % 6) as usize;
            let cols = 3 + (next() % 5) as usize;
            let mut table: Vec<Vec<String>> = Vec::with_capacity(rows);
            for r in 0..rows {
                let mut row: Vec<String> = Vec::with_capacity(cols);
                for c in 0..cols {
                    row.push(format!(
                        "表{u}-{r}-{c}：{}",
                        pool[(u.wrapping_add(r * 7 + c * 3)) % pool.len()]
                    ));
                }
                table.push(row);
            }
            w.table(&table);
        }
        // 每 10 单元一组嵌套列表（4 级）。
        if u.is_multiple_of(10) {
            for depth in 0usize..4 {
                let runs = [RunSpec::text(format!(
                    "嵌套层级 {depth}（单元 {u}）——分项要求 {}",
                    pool[u.wrapping_add(depth) % pool.len()]
                ))];
                w.list_item(&runs, mdc_office_io::docx::ListKind::Ordered, depth)
                    .expect("list_item");
            }
        }
        // 每 20 单元一个脚注。
        if u.is_multiple_of(20) {
            w.paragraph(&[
                RunSpec::text("本条技术参数"),
                RunSpec::footnote(format!("脚注 {u}：引用国家标准 GB/T {u} 相关条款。")),
            ]);
        }
        emitted_units += 1;
    }

    let bytes = w.save_to_vec().expect("save");

    // 自读回验（写读双面互证——引擎读面重导入计块）。
    let doc = mdc_office_io::docx::read_docx_from_bytes(&bytes).expect("self-verify reimport");
    let mut headings = 0usize;
    let mut tables = 0usize;
    let mut paragraphs = 0usize;
    let mut list_paras = 0usize;
    for b in &doc.blocks {
        match b {
            mdc_office_io::docx::Block::Paragraph(p) => {
                if p.heading_level.is_some() {
                    headings += 1;
                } else if p.numbering.is_some() {
                    list_paras += 1;
                } else {
                    paragraphs += 1;
                }
            }
            mdc_office_io::docx::Block::Table(_) => tables += 1,
            _ => {}
        }
    }
    // 估算页数（行数模型口径——真实页数需 Word/LO 实机渲染核定，
    // 本沙箱无实机[诚实登记]；保守模型 + 27% 余量下估算值即下界）。
    let est_pages = total_units as f64 / UNITS_PER_PAGE;
    println!(
        "docx-real metrics: {{\"target_pages\": {target_pages}, \"est_pages\": {est_pages:.0}, \"headings\": {headings}, \"tables\": {tables}, \"paragraphs\": {paragraphs}, \"list_paras\": {list_paras}, \"footnotes\": {}, \"bytes\": {}, \"headings_per_page\": {:.3}, \"tables_per_page\": {:.3}, \"nesting_depth\": 4}}",
        doc.footnotes.len(),
        bytes.len(),
        headings as f64 / est_pages,
        tables as f64 / est_pages,
    );
    bytes
}

fn gen_xlsx(rows: u32) -> Vec<u8> {
    use mdc_office_io::write::{CellValue, WorkbookWriter};
    const COLS: u32 = 250; // 250×250=62,500 格/表 × 16 表 = 1M 格（写面上限）
    let mut w = WorkbookWriter::new();
    // 确定性伪随机（xorshift64*——固定种子，夹具可重现）：抗压缩填充
    // 使 1M 格在 ~100MB 量级（真实语料重复度高、zip 后仅 ~9MB——
    // 无法达到边界档；随机基36文本压缩率 ~65%，1M×~155B ≈ 100MB）。
    let mut state: u64 = 0x5A17C0DE;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state.wrapping_mul(0x2545F4914F6CDD1D)
    };
    let mut text_of = |r: u32, c: u32| {
        let mut s = format!("R{r}C{c}");
        // ~126 个基36字符（126 × 0.66 ≈ 100MB 实测校准）（抗压缩主体）。
        let mut v = next();
        for i in 0..126 {
            if i % 11 == 10 {
                v = next();
            }
            s.push((b'a' + (v % 36) as u8) as char);
            v /= 36;
        }
        s
    };
    for s in 0..16 {
        let mut sheet = w
            .add_sheet_streaming(&format!("S{s}"))
            .expect("streaming sheet");
        for r in 0..rows {
            let mut row = Vec::with_capacity(COLS as usize);
            for c in 0..COLS {
                row.push(CellValue::Text(text_of(r, c)));
            }
            sheet.write_row(r, &row).expect("row");
        }
    }
    w.save_to_vec().expect("save")
}
