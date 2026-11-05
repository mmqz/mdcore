# 阶段 B 竞品体检报告 · 批 1（xlsx 面 / calamine 语料）

> 体检日期：2026-10-29 · runner：`crates/mdcore-wasm/tests/health_check_xlsx.rs`（常驻回归门）
> 语料：tafia/calamine `tests/` 75 件 .xlsx（MIT，钉定 commit `c922fc0`，provenance.json 在档）
> 基线：`BASELINE.json` 节 `hc_xlsx_calamine`（下降即红——M-1 同构语义）
> 全量证据：`results/calamine.json`（75 件逐件六维记录）
>
> **范围声明**：本报告只覆盖引擎（mdcore/office-io/mdcore-wasm）面。标书侧业务件
> 的召回发现（如 0.386 否决项召回）属标书仓问题域，按 Lead 指示不混入本报告。

## 六维总览

| 维 | 判据 | 实测 | 评价 |
|---|---|---|---|
| 崩溃率 | 零 panic | **0/75** | ✅ 含 OOM_alloc 资源炸弹系全过——安全门有效 |
| 解析成功率 | 三分归类 | **ok 72 / err 3 / panic 0** | ✅ 3 err 均 typed 拒绝（见 #7/#8） |
| 内容保真 | 字符级召回（vs 值面 GT） | **均值 0.8633**；≥0.99 者 50 件 | ⚠️ 双峰：值面簿健康，公式/窗口面塌陷（#1/#2/#3） |
| 结构保真 | 四断言聚合 | **67/72** | ⚠️ 5 件窗口截断（#1） |
| 往返一致 | import→export→import 精确 | **0/72**（line_eq p50=0.095） | ❌ 系统性失败（#4/#5） |
| 幂等 | 二轮定点 | **0/72**（8 轮内均未定点） | ❌ 不收敛（#4） |

## Top 10 发现（按严重度排序）

### 1. 渲染窗硬截断：真实数据簿 74.5% 内容损失 🔴

**现象**：`minimal_package.xlsx`（真实逮捕登记簿，196 行×31 列）导入 md 召回 **0.242**。
**证据**：`rows_dropped=146, cols_dropped=4`——50/32/16 三窗（`MAX_ROWS/MAX_COLS/MAX_SHEETS`，xlsx_import.rs:155-159）截掉 146/196 行。
**根因**：窗口是防御性设计（R-1 系 50MB 件的防线），但默认值对真实数据簿过紧：50 行连一张中等报表都装不下。
**影响面**：任何 >50 行的 xlsx 导入都静默丢内容（P2-4 警告面会显示 error 级 `rows_dropped`，但**内容不可恢复**）。窗口截断案共 5 件（146/50/26/16/16 行）。
**自家测试盲区**：r 系列/office-io 测试全部用 ≤50 行合成夹具——窗口**恰在边界内**，从未有真实数据测过窗口本身的召回代价；R-1 只测了极端 50MB。
**建议**：窗口上限提级（如 5,000 行/表 + 常驻警告）或开放用户配置；`import_xlsx` 文档标注硬窗。

### 2. 公式格缓存值面系统性缺席：公式主导簿召回 0.0 🔴

**现象**：3 件公式主导簿（`column_row_ranges` / `issue_565_multi_axis_shared` / `issue_567_absolute_shared`）召回 **0.000**。
**证据**：md 渲染 `` `=SUM(E:F)` ``（公式文本），GT 值面是缓存数值——两字符集**零交集**。混合簿按公式密度线性滑落（`string-ref` 0.646：跨表引用格的值面缺席；`shared_formula_*` 0.33-0.67）。
**根因**：P2-3 双通道设计——公式格 md 单元格载公式通道（可编辑性优先），值通道只进 `stats.formulas[].cached`，**不进 md 正文**。
**影响面**：内容提取/引用场景（把 xlsx 当数据源读）公式格的值全丢——标书材料表格里公式格（合计行！）读出来是公式文本而非数字。
**自家测试盲区**：P2-3/Q3-2 测试断言公式**往返**成立（backtick ↔ Formula 格），从未度量公式格**值面召回**；测试夹具的公式格恰有显式文本邻格掩盖了值缺席。
**建议**：公式格渲染改为 `` `=SUM(E:F)`=45306 `` 双载（或 stats 外加值面 md 附件）——往返语义与 Q3-2 对称性需回归验证。

