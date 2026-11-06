# 10 · mang-shell（GPUI 桌面编辑器）深度审计（main@96596d7，2026-11-04）

> 只读审计；证据口径：`文件:行号` 均为 HEAD 实测（mang-shell/ 为独立 workspace，
> src 33,780 LOC / 43 文件，tests ≈6,900 LOC / 14 文件，合计 566 个 `#[test]`/`#[gpui::test]`；
> editor/ 18,503 LOC 占 55%）。上游参照：`01-structure.md`（§1.3/§2.4 编辑器双轨结论）。
> 活跃度实测：`editor/mod.rs` main 线 49 commits（非合并约 45——最热文件口径成立）；
> #205 增量围栏修复（177f98e）经 PR #214 于 **96596d7 合入 HEAD**；
> keymap 可配置（#217, 0552798）仅在远端分支 `feat/68-stage-e-keymap`，**未进 HEAD**。

---

## 0. 一分钟总览

- **定位属实**：mang-shell 是实际主线 GUI——Zed `text::Buffer` 基座 + 自研 18.5K LOC 编辑器内核 + docx/xlsx/pdf/canvas 四视图 + 27 命令 RPC 控制面（`rpc.rs:1-146`），对 mdcore 核零依赖（Cargo.toml 全文 + `buffer.rs:51` `use text as zed_text`）。
- **GPL 面**：编译进二进制的 GPL-3.0 系 crate **10 件 / 48,562 LOC**（text/rope/clock/ui/theme/component/icons/menu/syntax_theme/ui_macros——Cargo.lock 逐一验证）；闭源分发被法律性阻断；NOTICES.md「整体 GPL-3.0」记账**属实**，但与 COMPLIANCE.md「GPL 零容忍」姿态自相矛盾。
- **#164 性能**：根因（refresh_caches 每编辑 O(doc) 逐行 highlight 重建）**已在 HEAD 修复**（#205 增量围栏，算法层 µs 级）；但渲染帧内仍存 4 处 O(doc) 残项（帧级 `to_plain_text` 全文物化最重），且**修复后无端到端复测记录**；perf_meas.sh 仅 debug 构建测量（提交自认「红线判定需 release+实机」）。
- **工程质量**：测试证据链纪律罕见地好（运行时读源码锚点测试、真黄金夹具、独立 zip/XML 解析对账、仓外 8000 步随机等价性 harness）；RPC 协议三面（服务端/文档/Python SDK）常量与语义逐字段一致。
- **一句话**：**工程质量高于仓内其他所有产品面，但商业闭源路线被 GPL 面结构性锁死，且性能红线只完成「根因修复」未完成「红线复测」。**

---

## 1. GPL 污染定量（问题①）

### 1.1 编译进二进制的 GPL 面清单（Cargo.toml license 字段 + Cargo.lock 双重验证）

直接依赖（`mang-shell/Cargo.toml:59-80`）：gpui(Apache)、gpui_platform(Apache)、**theme(GPL)**、**ui(GPL)**、zlog(Apache)、**text(GPL)**。
传递闭包（Cargo.lock 实测全在解析图内）：

| GPL crate（10 件） | license | LOC（实测 wc -l） | 进入路径 | 在 mang-shell 的角色 |
|---|---|---:|---|---|
| **ui** | GPL-3.0-or-later | 29,844 | 直接依赖 | 全部控件渲染（src 内 ~190 处 `ui::` 使用点，shell.rs 48 处居首） |
| **text** | GPL-3.0-or-later | 6,746 | 直接依赖 | **编辑缓冲基座**（`buffer.rs:200` `inner: zed_text::Buffer`） |
| **theme** | GPL-3.0-or-later | 5,957 | 直接依赖 | 主题/颜色系统（`cx.theme()` 全渲染面） |
| **rope** | GPL-3.0-or-later | 4,166 | text 传递 | rope 数据结构本体 |
| **component** | GPL-3.0-or-later | 530 | ui 传递 | 组件注册面 |
| **clock** | GPL-3.0-or-later | 338 | text 传递 | 时钟原语 |
| **syntax_theme** | GPL-3.0-or-later | 345 | theme 传递 | 语法主题（theme/src/styles/syntax.rs 消费） |
| **icons** | GPL-3.0-or-later | 360 | ui 传递 | 图标 |
| **ui_macros** | GPL-3.0-or-later | 239 | ui 传递 | UI 宏（proc-macro） |
| **menu** | GPL-3.0-or-later | 37 | ui 传递 | 菜单原语 |
| **合计** | | **48,562** | | |

