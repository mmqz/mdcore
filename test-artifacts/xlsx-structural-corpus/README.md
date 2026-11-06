# xlsx-structural-corpus —— #265 XLSX 结构面语料（28 件 / 1.2MB）

**结构面验证语料**（issue #265：XLSX 语料缺口——隐藏行列/外部链接
各 1 件、样式继承 0 件）。来源 = Apache POI test-data/spreadsheet
（Apache-2.0，真实 bug 附件/真实文档），逐件 provenance 见
`PROVENANCE.md`。

## 纪律（`test-artifacts/pdf-corpus/` 先例同构）

- **取件脚本 + PROVENANCE + README 入库；xlsx 二进制不入库**
  （.gitignore）。
- 取件：`python3 test-artifacts/xlsx-structural-corpus/fetch_corpus.py`
  （幂等；依赖 curl）。**CI 同款**：windows-test.yml 在
  Test mdc-office-io 前跑本脚本——结构面锁在云端真跑。
- 语料门 = `crates/office-io/tests/xlsx_structural_corpus.rs`：
  #262 三态防御（Lead 评审 2026-11-06 修正：**清单件全缺 = 未取件
  skip** / PROVENANCE 登记件在盘但收集 0 件 = 收集逻辑坏红 /
  缩水红）+ 每类数量下限（≥5）+ 全件 catch_unwind + 形态
  实值断言。

## 分类目录

| 目录 | 件数 | 覆盖面 | 引擎 API（`xlsx_structural.rs`） |
|---|---|---|---|
| `hidden_rows/` | 8 | 4-47 行隐藏、宽列段隐藏（AA..AZ+）、组折叠 | `SheetStructure.hidden_rows/hidden_cols`（范围展开） |
| `external_links/` | 8 | 双外部链接、跨簿引用、链接对 | `ExternalLink`（ECMA-376 §15.2 两级 rels） |
| `num_formats/` | 6 | 5 自定义格式、builtin 日期段 | `StyleTable.effective_num_fmt`（builtin 表 + numFmts） |
| `style_inherit/` | 6 | 466 cellXfs 洪水、3088 cellStyleXfs 巨型 | `XfDef` 继承三元组（applyNumberFormat/xfId 链） |

## 实测基线（2026-11-06，语料门首跑）

```
28 件全解析零 panic；隐藏行总量 ≥30 行次；外部链接 8/8 全命中
（54399 双链接）；custom numFmt 总量 ≥20；53568 cellStyleXfs=3088；
builtin 日期（m/d/yyyy）在 effective 链命中。
```