### 3. 错误格值面同样缺席 🔴

**现象**：`errors.xlsx` 召回 0.525——GT 是 `#DIV/0! #NAME? #VALUE! #NULL! #REF! #NUM! #N/A` 七种错误码文本，md 只有 `` `=5/0` `` 公式文本。
**根因**：与 #2 同族——错误格是「值=错误码」的公式格变体，md 只载公式通道。
**影响面**：审计/校对场景无法从 md 看到哪些格出了错。
**自家测试盲区**：cell_types.rs 断言错误格 typed 分类正确，但 md 渲染面的错误码缺席无断言。
**建议**：错误格 md 渲染错误码文本（`#DIV/0!` 本身就是 Excel 的显示语义——显示面忠实原则）。

### 4. 往返结构膨胀不收敛：8 轮 31 行 → 87 行 🔴

**现象**：72/72 件无精确往返；72/72 件 8 轮内未达定点；line_eq p50=**0.095**（半数件往返后行级等价 <10%）。
**证据**：`date_east_asian.xlsx` md1 31 行 → 8 轮后 87 行单调增长。
**根因**：非表块（md 标题等）导出时进合成 `Document` 表成为文字格；再导入时该表又生成新节——**每轮净增一节**。表名从首格值派生（`Sheet1`→`45306`）进一步破坏身份。
**影响面**：xlsx↔md 反复转换（编辑器保存/加载循环）文档无界膨胀——用户每存一次、读一次，内容多一层壳。
**自家测试盲区**：Q3-2 往返测试用**单轮** import→export→import 内容等价断言（合成夹具恰好无标题块）；**无多轮收敛测试**。这是 r 系列全部测试都没覆盖的面。
**建议**：①导出面把纯标题文本行并入目标表头而非新开 Document 表；②表名保真（sheet 名从节标题回取）；③补多轮收敛门（roundtrip_n(8) 收敛）。

### 5. 表身份（sheet 名）不保真 🟡

**现象**：`## Sheet1` 节经往返变 `## Document` + `## 45306`。
**根因**：xlsx 侧无节标题概念，导出以表首格值命名（`table_sheet_name`）；导入以 sheet 名做节标题——两个方向的命名语义不对称。
**影响面**：多表簿往返后表序/表名漂移，用户可见结构变化。
**自家测试盲区**：write_roundtrip.rs 测单表合成件，表名恰好是首格文本——不对称性被夹具掩盖。
**建议**：与 #4 一并处理（表名→节标题回写通道）。

### 6. `_x000D_` 实体面 🟡

**现象**：`has_x000D_str.xlsx` 召回 0.538。GT 值含字面 `_x000D_`（xlsx 转义序列的字面保留），md 侧该格是公式 `` `="ABC"&CHAR(13)&CHAR(10)&"DEF"` ``。
**根因**：同 #2 公式族 + xlsx OOXML 转义实体的显示语义边界（`_x000D_` 在 Excel 显示面就是 CR）。
**建议**：随 #2 一并评估；实体面登记 CAPABILITY_NOTES。

### 7. `renamed_workbook_part` 解析失败：版本偏差面 🟡

**现象**：`err:xlsx_parse`（唯一 3 个 err 中的非容器件）。
**根因**：语料取自 calamine **master@c922fc0**（2026-10-22），本仓锁定 **calamine 0.36.1**——该测试件对应的能力（重命名工作簿部件）可能在 0.36.1 之后的版本。GT 读面（office-io 同链）同样失败，非 mdcore 独有缺口。
**建议**：calamine 升版评估时以本件为验收锚点（升版后 class 应 ok→ 基线改善提示再生）；本轮登记为已知版本偏差。

### 8. 容器防线正确性确认 ✅（通过面登记）

`issue252.xlsx` / `pass_protected.xlsx` 均 `err:container_corrupt` typed 拒绝（损坏 zip / 密码保护件）——与 calamine 自家测试意图一致（它们测的就是容器异常），**正确行为**。连同 OOM_alloc 系零 panic，安全门（门 1/门 2/门 3）在外部真实语料上验证通过。

### 9. 往返 line_eq 分布：结构性而非内容性失败 🟡

**数据**：line_eq min=0.011 / p50=0.095 / max=0.667，≥0.99 者 0 件——但**内容**字符召回（md2 vs md1）普遍不低（膨胀壳占行数大头）。行级失败主因是 #4 的 Document 节注入与表名漂移，**数据行本身多数幸存**。修 #4/#5 后此维应大幅回升——留基线观测。

