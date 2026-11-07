# API 参考（D-3 · issue #29 A12——给外部消费者，标书侧 bid-engine 会用）

> 口径：main @ f4328eb 实测面（签名以代码为准，本文是人读入口）。
> **维护纪律**：新增/变更公开 API 的 PR 须同步本文对应小节（一处一行）。
> 权威细节永远在 rustdoc：`cargo doc --workspace --no-deps --open`。

## 0. 消费路径总览

| 路径 | 适用 | 入口 |
|---|---|---|
| **原生 Rust**（标书侧 bid-engine 唯一许可路径） | 服务端/桌面进程 | `mdcore::Core`、`mdc_office_io::docx::*`、`mdc_office_io::docx_edit::DocxEditor` |
| **wasm/JS**（web 编辑器） | 浏览器 | `web/pkg/mdcore_wasm.js`（`WasmCore` + 自由函数） |
| **Python RPC SDK**（`mang-rpc`） | AI 宿主 / 自动化 / 测试遥控编辑器 | `pip install mang-rpc`（源码 `sdk/python`——见 §7） |
| **MCP server**（`mang-mcp`） | Claude Desktop / ChatGPT 等 MCP 宿主 | `pipx install mang-mcp`（源码 `sdk/mcp`——见 §8） |

三域纪律：`office-io` 不依赖 `mdcore`（并列域）；wasm 桥与 mang-shell 桥都是消费方。mang-shell 是**独立 workspace**（`mang-shell/Cargo.toml`，主仓 CI 另有 `test-shell` job），其功能面见 §6。

## 1. office-io · DOCX 读模型

### 1.1 读入口

```rust
read_docx(path: &str) -> Result<DocxDocument, DocxError>                 // 严格模式（默认）
read_docx_with_options(path, &ReadOptions)                               // compat=true：畸形部件降级为结构化警告
read_docx_from_bytes(bytes: &[u8]) -> Result<DocxDocument, DocxError>    // 字节入口同过容器三门（不绕防线）
```

防线口径：输入字节上限 50MB（前置）+ zip/OPC 容器三门（声明预扫/尺寸诚实/部件良构）——敌意输入 typed reject，永不静默空数据。

### 1.2 `DocxDocument`（文档级）

| 字段 | 类型 | 语义 |
|---|---|---|
| `blocks` | `Vec<Block>` | 正文块序（有序）——`Paragraph(ParagraphData)` / `Table(TableData)` / `TextBox(TextBoxData)`（内层块与正文同构） |
| `headers` / `footers` | `Vec<HeaderFooterData>` | 实际挂接槽位（default/first/even 变体透传；引用悬空严格拒或兼容警告+跳过） |
| `images` | `Vec<ImageAsset>` | `word/media/*` 容器级清单（字节/内容类型/引用关系；块序内位置不入块模型） |
| `comments` | `Vec<CommentData>` | 批注（锚关联最终块序；孤儿批注 `anchor_block_idx = None`） |
| `footnotes` / `endnotes` | `Vec<FootnoteData>` | 脚注/尾注（分隔符条目已滤） |
| `warnings` | `Vec<CompatibilityWarning>` | 结构化警告（畸形 XML/断引用等——`kind`/`part`/`detail`） |

### 1.3 `ParagraphData`（段落）

| 字段 | 语义 |
|---|---|
| `style: Option<String>` | 样式 ID 透传（如 `Heading1`/`Normal`——语义判定见下一行，勿自猜） |
| `heading_level: Option<usize>` | **读面单一真源解析**的标题层级（1-9）：解析链 = 段落级 `w:outlineLvl` → pStyle `HeadingN` → 样式名 heading 形（含 WPS 中文「标题 N」）→ 样式级 outlineLvl |
| `runs: Vec<RunData>` | run 序（富文本） |
| `numbering: Option<ParagraphNumbering>` | 列表段承载（`num_id`/`ilvl` 透传 + `kind`[Bullet/Ordered] + `format`[原生 numFmt 透传，可诊断降级]）；`numId=0` OOXML 去编号惯例 → None |
| `indent: Option<ParagraphIndent>` | `w:ind` 全属性（left/right/special——Chars 变体登记在案） |
| `fields_dropped: usize` | 本段剥离的表现型域计数（PAGE/NUMPAGES 类；内容型域 REF/HYPERLINK/TOC 保留） |
| `source: BlockSource` | 见 §1.6 |

