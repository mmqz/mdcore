# 06 — XLSX 读写链路与公式引擎 审计报告

- 审计对象：`mdcore-audit` main@96596d7（仓库只读）
- 范围：`crates/office-io`（io.rs / xlsx_edit.rs / write.rs / xlsx_chart.rs / xlsx_condfmt.rs / rebuild.rs + 8 组测试）、`crates/formula`（lib.rs / refs.rs + 3 组测试）、消费面 `mang-shell/src/editor/xlsx_doc.rs` + `tests/render_170_xlsx.rs`、`crates/mdcore-wasm/src/xlsx_grid.rs`
- 方法：全文件精读 + git 考古（5146f98）+ 研究仓底稿对照（`mang-research-archive/cleanroom-analysis/office-line/xlsx-r1/DEPS.md`）+ 只读 grep 交叉验证消费方
- 代码量基线：XLSX 面核心源码约 6,886 行（io 2407 + xlsx_edit 1806 + write 1199 + rebuild 796 + chart 387 + condfmt 291），formula 1,358 行；测试面另约 5,700 行（见 loc-stats.md:14）

---

## 0. 结论速览

| 级别 | 编号 | 发现 | 置信 |
|---|---|---|---|
| P0 | — | **无**。未发现数据损坏级缺陷或可利用安全绕过 | 高 |
| P1 | X-1 | 原位编辑路径（xlsx_edit）**无重算集成**：改数据格后依赖公式的 `<v>` 缓存陈旧、calcChain 保留旧账 | 高 |
| P1 | X-2 | **双编辑路径语义分叉 + 消费倒挂**：唯一产品编辑面（mdcore-wasm `export_with_edits`）走 rebuild（结构性丢样式/图表/条件格式/数据验证），而保真路径 `XlsxEditor` **零产品消费方**（仅测试） | 高 |
| P2 | X-3 | `formula/src/lib.rs:276-281` 文档「构建：灌入全部表并**立即首次求值**」与实现不符（`new()` 不调 `evaluate`；rebuild.rs:648-653 探针钉定为 #112 遗留文档缺陷，HEAD 未修；perf.rs:44 同病） | 高 |
| P2 | X-4 | rebuild 模块文档（rebuild.rs:8-11）声称「结构性丢失（样式原档/文档元数据/列宽行高**等**）由 `RebuildStats::skipped` 显式登记」，实现中 skipped 只记 表名清洗/合并跳过/超链接跳过/重算降级——**样式/图表/条件格式/数据验证丢失不进账本**，机器可读丢失清单缺失 | 高 |
| P2 | X-5 | 重算语义与 Excel 的三处可感知偏差（均已在档但集中影响重算可信度）：错误常量格排除后依赖公式按空格求值（`=E1+1`→1，Excel 为 #VALUE! 传播）；循环引用落 `#CIRC!` 文本（Excel 默认迭代关闭显示 0）；`[` 启发式外部引用识别假阳性 | 高 |
| P3 | X-6 | 布尔显示小写 `true/false`（Excel 显示 `TRUE/FALSE`）——render_170_xlsx.rs:604 钉定为口径，viewer 显示偏差 | 高 |
| P3 | X-7 | `set_cell` 可写合并覆盖格（ECMA 覆盖格不应有值、Excel UI 禁止）——模块「已知边界」未登记 | 中 |
| P3 | X-8 | 超链接无 URL scheme 白名单（`javascript:` 等透传；仅长度/NUL/tooltip 防线）；当前消费面不渲染可点击链接，风险低 | 高 |
| P3 | X-9 | 零编辑 save 的「文件级字节恒等」仅对本 crate 自产 zip 成立；rust_xlsxwriter/LO 产物仅部件级恒等（本地头规范化，xlsx_edit_roundtrip.rs:284-293 注记）；另 dimension/@spans 不更新、SST 孤儿串（均已诚实登记） | 高 |
| P3 | X-10 | GridScan 镜像 calamine 0.36.1 cells_reader 语义（io.rs:520-538）——calamine 升版若语义漂移，稀疏包围盒防线可能失配（复核纪律在档） | 中 |

**一句话判断**：XLSX 原位编辑（外科手术式）与公式引擎（ironcalc_base 包装）工程质量高、边界诚实，但两条编辑路径的「保真↔重算」能力互补而消费倒挂——继续投入的**价值成立且方向应是把 XlsxEditor 接上重算与产品消费面**，而非扩大自研面。

---

## 1. Q1：读改写（原位编辑）保真机制

### 1.1 机制实现（单元格级外科手术 + 部件透传）

`xlsx_edit.rs` 的 `XlsxEditor`（#177，xlsx 版 DocxEditor）实现为：