### 10. 性能面健康 ✅

import_ms：p50=3ms / p90=15ms / max=548ms（75 件含资源炸弹）——读面性能无异常件（max 是 OOM_alloc 系的合理防御开销）。

## 自家测试盲区汇总（Lead 核心关切）

| # | 盲区 | 暴露源 | 补测建议 |
|---|---|---|---|
| 1 | 窗口截断的召回代价（真实数据 >50 行） | minimal_package 等 5 件 | 大于窗口的合成件 + 召回断言 |
| 2 | 公式格**值面**召回（非往返） | 17 件公式族 | 公式格值面在 md 可见的断言 |
| 3 | 错误格错误码 md 渲染 | errors.xlsx | 错误码文本断言 |
| 4 | **多轮**往返收敛性 | 全部 72 件 | roundtrip_n(8) 收敛门 |
| 5 | 表名/节标题双向保真 | date_east_asian 等 | 多表簿往返表名断言 |
| 6 | 真实世界数据形态（长文本格/多行地址/日期混排） | arrest 簿 | 真实形态夹具（脱敏） |

## 基线与复跑

```bash
# 常驻门（CI / 提交前——与 BASELINE.json hc_xlsx_calamine 节比对，下降即红）
cargo test -p mdcore-wasm --test health_check_xlsx

# 基线+证据再生（评审物——写后提交入库）
HC_REGEN_BASELINE=1 cargo test -p mdcore-wasm --test health_check_xlsx
```

后续语料批（LO ooxmlexport / docx4j·python-docx / pdf.js——docx/pdf 面）到位后以同构 runner 扩展，六维口径不变。

---

# 批 3 报告（docx 面 / POI 语料）

> 体检日期：2026-10-29 · runner：`crates/mdcore-wasm/tests/health_check_docx.rs`
> 语料：apache/poi `test-data/document/` 129 件 .docx（Apache-2.0，钉定 `ca95db9`，provenance.json 在档）
> 基线：`BASELINE.json` 节 `hc_poi_docx`（下降即红） · 全量证据：`results/poi-docx.json`

## 六维总览

| 维 | 判据 | 实测 | 评价 |
|---|---|---|---|
| 崩溃率 | 零 panic | **0/129** | ✅ 含 clusterfuzz 最小化崩溃件与密码保护件 |
| 解析成功率 | 三分归类 | **ok 111 / err 18 / panic 0** | ✅ 16 件 container_corrupt = 密码保护/容器畸形正确拒绝 |
| 内容保真 | 字符级召回 | **1.0000（全满）** | ✅ 111 件可读件内容零丢失 |
| 结构保真 | 标题数精确等 | **111/111** | ✅ 含文本框扁平化 + 页眉页脚吸收同口径 |
| 往返一致 | 精确 / 行等价 | **85/111 精确**；非精确 26 件 line_eq p50=0.607 | ⚠️ 全部二轮收敛（见 #D4） |
| 幂等 | 二轮定点 | **111/111** | ✅ 与 xlsx 面（0/72）形成鲜明对照 |

## 发现（docx 面）

### D1. docx 面内容保真全满——与 xlsx 面反差本身是发现 ✅

111 件可读件召回全部 1.0000（含 51921-Word-Crash067 等崩溃回归件）。**xlsx 面的召回塌陷（公式/窗口）在 docx 面不存在**——docx 导入的 GT 面（全块文本）与 md 渲染面语义同构。这佐证 xlsx 面的损失是**特定于 xlsx 渲染设计**（公式通道选择 + 窗口限制），不是系统性保真缺陷。

### D2. deep-table-cell 解析失败：docx-rs 依赖层深嵌套缺口 🟡

`deep-table-cell.docx` → `err:docx_parse`（"Failed to read from zip"——docx-rs 0.4.22 锁定版读层失败）。POI 将该件视为有效文件（其测试面向深嵌套表）。**依赖层缺口非 mdcore 自有代码**，但用户面表现就是我们读不了。自家测试 docx_nested_tables.rs 用合成缩影（2 层），真实件更深。建议：登记依赖面缺口 + docx-rs 升版评估时以此为验收锚点。

### D3. ExternalEntityInText 拒绝 = XXE 防御正确行为 ✅

