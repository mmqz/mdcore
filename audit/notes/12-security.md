# 12 · 安全边界与攻击面审计（mdcore-audit main@96596d7，2026-11-04）

> 只读审计（零仓库写入/零 git 操作/零 cargo 运行）。威胁模型：**桌面文档编辑器 × 敌意文档**——用户打开敌意构造的 .md/.docx/.xlsx/.pdf/.canvas、粘贴敌意内容、AI 返回敌意内容、协作对端发送敌意 op、SDK/RPC 远程调用。
> 证据口径：`文件:行号` 均为本轮 HEAD 独立实测（与 01/05/06 号笔记交叉但不依赖——其安全章节结论已逐项复核，一致项标注「复认」，新发现标注「本轮新增」）。
> 判定框架：**A** 已防护+有负向测试 / **B** 已防护无测试 / **C** 防护存在但未接线（死代码或旁路）/ **D** 无防护。

## 0. 一分钟结论

- **引擎内嵌防线真实且测试密度高**：Office 容器三门（zip 炸弹/路径穿越/XXE/谎言头）、collab 敌意输入面、PDF 限额族、canvas 52MB 门——全部有 typed 拒绝 + 负向测试（A 级）。
- **产品边界防护系统性缺位**：`mdc-office` 消毒六面（905 LOC + 908 测试）**零产品接线**（复认 01-§6.5，本轮再证 + 补两条活绕过路径）；RPC 沙箱只盖 8 条 FS 命令，`open_file`/`save_as` 裸奔；md 深嵌套递归无任何深度门（html.rs 有 256 门、markdown.rs 没有——同族威胁双标）。
- **最危险的五条绕过**：① TSV 剪贴板公式注入（web 网格复制→Excel DDE）；② docx 超链接 `javascript:` 无策略透传成 GFM markdown；③ RPC `save_as` 任意路径写 + 无认证 socket；④ md 深嵌套 → `blocks_json()` 递归栈溢出（web 打开即 trap）；⑤ docx-rs 4 处 panic 直达 native 崩溃（无 catch_unwind）。
- **正面资产**：全仓**零网络拉取面**（无 reqwest/ureq/fetch 资源加载）、零敌意图片/字体解码面、外置进程调用全走 args 数组不经 shell。

---

## 1. 入口 × 防护覆盖矩阵