Apache 侧（无闭源阻塞）：gpui 102,096 + gpui_macros 3,631 + sum_tree 3,327 + scheduler 3,094 + bench_metrics 2,234 + zlog 1,710 + gpui_util 834 + refineable 680 + http_client 669 + ztracing 572 + derive_refineable 548 + collections 418 + gpui_shared_string 210 + ztracing_macro 7 + r39 平台层 8 crate 62,164（gpui_platform/linux/wgpu/macos/windows/ios/web/apple）≈ **182,194 LOC**。vendored 总量 32 crate / 392 个 .rs 文件 / 230,756 LOC（PROVENANCE.md:83 与实测一致）。

### 1.2 法律阻塞面判定

- **静态链接即触发**：ui/theme/text/rope 全部静态编译进 `mang-shell` bin——分发二进制即承担 GPL-3.0 完整源码披露义务（含 mang-shell 自身 33.8K src）。**闭源商业分发当前不可达**；NOTICES.md:16-18 的「Lead 规则：用着不阻断；闭源时需替换」是准确的成本记账。
- **替换成本错位**：最贵的两块恰是编辑基座（text+rope 10.9K LOC——但见 §2.1，实际消费 API 面极窄）与 UI 全家（ui+theme 35.8K LOC——消费面宽，~190 使用点）。**与 01-structure P1-1（编辑器双轨）叠加时，「换掉 Zed 栈」与「收敛编辑核到 mdcore」是同一个决策的两面**。
- **NOTICES.md 核对**：「本项目整体采用 GPL-3.0-or-later」**属实**——仓根 LICENSE=GPL-3.0，mang-shell `Cargo.toml:20` `license = "GPL-3.0-or-later"`，10 件替换清单与 LOC 逐项对上（ui 29,844 等，PROVENANCE.md:20-40 实测吻合）；上游 commit `1663af4`（2026-10-29）与 PROVENANCE.md:11 一致；「源码零修改 + 1 登记补丁（gpui_windows zoom toggle，PROVENANCE.md:108-115）」与 `scripts/p1_gpui_windows_zoom_toggle.patch` 在档一致。
- **姿态矛盾（继承 01-structure D-6）**：COMPLIANCE.md:8「闭源最严：GPL/AGPL 零容忍」↔ 仓根 LICENSE=GPL-3.0；COMPLIANCE.md:19「Zed 系全量源 0 命中」系 `compliance/scan.sh` 的品牌词门——其 PRODUCT_PATHS **不含 third_party**（PROVENANCE.md:161 自述），即 GPL 代码根本不进该门的扫描域；COMPLIANCE 审计终态 2026-10-10 早于搬运 19 天。**合规文档体系对当前主线 GUI 无解释力。**

---

## 2. 编辑器架构评估（问题②）

### 2.1 buffer.rs（2,583 行）：Zed text::Buffer 的「窄口消费」+ 全自研语义层

- **存储层**：`inner: zed_text::Buffer`（buffer.rs:200），构造走 `new_normalized`（:429-437，`\r\n` 直通保 roundtrip）；实际消费的 inner API 面极窄——全文件仅 `inner.edit`（5 处）、`inner.branch`（Clone ×2，:257）、`inner.snapshot`（:1369）+ 经 `snap()` 的 `max_point/row_text/text`（:457-479）。**text 层的锚点（Anchor）能力完全未用**（头注释 :12 称「阶段 C 多光标取用」，但 multi_cursor.rs 实际用自研 Point 算术）——GPL 替换评估时这是利好：换成 ropey 等自研 rope 的迁移面小。
- **语义层全自研**：undo 四元组（Edit/BatchEdit/Undoable，:115-162）、选区锚点+粘性列（:201-211）、批量查找替换的批撤销、`revision` 版本号 + `pending` 事件（doc_events 总线，:212-224）、`EditHook/UndoHook` 编辑门禁钩子（docx 锁定行防线，:230-241）、`FenceDirty` 脏行记账（#205，:242-248）。text::Buffer 内建 undo 被刻意弃用（编辑落入 inner history 属「无害堆积」，:197-199 注释自认）。
- **坐标语义**：对外 col=char 索引（CJK 一字一列），char↔byte 换算 O(行长)（:171-180）；`line()` 每次返回 String 分配（:472-474，头注释 :15-17 登记为大文档边界）。

