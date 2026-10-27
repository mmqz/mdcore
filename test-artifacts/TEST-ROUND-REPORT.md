# TEST-ROUND-REPORT — 全链路敌意测试轮次（issue #8）

> 执行环境：云端沙箱（Linux x86_64）。外部独立工具：LibreOffice 26.2.6.3
> （docx/xlsx 夹具生成与导出物开验）、PyMuPDF（PDF 内容面验证）、
> playwright chromium（真实交互驱动）。**夹具独立性**：全部由 LO 独立
> 生成（不依赖被测引擎构造——首轮手搓最小 OOXML 被 docx-rs 正确拒绝
> `docx_parse: Failed to read from zip`，本身即容器门的一次跨层验证）。
> 驱动脚本：`/home/z/my-project/scripts/testround_{a,b,cd,ef,gj}.py`。

## 总览

| 组 | 用例 | 实质缺陷 | 登记（方言/设计内/后续项） |
|---|---|---|---|
| A Markdown 解析/往返幂等 | 39 | **0** | 0 |
| B Unicode 边界 | 24 | 0 | 2（B-1 P2 / B-2 P3） |
| C 导入保真（字符级召回） | 6 | **0**（召回 100%×6） | 0 |
| D 敌意容器 | 5 | **0**（全拒+页面存活） | 0 |
| E Word 互操作+已知重核 | 2 | 0 | 3（#2/#3/#4 定谳） |
| F 编辑/撤销/重放 | 3 | 0 | 1（#1 定谳） |
| G-J UI 交互/性能/安全/幂等 | 8 | 0 | 1（链接方言） |
| **合计** | **87** | **0 crash / 0 数据损坏 / 0 P0** | **7 项登记** |

## 核心不变量结果（全绿）

1. **幂等性** `f(f(x)) = f(x)`：39+24 用例全过（含 100k 单行、3000 段、
   嵌套深度 12 越界、四反引号嵌三反引号、setext、task list、HTML 块、
   围栏内 marks 字面、表格缺分隔行/列不齐、ZWJ 家庭 emoji、零宽字符、
   bidi 覆盖字符、CJK 扩展 B、BOM 中缀、5000 emoji 单行）。
2. **镜像一致** `__md() = CM6 doc`：除 B-1（CR 字符，见登记）外全过；
   1MB/12000 块文档页内自校验 `equal=true` 且长度恒等。
3. **导入保真**：LO 生成夹具（中文/表格/特殊字符/emoji/长段/xlsx）
   字符级召回 **100%**。
4. **导出确定性**：同文档两次导出 SHA-256 同一（J1）。
5. **导入→导出→导入回环**：召回 **100%**（J2）。
6. **敌意面**：伪 zip/空字节/非 OOXML zip/20000 段巨文档/billion-laughs
   XML 实体——**全部受控拒绝或导入，页面存活零崩溃**（D1-D5）。
7. **撤销**：50 句真实键入（30ms 间隔，均在 1000ms 合并窗内）→ 1 栈项
   一次回退到基线（F1 重做定谳）——**合并窗设计行为**，无限栈无 8 步限制。

## 量化数据（H 组）

| 指标 | 值 |
|---|---|
| 1MB（780,889 B）文档注入 | **220 ms**（12000 块，块计数 4 ms） |
| 全文投影 to_markdown | **73 ms** |
| 12000 块 docx 导出 | **189.8 ms** |
| 5000 emoji 单行往返 | 幂等恒等 |
| 100k 字符单行 | 保长 100,000 恒等 |

## 登记项（7 项，无 P0）

| # | 级别 | 发现 | 证据 |
|---|---|---|---|
| TR-1 | **P2** | **CR 镜像不一致**：`__inject("a\r\nb")` 后 `__md()` 保留 `\r`（模型原文）而 CM6 doc 规范化为 `\n`（视图）——宿主经 `__md()` 保存的内容与用户所见不同。导出面无污染（docx 段落文本已验证干净） | `testround/B/findings.json` crlf/cr 用例 + crlf-export.docx |
| TR-2 | P3 | **方言差异**：块间投影用单 `\n`（markdown.rs `lines.join("\n")`）+ 空行解析跳过——引擎内往返自洽（幂等已证），但与标准 markdown「双换行分段」方言不兼容：导出 .md 粘贴到标准编辑器时段落粘连 | `testround/B/` multi-blank 用例；markdown.rs:32 |
| TR-3 | P3 | **行内 marks 不解析**（已知问题#2 定谳）：`**壹佰贰拾万元整**` 字面保留（`<w:b/>` 不在场）——v0 设计（markdown.rs 模块注释「marks 不写入 MD」设计内） | e1 用例 |
| TR-4 | **P2** | **web 导出链未消费 G2 格式 API**（已知问题#3/#4 定谳）：默认导出仍 10.0pt/85.2pt 边距（引擎默认态）——G2 `default_typography`/`page_geometry` API 已供（main `3f16779`），`docx_export.rs` 映射层消费=后续项 | e2 用例 |
| TR-5 | P3 | **链接为纯文本方言**：`[t](url)` 不走 docx 超链面（rels 零外链）——javascript: 字面留在文本层（docx 无执行面，非安全缺陷；与锚点写面 r-o2-t5 的 w:hyperlink 能力是两条路径） | i1 用例 |
| TR-6 | 信息 | undo 合并窗语义（已知问题#1 定谳）：文档编辑面无 8 步限制（无限栈+1000ms 合并窗，`undo_coalesce_ms` 可调）；**canvas 面 8 步限制为 canvas 自有栈（issue 立项已知，本轮未重测 canvas）** | f1 用例 |
| TR-7 | 信息 | `<script>alert(1)</script>` 注入后导出文本层无 script 字面（引擎解析为普通段丢弃或吸收）——无害且符合预期 | i2 用例 |