| # | 入口 | 威胁 | 防护层（文件:行号） | 判定 | 负向测试 |
|---|---|---|---|---|---|
| 1a | .md 打开（web 线） | 超大输入内存耗尽 | mdcore-wasm lib.rs:54-60 `WasmCore::new` → `Core::from_markdown` **无字节上限** | **D** | 无 |
| 1b | .md 打开（GPUI 线） | 同上 | document.rs:99 `read_to_string` **无门**（docx/pdf/xlsx/canvas 四族全有门，唯 md/txt 无） | **D** | 无 |
| 1c | .md 深层嵌套 | 递归栈溢出 | **无**。markdown.rs:276-295 blockquote 深度=行首 `>` 计数；:297-322 list depth=indent/2+1，均无上限 → 树深无界 → 递归消费者：markdown.rs:40-99 `render`、html.rs:1560-1648 `render_blocks`、debug.rs:25-37 `collect`（= wasm `blocks_json`，web 启动即调，main.ts:1579）、word_count.rs:213 `walk`、snapshot.rs:190-238 `to_snap/from_snap`、fingerprint.rs:135-136 | **D** | 无（r14_gates 有 1MB progressive 测试，无深度测试） |
| 1d | .md 假围栏 | 渐进打开状态污染 | markdown.rs:20-28 `is_complete_chunk` + `Reject::IncompleteChunk` 零污染 | **A** | document_progressive.rs |
| 1e | HTML 导入（mdcore） | 深嵌套 | html.rs:546 `MAX_DEPTH=256`，:736 超深→RawBlock 降级 | **A** | r13_html_roundtrip.rs:390 nasty 用例 |
| 1f | HTML 导出（mdcore） | XSS（script/onerror/javascript:/data:） | html.rs:1546-1555 `inline_html` 全文本转义 `& < > " '`；:436-449 `escape_html`；:1600-1607 code lang 转义；**无任何 href/属性注入位**（导出仅 5 种 mark + class/ol start）；`<a>` 永不发射 | 构造性安全（**B**） | r13:361 `&lt;script&gt;` 往返 |
| 1g | HTML 导出消费面 | — | `tree_to_html` **零产品消费者**（rg 全仓：仅 r13 测试；WasmCore 23+ 导出面无 to_html；web 渲染 = CM6 纯文本 + pdf.js，无 markdown→HTML 渲染器） | 死代码 | — |
| 2a | DOCX zip 炸弹 | 解压炸弹 | 门0 输入字节 50MB（docx.rs:1698/1719 + mang-shell docx_doc.rs:164 前置 metadata）→ 门1 声明值预扫（io.rs:222-266：条目 10k/单条目/总量/`enclosed_name` :246-251）→ 门2 谎言头（CountingReader io.rs:322-366 + verify_pass :655-781，实际>声明即 `PrescanRejected`） | **A** | io.rs:1597 lie_header、:2203/:2238 input_size |
| 2b | zip 压缩比 | 高压缩比炸弹 | **无显式压缩比检查**；有效闭环 = 声明总量 50MB + 谎言头全量解压计数 + 输入字节 52MB 上限（复认 06-§8.1，本轮核验常量 io.rs:94-111 与检查点链成立） | **A**（等效） | 同上 |
| 2c | entry 名路径穿越 | `../`、绝对路径、反斜杠 | 门1 `enclosed_name().is_none()` 即拒（io.rs:246-251）+ 自有 `normalize_part_path` 词法越根检出（docx.rs:891-908）+ 编辑面 `resolve_worksheet_part` 独立正规化（xlsx_edit.rs:470-500）；且**永不解包落盘**（无放大面） | **A** | io.rs:1470 path_escape_rejected；xlsx_edit.rs:1445 |
| 2d | XXE/DTD/实体 | 十亿笑声/外部实体 | quick-xml 0.41 `check_end_names=true`、GeneralRef 透传不展开（io.rs:371-374 + Cargo.toml:39-43）；docx-rs 同为 quick-xml 系；POI `ExternalEntityInText.docx` 语料实证拒（复认 05-§6.2） | **A** | 语料门（health_check） |
| 2e | XML 嵌套炸弹 | 深嵌套 | 门3 深度 1000（io.rs:396-398 `LIMIT_XML_DEPTH`，validate_xml_structure :378-428） | **A** | io.rs:1761 gate3_depth_limit |
| 2f | docx 外部超链接 | `javascript:`/`data:` scheme | **防护未接线**：url_policy 白名单（office/src/sanitize/url_policy.rs:80-137，http/https/mailto/internal + NUL/UNC/非法字符 + 15 载荷负向测试）存在但全仓 `mdc_office::` 生产调用=0；读面透传（docx.rs:1895-1956）→ wasm docx_import.rs:204-243 `link_destination`/`render_inline` 直转 `[text](javascript:alert(1))` GFM markdown | **C** | 防护有测试、产品路径无 |
| 2g | docx-rs panic | 崩溃 | **无**。4 处 panic（word365 run 属性族/strict OOXML，BASELINE.json 实证）过三门直达引擎；mang-shell docx_doc.rs:171/208 仅 `map_err` 无 catch_unwind（rg 实证全 office-io/mdcore-wasm src 零 catch_unwind） | **D**（native 崩）/ trap（wasm） | 「锁基线」≠修复（复认 05-P0） |
| 3a | XLSX 公式注入（CSV） | `=cmd\|`/`=HYPERLINK` DDE | guard_csv_cell 存在（office/src/sanitize/csv_formula.rs:74-89，`=`/`+`/`-`/`@` 三模式+测试）**零接线**；且仓内**无 CSV 文件导出面**（rg：csv 仅测试脚本 soffice 产物） | **C**（无对应出口） | 防护有、出口不存在 |
| 3b | XLSX 公式注入（**TSV 剪贴板**） | 同上 | **无防护**：web xlsx_grid.ts:945-966 `onCopy` 把敌意 cell 显示文本原样 `setData('text/plain', TSV)`——复制→粘贴 Excel 即公式重解释（csv_formula.rs:1-13 文档明言此为设计威胁面） | **D**（活路径，**本轮新增**） | 无 |
| 3c | XLSX 超链接 scheme | javascript: 重放 | rebuild.rs:398-445 仅长度/NUL/tooltip 限，无 scheme 白名单（url_policy 未接线，复认 06-X8）；当前消费面不渲染可点链接（mang-shell xlsx_doc.rs:33，rg 实证 viewer 无 clipboard） | **C**（低危悬空） | 无 |
| 3d | calamine panic/稀疏炸弹 | 分配 abort | GridScan 同趟稠密面积 ≤4M 格（io.rs:520-655，1588B→550GB 实证闭项）+ 门恒严格（xlsx_edit.rs:860-880） | **A** | io.rs:2013 sparse_grid_bomb 双模式 |
| 3e | XLSX 编辑入口 | 容器绕过 | wasm 唯一编辑面 export_with_edits → rebuild（xlsx_grid.rs:37/232-233）——字节入口经 read_book_from_bytes 同门（xlsx_grid.rs:323）；XlsxEditor（高保真路径）零产品消费（复认 06-X2） | **A**（门）/ **C**（保真倒挂） | 容器门有 |
| 4a | PDF 输入大小 | 资源耗尽 | mang-shell：INPUT_CAP_BYTES=100MB 读盘前置（pdf_doc.rs:62/148-155）；mdc-pdf 引擎层：open_file_max_bytes=1GiB + open_budget 3s（security.rs:203-207/286-307，pdf-wasm/src/lib.rs:102-105 接线） | **A**（两层，数值不一致=登记项） | pdf_doc.rs:592 断言 100MB |
| 4b | lopdf 解析深度 | 递归对象/深嵌套 | **无挂点**：Limits.object_depth_max=128 只作用于 security_scan 对象图游走，lopdf 解析器内部无深度钩（security.rs:179-184 crate 自认「[v0 边界登记]」） | **D**（自认）/ 需外部验证 | 无 |
| 4c | xref 链完整性 | 链断裂静默丢失 | pdfi_xref_chain 4 锁：Guard A 写入口拒重建档 + Guard B 读面拒零页树（writer_lopdf from_bytes_with_limits / annotate read_annotations）；⚠ 本质是**写路径完整性**防 CRLF 污染产物，非敌意深度防御 | **A**（写完整性） | tests/pdfi_xref_chain.rs 4 锁 |
| 4d | JS/Launch/URI 动作 | 执行 | 八族检测（security.rs:65-97 含 OpenAction/AA/Launch/URI/JavaScript/NamesJavaScript/RichMedia/EmbeddedFiles）→ OpenAssessment **只报告不阻断**（S2.4「只读不阻断打开」security.rs:121-122）；实际执行防线=**架构性**（不链 JS 引擎、pdfium 渲染/抽文本不执行动作） | **B**（报告型+架构性免疫） | pdfe_security.rs |
| 4e | PDF 安全告警消费 | 用户不可见 | web 线：pdf_view.ts:1965-2012 展示 encrypted/warnings；**mang-shell 线不跑 security_scan**（pdf_doc.rs rg 零调用，直连 PdfiumEngine） | **C**（原生线告警缺失，**本轮新增**） | web 线有 |
| 4f | 嵌入文件提取 | 落盘执行 | 无提取落盘面（八族 EmbeddedFiles 仅检测告警；organize.rs extract 是页范围抽取） | 架构性免疫 | scan 测试 |
| 4g | 字体/图像解码资源耗尽 | 解码炸弹 | L2 钳制：raster 8192 边/64M px + L0 decode_output 1GiB 先于分配（security.rs:187-193/273-280）+ 渲染看门狗 20s ❸弃置（worker.rs）；字体解码在 pdfium 内部（C++） | **A** | pdff3_render_ext |
| 5a | WASM JSON 输入 | 深 JSON | serde_json 默认 128 层递归限（apply_ops lib.rs:64-78、canvas parse_cmd_core） | **A**（库默认） | 无显式测试（依赖库行为） |
| 5b | WASM 坐标/越界 | 越界 panic | blocks_json_range 越界截断（lib.rs:161-163）；replace_text_at_anchor → core typed Reject（history/anchor 校验） | **A** | mdcore tests |
| 5c | WASM 非法 UTF-8 | glue panic | wasm-bindgen &str 边：JS UTF-16→UTF-8 经 TextEncoder，孤立代理对替换 U+FFFD（不 panic） | **A**（库行为） | 无显式 |
| 5d | WASM 超长字段 | 内存耗尽 | **无 cap**（new/apply_ops/sync_from_markdown 全无长度门）；内存策略=无显式 Memory maximum，靠引擎门（office 族有、**md 族无**——同 1a） | **D**（md 面） | 无 |
| 5e | WASM panic 面 | trap | mdcore-wasm **无 console_error_panic_hook**（Cargo.toml 无依赖；对比 pdf-wasm lib.rs:40 有 set_once）→ panic = 裸 trap 无栈信息；62 unwrap/167 expect 抽样实测：产品路径全部为 `serde_json::to_string(owned data).unwrap()`（不可失败）+ `lock().unwrap()`（仅毒化）——**统计面良性，真 panic 面=①docx-rs ②深嵌套递归**（本轮校正 01-P3.12 的「高度怀疑」） | **B**（诊断面缺口） | health_check（catch_unwind 在测试不在产品） |
| 6a | RPC FS 命令沙箱 | 任意读写删 | resolve_sandboxed（rpc.rs:1263-1313）：canonicalize+组件级前缀+不存在尾段经最近祖先摊平；无工作区全拒；8 条 FS 命令全接（:1358/:1444/:1472/:1527/:1557/:1577/:1627-1628/:1921） | **A** | rpc.rs:2843-2897（`..`/深逃逸/符号链接/越根全负向） |
| 6b | RPC open_file | 沙箱旁路读 | **无沙箱**（rpc.rs:959-989 `std::path::absolute` 直开任意路径进 UI；无工作区要求） | **D**（设计如此，**本轮新增定性**） | :2118 测试用 /tmp/a.md（无越界负向） |
| 6c | RPC save_as | 沙箱旁路写 | **无沙箱、无 cap**（rpc.rs:1213-1252 任意绝对路径写盘=当前缓冲内容；前置 insert_text/replace_selection 可注内容） | **D**（同上） | 无 |
| 6d | RPC socket 认证 | 本机越权 | **无认证**（rpc.rs:32 设计自认）+ **socket 权限未收紧**（:648 `UnixListener::bind` 默认 0755&~umask，无 set_permissions；回退端点 `/tmp/mang-shell-$UID.sock` :33-35）→ 同机其他用户可连接并发全部 27 命令 | **D**（多用户主机）；单用户桌面=低 | 无 |
| 6e | RPC 洪水 | 内存耗尽 | MAX_LINE_BYTES=1MiB 超限断连（rpc.rs:161-163）+ ACCEPT_ERROR_LIMIT | **A** | 协议测试 |
| 6f | RPC batch_replace regex | ReDoS | Rust `regex` crate 线性时间（无回溯）——is_regex 安全 | **A**（库性质） | — |
| 6g | native_dialog | 命令注入 | args 数组直传不经 shell（native_dialog.rs:66 注释自证 + :95-115 zenity 形态）；powershell/applescript 单参数+引号转义（:45-55/:67-72） | **A** | :236-277 受控 PATH 矩阵 |
| 6h | settings | 注入 | mang-shell settings.rs = JSON typed schema（字号/主题），零命令执行面；mdc-settings TOML 链（死代码）纯数据 + degrade 警告 | **A**（无面） | settings.rs:396 |
| 6i | TempDirGuard | 临时文件 | **测试专用**（render_170_pdf.rs:136-151）；产品临时文件面 = rust_xlsxwriter constant_memory 内部 tempfile（native xlsx 写）——非自研可控面，未发现命名/权限缺陷证据 | 无产品面 | — |
| 7a | collab 敌意 op | 非法参数/洪水/毒化 | hostile.rs 全覆盖：client_id 0/≥2^53 拒（:24-47）、BadRange/BlockNotFound/KindIncompatible/id 撞号（:49-209）、input_byte_limit 四面（命令/编辑/apply_update/sidecar，:211-260）、敌意 sidecar（坏 magic/版本/count=2^32 物理上界拒/尾字节，:262-302）；会话状态零污染断言 | **A** | hostile.rs 302 行 |
| 7b | collab 伪造身份 | 身份冒充 | Actor=本地枚举无身份面；信任边界=传输层（web: BroadcastChannel 同源） | 架构性（同源信任） | — |
| 7c | relay 信任边界 | — | 226 LOC 传输无关状态机，**零生产消费者**（复认 01-§1.2）——边界未定型 | 蓝图 | relay tests 9 |
| 7d | web 协作入向 | 恶意 update | collab_channel.ts:198-225 try/catch typed 记账不抛出；remote 永不回发（无回声环） | **A** | 29 e2e 套件 |
| 8a | URL 拉取 | SSRF/外联 | **全仓零网络库**（rg reqwest/ureq/curl/fetch 资源面=0）；下载走 Blob objectURL（main.ts:2327-2335/3042-3050） | 无攻击面（资产） | — |
| 8b | 敌意图片解码 | 解码炸弹 | 无面：office-io docx-rs default-features=false 关 image 特性（Cargo.toml:47-48）；mang-shell image crate 仅 canvas 光栅**输出**（canvas_view.rs:52 ImageBuffer 构造 + :68-70 RASTER 4096/8M px 护栏）；图片字节原样透传不解码 | 无解码面（资产） | render golden |
| 8c | 字体加载 | 敌意字体解析 | 运行时=fontconfig 系统字体（settings.rs:7）；smoke_font.ttf=烘焙入库测试夹具（pdf_doc.rs:615-620）；PDF 字体解码在 pdfium 内部 | 无自研解析面 | — |
| 8d | pdf_view innerHTML | XSS sink | :1741/:1828 注入值全为数值/固定 palette（toFixed 数字、rgb(r,g,b) 数值、UI 选择器色）——类型封闭，无字符串注入位 | 构造性安全 | e2e |
| 8e | evidence 下载 | — | downloadBlob objectURL+revoke（evidence_panel.ts:238-246） | **A** | — |

