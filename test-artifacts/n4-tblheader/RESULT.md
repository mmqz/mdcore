# N-4 表格跨页表头重复验收（#29 · tblHeader）

> 日期：2026-10-28 · 引擎 rev = 本提交 · 渲染甲骨文：LibreOffice 25.2.3.2（Word 实机待 #28 Windows CI 窗口——同 N-1 口径）
> 复现：`python3 test-artifacts/n4-tblheader/run_n4.py`（需 harness release 构建 + soffice）

## 实现

docx-rs 0.4.22 `TableRowProperty` 无 tblHeader 面（版本钉死不可升——P2-2 serde 通道锁 0.4.22），故：

- `DocxWriter::table_header_repeat(&rows)`：发射同 `table()` + 文档序记账（`header_tables`）；
- save 漏斗内 zip 后处理 `mark_table_header_rows`：document.xml 锚定注入首行 `<w:trPr />` → `<w:trPr><w:tblHeader /></w:trPr>`（三形态防御：自闭合两拼写 / 空开闭形 / 开形含子 / 无 trPr 兜底；`<w:trPr></w:trPr>` 替换而非追加——杜绝双 trPr[CT_TrPr maxOccurs=1]）；非目标部件 raw_copy 逐字节保持；
- wasm 导出链 Table 分支接线（GFM 首行即表头同构；≥2 行才打标——单行表无跨页面）；
- **纯 `table()` 消费方字节级零变化**（office-io 既有产物兼容锚，测试钉定）；
- 读面不回读（docx-rs 反序列化丢 trPr 扩展——round-trip 缺口登记于映射表）。

## LO 渲染验收（80 行长表 × 引擎缺省档 → 2 页）

| 版本 | 第 2 页顶部文本 | 表头在场 |
|---|---|---|
| **注入后**（tblHeader） | **序号 / 项目名称 / 金额** / 45 / 跨页测试项目第45项名称… | ✅ |
| 对照组（无 tblHeader） | 45 / 跨页测试项目第45项名称 / 45.00 / 46… | ❌ |

差异归因干净：唯一变量 = 首行 tblHeader（对照组由注入产物反改 zip 制备——同源对照）。**判定 PASS**（机器可读 = result.json）。

## 测试

- office-io `tests/n4_table_header.rs` 6 条：恰首行打标 / 纯 table() 零注入（字节稳定锚）/ 混合表文档序记账（a0 < mark < A1）/ 单行表不打 / 注入产物读回可解析 / 非 document 部件 raw_copy 逐字节等；
- wasm `gfm_table_first_row_gets_tblheader`：GFM 分隔行剔除 + 恰表头行打标 + 读回可解析。

## 开发期缺陷记录（诚实登记）

注入器首版两处缺陷经真语料探针定谳后修复：① `strip_prefix` 返回值误用为长度（`rest[stripped.len()..]` → 按剩余串长度跳切——产物截断/乱序，Python 独立解压定位）；② 测试断言坐标误写（trPr 先于单元格文本）。修复后 6+1 测试 + LO 双盲对照全绿。
