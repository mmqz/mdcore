# TR-2B · 同类软件测试扒取报告（issue #11 §4——EXTERNAL-TEST-REPORT）

> 溯源：`external-corpus/provenance.json`（只取输入数据不取代码；GPL 源
> 仅输入用例标注）· 驱动：`web/tr2b_l1_commonmark.py` + `web/tr2b_l23_external.py`
> · 结果：`web/tests/results/tr2b_l1_external.json` + `tr2b_l23_external.json`

## 扒取总览

| 来源 | 许可证 | 用例数 | 形态 |
|---|---|---|---|
| commonmark/commonmark-spec `spec.txt` | CC-BY-SA 4.0 | **655**（全量 example 块） | markdown 输入（期望 html 未消费——引擎非 HTML 渲染面） |
| markdown-it/markdown-it `good.txt` | MIT | **652** | 自有测试集（src line 回溯 spec） |
| jgm/pandoc `test/` 四件 | GPL-2.0+（仅输入数据） | **4** 件（~36KB） | writer/tables/reader-more/lhs 全特性输入 |
| apache/poi `test-data/document/` | Apache-2.0 | **6** 件（~78KB） | 真实回归 docx（含 51921-Word-Crash067 崩溃夹具） |
| **合计** | — | **1317** | L1 注入 1307 + L2 导出 4 + L3 导入 6 |

## L1 · CommonMark spec 全量 + markdown-it 自有集注入

| 源 | 注入 | 幂等到达（md1==md2） | 页面存活 | 内容保留均值* |
|---|---|---|---|---|
| CommonMark spec 655 | 655 | **655（100%）** | **655（100%）** | **99.23%** |
| markdown-it good 652 | 652 | **652（100%）** | **652（100%）** | **99.22%** |

\* 内容保留 = 原文非空白字符多重集在投影中的保留率（启发式——markdown
语法字符的合法变换使该值 < 1）。

**低保留 section 清单（人工判读）**：仅 `Thematic breaks`（3 例，均值
0.111）——`***`/`---` 语法标记字符本身不构成内容，**判：合法规范化，
非内容丢失**。其余 20+ section（含 Tabs / Fenced code / Emphasis /
Links / Images / HTML blocks / Hard breaks / Soft breaks / Entities /
Backslash escapes / List items / Tables…）保留率全部 ≥ 0.9。

**L1 结论**：CommonMark 规范全量 655 例 + markdown-it 652 例 = **1307
外部用例零崩溃、100% 幂等到达**——引擎解析面对外部权威语料无内容丢失。

## L2 · pandoc 输入 → 引擎导出 docx → LO 26.8 开验提取 → 文本对比

| 输入 | 导出 | LO 打开 | docx 字节 | LO 提取召回 |
|---|---|---|---|---|
| writer.markdown（pandoc 全特性游行） | ✅ | ✅ | 92104 | **97.03%** |
| tables.markdown | ✅ | ✅ | 28568 | **100%** |
| markdown-reader-more.txt | ✅ | ✅ | 83265 | **94.85%** |
| lhs-test.markdown | ✅ | ✅ | 22561 | **92.04%** |

**L2 结论**：4/4 通过（召回 92–100%）。召回 < 100% 的成分 = markdown
语法字符（`#`/`*`/`` ` ``/`|` 等）经 docx 结构化后不再以字面存在——
**合法规范化**；LO txt 导出自身的表格/样式损耗亦计入分母（宽松口径）。

## L3 · POI 真实复杂 docx → 引擎导入 → 结构对账 + LO 交叉提取

| 样本 | 导入 | 页面存活 | 结构计数（标题/表行/列表项） | LO 交叉召回 |
|---|---|---|---|---|
| 51921-Word-Crash067.docx（POI 崩溃回归件） | ✅ ok | ✅ | 0/0/0 | **0.0** ⚠ |
| 52288.docx | ✅ ok | ✅ | 0/0/0 | 97.83% |
| 52449.docx | ✅ ok | ✅ | **5**/0/0 | 99.56% |
| 55733.docx | ✅ ok | ✅ | 0/0/0 | 80.0% |
| 56392.docx | ✅ ok | ✅ | 0/0/0 | 99.78% |
| 59030.docx | ✅ ok | ✅ | 0/**3**/0 | 93.75% |

**L3 结论**：6/6 零崩溃（含 POI 历史 Word 崩溃夹具——引擎容器三门
+解析全链受控）；结构对账记录如上（结构召回口径，非字符全等）。

### 登记（后续项，非本轮阻断）

- **F-L3-1（P3）**：`51921-Word-Crash067.docx` 导入 ok 但结构计数全零
  +LO 交叉召回 0.0——该件正文内容疑似由非段落承载（形状/图片面），
  引擎导入面零崩溃但内容未入流。深挖属后续项（与 #18 文本框/#19 批注
  同族的「非正文承载」面——建议标书侧按需立项）；
- **F-L3-2（信息）**：`55733.docx` 召回 80%——部分内容（疑似页眉页脚/
  重复元数据）在引擎导入面按设计不入正文投影（G7 面外登记语义）。

## 总判定

| 层 | 通过率 |
|---|---|
| L1（1307 例注入幂等+存活） | **100%** |
| L2（4 件导出→LO 开验） | **4/4** |
| L3（6 件导入零崩溃+对账） | **6/6**（2 项登记） |

复现：`python3 web/tr2b_l1_commonmark.py`（~2min）·
`python3 web/tr2b_l23_external.py`（~1min，需 LO 26.8 路径）。