### 2.2 display_map.rs（678 行）与 multi_cursor.rs（743 行）：设计抄 Zed、实现全自研

- display_map.rs:3-5 明言「抄 Zed display map 的**设计**（上游实现在 editor crate 内——62 crate 耦合面，不搬）」。fold+wrap 两级变换（buffer Point→行系→DisplayPoint）、RowTable 缓存（失效后 O(行数) 重建、O(log) 二分查询，:324-358）、渲染层惰性回填断点（贪心 `wrap_points_from_advances` 纯函数，:373-401）。13 项单测覆盖映射/折叠/越界防御。
- multi_cursor.rs:1-13 同纪律：「抄 Zed `selections_collection` 的设计」——Buffer 保持单光标契约（41 测试锚定），多光标在编辑器层 `Cursors`，多点编辑按文档序尾→头应用 + 批撤销锚定。
- **自研 vs 移植量结论**：mang-shell src 33.8K LOC **100% 自研**；借用的「逻辑」只有 text::Buffer 一件（且窄口）；ui/theme/gpui 是基础设施不是编辑器逻辑。这与「18.5K LOC 编辑器内核」的宣称一致——不是把 Zed editor crate 搬进来了。

### 2.3 fence.rs（706 行，#214 最新）：增量围栏缓存——高质量

- 算法：种子（脏行 d 的旧行首态）/ 推进（`FenceCursor::advance` 单行函数，全量与增量共用，杜绝两实现漂移）/ 停机（j≥e_new 后行首态双等即停）/ 回退（前置不满足返 false 不改缓存）——fence.rs:159-227。奇偶翻转「永不回合」的最坏情况有归纳论证（:50-53）。
- 测试面：15 项内嵌单测含「锚定=旧 highlight_line_ex 循环逐位一致」（:314-335）、随机等价性 40 种子×30 步（:658-705）、回退前置全枚举（:611-637）；另有**仓外 harness-205**（提交 177f98e 记载：#[path] 引真实源码、50 种子×40 步 + 6 种子×1000 步 = 8000 步全绿 + SimEditor 系统级模拟 30×120 步、增量快路径 2325 次 > 回退 606 次真实行使）。
- 接线：Buffer 侧 `FenceDirty` 三态记账（单区间 Single{line, inserted_lines, pre_lines}/多区间 Full/无变更 Clean）→ Editor `refresh_caches` 收割（mod.rs:2172-2223）：Clean+缓存有效+长度一致 → O(1) 复用快速路径；Single+pre_lines 匹配 → 增量；否则全量。docx/xlsx 态整体跳过。**这是教科书级的增量缓存修复**。

### 2.4 与 Zed 上游的跟随维护成本

- **无 upstream 跟踪机制**：无 .gitmodules、无 git remote；上游钉死在 `1663af4`（2026-10-29 快照，PROVENANCE.md:11）。升级 = 手工四步（PROVENANCE.md:164-170：fetch → 改 `zed_port.py` 顶部 `ZED` 指针（默认 `/home/z/my-project/zed-upstream` 本地克隆，scripts/zed_port.py:16）→ `--clean` 重建 → 全绿后更新 PROVENANCE）。
- 风险面：Zed 上游日更，快照必然老化；消费面越宽老化越痛——text 窄口（低风险）vs ui/theme 宽口（~190 使用点，上游 API 变更直接红）。1 个登记源码补丁（gpui_windows zoom）有重放文件与上游修复观测点（PROVENANCE.md:108-115）。`third_party/zed` 独立 workspace **不在任何 CI 内**（编译证据是人工贴的 2026-10-30 `cargo check` 输出，PROVENANCE.md:134-155）；mang-shell 自身有 Windows CI job（windows-test.yml，见 01-structure §1.3）但 Linux 基线无 CI。

---

## 3. #164 性能问题现状（问题③）

### 3.1 根因归因与修复核验