## 2. 绕过路径清单（防护函数存在但产品路径绕过/缺位）

| # | 防护资产 | 应拦威胁 | 实际产品路径 | 绕过证据链 | 定级 |
|---|---|---|---|---|---|
| B-1 | `url_policy::check`（白名单+15 载荷测试） | `javascript:`/`data:`/`vbscript:` scheme | ① docx 读面超链接透传（docx.rs:1895-1956）→ wasm docx_import.rs:204-243 直转 `[text](javascript:alert(1))` markdown 进编辑器；② rebuild.rs:398-445 超链接重放仅长度/NUL 检查；③ io.rs 超链接四字段透传（X-8 复认） | `rg mdc_office::` 生产=0；workspace 16 份 Cargo.toml 无 mdc-office 边（本轮全量复核） | **P1**（数据卫生+未来出口裸奔） |
| B-2 | `csv_formula::guard_csv_cell`（三模式+测试） | `=cmd\|...`/`=HYPERLINK` DDE 注入 | web xlsx_grid.ts:945-966 `onCopy`——敌意 xlsx cell 显示文本 → clipboard `text/plain` TSV → 用户粘贴 Excel/WPS 即执行（csv_formula.rs:1-13 文档自认的设计威胁面，**接线为零**） | onCopy 代码全读：`rec ? editDisplayText(rec) : slice.cells[i][j].d` 直出 | **P1**（**本轮新增**——01/06 号笔记均未发现此活路径） |
| B-3 | `resolve_sandboxed`（+逃逸/符号链接负向测试） | RPC 任意路径读写 | `open_file`（rpc.rs:959-989）/`save_as`（rpc.rs:1213-1252）先于 #134 沙箱面存在，**不经 resolve_sandboxed**——open=任意路径读入 UI，save_as=任意路径写盘（内容可经 insert_text 注入）；叠加 socket 无认证无权限收紧（:648）→ 本机任意用户可组合成「读任意文件（open+get_selection）+写任意路径」原语（受害者权限内） | 命令分派表 :910/:928 无沙箱调用；测试 :2118 用绝对 /tmp 路径 | **P1**（多用户主机）/ P2（单用户） |
| B-4 | html.rs `MAX_DEPTH=256`（导入面） | 深嵌套递归溢出 | 同族入口 markdown.rs **无对应深度门**（:276-322 深度无上限）→ 树深无界 → `blocks_json()`（debug.rs:25-37 递归 collect）在 web 启动即调用（main.ts:1579）→ wasm trap；`to_markdown`/`word_count`/`snapshot` 同病 | 上述行号链；html 有门、md 无门的**双标实证** | **P1**（DoS，本轮新增） |
| B-5 | PDF `security_scan` 八族 | JS/Launch/URI 用户告知 | **mang-shell 原生线不调用**（pdf_doc.rs 零 security_scan 引用，直连 PdfiumEngine）→ 原生用户打开含 /OpenAction//Launch 的 PDF **零警告**（执行免疫仍由架构保证，告警面缺位） | rg 实证；对比 web 线 pdf_view.ts:1965-2012 有展示 | **P2** |
| B-6 | mdc-office xml_control/xml_entities 六面之二 | 重放消毒 | docx_edit.rs:2026 `escape_text` / xlsx_edit.rs:238 同名 = **独立第二实现**（语义重叠双源漂移）；write.rs 依赖 rust_xlsxwriter 内建 `_xHHHH_` 转义（write.rs:63-66 自核验）——**等效防线存在但非同一实现**（与 B-1/B-2 的纯缺位不同） | 三处代码并读 | P2（漂移风险） |
| B-7 | PDF `Limits.object_depth_max=128` | lopdf 深递归 | 只作用于 security_scan 游走；lopdf 解析器内部无挂点（security.rs:179-184 自认边界登记）→ 敌意深嵌套 PDF 对象可直达 lopdf 递归下降——暴露面=pdf-wasm 写回链/mdc-pdf annotate 链（mang-shell 主面走 pdfium 不经 lopdf 读） | crate 文档自认 + 无深度测试 | P2（**需外部验证**：lopdf 0.45 实际栈行为） |