1. **开门**：`open()`（xlsx_edit.rs:860-919）走与读路径同一套防线且**恒严格**（不容 compat 降级）：门 0 输入字节上限（:861-866）→ zip 打开 → 门 1 `io::prescan` 声明值四检查（:870）→ 门 2+3 `io::verify_gates` 尺寸诚实 + 部件良构（:872）→ `xl/workbook.xml` 在场 fail-closed（:880-887）。
2. **表名→部件映射**：扫 `workbook.xml` `<sheet name/r:id>`（:422-440）+ `xl/_rels/workbook.xml.rels`（:444-465，TargetMode=External 不入部件面）→ `resolve_worksheet_part`（:470-500）做 `..`/`.` 段正规化、逃逸包根拒、**只接受 `xl/worksheets/*.xml`**（chartsheet/macroSheet/dialogSheet → 可列拒改）。
3. **惰性单趟扫描**：首次触及某表才 `scan_sheet`（:506-766）——quick-xml 0.41 事件流 + `buffer_position` 位置补偿（fwd_to_lt/fwd_to_gt/back_to_lt，:357-388，docx_edit 同款复制），建 `<row>`/`<c>` 字节区间索引（RowScan/CellScan 含 `@s`、`@t`、`<f>` 在场、`<v>`/`<is><t>` 文本、节点域 (start,end)）与 `sheetData` 几何（SheetDataGeom，:313-323）。自闭合 `<sheetData/>`/`<row/>`/shared formula 自闭合 `<f t="shared" si/>` 均有专门分支（:631-686）。
4. **待落账模型**：编辑存 `pending: Vec<BTreeMap<(u32,u32), CellEdit>>`（:855），坐标恒为 open 时原文坐标系；同格后写胜；`cell_info`（:936-1013）pending 优先、`t="s"` 惰性经 `scan_shared_strings`（:772-825，富文本多 `<r><t>` 拼接）解析。
5. **手术**：`save_to_vec`（:1066-1085）→ `apply_cell_edits`（:1178-1264）单趟拼接：`xml[..sheetData 起点]`（**原文前缀字节透传**）+ sheetData 内容区逐行——未编辑行整段字节透传（:1247）、编辑行「开标签原文 + emit_row_cells + 闭标签原文」（:1240-1242）、行内未编辑格节点域字节透传（:1165）、替换格仅保留原 `@s` 样式索引（emit_cell :252-268，文本恒写 `t="inlineStr"`+`xml:space="preserve"`）+ `xml[sheetData 终点..]`（**原文后缀透传**）。插行按行号升序落位（:1216-1259），自闭合行/表展开保留属性（:1231-1238）。
6. **重打包**：`repack_xlsx`（:1271-1307）遍历原 zip 全部条目，目标 sheet 部件重写（deflate 重压缩），**其余全部 `raw_copy_file` 逐字节透传**（:1293-1300）。零编辑 = 全部件 raw_copy（测试 `zero_edit_save_is_byte_identical` 文件级恒等，:1506-1514）。畸形防御：多表名映射同一部件且都有编辑 → save 拒（:1073-1079）。

### 1.2 读改写保真矩阵

| 部件/面 | 处置 | 保真度 | 证据 |
|---|---|---|---|
| styles.xml / theme / numFmt | 非目标部件 raw_copy | **字节恒等** | repack :1293-1300；测试 assert_other_parts_identical（roundtrip:130-143） |
| sharedStrings.xml | 恒不写（读面惰性解析） | 字节恒等；编辑 `t="s"` 格留孤儿串（无害垃圾，登记 :25-29） | :769-825, :988-994 |
| `<cols>`/`<mergeCells>`（sheetData 外元素） | 后缀区字节透传 | 字节恒等 | :1260-1262；roundtrip:252-274 |
| conditionalFormatting / dataValidations / hyperlinks（sheet XML 内、sheetData 后） | 同上后缀透传 | 字节恒等（值编辑后规则照常生效——条件格式是消费方显示层语义） | ECMA CT_Worksheet 序：均在 sheetData 之后 |
| 图表（chartN.xml/drawingN.xml/rels/[Content_Types] 增量） | 非目标部件透传 | 字节恒等；**引用范围不更新**（但 API 只有 set_cell 值写、无行/列移动操作 → 当前不可能错位）；numCache 陈旧仅为打开期显示（`c:f` 引用权威，xlsx_chart.rs:24-26 注记同口径） | — |
| 外部链接（externalLinks + 注释/vml/修订部件） | 透传 | 字节恒等 | — |
| 公式格 `<f>`+缓存 `<v>` | **拒改**（`<f>` 在场即 `FormulaCell` 错，含 shared 自闭合形） | 字节恒等（零接触） | :21-24, :1053-1059；roundtrip:243-251 |
| 依赖公式缓存值（编辑数据格后） | **不失效不重算** | **陈旧**（X-1） | 无任何失效机制；calcChain 部件也透传旧账 |
| `dimension`/`row/@spans` | 不更新 | 优化 hint，Excel/LO 自算（登记 :36-38） | :34-39 |
| docProps（创建者/时间） | 透传 | 字节恒等（vs rebuild 全丢） | — |
| 编辑格自身样式 `@s` | 保留原文 | 保真（插入新格无样式——None） | :1147, :1156-1160 |