`err:docx_parse`（"Failed to read xml"）——文本内外部实体被 docx-rs 拒读。POI 该件测的是**外部实体处理**；拒绝实体展开正是 XXE 防线。登记为通过面（安全优先于可读）。

### D4. 26 件非精确往返：两类良性模式 🟡

- **标题内联强调丢失**：`## *Introduction*` → `## Introduction`（61787.docx——标题内斜体记号经导出再导入后丢弃；内联 emphasis 在标题块内的往返面）
- **页眉/页脚结构不对称**（9 件嫌疑：DiffFirstPageHeadFoot / FancyFoot / HeaderFooterUnicode 等）：导入时页眉→文档前导文本；导出为正文；再导入不再识别为页眉——**md 无页眉概念的结构性不对称**，内容不丢（召回 1.0）但 md 形态漂移
- **关键对照**：全部 111 件二轮定点收敛——docx 往返**稳定**（对比 xlsx 面 0/72 不收敛 + 无界膨胀）。差异根源：docx 导出把表块与文本块都映射进正文流，无 Document 合成表注入机制

### D5. 性能离群件 🟡

`bug65649.docx` 导入 3,768ms（p50=8ms 的 470 倍）——深/退化结构件的单件长尾。其余 128 件 p90=26ms。建议：perf 面后续专项（非阻断）。

## docx 面结论

**docx 导入面健康度显著优于 xlsx 面**：内容保真全满、结构全等、往返稳定收敛、安全拒绝正确。仅有的两个缺口都在依赖层（docx-rs 深嵌套 + 实体拒读后者实为安全优点）。**修复优先级应全部投向 xlsx 面**（REPORT 上半部 #1-#5）。

---

# 批 4 报告（docx 面 / docx4j 语料）

> 体检日期：2026-10-29 · runner：`health_check_docx.rs`（双语料门之二）
> 语料：plutext/docx4j `core-tests/src/test/resources/` 501 件（Apache-2.0，钉定 `b21e0172`）——修订 147/域 113/run 属性 72/MC 33/图表 32/OpenDoPE 18/文本抽取 16/AlteredParts 13/页眉脚 10/2010 8/OLE 5/strict 4 + 根级
> 基线：`BASELINE.json` 节 `hc_docx4j` · 证据：`results/docx4j.json`

## 六维总览

| 维 | 实测 | 评价 |
|---|---|---|
| 崩溃率 | **panic 4/501**（docx-rs 层，已锁基线——新增即红） | 🔴 依赖层崩溃实锤（见 #E1） |
| 解析成功率 | ok 476 / err 21 / panic 4 | ⚠️ err 集中 word365 属性形态族（#E2） |
| 内容保真 | **1.0000 全满**（476 件可读件） | ✅ 与 POI 批一致 |
| 结构保真 | **476/476 标题数精确等** | ✅ |
| 往返一致 | 382/476 精确（80.3%） | ⚠️ 94 件非精确（同批 3 两类良性模式） |
| 幂等 | **476/476 二轮定点** | ✅ docx 面往返稳定性再次确认 |

## 发现（docx4j 面）

### E1. docx-rs 0.4.22 四处 panic——wasm 面生产即 trap 🔴

| 件 | panic 位点 |
|---|---|
| runprops_word365_props-{b,enum,u}-styles.docx（3 件） | `run_property.rs:110/155` **index out of bounds**（空 vec 取 [0]） |
| strict_strict-sample-docx.docx | `width.rs:18` **ParseFloatError unwrap** |

catch_unwind 在 runner 内捕获（测试不炸），**生产 wasm 面 = trap**。POI 批 129 件零 panic，docx4j 批 501 件抓出 4 处——语料多样性的直接价值。**修复面在依赖层**：上游修复 / 升版评估 / 我方预检防御。基线已锁（新增 panic = 红，消除提示再生）。

### E2. Word 365 非规范属性形态族 22 件失败（19 err + 3 panic）🔴

`runprops_word365_*` 系列失败 **22/23 件**：colour-case/hex/names 变体、onoff={TRUE,empty,space,yes} 变体、props-enum-{missing,other}、props-u-{case,other}、props-toggles——全部 `err:docx_parse` 或 panic。**根因**：docx-rs 读层对 ST_OnOff/ST_Color/枚举的**非规范形态**（大小写、空串、缺省值）不容忍，而 OOXML 规范允许且真实世界存在（老生成器/第三方工具产物）。Word 365 是当前最主流 docx 生产者——**现代 Word 文档导入面在依赖层有结构性风险**（规范形大概率能读、边缘形大面积失败）。自家测试全部用引擎写面生成的规范形——这正是「自家测试没覆盖到」的典型面。