> 纯缺位（无任何防护函数，非「绕过」）：D 级项 = 1a/1b（md 大小门）、1c（md 深度门）、2g（docx-rs panic 无 catch）、3b（TSV 剪贴板）、6b/6c/6d（RPC 三面）。

## 3. P0-P3 分级汇总（含置信）

### P0（敌意文档可达的崩溃/越权）

| # | 问题 | 置信 | 证据链 | 修复面 |
|---|---|---|---|---|
| P0-1 | **docx-rs 读路径 4 panic + word365 族拒，native 无 catch_unwind → 打开即崩**；wasm=trap（复认 05-P0，本轮复核维持：rg catch_unwind 于 office-io/mdcore-wasm src = 0） | 已确认 | BASELINE.json panic:4；docx.rs:1799 门后直达；docx_doc.rs:171/208 map_err | 读前置预检或 catch_unwind 包读入口；docx-rs 升版立项 |

### P1

| # | 问题 | 置信 | 证据链 |
|---|---|---|---|
| P1-1 | **md 深嵌套递归栈溢出**：blockquote `>` 计数/list indent 深度无上限（markdown.rs:276-322）→ 树深无界 → 7 处递归消费者（debug/markdown/html-export/word_count/pagination/snapshot/fingerprint）；web 打开敌意 .md → `blocks_json()`（main.ts:1579 启动即调）→ trap；html 导入面有 256 门而 markdown 无（双标） | 高度怀疑（代码路径已确认，未运行构造 PoC；wasm 栈深阈值需实测） | markdown.rs:321 depth=indent/2+1；debug.rs:25-37；html.rs:546 对照 |
| P1-2 | **TSV 剪贴板公式注入活路径**：web 网格 onCopy 无消毒 → Excel 粘贴执行 `=cmd\|` DDE / `=HYPERLINK` 钓鱼（csv_formula 设计威胁面零接线） | 已确认（代码路径完整） | xlsx_grid.ts:945-966；csv_formula.rs:1-13 |
| P1-3 | **RPC 沙箱旁路三件套**：open_file/save_as 不经沙箱 + socket 无认证且默认权限（/tmp 回退端点）→ 本机任意用户组合原语（读任意路径入 UI + 写任意路径） | 已确认（代码）；可利用性=多用户主机场景 | rpc.rs:959-989/1213-1252/648/33-35 |
| P1-4 | **mdc-office 消毒六面零接线**（B-1/B-2/B-6 根因；01-§6.5 复认 + 本轮补 docx_import javascript: 透传与 TSV 活路径两条具体后果） | 已确认 | rg/Cargo.toml 全量 |

