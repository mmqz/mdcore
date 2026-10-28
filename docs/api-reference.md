# API 参考（D-3 · issue #29 A12——给外部消费者，标书侧 bid-engine 会用）

> 口径：main @ 4a84bd5 实测面（签名以代码为准，本文是人读入口）。
> **维护纪律**：新增/变更公开 API 的 PR 须同步本文对应小节（一处一行）。
> 权威细节永远在 rustdoc：`cargo doc --workspace --no-deps --open`。

## 0. 消费路径总览

| 路径 | 适用 | 入口 |
|---|---|---|
| **原生 Rust**（标书侧 bid-engine 唯一许可路径） | 服务端/桌面进程 | `mdcore::Core`、`mdc_office_io::docx::*` |
| **wasm/JS**（web 编辑器） | 浏览器 | `web/pkg/mdcore_wasm.js`（`WasmCore` + 自由函数） |

三域纪律：`office-io` 不依赖 `mdcore`（并列域）；wasm 桥是消费方。

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
| `save_to_vec()` / `save_to_path()` | 打包产出（save 漏斗域校验 V1-V9） |
| `anonymize_docx_props(bytes)`（自由函数） | 暗标元数据后处理：core.xml → 匿名常量（unknown 作者 + epoch 零时）+ app/custom/people 清空 + 批注作者匿名化；幂等 |

## 3. mdcore 核心面（原生消费者）

| API | 语义 |
|---|---|
| `Core::from_markdown(md)` | 解析建核 |
| `core.tree` + `markdown::tree_to_markdown(&tree)` | 块树直访 + 序列化（幂等锚） |
| `Core::word_count()`（**PR #37 在飞**） | 七字段口径：cjk_chars/western_words/total_words[=和]/total_chars/paragraphs/headings/tables |
| `Core::chunk_document(&ChunkPolicy)`（**PR #36 在飞**） | 检索切块（父子块 + sha256 内容寻址 chunk_id + heading_path 三级 + Anchor 定位；确定性） |

## 4. wasm 导出函数清单（`mdcore_wasm.js`）

### 4.1 `WasmCore`（编辑器核实例）

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

### 4.2 导入/导出（自由函数）

| 函数 | 输入 → 输出 | 备注 |
|---|---|---|
| `import_docx(bytes)` | `Uint8Array` → JSON 串 `{"markdown", "stats", "warnings"}` | stats 键族见模块文档（headers/footers_imported、merged_ranges、fields_dropped…）；warnings 三档 severity（error/warn/info） |
| `import_xlsx(bytes)` | 同上 | 公式反引号形 + `[text](url)` GFM 链接形 + formulas/hyperlinks 明细清单（≤200） |
| `export_docx()` | `Uint8Array` | 默认排版导出 |
| `export_docx_styled(optionsJson)` | `Uint8Array` | options：`{"typography": {...}, "page_geometry": {...}}`（G2 面）+ `toc`/`anonymize`（**PR #38 在飞**）——缺省键 = 现状行为 |
| `export_xlsx()` | `Uint8Array` | BlockTree → 工作簿 |

错误面统一形状：`{ok: false, code, message}`（typed 错误码，wasm 桥逐码映射——见各模块文档错误码表）。

## 5. 常驻回归门（消费者视角的信任基线）

- `cargo test --workspace`：全量语义锁（850+ 用例）；
- 外部语料门（M-1，**分支在飞**）：`cargo test -p mdcore --test external_corpus`（commonmark 655 + markdown-it 652 例幂等/保留率门）+ `-p mdc-office-io`（POI 6 真实件含崩溃夹具）+ `-p mdcore-wasm`（pandoc 4 件导出容器门）——基线 `test-artifacts/external-corpus/BASELINE.json` 下降即红。