### E3. vml_textbox.docx 容器拒绝——**自家门 3 误拒（已修复）** ✅→🔧

`err:container_invalid_part`——诊断定性为**误拒**并当场修复：
- **根因**：quick-xml 0.41 `buffer_position` 边界计量偏移（实测区段起点含前一事件尾字节/前导空白，终点可截断闭合标签中段）→ docx-rs EventReader 首事件成 `Whitespace`/`Characters` → 门 3 "textbox region malformed" 拒绝**整个文档**。紧凑 Word 件因"首事件任意 StartElement 均可"**恰巧**工作过——pretty-printed 形态（docx4j 等工具输出）是真实盲区
- **连带发现**：残缺闭合标签会令 docx-rs `TextBoxContent::read` 在 EOF 上**死循环**（`_ => {}` 不终止）——修复过程中实证（首版修复只裁起点未补终点即触发）
- **修复**（docx.rs）：scan 区段双端前向修复（起点走到下一 `<`、终点补到下一 `>`）+ parse 首事件空白容忍；防回归锁 tr18 新增 pretty-printed 最小夹具测试
- **实效**：vml_textbox err→ok（召回 1.0）；docx4j 面 ok 476→477 / roundtrip 382→383；poi 面零漂移（111/18 恒——既有行为不受扰动）；tr18 5/5
- **这是体检批的直接产出**：语料 → 发现 → 定性 → 修复 → 回归锁 → 基线改善，全链闭环

### E4. fields_docprop-case 解析失败 🟡

`err:docx_parse`——docProps 域大小写变体（docx4j 专测 case 容忍）。同 #E2 族（大小写不容忍）。

### E5. 其余面健康 ✅

476 可读件召回 1.0 全满、结构全等、全定点收敛、性能 p50=12ms/p90=39ms/max=363ms（无 POI 批的 3.8s 离群）。

## 修复优先级综合（docx 面，跨批 3+4）

1. **docx-rs word365 属性形态族**（22 件）——升版评估或上游 issue（含 4 panic 的崩溃位点在档）
2. deep-table-cell 深嵌套（批 3 #D2）
3. vml_textbox 门交互复查（#E3）
4. 26+94 件非精确往返的两类良性模式（标题内联强调 + 页眉脚不对称——低优先）

---

# 批 5 报告（xlsx 三大发现修复——Lead 01:51 指定优先级）

> 修复日期：2026-10-29 · 修复面：`crates/mdcore-wasm/src/xlsx_import.rs` + `xlsx_export.rs`
> 依据：本报告批 2 Top 发现 #1/#2/#3/#4/#5 + Lead 裁决「修复优先级应全部投向 xlsx 面的三大发现」
> 验证（r38 重跑实证）：G1 989/0（批 4 基线 984 + 本批新增 5 测试）· fmt 0 · clippy 双面恰存量 6 零新增（6 条定位均在非本批文件——canvas/collab/lib×2/workbench/docx_export）· scan 存量红恒（5 命中零新增）· xlsx web 套件全绿零回归（xlsx_import 12/12 + xlsx_export 9/9 + openpyxl 4/4 + xlsx_grid 20/20 + oversized 18/18；连带 web_smoke 38/38 + empty_state/evidence_panel/i18n_ui/integration_journey/sidecar_faults 共 11 套件 159 checks 全绿）

## 修复总览（六维前后对照）

| 维 | 修复前（批 2） | 修复后 | 变化 | 根治的发现 |
|---|---|---|---|---|
| 崩溃率 | 0/75 | 0/75 | 恒 | — |
| 解析成功率 | ok 72 / err 3 | ok 72 / err 3 | 恒（3 err 恒为正确拒绝/版本偏差） | — |
| 内容召回（均值） | **0.8633** | **0.9998** | **+0.1365** | #1 窗口截断 + #2 公式值面 + #3 错误格 |
| 结构保真 | 67/72 | **72/72** | **+5** | #1 窗口截断（5 件窗口案全解） |
| 往返精确 | **0/72** | **59/72** | **+59** | #4 Document 膨胀 + #5 表名漂移 |
| 幂等（二轮定点） | **0/72** | **72/72** | **+72** | #4 膨胀不收敛根治 |