### P2

| # | 问题 | 置信 |
|---|---|---|
| P2-1 | md/txt 双线无输入大小门（mang-shell document.rs:99；wasm new 无 cap；web file input 无 cap）——docx/pdf/xlsx/canvas 四族全有门的对照下独缺 | 已确认 |
| P2-2 | mang-shell PDF 线不跑 security_scan（告警面只在 web 线） | 已确认 |
| P2-3 | lopdf 解析深度无挂点（crate 自认）——pdf-wasm 写回链暴露 | 需外部验证 |
| P2-4 | mdcore-wasm 无 console_error_panic_hook（pdf-wasm 有）——trap 不可诊断 | 已确认 |
| P2-5 | PDF 双输入门数值不一致（mang-shell 100MB vs mdc-pdf 1GiB）+ wasm 侧无 100MB 对齐 | 已确认 |
| P2-6 | 消毒双源漂移（B-6：escape_text×2 + rust_xlsxwriter 内建 vs mdc-office 六面） | 已确认 |
| P2-7 | RPC 无认证无 socket 权限收紧（单用户桌面定级降 P2；与 P1-3 同根） | 已确认 |

### P3

| # | 问题 | 置信 |
|---|---|---|
| P3-1 | docx import 的 `javascript:` 字面落 GFM markdown（产品内无渲染执行面，跨生态数据卫生） | 已确认 |
| P3-2 | native_dialog PATH 候选工具（敌意环境变量前提下的 PATH 劫持） | 已确认（低前提） |
| P3-3 | wasm serde 深度限/UTF-8 容错依赖库默认行为，无显式钉定测试 | 已确认（缺测试） |
| P3-4 | RPC open_file/save_as 无负向测试（设计如此但无文档化边界断言） | 已确认 |