### 1.4 `RunData`（run）

| 字段 | 语义 |
|---|---|
| `text: String` | 文本 |
| `bold` / `italic` | `bool` |
| `hyperlink: Option<HyperlinkTarget>` | run 所在超链接（同一链接内各 run 共享目标） |
| `revision_inserted: bool` | 来自 `w:ins` 修订容器（当前生效文本，正常提取；标记面供修订通道识别） |

### 1.5 `TableData`（表格）

| 字段 | 语义 |
|---|---|
| `rows: Vec<Vec<String>>` | **栅格展开形**：gridSpan 覆盖列/vMerge 续格置空串，内容只居原点格（Word 显示语义）——消费方渲染零重建 |
| `merges: Vec<MergeRange>` | 合并矩形清单（0 基含端点对角）：纵/横/块合并各一条；单行未续的 vMerge restart **不是**合并（不发） |
| `source: BlockSource` | 表所在文档位置（表内段落继承表来源） |

### 1.6 `BlockSource` 语义（#22 P2-1）

```rust
pub enum BlockSource { Body, Header, Footer }
```

- **Body** = 正文内容（含脚注/尾注/批注——「文档内容 vs 页面装置」二分：批注是内容）；
- **Header/Footer** = 页眉/页脚槽位内块（消费方决定回放策略：mdcore-wasm 导入链把 Header 前导/ Footer 尾接进 markdown 单流，带 `<!-- mdcore-import: header:default -->` 标记行）；
- 构造面注意：字段无默认值（字面量构造须显式给 `source`——[BREAKING] 语义见 P2-1 PR 描述）。

## 2. office-io · DOCX 写面（`DocxWriter`——仅新建语义）

| 方法 | 语义 |
|---|---|
| `heading(level: u8, text)` | Heading1-9 样式 + outline 级 |
| `paragraph(runs: &[RunSpec])` / `paragraph_text(text)` | 富文本段 / 纯文本段 |
| `bookmark_paragraph(runs, name)` | 书签对段（`_Toc` 保留名拒绝；书签名 = 公开身份） |
| `table(rows: &[Vec<String>])` | 文本表格 |
| `list_item(runs, kind, ilvl)` / `list_item_lines(line_runs, kind, ilvl)` | 编号/子弹项（ilvl 0-8）；多行项 = 单段 + `w:br` 软换行 + 悬挂缩进 |
| `code_line(text)` | 等宽双通道行 |
| `quote_paragraph(runs, depth)` / `horizontal_rule()` | 引用（缩进+左边框，深度步进）/ 水平线 |
| `header(runs)` / `first_header` / `even_header` / `footer` / `first_footer` / `even_footer` | 六槽位 |
| `image(png_bytes, w_px, h_px)` | PNG-only 写面 |
| `RunSpec::` `text` / `styled` / `link` / `anchor_link` / `footnote` / `field_page` / `field_num_pages` / `field_toc` / `field_toc_heading_range(lo, hi)` | run 构造器族（TOC 域 V8 校验：1-9 且 lo≤hi） |
| `page_geometry(PageGeometry)` / `default_typography(DefaultTypography)` | G2 排版参数（twips 全八字段 / 字体三槽 + 半磅字号 + 行距） |
| `section_break(SectionBreakSpec)` | #257/#423 节属性面：闭合一节（geometry/titlePg/pgNumType/六槽位 + `section_type`[w:type 五值] + `columns`[w:cols 全字段——equalWidth/sep/`w:col` 子表]；后两项 docx-rs 0.4.22 无中段节通道，save 漏斗打包后注入） |
| `section_type(SectionKind)` / `columns(ColumnsSpec)` | #423 末节文档级单值（原生 w:type 直赋 / 全形态注入置换——None 语义精确往返；V10 域校验 num 1-63） |
| `save_to_vec()` / `save_to_path()` | 打包产出（save 漏斗域校验 V1-V9） |
| `anonymize_docx_props(bytes)`（自由函数） | 暗标元数据后处理：core.xml → 匿名常量（unknown 作者 + epoch 零时）+ app/custom/people 清空 + 批注作者匿名化；幂等 |

## 3. office-io · DOCX 编辑面（`DocxEditor`——字节级外科手术）

源：`crates/office-io/src/docx_edit.rs`。**四面貌**（段落 / 表格 / 域码段 / 批注脚注）#79 T1 起家、#115 表格、#140+#143 域、#141+#145 批注脚注、#156 sdt 对账。

