# N-2 TOC 1200 条目验收报告（#29）

> 日期：2026-10-28 · 引擎 rev = 本提交 · 渲染甲骨文：LibreOffice 25.2.3.2（Word F9 实机待 #28 Windows 窗口——诚实登记）
> 复现：`python3 test-artifacts/n2-toc-1200/run_n2.py`（需 harness release 构建 + soffice + LO Standard 库宏）

## 方法（Word F9 的沙箱等价）

1. 1200 标题夹具（400 单元 × H1/H2/H3，确定性）→ 引擎导出 `{"toc":{"enabled":true,"level_from":1,"level_to":3}}`（W-1 面）；
2. **LO Basic 宏 `UpdateTocExport`**（`~/.config/libreoffice/4/user/basic/Standard/Module1.xba`——注入 LO 用户配置）：隐藏打开 → 两轮 `refresh()` + `DocumentIndexes.update()`（目录自身占 ~31 页改分页——第二轮收敛页码引用）→ PDF 导出；
3. PyMuPDF 验收三件：条目计数 / 层级序 / 页码引用真实性抽查。

## 验收结果（result.json 机器可读）

| 指标 | 要求 | 实测 | 判定 |
|---|---|---|---|
| TOC 条目数 | 恰 1200 无截断 | **1200**（H1 400 + H2 400 + H3 400） | ✅ |
| 层级序 | 无层级错乱 | 前 12 条 = 1→2→3 严格循环 | ✅ |
| 页码引用 | 引用实际分页 | 抽查 20/20 命中（100%）——标题文本确在引用页 | ✅ |
| `level_from/to` 范围控制 | 可配 | XML 指令面 `TOC \o "2-3"` ✓ / `"1-3"` ✓ | ✅ |

页数：81（未更新域）→ **112**（目录展开 +31 页）——目录域条目密度 ~38.7 条/页。

## 诚实登记

- **LO 导入局限（非引擎缺陷）**：`TOC \o "N-M"` 层级范围开关在 LO writerfilter 导入面丢失（宏 dump 定谳：ContentIndex 仅 CreateFromOutline=True 无 level 属性，`update()` 恒全层级）——故范围控制验收落在 **XML 指令层**（导出侧正确性）而非 LO 渲染层。Word 消费方遵循 `\o` 范围开关，实机 F9 验证待 Windows 窗口（#28 同口径）；
- **两轮更新**：目录展开改变分页 → 页码引用需第二轮 update 收敛（Word「更新域→打印前更新域」同语义）；
- 宏驱动 vs Word F9 的语义差：宏 = `DocumentIndexes.update()`（等价 F9 对目录域的更新）——引擎产物侧无差异（同一份 docx）。