**结论**：带图表+条件格式+数据验证+修订记录的文件读改写，**部件层面零损失**（这是「不碰」式保真，:13-15 明言）；语义层面唯一实质损失 = **依赖公式缓存陈旧**（X-1）+ 合并覆盖格可被写入（X-7）。与 docx_edit 同构的纪律，验证密度高（三层：引擎富夹具字节断言 / fin_lo.xlsx 真实 LO 产物 / soffice 开档门——xlsx_edit_roundtrip.rs:1-12）。

---

## 2. Q2：公式链路（ironcalc_base ↔ mdc-formula ↔ rebuild_recalc）

### 2.1 职责划分

- **ironcalc_base 0.9.1**（精确钉定，formula/Cargo.toml:30）：**解析**（`new_parser_english` AST）、**求值/重算**（`Model::evaluate` 全簿）、单元格模型（`Cell`/`FormulaValue`/`SpillValue`）、CIRC 循环检测、13 种错误码枚举、求值深度/栈限额（DoS 防护，lib.rs:447-455 透传 `set_max_evaluation_depth/stack`）。~470 函数（#110 必备 36 个全在）。
- **mdc-formula**（自研包装，1,358 行）：引擎类型零泄漏公开面（`SheetInput`/`CellContent`/`Value`/`CellRef`/`FormulaError`/`FormulaEngine`）；限额四件套（10M 格 / 1024 表 / 8192 公式字节 / 32767 文本字节，lib.rs:59-66）；typed 错误映射（:221-251）；`set_cell` 编辑（**不自动重算**，:327-341——调用方统一 `recalculate`）；`dependencies_at` 静态依赖提取（AST 走访 + 相对引用加回 context，:395-430、:562-680——**当前无消费方用它做脏标记**）；refs.rs 仅 **A1 单格坐标解析/格式化**（parse_a1/format_a1，refs.rs:20-87——范围解析在各模块自持：xlsx_condfmt.rs:118-144、xlsx_chart.rs:120-180、io.rs:484-518）。

### 2.2 缓存值（`<v>`）与重算语义

- 读面：`<v>` = 公式缓存值，经 calamine values 通道；公式文本独立通道 `formula_at`（io.rs:1140-1148）——**两通道明确区分**（禁止把缓存值当公式文本，io.rs:47-49）。
- **rebuild_recalc**（issue #113 任务 1，rebuild.rs:31-61）：`RebuildOptions::recalculate`（默认 true）→ `prepare_recalc`（:542-709）：值面（typed + 1904 纪元归一）+ 公式面（`=` 前缀补全 :747-753）+ **编辑账本即落位**（编辑在引擎构建时灌入）→ `FormulaEngine::new` + 显式 `recalculate()` 一次（:652-653，**全簿重算**，非依赖链增量——rebuild.rs:40-44 明言「脏标记优化在 rebuild 面无预算意义」，实测 200k 格 release 0.018s）→ 公式格新缓存表 `RecalcOutcome.table` → 值面重放时**原位替换** `CellValue::Formula.cached`（:313-317，公式文本与 DateTime numFmt 配对样式保持）。
- **改一格后依赖链失效方式**：无脏链——**每次全簿重算**（perf.rs:94-139 实测单格编辑+全簿重算 release <3s 预算；10k×20 构建重算 <1s 验收线）。rebuild_recalc.rs:119-147 钉定两级链级联正确。
- **循环引用**：引擎 CIRC 检测 → `#CIRC!` 码文本落缓存（rebuild.rs:670-674；rebuild_recalc.rs 测试「循环引用 → #CIRC! 缓存（引擎 CIRC 检测——不死循环）」）——与 Excel 默认（迭代关闭显示 0）不同，已登记（rebuild.rs:55-57）。
- **降级纪律**（保真优先，绝不阻断）：引擎构建失败（非法/重复表名、超预算 `LIMIT_TOTAL_CELLS`）→ 整簿源缓存直通 + skipped 登记（:265-281）；单格求值 Err → 保留源缓存 + 逐格登记（封顶 20 条，:677-692）；外部引用公式（`[` 启发式）→ 不重算、灌源缓存常量供依赖者级联（:586-600）；错误常量格排除灌入（引擎输入面无错误常量——依赖者按空格求值，:609-622 登记「能力边界如实」）。
- 非重算模式（`recalculate:false`）：纯缓存直通；无缓存 `<f>` 无 `<v>` 格在 typed 使用区外不写 = 历史静默丢失行为保持（:57-61）。