### 3.1 定位（与读/写两面的边界）

模型级「读→重建→写回」会丢样式（xlsx FIDELITY v2 §4 定谳：组合语义 = 重建非修改）。编辑面走**字节级外科手术**：解压 → 定位 `word/document.xml` 段落（`w:p` 文档序扁平索引，body/表格内/文本框内统一计数）→ 只改目标文本节点 → 非目标部件 `raw_copy_file` 逐字节透传 → 重打包。保真矩阵见 `docs/docx-fidelity-matrix.md`。

### 3.2 生命周期与策略

```rust
DocxEditor::open(bytes: &[u8]) -> Result<Self, DocxError>                        // RevisionPolicy::Refuse（默认）
DocxEditor::open_with_policy(bytes, policy: RevisionPolicy) -> Result<Self, DocxError>
ed.save_to_vec() -> Result<Vec<u8>, DocxError>                                   // 一次性落账全部待编辑（段落/域/批注/脚注）；幂等
```

- `RevisionPolicy::Refuse`：段落含修订标记（`w:ins`/`w:del`/`moveFrom`/`moveTo`）→ `set_paragraph_text` typed 拒（默认——宁缺毋错）；
- `RevisionPolicy::Edit`：放行编辑——`w:ins`/`moveTo`（当前生效文本）参与首个文本位竞争，`w:del`/`moveFrom`（`w:delText`）原样保留。
- 容器防线与读路径**同门恒严格**（50MB 上限 + 三门 + UTF-8 fail-closed）——编辑面不做兼容降级。
- 编辑为**待落账态**：`paragraphs()` 的 span 是 open() 时原文坐标系，不随编辑漂移；同一目标后写胜。

### 3.3 面貌一：段落编辑

| API | 语义 |
|---|---|
| `paragraphs() -> &[ParagraphRef]` | 段落索引（`span` 字节域 + 旗标面） |
| `paragraph_text(idx) -> &str` | 当前文本（待落账值优先） |
| `set_paragraph_text(idx, text)` | 待落账设置（同段后写胜） |
| `revision_policy()` | 当前策略 |

`ParagraphRef` 旗标（消费方判定面）：

| 旗标 | 语义 |
|---|---|
| `field_code` | 域代码在场（`w:fldChar`/`w:instrText`/`w:fldSimple`）——整段替换拒改（域显示值走 §3.5） |
| `revision` | 修订标记在场——Refuse 策略拒改 |
| `fallback_copy` | mc:Fallback 复制区——拒改（编辑对应 mc:Choice 分支条目） |
| `in_table` / `in_textbox` | 表格内 / 文本框内段落（仍是扁平段落序成员） |
| `in_sdt` | `w:sdt` 容器内段落（#156：宽面读门结构性丢弃 sdt 内容——引擎可见、宽面不可见的**结构性盲区**，消费方据此对账/降级） |

`set_paragraph_text` 拒改面（typed `DocxError::InvalidInput`，fail-closed）：索引越界 / 域代码段 /（Refuse）修订段 / Fallback 复制区段 / 嵌套链互斥（外层段与文本框内层段同时待编辑）/ 新文本含 XML 1.0 非法控制字符或非字符。

### 3.4 面貌二：表格导航

| API | 语义 |
|---|---|
| `table_cells() -> Vec<Vec<Vec<usize>>>` | 表 → 行 → 栅格列 → **单元格段落号**（权威坐标；gridSpan 覆盖列/vMerge 续格重复指向原点格） |
| `table_count()` | 表数 |
| `table_first_paragraph(t)` / `table_grid_width(t)` / `table_shape(t)` | 文档序首段号 / `tblGrid` 声明列数 / `(rows, max_cols)`（越界 = `None`） |

编辑经 `set_paragraph_text`（单元格 = 普通段落，`in_table` 为真）；嵌套表不进任何栅格（无稳定平面坐标——猜坐标即静默改错格，设计 §2 三类静默错）。

### 3.5 面貌三：域码段编辑

| API | 语义 |
|---|---|
| `fields() -> Vec<FieldInfo>` | 域清单（`(para, field_idx)` 双键坐标系——与 `paragraphs()` 同源 + 段内序） |
| `set_field_value(para, field_idx, value)` | 只改结果区显示值——OOXML 域五段结构（begin→instrText→separate→`w:t`→end）零接触，域在 Word/LibreOffice 打开仍是**活域** |