- **归因属实（历史核验）**：`git show 177f98e~1` 的旧 `refresh_caches` 确为每编辑全文档 `for i in 0..line_count()` 逐行 `highlight_line_ex` 重建 fence_cache（与提交描述一致）；提交 f78d353 的实测（Xvfb+lavapipe+debug）：1.5MB/32K 行逐键 p50=324.78ms、7.5MB/160K 行 p50=1764.69ms——线性放大确认 O(doc)/键。
- **修复已在 HEAD**：177f98e 经 PR #214（96596d7）合入。算法层基准（提交记载，2 核沙箱）：32,101 行旧全量 6232µs/键 → 围栏外打字 1.6µs（~3900x）、围栏内 2.2µs；160,231 行旧 29493µs → 6.9µs/21µs。全量回退路径也快 ~2.5x（advance 不再整行 token 化）。
- **登记是否仍成立**：**根因登记已过时（该 O(doc) 项已消除），但绝对数字无人复测**——提交明言「实机 rope 取行由 owner 侧 perf_meas.sh 复测」，仓内无修复后端到端数据；`perf_meas.sh:101` 的键量/时长标定注释仍写着修复前口径（「逐键处理 ≈ 0.33s@32K 行」）。

### 3.2 残余 O(doc) 面（HEAD 实测代码定位——#205 未覆盖项）

| # | 路径 | 证据 | 触发条件 | 量级估计 |
|---|---|---|---|---|
| R-a | **渲染帧内全文 String 物化**：bracket 匹配路径每帧 `to_plain_text()` | mod.rs:4183（`let text = self.buffer.to_plain_text();`——在 `bracket_cache.get` 判缓存**之前**无条件执行；BracketCache mod.rs:297-321 只省重扫不省物化） | md 态每帧 | 7.5MB 文档 ≈ 每帧 7.5MB 分配+rope 展平（debug 构建数十 ms 级） |
| R-b | display_map 行表 O(n) 重建 | display_map.rs:332-358（build_table 全行扫）；refresh_caches 每编辑 `reset_wraps()`（mod.rs:2175）→ invalidate → 首次映射重建 | 软换行激活时每键一次 O(行数) | 160K 行 × 低价迭代（debug 数 ms） |
| R-c | FenceDirty::Full 回退全量 | mod.rs:2191-2206（Single 不匹配即全量）；buffer.rs:344-350（多光标连编/批量/批撤销记 Full） | 多光标打字、批量替换、批撤销 | advance 版全量（cheap，~2.5x 快于旧但仍 O(doc)） |
| R-d | 查找条打开时每编辑全文重扫 | mod.rs:2220-2222 → find_bar.rs:134-157（`refresh` 全文档 `find_matches_opts`/regex） | find 条打开时每键 | 与 find 同量级（7.5MB 1.6-1.8s 口径的复用） |

R-a 是最值得做的一项（一行改动可移入缓存判定位）；R-b/R-c/R-d 属可接受工程债但有登记价值。

### 3.3 perf_meas.sh 可信度

- **只在 debug 构建测量**：脚本 `CARGO_INCREMENTAL=0 cargo build --bin mang-shell`（无 --release）+ 运行 `./target/debug/mang-shell`；profile.dev 仅做了 `incremental=false/codegen-units=16/debug="limited"` 的构建加速（Cargo.toml:112-116），**优化等级仍是 debug**。提交 f78d353 自认「红线判定需 release+实机（owner 清单）」——诚实但意味着 324ms/1765ms 与 <50ms 红线的比对**只具归因意义（O(doc) 缩放互证）不具判定意义**（debug 无优化常放大 10-50x）。
- 测量口径本身可靠：MANG_PERF 门控零常态开销（perf.rs:31-43 OnceLock）；逐键口径= dispatch_key 进入到处理完毕不含排队（perf_meas.sh:118-121）；first-frame 以 main 首行 `perf::boot()` 为基（perf.rs:36-43，perf_wiring.rs:41-60 钉位「boot 距 main 入口 <400 字节」）；VmHWM 退出期 /proc 采集（main.rs:805-815）。5 项锚点由 tests/perf_wiring.rs 源码扫描钉住（采集点失线红灯）。

---

## 4. docx/xlsx/pdf 三视图接线质量（问题④）

**总体：接线质量高，「降级不崩溃 + 边界显式登记」哲学贯穿。**