### 2.3 文档-实现不符（X-3）

`formula/src/lib.rs:276-281`：「构建：灌入全部表并**立即首次求值**（新公式即时得值——issue #110『新建公式算不出结果』缺口闭合点）」——但 `new()`（:282-325）只灌入（`update_cell_with_*`），**无 evaluate 调用**；构建后直接 `evaluate/cell_value` 会命中 `FormulaValue::Unevaluated` → `FormulaError::InvalidInput("公式未求值（先调用 recalculate）")`（:548-553）。rebuild.rs:648-653 注释已钉定此缺陷（「#112 遗留文档缺陷，本轮探针钉定（formula crate 本轮禁改——rebuild 侧显式 recalculate 补偿；PR 说明登记）」）——**HEAD 仍未修**，且 perf.rs:44「构建时已含一次首算」注释同病。所有自带测试都在 new 后手动 `recalculate()`（如 functions_vs_excel.rs:70、real_xlsx_lo.rs:104），故测试绿。这是面向未来消费者的行为陷阱。

---

## 3. Q3：calamine 读取限制的补偿层完整性（io.rs）

补偿/钉定状况逐项：

| calamine 已知坑 | 补偿/处置 | 证据 |
|---|---|---|
| 日期/时间序列化 | **不猜日期**：kind 判定 = 存储类型 + numFmt 证据（builtin 14-22/45/47 或自定义格式码 token）——纯 Float 无证据 → Number；t="d" → DateTimeIso；Duration 需 `[h]` 类 TimeDelta 证据 | io.rs:809-854 依据表；cell_types.rs 七类格钉定（:15-31） |
| 1904 纪元 | `has_1904_epoch` 构造期解析；写回 `epoch_to_1900`（+1462）单点换算 | io.rs:863-872 |
| 显式日期转换 | `as_iso_datetime/as_iso_duration`——消费方显式调用，无据不转换 | io.rs:894-935 |
| 共享字符串 | 引擎内部处理；xlsx_edit 自有 `scan_shared_strings`（富文本 `<r><t>` 拼接，测试 :1748-1750 钉定） | xlsx_edit.rs:772-825 |
| 内联字符串 | 两路径均支持（calamine + CellScan.is_text） | cell_types.rs B1 钉定 |
| 错误值 | `CellData::Error` 码文本保留（#DIV/0! 等），可诊断不丢 | io.rs:822；cell_types.rs I1 |
| 布尔小写 true | **不补偿**（钉定为口径）：`display()` = Rust `true/false`，Excel 显示 `TRUE/FALSE`——viewer 显示偏差 X-6 | io.rs:884；render_170_xlsx.rs:604 |
| 超链接实体怪癖 | v2 补偿：`unescape_xml_entities5`（rels/sheet 属性值未做 XML 反转义，`&` 以 `&amp;` 字面读出）——四字段统一还原；双解歧义登记（概率≈0） | io.rs:1359-1403 |
| 合并单元格语义 | **不展开**：值路径不触 merged_regions——左上单点显示、非左上空串；#202 实证钉定、#203 勘误（xlsx_doc.rs:31-32 注释已修正——5146f98 登记的「注释与实现不符」**已闭环**） | render_170_xlsx.rs:602-603 |
| >2^53 整数精度 | 钉定为 Excel 同限（存储面 f64） | io.rs:816；cell_types.rs J1 |
| `Data::Int` | 登记为 xlsx 读路径不可达（pivot 缓存独占），防御性承载 | io.rs:994-996 |
| `Range::from_sparse` 稠密化炸弹 | **门 3 同趟 GridScan 坐标扫描**（1588B 输入→~550GB 分配 abort 的实证闭项）：单部件稠密面积 ≤4M 格 + 提取期全簿累计（values+formulas 两网格） | io.rs:520-655、:693-701、:1340-1351 |
| 稀疏包围盒（A1+XFD1048576） | 同上面积限（两格 = 171 亿格拒） | io.rs:537-538、tests :2012-2081 |
| 空表名洪水 / 表数 | LIMIT_SHEET_COUNT=1000（提取循环前） | io.rs:118-123、:1325-1330 |
| 公式缓存 Float(0.0) 两义性 | 在档（cached=None 写回读 Float(0.0) 与真 0 不可区分） | write.rs:77-79 |
| 0x00 控制字符入值 | 值面容忍 + ControlChars 结构化警告恒登记（两模式同） | io.rs:351-359、:740-748 |
| 样式/关系引用/图表部件读取 | **不宣称**（C1 边界——calamine 无样式可观测面，结构性限制） | lib.rs:75-77 |