召回最低件从 0.0（公式主导簿）升至 0.9831（hyperlinks_escaped——转义 URL 文本面，登记）；74/75 件 ≥0.99。13 件非精确往返**全部二轮定点**（空表/错误格/富文本/链接格族——首轮降级施加后收敛，与 docx 面同构语义）。

## 修复一：渲染窗提级（#1——74.5% 内容损失案）

- **常量**：`MAX_SHEETS` 16→64 / `MAX_ROWS` 50→5,000 / `MAX_COLS` 32→128（xlsx_import.rs）
- **依据**：体检实测 minimal_package 196 行丢 146 行（召回 0.242）+ 36 列簿丢 4 列 + 5 件窗口案；R-1 系 50MB 件 ~3,250 行亦全覆盖
- **安全层不变**：读面上限（1,000 表/4M 格）+ 容器预扫门（门 1/2）仍是大档防线；渲染窗仅消费子集截断，越限计数 + P2-4 error 档警告恒在
- **测试**：`window_raise_covers_real_scale_sheets`（196×36 全量导入零丢弃零警告）+ `caps_truncate_with_drop_stats` 稀疏比例化重写（夹具不随窗口提级线性变重）

## 修复二：公式格值面双载（#2 + #3——公式簿召回 0.0 案）

- **导入面**：缓存值非空 → 单元格 `` `=SUM(E:F)`=45306 `` 双载形（公式原文 + `=` + 缓存值显示面）；错误格缓存 = 错误码文本（`#DIV/0!` 即 Excel 显示语义——显示面忠实原则）；缓存空（calamine Empty）保持单载
- **导出面**：`cell_value` 双载解析（首/末反引号括号语义——内层可含反引号不双解）+ `parse_cache` 缓存重建（Boolean/Number/Text 三型）→ 写回 xlsx 缓存通道（`set_formula_result`）——**缓存值面完整往返**（原 `cached None → Float(0.0)` 降级根治）
- **错误码链**：引擎 `write_formula_cell` 对错误码写 `t="e"` → 读回 `Data::Error` → 显示面逐字保留——**错误缓存面完整往返**（非降级）
- **登记降级（引擎面，非缺陷）**：`write_formula_cell` 对可解析数值串**恒写数值面**（无 t 属性）——非规范形（前导零 `007`/科学形 `1e5`）读回规范化（`=7`）——测试钉定
- **测试**：`dual_load_formula_detection_matrix`（12 分支）+ `dual_load_formula_roundtrip_via_t17`（数值/文本/错误码三缓存面全环逐字 + 降级钉定）

## 修复三：表名通道 + 往返收敛（#4 + #5——31→87 行膨胀案）

- **导出面**：表块的**紧邻前驱 Heading**（同一兄弟序列、未被他表认领、清洗后非空非 History）→ 标题文本成为 sheet 名（去重），标题**不再落 Document 表**（单落）——导入面 `## {表名}` 节标题的对称回取；不可认领回退 A1 派生（原行为）
- **实效链**：`## Sheet1` + 表 → sheet `Sheet1` → 再导入 `## Sheet1` + 表（表身份保真，不再 `Sheet1`→`45306` 漂移）；Document 表逐轮注入膨胀（每轮净增一节）根治——纯表文档**首轮精确**，混排文档**二轮定点**（Document 前置重排为已知结构差异，峰值有界）
- **测试**：`heading_claims_adjacent_table_sheet_name`（紧邻认领/隔块不认领/History 排除/一标题两表/同名去重）+ `roundtrip_converges_with_heading_named_tables`（情形 A 首轮精确 + 情形 B 二轮定点 + 峰值有界）

## 修复四（连带）：Q3-2 对称性升级

- excel-complex 验收（openpyxl 独立读）：sheet 名 `["Document","序号","项目"]` → `["报价明细","评分表"]`（标题认领，无 Document 注入）；公式缓存 `{0,0}` → `{100,1}`（双载通道保真——openpyxl `data_only` 可读）；对称判据从「去 Document 流的表块等价」升级为**精确对称**（2 表 == 2 表逐格全等）
- `tables_eq`/`formulas_ok` 断言重写 + `openpyxl_dualload_cached_value` 新检查项（4/4 checks）

## 剩余登记（非本批范围）