### 正面资产（保持）

1. 容器三门「声明值+谎言头+输入上限」闭环 + 五组负向测试（io.rs:1470/1597/1761/2013/2203/2238）——zip 生态 8.6 无内建策略层的**正确自研补面**。
2. collab/canvas 敌意输入面全覆盖 fail-closed（hostile.rs 302 行 + 52MB 门 + 物理上界拒）。
3. 零网络拉取、零敌意图片/字体解码面、外置进程 args 数组化——**攻击面最小化的架构纪律**。
4. html.rs 导出转义 by construction（若未来接线预览，XSS 基础已具备）。

## 4. 总判定（回答核心问题）

**现有安全策略没有真正覆盖入口——它是「引擎内嵌真防线 + 产品边界系统性缺位」的马赛克。**

- 真覆盖（A 级，防护在产品路径上生效）：Office 容器四类威胁、collab 敌意 op、canvas 52MB、PDF 渲染限额、RPC FS 命令沙箱（8/27 命令）。
- 悬空（C 级，防护函数存在+自带测试，但产品路径绕过）：**mdc-office 六面全部**（url_policy→docx_import/rebuild 两路、csv_formula→TSV 剪贴板一路、xml_control/xml_entities→双源第二实现）、PDF security_scan→原生线、html MAX_DEPTH→markdown 侧、lopdf depth limit→解析器本体。
- 缺位（D 级，无任何防护）：md 大小门（双线）、md 深度门、docx-rs panic 包裹、TSV 消毒、RPC open_file/save_as 沙箱、socket 认证/权限。

**接线优先序建议**（最小改动最大收敛）：① onCopy 三字符前缀消毒（TSV，一处 ~5 行）；② RPC open_file/save_as 接 resolve_sandboxed 或显式文档化「本机信任边界」+ socket 0600；③ markdown.rs 深度门（对齐 html.rs:546 的 256 上限，parse 侧一处）+ wasm new 输入 cap；④ docx 读入口 catch_unwind（对齐 health_check 既有模式）；⑤ mdc-office 接线三选一裁决（加 Cargo 边/降级为出口库/撤下 README 防线图——01-§7 已立案）。

—— 完 ——（本轮零仓库写入；产物仅本笔记）