**评价**：补偿层完整性在同类自研包装中属上乘——每个怪癖都有「一手源码核验 + 探针实测 + 钉定测试 + 升版复核纪律」四件套（cell_types.rs:5-8 明言「升版 calamine 须重跑本钉定」）。**未补偿面**（布尔显示大小写、合并格展开）均为有意钉定且文档化，非疏漏。

---

## 4. Q4：写路径——rust_xlsxwriter 还是自研 XML 序列化？

- **写引擎 = rust_xlsxwriter 0.99**（write.rs:7, Cargo.toml:88-92）。Cargo.toml 中出现**两次**是 t19 的**目标条件分化**（非重复引入、非两套实现）：`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` 带 `constant_memory`（流式 tempfile 面，wasm32 无文件系统会 panic）；`[target.'cfg(target_arch = "wasm32")'.dependencies]` 仅 `wasm` 特性——单一逻辑依赖、lock 单条目。
- **rebuild 不是自研 XML 序列化**：`rebuild_book_with`（rebuild.rs:239-457）是读面 `Book` + 编辑账本 → 经**产品写 API `WorkbookWriter`（rust_xlsxwriter）全量重放**的编排层（796 行纯 orchestration）。
- 自研手写 XML 仅存在于 **xlsx_edit 的格级手术**（`emit_cell` 字符串拼接 + 转义，xlsx_edit.rs:237-268）——窄面（三种值形态 + 保留 `@s`），非通用序列化器。
- **两写路径并存 = 设计使然**（C2 新建 write.rs / C3 原位编辑 xlsx_edit.rs，lib.rs:39-48），非行为分叉事故。但存在**真实分叉面**（见 X-2）：文本编码（inlineStr vs 引擎 SST）、样式（保留 vs 全丢）、公式（拒改+缓存保真 vs 可写+重算）、图表/条件格式（透传保留 vs 重建丢失）。两路径共用同一容器门与同一引擎边界纪律，分叉点全部有文档登记。

---

## 5. Q5：xlsx_chart / xlsx_condfmt 实现深度

- **两者均为纯写面翻译层**（rust_xlsxwriter 包装，引擎类型零泄漏），**不是解析器**：
  - xlsx_chart.rs:51：「读取面（calamine）不读图表——图表是写面能力」；xlsx_condfmt.rs:32-33：「读取面（calamine 条件格式读）不在本轮——issue 现状注记『calamine 能读到，没有写』，本层补写面」。
  - 图表：4 类（柱/条/折线/饼+环形/散点）ChartSpec → 引擎 Chart，系列语义=表头惯例（范围引用非值快照），EMU→px 换算，校验全在引擎调用前（typed）。测试覆盖 XML 部件/关联链/`<c:f>` 范围引用断言 + LO 渲染墨迹对照（环境门 soffice/pdftoppm/PIL，tests/xlsx_chart.rs:358-439）。
  - 条件格式：5 变体（DataBar/ColorScale 2-3 色/IconSet/CellRule 单值比较/Duplicate），A1 范围解析 + 颜色校验；Between/多区间未暴露（登记）。
- **编辑后图表引用范围是否随单元格移动更新**：**不存在移动操作**——xlsx_edit API 仅 `set_cell`（值三形态），无行/列插入-位移/删除；因此当前**不可能**因编辑造成图表错位。图表部件字节透传 + `c:f` 引用权威（xlsx_chart.rs:24-26），值编辑后 Excel/LO 开档按引用重算重绘（numCache 仅打开期显示缓存）。**但 rebuild 路径直接丢图表**（重放面只有 值/公式/合并/超链接 四面——图表/条件格式/数据验证/图片无重放通道，X-2）。

---

## 6. Q6：差分验证现状（LibreOffice 对账的真实性）