`FieldKind`（指令首词切分，与读面 FieldMachine 同口径）：`Page` / `NumPages` / `Date` / `Time` / `Toc` / `Ref(String)` / `Custom(String)`（未映射指令装原始串——不发明映射）。

`FieldInfo` 字段：`para`/`field_idx`（= set 入参）/`kind`/`instr`（完整指令含开关）/`display_value`（separate 与 end 间 `w:t` 拼接）/`same_para`/`editable`（= same_para && 非嵌套 && 有结果区）。

`set_field_value` 拒改面：坐标不存在 / 跨段域（TOC 型）/ 嵌套域 / 无结果区（无 `separate` 的退化域或 separate 后无 `w:t`）/ 非法字符。结果区规则：首个 `w:t` 换新值，其余结果区 `w:t` 移除（run 壳保留）。域编辑不反映在 `paragraph_text`（显示值读面 = `fields()` 的 `display_value`——save 后 reopen 即新值）。

### 3.6 面貌四：批注/脚注编辑（部件级手术）

| API | 语义 |
|---|---|
| `footnotes() -> Vec<FootnoteInfo>` | `id`/`text`/`anchor_para`（正文引用段；孤儿 = None）/`paras`；分隔符条目不入列 |
| `comments() -> Vec<CommentInfo>` | `id`/`author`/`date`/`text`/`anchor_para`/`paras` |
| `set_footnote_text(id, text)` / `set_comment_text(id, text)` | 首个 `w:t` 换新值、余 `w:t` 移除（run/段落壳保留——最少侵入）；`w:footnoteRef`/`w:annotationRef` 零接触；批注作者/日期不动 |

拒改面：id 不存在 / 部件缺席（无 `word/footnotes.xml`·`comments.xml`）/ 条目无文本节点（不发明内容区）/ 非法字符。部件缺席时列面 = 空集（保真透传）。

### 3.7 文本约定（两面同构）

读：`w:t` 文档序拼接；`w:tab`→`\t`、`w:br`→`\n`、`w:cr`→`\r`；`w:delText` 不计。写：反向映射（CRLF 合一）；重写 `w:t` 恒带 `xml:space="preserve"`；`&`/`<`/`>` 实体转义。

## 4. mdcore 核心面（原生消费者）

| API | 语义 |
|---|---|
| `Core::from_markdown(md)` | 解析建核 |
| `core.tree` + `markdown::tree_to_markdown(&tree)` | 块树直访 + 序列化（幂等锚） |
| `Core::word_count()` | 七字段口径：cjk_chars/western_words/total_words[=和]/total_chars/paragraphs/headings/tables |
| `Core::chunk_document(&ChunkPolicy)` | 检索切块（父子块 + sha256 内容寻址 chunk_id + heading_path 三级 + Anchor 定位；确定性）——`mdcore::chunk` 模块 |

## 5. wasm 导出函数清单（`mdcore_wasm.js`）

### 5.1 `WasmCore`（编辑器核实例）

| 方法 | 返回 | 语义 |
|---|---|---|
| `new(markdown?)` | `WasmCore` | 建核（可空文档） |
| `to_markdown()` | `string` | 序列化 |
| `apply_ops(opsJson)` | `string` | op 流应用 |
| `replace_text_at_anchor(...)` | `string` | 锚定替换 |
| `undo()` / `redo()` | `Option<string>` | 历史 |
| `take_events()` | `string` | 事件流取走（重建清零） |
| `blocks_json()` / `blocks_json_range(from, count)` / `blocks_count()` | `string`/`usize` | 块面投影 |
| `sync_from_markdown(md)` / `extend_from_markdown(chunk)` | `Result` | 整体重建 / 渐进追加 |
| `document_id()` / `set_document_id(id)` | `Option<string>` | 文档身份（保存/复原） |
| `snapshot()` / `restore(json)` / `replay(jsonl)` / `history_json()` / `set_history_json(json)` | — | 快照/回放/历史面 |
| `core_mem_stats()` / `debug_tree()` / `revision()` / `set_clock_ms(ms)` | — | 诊断面 |

### 5.2 导入/导出（自由函数）