## 已知 4 问题重核结论（issue §7 要求）

| 已知问题 | 重核结论 |
|---|---|
| ① Canvas 向日葵 undo 只有 8 步 | canvas 自有栈问题维持已知（本轮文档面定谳无此限制）；canvas 面修复未在本轮范围 |
| ② 行内粗体导出丢失 | **仍在，定性为 v0 方言设计内**（TR-3）——修复 = marks 解析立项（模型扩面） |
| ③ 全文 10pt | **仍在**：G2 API 已供，web 导出链消费=后续项（TR-4） |
| ④ 页边距写死 | **仍在**：同上（TR-4） |

## 复现

```bash
python3 /home/z/my-project/scripts/testround_a.py     # A 组
python3 /home/z/my-project/scripts/testround_b.py     # B 组
python3 /home/z/my-project/scripts/testround_cd.py    # C+D 组（需 /tmp/tr-cd LO 夹具）
python3 /home/z/my-project/scripts/testround_ef.py    # E+F 组（需 LO）
python3 /home/z/my-project/scripts/testround_gj.py    # G-J 组
```

产物：`mdcore/test-artifacts/testround/{A,B,CD,EF,GJ}/findings.json` +
`B/crlf-export.docx`（TR-1 证据）。

## 重核补遗（issue #8 K1-K6 全量闭环——2026-10-27，当前 main 0a622a9）

> 首轮报告重核表只覆盖已知问题①-④（当时口径）；issue §7 的 K4/K5/K6
> 由本补遗以当前构建实测补齐。K1-K6 现状（六条全闭环）：

| # | 问题 | 当前实测状态 | 证据 |
|---|---|---|---|
| K1 | 行内 marks 导出丢失（`**粗体**` 星号原样进 Word） | **已修复**：#15（fd3f884）导出映射层行内解析——五位格式（粗体/斜体/删除线/行内码/粗斜体）run 化零残留 | #15 tr15 回归套件 + #18 关单链 |
| K2 | 字体/字号/行距/首行缩进全不可设（全文 10pt Times New Roman） | **已修复**：#10（7f47558+d991cde）G2 `DefaultTypography` 引擎层 + #16（1b8e1ab）web 设置面产品化消费（CONTRACT §4 v1.4 + ⚙面板 + `__exportDocxStyled` 测试桥 + XML 直读验收） | #16 验收（TR-2D W1/W5 收口） |
| K3 | 页边距写死 3.5/3.0/3.0/3.0 cm | **已修复**：同 G2 `PageGeometry` 链（#10 引擎 + #16 消费） | 同上 |
| K4 | Canvas 只支持矩形节点 | **设计内形态清单**（非缺陷）：模型对象面 = node/edge/group 三类（流程图形态），node 渲染 = `g > rect` 矩形框——非矩形形状属能力清单扩展项（后续项登记，不立案） | `web/k45_recheck.py`：`canvas_node_svg_element = "g"`、state objects/nodes/edges/groups 计数面 |
| K5 | canvas undo 栈实测 8 步 | **证伪（当前构建）**：连续 add×20 → undoDepth=20，20 次 undo 全撤回 objects=0——**无 8 步截断**（与文档面 TR-6 定谳一致：无限栈 + 合并窗） | `web/k5_recheck.py`：`NO-8-CAP` |
| K6 | 表格行之间多出空段落 | **证伪（当前构建）**：3×4 表导出 docx（22455B）——表内行间空段 0、表后空段 0、全文档空段 0 | `web/k45_recheck.py`：`empty_p_between_rows_in_tbl=0 / total_empty_p=0` |

复现：`python3 web/k45_recheck.py`（K4/K6）· `python3 web/k5_recheck.py`（K5）。