- **real_xlsx_lo.rs 真调用 LibreOffice，但分两层**：
  1. **恒跑层**（无环境依赖）：`fin_lo.xlsx`（**soffice 预重算产物**，缓存值 = LibreOffice 真值 oracle）→ calamine 读公式+缓存 → mdc-formula 重算 → 逐公式格对照（相对容差 1e-9），≥45 格护栏 + 手算锚点（Python 独立复核的 6 个总值）+ 编辑重算闭环（改 C2 → 级联断言，real_xlsx_lo.rs:176-260）。
  2. **环境门层**：`soffice --version` 探测失败即 SKIP（:263-269）——本审计环境 soffice 25.2.3.2 在位、语料在位（fin_lo/fin_raw.xlsx 均存在），即**本机两层都会真跑**；无 soffice 的 CI 上只有第 1 层（oracle 为入库时的 LO 缓存，陈旧风险由第 2 层防，但第 2 层被跳过时该防线名存实亡）。
- **functions_vs_excel.rs 并不调用 Excel**：文件名「vs_excel」名不副实——期望值是**预先独立计算**（「全部期望值经独立计算预验证（Python 日期/算术复核，见 PR 描述）」，functions_vs_excel.rs:3-5），覆盖 36 必备函数 6 族 + 错误码 typed 面。它是 ironcalc 升版守门（Cargo.toml:23），非 Excel 对账。
- xlsx_edit_roundtrip.rs 第 3 层 soffice 开档门（--convert-to csv 成功 + 值面双证）。
- **结论：「Excel/WPS 无验证」成立**：全链路无 Excel/WPS 自动化；Excel 实机面被显式推迟到 PR 人工验证清单（xlsx_edit_roundtrip.rs:11-12「『用 Excel 打开确认格式未丢』的沙箱可自动化替身——Excel 实机面进 PR 验证清单」）。LibreOffice 对账真实存在（缓存 oracle 恒跑 + live 环境门），深度为单财务模型 ~50 公式 + 36 函数点测——广度有限（无大规模函数矩阵、无边界值 fuzz 对账）。

---

## 7. Q7：与成熟方案对标的边界

研究仓底稿链（office-line/）：ADR-O1 选型 + office-a Univer 深读 + office-b ONLYOFFICE 深读 + office-c HyperFormula 深读 + office-e Rust 生态选型 + xlsx-r1/DEPS.md（版本/许可证/闭包/适用边界四栏锁定，office-io Cargo.toml 注释逐条对应）。

当前栈 = **读（calamine）+ 写（rust_xlsxwriter）+ 算（ironcalc_base）+ 原位编辑（自研手术）+ 重建（自研编排）**，五层各一引擎、每层引擎类型零泄漏包装。对标判断：

- **继续采用既有引擎、边界清晰合理**：ONLYOFFICE/Univer 是 JS/TS 全栈应用（不可嵌入 Rust 核心）；HyperFormula 是 AGPL/商业 JS 引擎（许可证+栈不合）；IronCalc 生态已用 `ironcalc_base` 纯计算面——姊妹 crate `ironcalc`（含 xlsx IO）被明确**不引入**（formula/Cargo.toml:13-15「xlsx IO 在姊妹 crate ironcalc，不引入——mdcore 的 xlsx 读写已由 office-io 承载」），避免双 IO 栈。此决策与 DEPS.md 一手核验链一致。
- **自研 rebuild 边界**：796 行纯编排（无序列化、无公式语义），本质是把三引擎黏起来——合理；风险不在自研量而在**黏合点的类型转译损耗**（错误常量不可灌入 ironcalc、DateTimeIso 降级文本、合并非文本左上扁平化——每点有登记）。
- **自研手术编辑边界**：无 Rust 库提供 xlsx 原位外科手术（calamine 只读、rust_xlsxwriter 只新建、ironcalc 全模型重载会丢未知部件）——自研 1,806 行填补真实空档，且纪律（门恒严格/字节透传/拒改公式）与 docx_edit 同构。
- **缺口（对标成熟套件）**：无统一内部模型（Univer/IronCalc 的 model+engine+persistence 一体），导致「编辑→重算→写回」必须跨 calamine↔ironcalc↔rust_xlsxwriter 三套类型系往返，X-1/X-5 类语义缝隙皆源于此。若未来要做「交互式电子表格」而非「文档查看器」，应评估把核心数据面迁到 ironcalc Model 长驻会话（当前 rebuild 每次重建引擎的 one-shot 模式）。

---

## 8. Q8：容器预扫双门的绕过面

双门 = 门 1 声明值四检查（prescan_family，io.rs:222-266）+ 门 2 尺寸诚实（谎言头闭环，CountingReader io.rs:322-367 + verify_pass :662-751）+ 门 3 部件良构（validate_xml_structure :378-428）。逐项绕过面分析：