- **docx_doc.rs（1,639 行）**：双装载——`load`（只读渲染）与 `load_editable`（宽面 `read_docx_from_bytes` 行模型 + 窄面 `docx_edit::DocxEditor` 段索引，**按段落序号对账，不一致整体降级只读**「绝不带着错位映射开工」，docx_doc.rs:10-14）；可编辑上限 = 宽面输入限 ×4 超限降级只读（:86-88）；页眉/页脚/图片等不入正文流但可编辑态出**只读清单行**（「看得见 + 为什么改不了」，:33-36）——这是很成熟的产品思维。
- **docx_edit_bridge.rs（1,457 行）**：核心不变量「未编辑段落必须回写原文全等否则不 set」防引擎外科手术副作用（:9-23）；行归属四态 Para/Foreign/User/Locked（:80-118）；`EditHook::before_edit` Err = 拒改且 Buffer 零变更（锁定行防线在 Buffer 唯一变更入口生效，buffer.rs:230-239）；拒改有 GUI 可见反馈链（#178：take_rejection → 状态栏 Error 色，module_wiring.rs:389-416 三段锚点钉定）；`DocxPort` 窄面 trait + 内存假实现单测（:166-181）。第一版边界全部显式登记（表格/文本框只读、Foreign/Cell 变体当前构造点为 0——:76-78 「显式登记，不是遗漏」）。端到端证据：docx_edit_roundtrip（不编辑→保存逐字节恒等；改一段→非目标部件逐字节不变）、docx_edit_corpus（语料装载-往返-单编辑）、docx_bid_e2e（真实标书 sdt 盲区对账）。
- **xlsx_doc.rs（633 行）**：office-io（calamine）→ 行模型；**渲染护栏 1000 行 × 64 列 + 显式截断提示行**（:46-49，「绝不静默截断」）；合并格=左上单点语义（#202 实证 + #203 勘误注释漂移——xlsx_doc.rs:31-32，注释分歧转 issue 的处理方式本身是纪律加分项）；公式格显示缓存值/无缓存回退 `=公式` Italic。
- **pdf_doc.rs（816 行）**：INPUT_CAP_BYTES=100MB 读盘前前置门（:62、:148——render_170_pdf 用 105MB 合法头坏件证明拒绝来自前置门而非引擎装载失败）；页分隔行做行锚（书签跳转 + `page_of_line` 页码区间映射，:85-103、:338-344）；扫描件提示。三个 render_170_pdf 断言组含尾部截断坏件「Err 或有界 Ok 绝不 panic」。

---

## 5. RPC 层真实能力面（问题⑤）

- **27 命令全部真实现**：`ControlCommand` 枚举（rpc.rs:174-176 起）经 `execute_command` 穷尽 match 分派（rpc.rs:903-957，Subscribe 在 dispatcher 侧特判 ：860-869）；生命周期 6 + 导航/历史/保存/订阅 9 + FS 7 + 编辑器命令 4 + 批量 1 = 27，与 rpc-protocol.md:103 速查表、Python SDK `COMMANDS` 全集一致。无空桩（command_coverage.rs:200-261 静态零桩断言 + rpc_smoke.sh 真实 socket 端到端全链）。
- **架构**：AF_UNIX JSON-Lines；每连接独立 out 通道（响应与事件同出口保序）；单任务 dispatcher 主线程串行（rpc.rs:873-878 `cx.update → window.update`）；事件泵广播 doc_changed（source=user/rpc 区分 + revision 跳号检测，doc_events.rs:26-40）。无认证/加密/网络——本地单机边界在协议文档 §1 明示（rpc-protocol.md:28-34）。
- **沙箱**：FS 七命令 + batch_replace 全部限定工作区根内；`resolve_sandboxed` canonicalize + 组件级前缀检查（`..`/symlink 逃逸拒；目标不存在取最近存在祖先——新建文件也逃不出）；无工作区=全拒（「无边界比错误边界更危险」，rpc-protocol.md:472-486）；delete 递归无回收站（取舍登记）+ 脏标签路径拒。
- **三面一致性（逐字段核验）**：服务端常量 rpc.rs:163/1342/1345/1847/1851/2096（MAX_LINE_BYTES=1MiB、LIST_DIR=5000、READ_FILE_CAP=1MiB、BATCH_MAX_FILES=200、BATCH_MAX_HITS_PER_FILE=10K、SELECTION_TEXT_CAP=2000）↔ Python SDK protocol.py:83-98 **全等**，且 `test_protocol_constants`（test_client.py:58）钉死漂移；protocol.py 头注释声明「逐字段核实源码，非凭文档示例编写」。Python 侧另有乱序响应用 id 匹配、超时连接复用、畸形响应协议违规等 12+ 客户端行为测试。
- **结论**：这是仓内三条 AI 通道里设计最完整、验证最实的一条（对照 mdcore Source::Ai 事件流与 web __bridge）。已知边界（单实例端点、错误消息中文非错误码、run_command 名可变）在 rpc-protocol.md §11 老实列了 10 条坑。

---