| 函数 | 输入 → 输出 | 备注 |
|---|---|---|
| `import_docx(bytes)` | `Uint8Array` → JSON 串 `{"markdown", "stats", "warnings"}` | stats 键族见模块文档（headers/footers_imported、merged_ranges、fields_dropped…）；warnings 三档 severity（error/warn/info） |
| `import_xlsx(bytes)` | 同上 | 公式反引号形 + `[text](url)` GFM 链接形 + formulas/hyperlinks 明细清单（≤200） |
| `export_docx()` | `Uint8Array` | 默认排版导出 |
| `export_docx_styled(optionsJson)` | `Uint8Array` | options：`{"typography": {...}, "page_geometry": {...}}`（G2 面）+ `toc`（TOC 域发射）+ `anonymize`（暗标后处理）——缺省键 = 现状行为 |
| `export_xlsx()` | `Uint8Array` | BlockTree → 工作簿 |

错误面统一形状：`{ok: false, code, message}`（typed 错误码，wasm 桥逐码映射——见各模块文档错误码表）。

## 6. 编辑器层（mang-shell）功能面

mang-shell = 桌面编辑器（独立 workspace）。外部触达主通道 = **RPC**（§6.6 / `mang-shell/docs/rpc-protocol.md`）；内部模块面供二开参考（`mang-shell/src/editor/`）。

### 6.1 命令系统（`editor/commands.rs`）

注册表模式（`CommandSpec` 静态表：id/标题/默认键位/分组），命令面板 Ctrl+Shift+P 模糊搜索执行，与键位双入口同一批方法。RPC 侧 `list_commands()` 先看再 `run_command(name)`——**别硬编码命令名**。

### 6.2 折叠（#132 任务3）

| 命令 | 键位 | 语义 |
|---|---|---|
| `toggle_fold` | Alt+F | 光标处折叠态切换 |
| `fold_current` / `unfold_current` | Ctrl+Shift+[ / Ctrl+Shift+] | 折叠/展开光标所在块 |
| `fold_all` / `unfold_all` | Ctrl+K Ctrl+0 / Ctrl+K Ctrl+J | 全部折叠/展开 |

折叠范围判定（`foldable_span_at`，先到先得）：**围栏代码块**（整块含开闭行，闭行可见）→ **ATX 标题块**（到下一个同级或更高级标题前，含所有下级小节；文档末标题折到文末）；docx/xlsx 文档不折叠（无语义块面）；区间不足两行 = 不可折叠。折叠跨会话**持久化**（`persist_folds`）。坐标系：`editor/display_map.rs` 的 `DisplayMap`——buffer 点 ↔ display 点互转（`to_display`/`to_buffer`/`display_row_count`），折叠行 + 软换行共用该投影。

### 6.3 括号匹配（#132 任务1 + #157 修复）

`editor/brackets.rs`（纯逻辑、零 gpui 依赖）：

| API | 语义 |
|---|---|
| `find_match(text, cursor)` / `find_match_lang(...)` | 光标点 → `BracketMatch`：`Matched{open, close}` / `Unmatched{pos}`（渲染标红）/ `None` |
| `highlight_cols(text, cursor, line)` / `highlight_cols_lang(...)` | 指定行需要高亮的列集 |
| `infer_code_lang(text)` | 代码语言推断（无围栏标注时） |

VS Code 口径：三种成对括号（圆/方/花）+ 单侧裸括号判定；**字符串/注释内的括号不算**（复用 G5 语法扫描器 token 归属；围栏语言态决定什么算字符串/注释——rust/python/js 已识别，未标注围栏整行 BlockCode 全括号参与）；栈深配对（跨行全文档）；光标前字符优先。#157 修复：whole_lang 围栏推进语义（字符串内的括号曾被当成真括号）。

### 6.4 查找/替换与跳转（#132 任务2）

`editor/buffer_find.rs`（独立实现）+ `find_bar.rs`：查找历史 + 正则查找；命令 `open_find`（Ctrl+F）/ `open_replace`（Ctrl+H）/ `goto_line`（Ctrl+G——只读态可用）。

### 6.5 DOCX 可编辑桥（#115 + #156）

`editor/docx_edit_bridge.rs`：Buffer ↔ `DocxEditor`（§3）可逆双向映射——打开 = 宽面行模型 + 窄面段落索引 + 段落序对账（对不上降级只读）；编辑 = Buffer 钩子（`EditHook`/`UndoHook`）幂等回写；保存 = docx 字节写盘。#156：sdt 盲区（§3.3 `in_sdt`）跳过投影，sdt 包表格 → 整篇降级只读（栅格错位不可写）。设计详情：`mang-shell/docs/docx-edit.md`；不可编辑降级面分析：`mang-shell/docs/docx-degrade-analysis.md`。