1. **zip bomb（压缩比）**：**无显式压缩比检查**，但攻击面有界——声明总量 ≤300MB（:94）+ 单条目 ≤300MB + 输入字节 ≤302MB（:109，四入口先于任何全量读）+ 门 2 全量解压一趟逐字节计数（实际 > 声明即 `PrescanRejected`，zip 8.6 读界无内建强制的策略层补面——DEPS.md lie86 探针定谳）+ GridScan 稠密面积 ≤4M 格。高压缩比但声明诚实的 300MB 件会完整解压两次（门 2 流式 64KB 缓冲 + calamine），CPU/内存均有界（峰值预算 ~400MB 级登记 io.rs:127-130）。**结论：不构成有效绕过**。
2. **entry 名路径穿越**：门 1 `enclosed_name().is_none()` → 拒（:246-251，测试 path_escape_rejected :1470-1485）；编辑面 `resolve_worksheet_part` 独立做 `..` 正规化 + 逃逸拒（xlsx_edit.rs:482-491，测试 :1445）；重打包按原条目名原样写（prescan 已验）。**闭环**。
3. **外部链接注入**：读面超链接四字段实体补偿后透传（**无 scheme 白名单**——`javascript:`/`file://` 等照收，X-8）；rebuild 重放仅检长度/NUL/tooltip 限（rust_xlsxwriter write_url 自身有 URL 类型校验兜底，未知类型 Engine 错）；公式外部引用 `[Book1]Sheet1!A1` 以缓存常量参与重算（不解析目标——无 SSRF/文件读取面）。当前消费面（mang-shell viewer 不渲染可点击链接，xlsx_doc.rs:33）风险低。
4. 其他：门 3 仅结构级校验（GeneralRef 透传不展开——十亿笑声免疫，Cargo.toml:40-43）；深度限 1000；元素序不校验（畸形序部件兼容面容忍，非安全面）；兼容模式只降级良构性、**安全上限两模式恒等**（io.rs:26-27、:312）。

---

## 9. P0-P3 分级明细（含证据链与建议）

### P1-X1：原位编辑无重算集成（语义保真缺口）
- **证据**：xlsx_edit.rs:21-24（公式格拒改=唯一公式面处置）；全文件无 `mdc_formula` 引用；`save_to_vec` 后依赖公式 `<v>` 原样；roundtrip:243-251 把「缓存值恒等（不重算）」当**正确行为**断言（在该路径口径下自洽）。
- **影响**：经 XlsxEditor 编辑的文件，其自读面（calamine）显示陈旧公式值；Excel/LO 开档自动重算掩盖问题（默认 calcMode auto），但不可移植消费者（管道/程序读）拿到错值。
- **建议**：复用 rebuild 的 `prepare_recalc` 思路做「手术后选择性 `<v>` 回写」（手术产物重打包前对依赖格替换 `<v>`——位置索引已在 CellScan 手中，工程量可控）；或最短期在 XlsxEditor 文档与宿主 UI 声明「编辑含公式簿时缓存可能陈旧，重算需另走 rebuild 路径」。

### P1-X2：双编辑路径能力互补与消费倒挂
- **证据**：grep 全仓 `rebuild_book|XlsxEditor::open`——产品消费方仅 `crates/mdcore-wasm/src/xlsx_grid.rs:37,232-233`（`export_with_edits` = rebuild）；mang-shell 无任何编辑接线（xlsx_doc.rs 只读 viewer，G6 定位）；XlsxEditor 仅测试消费。
- **影响**：当前唯一可达用户的「编辑」能力是**保真最差**的那条（丢样式/图表/条件格式/数据验证/图片/文档元数据——write.rs:50-55 登记的 FIDELITY v2 五档矩阵）；保真最好的路径（X-1 缺重算但部件零损失）无人用。产品若以「支持 XLSX 编辑」叙事，用户实际拿到静默降级件（skipped 账本 + C3 宣称纪律部分兜底，但 wasm 宿主是否真的向用户警示 skipped 未审计）。
- **建议**：短期把 wasm 编辑面切到/增加 XlsxEditor 通道（值编辑场景保真跃升，公式簿可拼接 rebuild 重算产物）；中期统一「编辑会话」抽象：值编辑走手术、公式编辑走重算、二者产物合并。

### P2-X3：formula 文档「构建即首算」与实现不符
- **证据**：lib.rs:276-281 vs `new()` :282-325（无 evaluate）；`formula_value` Unevaluated → 错误 :548-553；rebuild.rs:648-653 钉定注释（#112）；perf.rs:44 同病注释。
- **建议**：一行代码修复二选一——`new()` 尾部调 `self.model.evaluate()`（对齐文档）或改文档（对齐实现）。倾向后者+保留显式 recalculate 语义（当前所有调用方已显式 recalculate，改行为零收益）。