1. `hyperlinks_escaped.xlsx` 召回 0.9831——转义 URL 文本面（GFM 链接形清洗），最低件但 ≥0.98
2. `_x000D_` 实体面（批 2 #6）——随双载已可见化（cached 值面含实体文本），实体语义边界维持登记
3. 13 件非精确往返的二轮定点点位（空表/richtext/链接族）——已收敛，模式登记不阻塞
4. `renamed_workbook_part` 版本偏差（批 2 #7）——calamine 升级评估时以该件为验收锚点

---

# 批 6 报告（#226 R2：docx 面 4 处 panic 修复收口——预检防御落地）

> 修复日期：2026-11-06 · 修复面：`crates/office-io/src/docx_panic_guard.rs`（新模块）+
> `docx.rs` 读管线接线 + mang-shell `docx_doc.rs` 打开入口 catch_unwind 兜底
> 依据：本报告批 4 #E1（docx-rs 0.4.22 四处 panic 实锤）+ 全仓审计 P0-2（issue #226）
> 基线动作：`HC_REGEN_BASELINE=1` 再生（panic 4→0 入档）· 全量证据：`results/docx4j.json` 再生

## 修复前后对照（docx4j 面，六维）

| 维 | 修复前（批 4 基线） | 修复后 | 变化 |
|---|---|---|---|
| 崩溃率 | panic 4/501 | **panic 0/501** | **−4（清零）** |
| 解析成功率 | ok 477 / err 20 / panic 4 | **ok 481 / err 20 / panic 0** | +4（四件 panic→**ok**，非降级 err） |
| 内容保真 | 1.0000（477 件） | **1.0000（481 件）** | 恒（新增四件全满） |
| 结构保真 | 477/477 | **481/481** | +4 |
| 往返精确 | 383/477 | **386/481** | +3 |
| 幂等（二轮定点） | 477/477 | **481/481** | +4 |

四件逐一：`runprops_word365_props-{b,enum,u}-styles.docx` panic→**ok**（召回 1.0 +
往返精确 + 定点）；`strict_strict-sample-docx.docx` panic→**ok**（召回 1.0 + 定点；
往返非精确——宽度归一化的形态差异，登记见下）。POI 面（129 件）数值零漂移
（`results/poi-docx.json` diff 仅 import_ms 计时噪音，classes/六维全等）。

## 根因与修复（要点；全文见 office-io `docx_panic_guard` 模块文档 + gates.md #226 节）

- **裸 run 属性**（3 件）：`<w:u/>`/`<w:highlight/>` 零属性 start tag——OOXML 规范允许
 （ECMA-376 17.3.2.42：`w:u` 的 `w:val` 可省）而 docx-rs `run_property.rs:110/155`
  直接索引 `attributes[0]` → index OOB。修复：注入规范缺省 `w:val`
 （u=single / highlight=none / vertAlign=baseline / sz=0——结构不变只加属性）；
- **通用度量**（1 件）：`w:w="178.05pt"` 等 ST_MeasurementOrPercent 合法带单位形态
  ——docx-rs `width.rs:18` 只剥 `%` → `f64::from_str(...).expect` panic。修复：
  换算 twips（pt×20 / pc×240 / in×1440 / cm×567 / mm×57），垃圾值归零；
- 修复走既有 Warning 通道（裸元素=SkippedElement / 度量=MalformedXml——每修复
  部件一条，不静默）；无命中 `Cow::Borrowed` 零成本直通（指针相等测试钉定）；
  两模式（严格/兼容）恒等启用（裸元素是合法 OOXML，非「兼容模式才放宽」形态）。

## 剩余登记（如实，不静默）

1. **err 20 件恒在**（批 4 #E2 的 Word 365 属性形态族）：docx-rs 对 ST_OnOff /
   ST_Color / 枚举非规范形态不容忍（read_bool / 枚举 from_str 层）——**不在 #226
   范围**（任务 = 4 处 panic）；docx-rs 升版评估时以这 20 件 + 本批 4 件为验收锚点；
2. **上游跟踪项**：docx-rs 同类 `attributes[0]` 位点 20+ 处未实证不修
 （paragraph_property / table / numberings / level / style 族——完整清单见
   gates.md #226 节），未知形态由 mang-shell 打开入口 catch_unwind 兜底（GUI 不崩、
   未保存编辑不丢）；
3. **strict 件往返非精确**（1 件）：表格宽度归一化（pt→dxa）后导出面差异——内容
   零丢失（召回 1.0）+ 二轮定点，形态差异登记非阻塞。