## 6. 测试可信度抽查（问题⑥）

### 6.1 module_wiring 的「红→绿双向实证」方法论

- **实现方式核实**：测试在**运行时**用 `fs::read_to_string` 读源码（module_wiring.rs:57-61 `src()`；fn_body 提取 :92-127 为字符串字面量感知的大括号配对扫描）。因此「编译后破坏源码 + 跑已编译二进制 → 红」在机制上成立：二进制不内嵌源码，每次运行重读当前文件。**但「双向实证」本身是开发期人工纪律**（见 c9b45b2/98de59b 提交描述「编译后破坏源码+跑已编译二进制——运行时读源口径」），仓内无自动化的破坏-恢复 harness——可信度依赖提交者自律，非可复验机制。
- **静态锚点的固有局限**：(i) `body.contains("git_panel")` 类断言可被注释/字符串满足（未发生——panels.rs:119-121 实际渲染 GitPanel——但机制上不设防）；(ii) 大括号扫描不感知注释与 raw string；(iii) `synthetic_dispatch_never_wraps_root_view_update` 的负向窗口仅「dispatch_event 前 4 行」（module_wiring.rs:486-498）——`window.update(` 出现在第 5 行以前即可绕过，且守卫只扫 main.rs。
- **红绿账表已过期（小瑕疵）**：文件头表格（module_wiring.rs:24-27）仍把 git_panel/命令面板分组/status_info/设置面板标 🔴，实际四处均已接线转绿（panels.rs:119-121、mod.rs palette_view/build_sections 10 命中、status_bar.rs status_info 6 命中、activity_bar.rs act-settings 在场 + settings_panel.rs 517 行）——表是手工维护的陈旧面，测试本体全绿。

### 6.2 command_coverage 的 Cmd::ALL 运行时面

- 静态面（tests/command_coverage.rs，6 测试）：枚举↔ALL 双向对账、exec_command 无 `_ =>` 通配 + 无空体 arm、default_key → dispatch_key(_inner) 双体并集字面量取证、title/键位唯一、RPC 机器名双射。静态解析精确覆盖「空桩/通配兜底」两个编译器盲区（:28-34 论证成立——match 无通配时 Rust 穷尽性已由编译器保证）。
- 运行时面（src/editor/mod.rs:5244-5412，`#[gpui::test]` 三组）：`runtime_every_cmd_in_all_executes` 以真实 gpui TestAppContext + EditorHost 最小宿主开窗（与 rpc run_command 同通道 `ed.update → run_command`）遍历 ALL 全量执行——**断言强度=零 panic + 末尾存活 sanity（cursor.line < line_count）**，属冒烟级；行为语义由另两组补（导航/选区/全选状态断言 :5311-5348；折叠计数/面板态/Undo-Redo 往返/Save 落盘一致性 :5352-5411）。组合后是合理的强度——单测「零 panic」弱，但配状态变化断言后不再是空转。dev-dep `gpui test-support` 不进 shipped 图（Cargo.toml:88-93 注释）。

### 6.3 render_170_* 的「独立解析口径」

- **独立性成立（实测代码）**：tests/render_170_xlsx.rs:78-142 自带极简 zip 读器（EOCD 扫 + 中央目录 + 本地头 + flate2 解压——**不依赖被测 office-io 读面**）；`xml_cell_display`（:298-341）独立实现 t="s"/b/str/inlineStr/数值五型分派与 SST 间接（:264-279）；期望值全部从源 XML 字节解析，再与引擎自报 props（formulas/merges/hyperlinks/cells）与渲染行模型取格**对账**——对账方向正确（源为真值，引擎为被验证方）。render_170_docx.rs 补精确标签扫描（自闭合 `a:blip`/`w:br`）与段落四轨对账（367 顶层 + 242 单元格 + 2 sdt 盲区 == 611）；render_170_pdf 分 Phase A（无引擎也真跑：页数=2 独立对账、UTF-16BE 书签标题源字节在场）与 Phase B（引擎门控 SKIP 横幅不伪装通过）。
- **夹具纪律**：黄金例进仓、缺失即 FAIL 不许 SKIP（render_170_xlsx.rs:56-68）；带图 docx 夹具正向对账 + 旧夹具零图守卫防漂移（render_170_docx.rs:32-44）。
- 唯一共享件 flate2 是解压原语（非被测读取管线），独立性结论不受损。

---

## 7. GPUI 重入 panic 修复（98de59b）健壮性（问题⑦）

