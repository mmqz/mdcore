# PROVENANCE —— #265 XLSX 结构面语料来源登记

> 逐件来源/测试点。机器可读版 = `PROVENANCE.json`（含 sha256/bytes）。
> 重建：`python3 test-artifacts/xlsx-structural-corpus/fetch_corpus.py`（幂等）。

## 许可证

| 来源仓 | 许可证 | 兼容性 |
|---|---|---|
| apache/poi（test-data/spreadsheet） | Apache-2.0 | 与 GPL-3.0 主仓分发兼容 |

## 分类目录

- `hidden_rows/`：8 件（隐藏行列）
- `external_links/`：8 件（外部链接）
- `num_formats/`：6 件（数字格式）
- `style_inherit/`：6 件（样式继承链）

## 逐件登记

| 分类 | 文件 | 大小 | 测试点 |
|---|---|---|---|
| external_links | `54399.xlsx` | 17KB | 双外部链接（Top 150 + Craft venue 两个外部源） |
| external_links | `56502.xlsx` | 11KB | 外部链接（POI bug 附件） |
| external_links | `63934.xlsx` | 13KB | 外部链接 + #,##0 builtin 格式 |
| external_links | `HsGetVal.xlsx` | 16KB | 三表外部链接（HsGetVal 跨簿引用） |
| external_links | `bug67784.xlsx` | 6KB | 外部链接（POI bug 附件） |
| external_links | `link-external-workbook-b.xlsx` | 3KB | 外部链接对之 b（a↔b 链接形态） |
| external_links | `ref-56737.xlsx` | 10KB | 跨簿引用（POI bug 附件） |
| external_links | `ref2-56737.xlsx` | 11KB | 跨簿引用 2（双链接） |
| hidden_rows | `55640.xlsx` | 6KB | sheet1 四行隐藏（r=1..4）——POI bug 附件 |
| hidden_rows | `AverageTaxRates.xlsx` | 22KB | 真实税率表：三表 19 行次隐藏 + B 列隐藏×3 |
| hidden_rows | `ConditionalFormattingSamples.xlsx` | 639KB | 条件格式样本：14 行隐藏 + 0%/$/mm/dd 格式 |
| hidden_rows | `FillWithoutColor.xlsx` | 18KB | 47 行 + AA..宽列段隐藏（大范围形态） |
| hidden_rows | `GroupTest.xlsx` | 7KB | 分组形态：13 行隐藏（组折叠） |
| hidden_rows | `WidthsAndHeights.xlsx` | 8KB | 宽高设置件：2 行 + 1 列隐藏 |
| hidden_rows | `bug65464.xlsx` | 13KB | POI bug 附件：5 行 + 3 列隐藏 |
| hidden_rows | `clone_sheet.xlsx` | 12KB | 克隆表形态：2 列隐藏 |
| num_formats | `45540_classic_Header.xlsx` | 39KB | 5 自定义格式（classic 页眉表） |
| num_formats | `45544.xlsx` | 42KB | 5 自定义格式（7 表） |
| num_formats | `47804.xlsx` | 28KB | 4 自定义格式（POI bug 附件） |
| num_formats | `48962.xlsx` | 49KB | 4 自定义 + builtin 日期段（m/d/yyyy/d-mmm） |
| num_formats | `52348.xlsx` | 5KB | 3 自定义格式（POI bug 附件） |
| num_formats | `56274.xlsx` | 12KB | 自定义格式（POI bug 附件） |
| style_inherit | `49156.xlsx` | 24KB | xf 继承（POI bug 附件） |
| style_inherit | `49273.xlsx` | 10KB | 466 cellXfs（样式洪水——结构面吞吐锚点） |
| style_inherit | `53568.xlsx` | 122KB | 3088 cellStyleXfs（巨型样式表） |
| style_inherit | `54288-ref.xlsx` | 41KB | 外部引用样式（6 自定义格式 + xf 链） |
| style_inherit | `54288.xlsx` | 28KB | 9 cellXfs + 19 cellStyleXfs（xfId 继承链） |
| style_inherit | `55406_Conditional_formatting_sample.xlsx` | 12KB | 641 cellStyleXfs（条件格式样式族） |

（生成时间 2026-11-06T11:09:13；URL = raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/<文件名>。）