### 6.6 RPC 协议

本机 unix domain socket + JSON Lines，27 条命令（编辑文本/管文件/取状态）+ `doc_changed` 事件流；工作区沙箱（无工作区文件系统命令一律拒）。不加密不认证——**不要暴露到可路由的地方**。全协议：`mang-shell/docs/rpc-protocol.md`。

## 7. Python RPC SDK（`mang-rpc`）

源码 `sdk/python`（包 `mang_rpc`）；`pip install mang-rpc`。**运行时零三方依赖**（纯标准库）；类型注解完整（mypy strict，PEP 561 `py.typed`）；与服务端 `mang-shell/src/rpc.rs` 逐字段对齐（漂移由单测钉住）。

| 面 | 内容 |
|---|---|
| 双客户端 | `Client`（同步）/ `AsyncClient`——`with Client.connect() as c:` / `async with await AsyncClient.connect() as ac:` |
| 27 命令 | 方法名 = 协议命令名，全部返回类型化 frozen dataclass（见 §6.6 协议表） |
| 事件流 | `c.subscribe()`——`event.source == "user"` 看用户编辑 / `"rpc"` 忽略回显；revision 跳号 = 漏事件，`get_state()` 重同步 |
| 批量替换 | `batch_replace(files, pattern, replacement, is_regex=False)`——双通道：已打开走 buffer（一条 undo 全撤），未打开直改盘 |
| 逃生口 | `c.call("future_command", **params)`——服务端新增命令 SDK 未跟上时可用 |
| 端点解析 | 显式路径 → `$MANG_RPC_SOCK` → `$XDG_RUNTIME_DIR/mang-shell.sock` → `/tmp/mang-shell-$UID.sock` |
| 错误面 | `RpcError`（业务拒绝——**勿字符串匹配**）/ `TransportError` / `ConnectionLost`（at-least-once：恢复后先 `get_state()`）/ `RequestTimeout` / `ProtocolViolation` |

细节：`sdk/python/README.md`。

## 8. MCP server（`mang-mcp`）

源码 `sdk/mcp`（包 `mang_mcp`）；`pipx install mang-mcp`。让 Claude Desktop / ChatGPT 等 MCP 宿主直接操控编辑器：`MCP 宿主 ⇄ stdio JSON-RPC ⇄ mang-mcp ⇄ unix socket ⇄ mang-shell`（转发层复用 mang_rpc，自动重连）。

11 工具：`open_file` / `close_file` / `get_state`（标签+光标+工作区全文）/ `insert_text` / `replace_text`（全部替换，buffer 通道可 undo）/ `batch_replace` / `get_outline` / `list_commands` / `run_command` / `read_file` / `write_file`。

自定义端点：env `MANG_RPC_SOCK`。配置示例与使用手册：`sdk/mcp/README.md`。

## 9. 常驻回归门（消费者视角的信任基线）

- `cargo test --workspace`（主仓）：全量语义锁；
- `cargo test --manifest-path mang-shell/Cargo.toml`（编辑器独立 workspace，CI `test-shell` job）：编辑器层 + 括号语料门（`tests/brackets_corpus.rs`）+ docx 桥对账；
- 外部语料门（M-1，已合并）：`cargo test -p mdcore --test external_corpus`（commonmark 655 + markdown-it 652 例幂等/保留率门）+ `-p mdc-office-io`（POI 6 真实件含崩溃夹具）+ `-p mdcore-wasm`（pandoc 4 件导出容器门）——基线 `test-artifacts/external-corpus/BASELINE.json` 下降即红；
- 标书链端到端（#156）：`mang-shell/tests/docx_bid_e2e.rs`（黄金例夹具 `tests/fixtures/bid-e2e/07_export.docx` 进仓——缺失即 FAIL 不许 SKIP；ai-bid-research 全链产物：六表 + 七格式件 + sdt 包裹 TOC）+ `mang-shell --smoke-docx-bid OUT` 启动钩子（四面貌编辑 + 三段验证：非目标部件字节恒等 / 6 表 7 件结构不少 / LibreOffice 开档编辑可见）。