- **根因与修复**：`window.update(root)` 闭包内 `dispatch_event` → 事件命中 shell 级 `cx.listener` → 重入 update 同一实体 → entity_map.rs:164 panic（提交记载 Xvfb 实机 exit 101 复现）。修复把 `run_mouse_script::step` 改 `update_window`（AnyView 闭包）口径——main.rs 现 13 处 `update_window`、5 个合成派发通道（key/mods/mouse/scroll/panel-resize）全部新口径（main.rs:961/1002/1072/1168/1221）；残余 `window.update` 仅测试代码（mod.rs:5295 等 TestAppContext）与合法 shell 更新（main.rs:892）。
- **守卫**：`synthetic_dispatch_never_wraps_root_view_update`（module_wiring.rs:467-509）负向（dispatch_event 前 4 行禁 `window.update(`）+ 正向（前 3 行须见 `update_window(`）+ 基线 ≥4 派发点。运行时面：--smoke-mouse + --smoke-panel-resize Xvfb 双烟雾 0 panic。
- **残余风险面**：(a) 负向窗口 ±4 行的源码扫描可被换行排版绕过（见 §6.1）；(b) 只守 main.rs——未来在其他文件新增合成派发点无守卫；(c) rpc.rs:873-878 的 `cx.update → window.update` 属主线程帧间调度（AsyncApp executor 派发回主线程，不在某实体的 update 闭包内），**非重入**；闭包内更新的是 ShellApp，命令内部再 `editor.update` 是不同实体——GPUI 允许。真正会复现同类 panic 的形态是「ShellApp 自身 update 闭包内同步触发命中 ShellApp 监听器的事件派发」——rpc 命令执行不派发合成事件，安全。(d) `dispatch_key` 拆层（mod.rs:2244-2268 计时包装 + inner 实体）语义零变更，perf 计时不在重入敏感路径。

---

## 8. 会话/多窗口/设置持久化 vs crates/shell（问题⑧）

- **mang-shell 无 session.rs**：会话态 = 内存 Tab 集（ShellApp）+ 工作区记忆（settings.rs:89-101，`workspace` 键）+ recent.rs（`recent.json`，文件 20/工作区 10，写穿透）+ canvas 会话走 mdc-canvas `CanvasSession`（yrs 二进制快照 authoritative_snapshot，Cargo.toml:75-80 注释）。**多窗口不支持**——单窗口（main.rs:320 唯一 `open_window`）+ 标签页模型；RPC 端点单实例（多实例第二个禁 RPC）。
- **crates/shell（mdshell，480 LOC）**：DocumentSession/SessionManager/Host trait + mdc-settings TOML 再导出——消费者仅 shell-native（蓝图、停滞，01-structure §1.1）。**两套会话管理零共享**：mang-shell 不依赖 mdshell/mdc-settings（Cargo.toml 全文无此二项）；设置第三栈并立（mang-shell JSON ~/.config/mang/ ↔ mdc-settings TOML 四层 ↔ web TS 镜像——01-structure P3-9 维持）。**判定**：mdshell 链在产品路径上是死重（contract-lock 测试专属）；mang-shell 侧自建方案（JSON 直存 + 静默容错「记忆是增强不是依赖」settings.rs:16-17）对单机产品够用——问题不在 mang-shell 的实现质量，在双轨未收敛（结构性，归 01-structure P1-1/P2-8 裁决域）。

---

## 9. 问题清单（P0-P3，含置信级别与证据链）

> 置信级别：已确认 = 本轮代码/git 实证；高度怀疑 = 证据充分未运行验证；需外部验证 = 需构建/运行或商业裁决；设计偏好 = 非缺陷。

**P0（数据丢失/安全/核心状态破坏）：本轮 mang-shell 范围内未发现。**
（RPC 无认证是登记过的本地单机边界 rpc-protocol.md:28-34，沙箱逃逸防护有单测+烟雾；docx 桥拒改防线在 Buffer 唯一入口。）

### P1-1 GPL 闭源阻塞 + 合规姿态自相矛盾（继承 01-structure P1-3，本轮定量复核）
- 证据：§1.1 表（10 crate / 48,562 LOC，Cargo.lock 逐项）；COMPLIANCE.md:8 vs 仓根 LICENSE vs NOTICES.md:16-23；scan.sh 扫描域不含 third_party（PROVENANCE.md:161）。
- 影响：闭源商业分发不可达；替换最贵面 = ui/theme（35.8K LOC 宽消费面）+ text/rope（10.9K LOC 窄口可换）。
- 置信：**已确认**（法律影响需外部验证）。