### P2-X4：rebuild skipped 账本不含结构性大丢失
- **证据**：rebuild.rs:8-11 文档 vs 实际 skipped 写入点仅 :291（表名）、:356-384（合并）、:406-426（超链接）、:273/:679-702（重算）；样式/列宽/图表/条件格式/数据验证丢失无登记——wasm `rebuild_stats_json`（xlsx_grid.rs:140-153）因此对宿主**不可见**。
- **建议**：rebuild 入口一次性检测源簿含 chart/conditionalFormatting/dataValidation/styles 部件 → skipped 注入汇总条目（如「structural loss: charts(N)/condfmt/styles — rebuild path」），让「非静默降级」承诺机器可读闭环。

### P2-X5：重算语义三偏差（登记在档，影响可信度）
- **证据**：错误常量排除（rebuild.rs:609-622 + rebuild_recalc.rs:12-13 实测 `=E1+1`→1）；#CIRC! 落盘（rebuild.rs:55-57）；`[` 启发式（rebuild.rs:563-567「保守：字符串字面量含 `[` 的普通公式会假阳性降级，方向为宁可不重算」）。
- **建议**：错误常量面可用「错误码文本 → 引擎可求值表达式」桥接（如灌 `=NA()` 类公式常量）或在 skipped 中把「受影响依赖格数」升级为显式警告；`[` 识别换成解析器级 defined-name/外部引用判定（dependencies_at 的 AST 已可判）。

### P3（低危/登记性）
- **X6** 布尔小写显示（io.rs:884；render_170:604 钉定）——若要贴 Excel 显示，display 层 upper-case 即可，注意会破坏「与 calamine to_string 逐字等值」钉定（需同步改 cell_types.rs:185-221）。
- **X7** 合并覆盖格可写（xlsx_edit set_cell 无 merges 感知；Excel UI 禁止编辑覆盖格）——建议 set_cell 前查 scan 期顺带收集的 mergeCells（需补一个轻量 `<mergeCells>` 扫描）或至少文档登记。
- **X8** URL scheme 白名单缺失（io.rs 读面、rebuild.rs:397-450 重放面）。
- **X9** 文件级字节恒等的适用域（自产 zip 才成立，roundtrip:284-293）；dimension/spans 不更新（xlsx_edit.rs:36-38）；SST 孤儿串（:25-29）。
- **X10** GridScan 镜像语义的版本耦合（io.rs:520-538「calamine 升版时此镜像须复核」；版本钉 0.36.1 + lock）。

### 正面发现（值得保持的资产）
1. **三层验证纪律**：每个怪癖 = 源码一手核验 + 探针实测 + 钉定测试 + 升版复核注释（cell_types.rs、io.rs、DEPS.md 全链一致）。
2. **诚实边界文化**：C1/C2/C3 宣称面、FIDELITY 丢失矩阵、「不支持面登记」「宁缺毋错」在全部六个模块文档中系统化存在。
3. **安全工程密度**：稀疏炸弹 550GB 分配实证 → GridScan 同趟闭合（零额外解压）；谎言头策略层闭环（zip 8.6 无内建）；流式纪律 typed 拦截反静默丢失（write.rs:534-539）；EMFILE 实测校准的表数限（write.rs:112-123）。
4. **#203 线索核实**：calamine 合并单元格语义「注释声称与实现不符」——**已闭环**：xlsx_doc.rs:31-32 注释现为正确口径（左上单点显示），render_170_xlsx.rs:602-603 以「源无 `<c>` 元素 → 空串」独立解析口径钉定。

---

## 10. 建议行动（优先级序）

1. 【P1-X2】wasm 编辑面接入 XlsxEditor 通道（或双通道并存按簿特征选择：含图表/样式簿走手术、需公式重算走 rebuild）——一次接线激活 1,806 行高保真资产。
2. 【P1-X1】手术路径的重算补面：编辑集 → 依赖闭包（可复用 `dependencies_at` 或 rebuild 的全簿方案）→ 手术产物 `<v>` 选择性回写。
3. 【P2-X3/X4】两处文档/账本诚实性修复（formula「构建即首算」；rebuild 结构性丢失入 skipped）——均为小改动大信任收益。
4. 【P2-X5】重算三偏差按消费场景决定是否收敛（若目标用户是财务口径，错误传播语义必须补）。
5. 【P3】布尔显示、合并覆盖格防线、URL 白名单——随下一轮 xlsx 迭代打包。
6. 差分验证扩展：functions_vs_excel 扩函数广度（~470 函数 vs 36 点测）；考虑 Excel 实机宏门（已有 LO 双层，Excel 面仍是承诺而非资产）。