### P1-2 #164 性能修复「根因已除、红线未复测」+ 帧内 O(doc) 残项
- 证据：§3.2 表 R-a~R-d（mod.rs:4183 帧级 to_plain_text 物化最重；display_map.rs:332-358；find_bar.rs:134-157）；#205 后无端到端复测记录（177f98e 提交自认 owner 侧待办）；perf_meas.sh debug-only（§3.3）。
- 影响：324ms/1765ms 的登记作为**归因结论已过时**、作为**红线状态未知**；7.5MB 文档在 debug+软渲下逐键大概率仍超 50ms 红线（R-a 单项每帧 7.5MB 物化）。
- 置信：残项**已确认**（代码定位）；量级**高度怀疑**（未运行）；修复有效性（算法层）**已确认**（harness 数据在案）。

### P1-3 vendored Zed 无 upstream 跟踪 + third_party 无 CI 触达
- 证据：§2.4（无 submodule/remote；PROVENANCE.md:164-170 手工四步；编译证据为人工贴录；Zed 上游日更）。
- 影响：快照老化后 ui/theme 宽接口升级成本随时间上升；任何环境变化（工具链/系统库）下 third_party 编译绿无人守护。
- 置信：**已确认**。

### P2-4 会话/设置双轨与多窗口缺位（结构性）
- 证据：§8（mdshell 480 LOC 零共享；settings 三栈；单窗口）。
- 置信：**已确认**（裁决属用户，归 01-structure P1-1 同域）。

### P2-5 静态锚点测试的方法论边界
- 证据：§6.1（contains 可被注释满足；±4 行窗口可绕；红绿账表 4 项过期；「双向实证」为人工纪律非自动 harness）。
- 影响：守卫强度依赖代码风格约定；新人用注释里的关键词即可误绿。
- 置信：**已确认**（机制层面）；当前无实际误绿案例**已确认**（四处均真实接线）。

### P2-6 harness-205 等价性对照在仓外（流程风险）
- 证据：177f98e 提交描述「仓库外独立 crate，#[path] 引本仓真实源码」；fence.rs:231-233 注释「编辑器仓 GPUI 面无法本地编译——纯层测试双环境共用」——仓内只有 15 项内嵌单测 + 随机 40×30，8000 步强度在仓外不可复验。
- 置信：**已确认**（存在性）；等价性结论本身**高度怀疑→倾向可信**（算法论证 + 仓内子集测试一致）。

### P3-7 文档漂移三件（低成本高收益）
- module_wiring.rs:24-27 红绿账表 4 项过期（标红实绿）；perf_meas.sh:101 标定注释为修复前口径；COMPLIANCE.md 审计终态早于搬运 19 天（01-structure D-6）。
- 置信：**已确认**。

### P3-8 微开销登记（buffer.line() String 分配、char↔byte O(行长) 换算、fence 闭栏语义放宽 CommonMark——highlight.rs:20-22）
- 置信：**已确认**（均已头注释登记，属已知边界非隐藏缺陷）。

### P3-9 docx 编辑第一版边界（表格/文本框只读、Foreign/Cell 零构造点、段内多 run 格式保留首 run rPr）
- 证据：docx_edit_bridge.rs:48-60、:76-78。
- 置信：**已确认**（显式登记）。

---

## 10. 结论与建议

**「值得继续」的证据**（供主审计员收敛决策）：
1. **测试证据链是全仓标杆**：运行时读源锚点 + 真黄金夹具 + 独立 zip/XML 解析对账 + 仓外随机等价 harness + 真实 socket RPC 烟雾——每条声称都能定位到可复跑的验证件。缺陷发现→issue→修复→锚点测试的闭环纪律（#178/#179/#182/#183/#203/#205）可追溯。
2. **#205 是高质量修复**，但收尾缺一环：复跑 perf_meas.sh（release 构建）+ 修 R-a（to_plain_text 移入缓存判定位，预计一行改动）即可宣告 #164 真正关闭。
3. **GPL 裁决建议**：若闭源是硬目标，text/rope 替换（窄 API：edit/branch/snapshot）成本低应先行；ui/theme 替换等于重写 UI 层——与「收敛编辑核」决策应一次性裁决，避免两次大手术。

**审计零写入仓库；本文件为唯一产物。**
