# C5 — mang-shell 编辑器内核深审（深化轮 3/3 收官：P1-3 B 轨实体）

- 基线：audit 分支 = main@6322f86（#218 keymap 已并入）；`git status` 零改动，**绝对只读达成**。
- 对象：`mang-shell/src/editor/`（15 文件 / 18,547 行，其中 md 编辑内核本体 14,003 行 + docx/pdf/xlsx 适配器 4,544 行；全 src 36,850 行）。
- 方法：纯静态阅读（GPUI 系统依赖无法在本沙箱编译）；vendored `third_party/zed/crates/text` 与 `gpui` 源码同读以核实集成语义。
- 置信四档：**已确认**（源码定位）/ **高度怀疑**（逻辑推演强但未运行）/ **需外部环境验证**（需编译/实机）/ **设计偏好**（裁决类）。
- 前序：笔记 10（概览）、11（P1-1 双轨量化）、19（keymap 增量）。本笔记为其内核本体首深审，**修正笔记 10 的 2 处行号漂移并新增 5 项残项**。

---

## ① 内核架构图（文字版）与文件职责矩阵

### A. 层级与数据流（已确认）

```text
main.rs
  └─ shell.rs ShellApp（GPUI 根 View/Render，shell.rs:1639-1640）
       ├─ editor_area → Entity<Editor> 嵌入（shell.rs:2096 editor.clone().into_any_element()）
       ├─ status_bar::status_bar(editor, ...)（shell.rs:1893——每帧重算，见 ③R-e/R-f）
       └─ panels.rs / tab_bar.rs / palette_view.rs 等外围
            └─ editor/mod.rs Editor（GPUI Entity + Render，mod.rs:365-469 状态面 36 字段）
                 ├─ Render::render（mod.rs:4825-4840）→ render_editor_area（3711-3892）
                 │    └─ uniform_list 虚拟化（3738-3750）→ render_lines(visible)（4104-4410）
                 │         ├─ display_map.row_source/row_text（显示行↔buffer 行段）
                 │         ├─ line_tokens → highlight::highlight_line_ex（md 行级高亮）
                 │         ├─ styled_runs_for_line（4415-4555：语法/选区/查找/IME/括号逐 char 合成）
                 │         └─ 行内 canvas：row_bounds 回填（4396-4406）+ paint 期窗口级 mouseup（3764-3768）
                 ├─ 输入三通路：
                 │    ① on_key_down → dispatch_key（2248，MANG_PERF 计时包装）→ dispatch_key_inner（2266-2544）
                 │       → keymap 表 resolve（#217/#218）→ EditorAction match（2385-2540）→ Buffer 语义 API
                 │       → after_edit()（2542——**无条件**，含纯光标移动键）
                 │    ② IME：EntityInputHandler（4896-5023，UTF16 坐标域）→ replace_range 同链
                 │    ③ 鼠标：dispatch_mouse_down/drag_move/autoscroll（3146-3400）→ mouse.rs 纯函数 + row_bounds hit-test
                 ├─ 命令第二入口：exec_command(Cmd)（1314-1371，面板/RPC/菜单共用）
                 ├─ after_edit（2777-2808）：sync_docx_pending → refresh_caches → scroll_to_item
                 │    → publish_doc_event（#111 doc_events 总线，RPC 启用时外推）→ 拒改提示维护
                 └─ buffer.rs Buffer（纯逻辑，零 gpui）
                      ├─ 外壳语义层：cursor/anchor/desired_col（201-205）、undo/redo 四元组栈（206-207）、
                      │   revision/pending 事件（215-224）、EditHook/UndoHook（239-241）、FenceDirty（242-248）
                      └─ inner: Zed text::Buffer（200，rope 持文 + 事务历史 + 锚点基座——仅作存储层）
                           └─ third_party/zed/crates/text/src/text.rs（3,879 行 vendored）
```

### B. GPUI 帧驱动模型（已确认，含关键语义核实）

- `cx.notify()` 标脏实体；绘制期 `invalidate_entities` → `mark_view_dirty` **会把祖先 View 一并标脏**（vendored gpui `window.rs:2328-2342`、`3661-3667`）。Editor 嵌在 ShellApp 树内 ⇒ **编辑器每次 notify 都触发 ShellApp 整树重渲**——这是 ③ 中 R-e/R-f「每键全量」结论成立的前提，已在 vendored gpui 源码定位。
- 视图隔离跳过条件 `!window.dirty_views.contains(&entity_id)`（vendored gpui `view.rs:550`）——ShellApp 被标脏即不满足跳过。

### C. 文件职责矩阵（行数为 wc -l 实测）

| 文件 | 行数 | 职责 | 纯度 |
|---|---|---|---|
| editor/mod.rs | 5,456 | Editor 视图：渲染管线/键鼠命令/面板/折叠/保存/RPC 面/IME | gpui 耦合 |
| editor/buffer.rs | 2,583 | 文本模型外壳：光标/选区/undo 四元组/批量编辑/事件/钩子/围栏记账 | 纯逻辑 |
| editor/docx_edit_bridge.rs | 1,456 | docx 可编辑桥（EditHook/UndoHook 宿主 + 回写计划） | 纯逻辑 |
| editor/find_bar.rs | 1,199 | 查找/替换状态机 + 历史环 | 纯逻辑 |
| editor/highlight.rs | 1,010 | md 行级 + 围栏代码（rust/py/js 词表）token 化 | 纯函数 |
| editor/pdf_doc.rs / xlsx_doc.rs / docx_doc.rs | 816/633/1,639 | 只读渲染态行模型（office 适配层） | 纯逻辑 |
| editor/fence.rs | 706 | #205 围栏缓存增量重建（种子/推进/停机/回退） | 纯函数 |
| editor/display_map.rs | 678 | fold+wrap 双变换显示映射（行表缓存 O(log n) 查询） | 纯逻辑 |
| editor/brackets.rs | 668 | 括号匹配（token 上下文感知、跨行栈配对） | 纯函数 |
| editor/multi_cursor.rs | 743 | 多光标/列选择（编辑器层，Buffer 单光标契约不变） | 纯逻辑 |
| editor/mouse.rs | 503 | 命中/clamp/词行选区/自动滚动纯函数 | 纯函数 |
| editor/commands.rs | 264 | Cmd 枚举 + ALL 注册表（面板/键位/RPC 三入口） | 纯数据 |
| editor/buffer_find.rs | 193 | Buffer 正则查找分离 impl | 纯逻辑 |

---

## ② Zed text::Buffer 集成审计（问题 B）

### B-1 wrap 形态与所有权（已确认）

- **组合式外壳**：`Buffer` 持 `inner: zed_text::Buffer`（buffer.rs:200），inner 只当「rope + O(log n) 编辑」存储层；光标/选区/undo/事件/钩子全部在外壳（buffer.rs:196-249 头注释自述 r42 换芯纪律）。text 层内建 undo 明确弃用（buffer.rs:9-12）。
- **所有权**：Editor 独占 `Buffer`（mod.rs:366）；无 Rc/Arc 共享、无跨视图借用。`Buffer::clone` 走 `inner.branch()`（buffer.rs:251-275；vendored text.rs:841-852——快照同构、历史不共享），**仅测试使用**，产线零分支克隆（rg 全 src 仅 buffer.rs:257 一处 + tests）。

### B-2 snapshot 语义（已确认——用法正确）

- `snap()` 返回 `&self.inner.snapshot()`（buffer.rs:1368-1370）；vendored 实现 `snapshot()` 直接回 `&self.snapshot` 字段（text.rs:833-835）——**零拷贝借用，无跨帧持有**。全文 `grep` 无任何 `BufferSnapshot` 存储字段。
- `to_plain_text()` = `snap().text()` = `visible_text.to_string()`（buffer.rs:457-459 → vendored text.rs:2193-2195）——rope 全量展平 O(doc) 分配。此正确性 OK，但其调用频度是 ③ 的核心残项。

### B-3 edit 接口封装（已确认）

- 单点漏斗 `apply_core` → 一次 `inner.edit([(zstart..zend, text)])`（buffer.rs:1061-1068），批量 `replace_batch` 单事务一次 `inner.edit(rope_edits)`（buffer.rs:793）——千处替换单事务的口径有实现支撑（buffer.rs:2291-2327 千处 <100ms 单测）。
- 坐标域：外壳 char 列 ↔ rope byte 列双向换算（char_to_byte/byte_to_char，buffer.rs:171-180）；`\r\n` 原样直通（`new_normalized` 收未归一 rope，vendored text.rs:755-760 只存不洗；测试 crlf_roundtrip_bit_fidelity buffer.rs:1441-1449 钉死位保真）。

### B-4 事件转译层：**不存在**（已确认）

- mang-shell **零调用** vendored text 的 subscription/BufferEvent 面（rg `subscribe|BufferEvent|on_edit` 于 src/editor 与 document.rs 均无命中）；事件体系是外壳自建 `revision` + `pending`（buffer.rs:212-224、1034-1047），`Editor::after_edit` 单点收割 → doc_events 总线（mod.rs:2813-2831）。inner 每次 edit 的 `subscriptions.publish_mut`（vendored text.rs:901）是**纯开销的死事件**。
- 后果：Zed 内建的 edits 增量事件（Zed 自家 DisplayMap 增量更新的数据源）被弃用，mang-shell 自研 display_map 行表 + 全量失效重建（见 ③R-b）——「抄设计不搬实现」的代价面。

### B-5 错误使用清单

| # | 发现 | 证据 | 置信 |
|---|---|---|---|
| B-5a | **inner 历史无界堆积**：vendored `Buffer::edit` 无条件 `history.push(op)` + `push_undo`（text.rs:885-886）；mang-shell 从不调 `inner.undo/redo`（rg 仅 Clone 用 branch）且无 prune API 消费 → 长会话内存随累计编辑量单调增长；undo 文本双重存储（外壳四元组 old/new + inner Operation new_text）≈ 2× undo 内存 | buffer.rs:198-199 注释自认「无害堆积」——**评估失准**：不是无害，是 O(累计编辑字节) 泄漏 | 已确认 |
| B-5b | 行级 String 分配密度：`line()` 每次 `text_for_range().collect()`（buffer.rs:1373-1384）、`line_len` = `line().chars().count()`（477-479，为计数分配整行）、`point_to_zoffset` 每换算分配一次行文本（1389-1397）——光标移动/clamp 全链路逐行分配 | buffer.rs 上述行号 | 已确认 |
| B-5c | 无跨帧快照持有、无忽略 edits 事件（因根本未订阅）、无 normalize 误用 | B-2/B-4 | 已确认 |

---

## ③ #164/#205 性能链与残项完整清单（问题 C）

### C-1 历史链（已确认，git 核验）

- #164 实测归因（f78d353 提交原文）：`refresh_caches` 每编辑全文档逐行 `highlight_line_ex` 重建 fence_cache——1.5MB/32K 行逐键 p50=324.78ms、7.5MB/160K 行 p50=1764.69ms（debug 构建）。
- #205 修复（177f98e，经 #214 于 96596d7 合入）：Buffer 侧 `FenceDirty` 三态记账（Clean / Single{line, inserted_lines, pre_lines} / Full，buffer.rs:584-611——所有变更入口含 undo/redo 均落账，连续单区间编辑升级 Full）；Editor 侧 `refresh_caches` 收割（mod.rs:2182-2223）：Clean+缓存有效+长度一致 → **O(1) 复用**（2189-2190）；Single+pre_lines 匹配 → `fence::rebuild_fences_incremental`；否则全量。
- fence 增量算法（fence.rs:159-227）：种子取旧缓存脏行值 → 脏行起 `FenceCursor::advance` 推进 → 旧尾一次 `copy_within` 预移到终位（191-204）→ `j ≥ e_new` 处计算态与预移旧值双等即停机（215-217）。复杂度 = O(编辑行 + 真实传播距离 + 尾部 memmove)；围栏外普通打字**零传播**；奇偶翻转最坏传播到文末（语义上本就该全量，注释给归纳论证 fence.rs:50-53）。**等价性由锚定测试钉死**（增量 ≡ 全量 ≡ 旧 highlight 循环三向对照，fence.rs:314 起）。算法质量：高。

### C-2 残项清单（更新版——R-a~R-d 承笔记 10，R-e~R-i 为本轮新增）

| # | 残项 | 证据（文件:行号） | 触发频度 | 量级 |
|---|---|---|---|---|
| R-a | 渲染帧内全文物化：md 可编辑态 `render_lines` 每次调用 `buffer.to_plain_text()`（在 BracketCache 判缓存**之前**无条件执行；缓存只省重扫不省物化） | mod.rs:4227（缓存结构 mod.rs:297-325）。**笔记 10 记的 4183 已漂移 44 行**（#218 并入后位移） | 编辑器每次重渲（每键/每 notify） | O(doc) 分配 + rope 展平 |
| R-b | display_map 行表全量重建：`build_table` O(行数×折叠数)（逐行 `folds.iter().any` 线性扫） | display_map.rs:332-358；`refresh_caches` 首行无条件 `reset_wraps()` → invalidate（mod.rs:2179） | **每次 after_edit——含纯光标移动键**（mod.rs:2542 无条件收尾） | O(doc)/键 |
| R-b′ | **新**：wrap 回填循环内反复重建——`set_wrap_points` 每回填一行即 invalidate（display_map.rs:102-106），循环内下一次 `row_source` 又触发 `with_table` 重建（display_map.rs:324-330）→ 单帧最多 O(可见行数) 次全表重建 | mod.rs:4160-4198 回填循环 + display_map.rs:90-107/324-330 | 每键后的首帧 | **O(可见行 × doc)**（160K 行 × 50 可见 ≈ 8M 行迭代/帧，debug 构建毫秒~数十毫秒级） |
| R-c | 查找条开启时每次 after_edit 全文扫描（含纯光标移动键） | mod.rs:2224-2226 无条件 `f.refresh`；find_bar.rs:134-157 → buffer.rs:1300-1353 逐行扫描 | find 开启期每键 | O(doc) |
| R-d | IME UTF16 换算 O(doc)：`utf16_to_point` 自文首逐行累加、`point_to_utf16` 逐行分配求和 | mod.rs:4875-4894、4860-4872；IME 每预编辑键 2~4 次调用（4952-4994） | 中文输入每键 | O(doc)/键 |
| R-e | **新**：状态栏每帧**同步全文件磁盘读**（md 态为探编码/EOL `std::fs::read` 整文件）+ git HEAD 读 + `.git` 目录上溯 walk | status_bar.rs:79（fs::read）、status_info.rs:209-240；ShellApp 因 gpui 祖先标脏机制每键重渲（vendored gpui window.rs:2328-2342） | ShellApp 每次重渲（≈每键/每拖拽 move） | O(doc) **磁盘 I/O**/帧——残项中唯一 I/O 类，量级最重 |
| R-f | **新**：字数标签每帧全文物化：`char_label → char_count → to_plain_text().chars().count()`；右栏属性面板同链 | buffer.rs:505-508；status_bar.rs:112；panels.rs:214 | ShellApp 每次重渲 | O(doc) 分配/帧 ×2 处 |
| R-g | **新**：软换行态每键全毁：`reset_wraps` 清空全部 wrap 点（display_map.rs:152-157），下一帧对可见行重新 shape+回填 | mod.rs:2179 ← after_edit ← 每键（含移动键） | 每键 | 可见行重新 shaping + R-b′ 重建链 |
| R-h | fence 增量尾部 memmove O(doc)/键（预移旧尾） | fence.rs:191-204 | 每单区间编辑 | memcpy 级常数，实害低（登记为 nuance） |
| R-i | 多光标/批量编辑走 Full 全量重建（连续单区间升级 Full） | buffer.rs:597（note_fence_single 升级逻辑）、788（批量 Full） | 多光标态每键 | O(doc)/键（#205 有意取舍，语义恒等） |

### C-3 #164 关闭判定

- **根因已除但红线未收口**（高度怀疑 → 需外部环境验证）：#205 只修了 fence 一条 O(doc) 链；R-a + R-e + R-f 叠加意味着**每键仍有 ≥3 次全文物化 + 1 次全文件磁盘读 + O(可见×doc) 行表重建**，7.5MB 文档逐键 <50ms 红线在 debug 构建下几乎必然仍破线；release 构建待实测。
- 复测基础设施缺口：perf_meas.sh 仅 `./target/debug/mang-shell`（perf_meas.sh:76、104）；#205 合入后无 release 端到端复测记录（承笔记 10 §3.3，本轮核实仍无）。
- 修复性价比排序（设计偏好）：R-e（一行缓存/门控即可）> R-a（物化移入缓存判定位）> R-b′（回填循环内批量 invalidate 或表内增量）> R-f（char_count 缓存于 revision 键）。

---

## ④ undo 双层关系与一致性风险（问题 D）

### D-1 双层结构（已确认）

- **活层 = 外壳四元组栈**：`Edit{start, end_del, old_text, new_text}` / `BatchEdit`（字节偏移双坐标域 z_del/z_applied，防批内 Point 漂移，buffer.rs:115-152）；undo 逆推用 `end_after_insert` 重算占位末点（buffer.rs:1070-1079）；批量 undo 单事务 `inner.edit(z_applied 域)`（buffer.rs:943-947）。LIFO 纪律保证 z_applied 恒有效（buffer.rs:939-941 注释论证）。
- **死层 = inner text::Buffer history**：每 edit 落 history + push_undo（vendored text.rs:885-886）永不消费 → 见 B-5a。
- 手势批撤销：多光标 `take_undo_batch`/`add_undo_batch`（multi_cursor.rs:248-260；消费 mod.rs:1236-1243、2474-2491）；查找替换批 = 一条 `Undoable::Batch`（buffer.rs:803）。

### D-2 只读模式与 undo 交互（已确认 + 一处不对称）

- 键盘面：`readonly_allows` 白名单不含 z/y（docx_doc.rs:1101-1118）→ 只读态 Ctrl+Z 整段拒绝；#218 动作级二道兜底 `!action.readonly_safe()`（mod.rs:2356-2363）。
- RPC 面：`cmd_undo`/`cmd_redo`（rpc.rs:1152-1174）**不查 `is_read_only()`**——与 insert/replace 的显式检查（rpc.rs:1189、1221）不对称。现况无害（只读态 undo 栈恒空、undo() 返 false），但这是门禁矩阵缺口（P3，已确认）。
- docx 可编辑态：undo 是「物理回放」不过 EditHook（buffer.rs:236-238 设计自述）——行归属同步靠 UndoHook before/after_history（buffer.rs:324-330）+ after_edit 里 `sync_docx_pending` 兜底（mod.rs:2781→954-963，逐段比对幂等）。「删行→撤销丢标题」的 #115 实坑有防御闭环。

### D-3 一致性风险清单

1. **无合并窗**：每个键入字符 = 一条独立 undo 记录（buffer.rs:663-668 无时间/词边界分组合并）；打完一个 20 字词 Ctrl+Z 需按 20 次。对照 mdcore 1s coalesce（history.rs:20、91-105）——双轨 undo 粒度语义分叉的实证（承笔记 11 P1-1，本轮补齐 B 轨侧完整行号链）。
2. **undo 不恢复选区**：四元组只带光标点（buffer.rs:936、953）；mdcore Entry 带 `selection: Option<Selection>` 快照（history.rs:57）——撤销后选区丢失是 B 轨体验级缺陷（已确认）。
3. `mark_saved` 清空 redo（buffer.rs:545-548）——保存点后 redo 历史永久丢失（多数编辑器同款，登记为语义选择而非缺陷；设计偏好）。
4. 内层历史与外壳栈永不交叉 → 无「两层 undo 打架」风险；代价即 B-5a 内存。

---

## ⑤ 内核代码质量统计（问题 E）

- **模块化**：5,456 行 mod.rs 以注释横幅分区（构造/键分发/多光标/鼠标/渲染/IME），纯逻辑已外迁 13 个子模块——「大文件但非大泥球」；**但 6 个 40 字段构造器逐字重复**（new_text/new_docx/new_docx_editable/new_pdf/new_xlsx/new_canvas，mod.rs:526-573、585-633、662-712、725-772、786-834、847-895，约 420 行样板），字段序已有漂移迹象（new_xlsx 与其余不一致）。
- **unsafe**：editor 目录 **0 处真实 unsafe**（highlight.rs:86 命中为 rust 关键字字符串表；全 mang-shell 仅 native_dialog.rs:248 一处 set_var）。
- **unwrap/expect（产线路径）**：editor 内仅 2 处且均有不变量论证——display_map.rs:329（刚构建即取）、buffer.rs:192（BufferId 非零恒 Ok）；其余 unwrap 全在 `#[cfg(test)]`（mod.rs:5329-5447 等）。对照 mdcore 面（lib.rs 7 处 unwrap）密度相当或更优。
- **panic 面**：`unreachable!()` 一处（mod.rs:1868，docx 烟雾脚本内——非用户路径）。
- **错误处理**：eprintln 诊断流 + `RejectNotice→LockedNotice` 用户可见通道（buffer.rs:372-382 → mod.rs:476-496）；保存失败返回 bool 由调用方提示——无 panic-on-error，风格统一但缺结构化错误类型（设计偏好）。
- **测试资产**：src/editor 内 **321 个测试函数**（buffer 75 / highlight 36 / find_bar 27 / brackets 23 / mod 23 / docx_edit_bridge 22 / multi_cursor 20 / display_map 15 / fence 15 / docx_doc 15 / mouse 17 / xlsx 10 / pdf 9 / commands 7 / buffer_find 7）；产线/测试行数粗分 ≈ 12.3K / 6.3K（首测试标记启发式）。GPUI 运行时测试三件（mod.rs:5325-5455，TestAppContext 全命令执行/状态变化断言）本沙箱不可跑。
- **集成测试面**（tests/）：command_coverage.rs（507 行，静态源码对账——枚举⇔ALL、match 臂无空桩、键位可达、RPC 机器名、面板搜索可达；笔记 19 已覆盖）；module_wiring/perf_wiring/wiring_integrity 为**静态锚点断言**（如 module_wiring.rs:392-401 钉 take_rejection 接线）。**内核面真实缺口**：`refresh_caches` 的 FenceDirty→增量/全量分发只在纯层（fence.rs 单测 + harness-205）有等价性证明，**Editor 层集成无测试**（rg tests/ 无 refresh_caches 命中）；render_lines/styled_runs 渲染口径、display_map 与渲染的回填协议、utf16 IME 链均无 tests/ 级集成覆盖（仅模块内单测）。
- 结论：内核代码质量在仓内属第一梯队（测试密度、unsafe 零、文档注释纪律、缺陷→issue→修复→锚测闭环），短板是 mod.rs 构造器样板与渲染路径零集成测试。

---

## ⑥ 双轨量化对比（问题 F：四件套各自在哪层 + 质量对比）

| 语义件 | B 轨（mang-shell） | A 轨（mdcore 核） | 漂移实证 |
|---|---|---|---|
| undo | buffer.rs 四元组栈（:115-152、904-1025）：无合并窗、无 Source 区分、无选区快照 | history.rs（300 行）：1s coalesce（:20、91-105）、View/Ai/System 源感知破组（R12）、Text/Structural 类型破组、op+逆序 inverses、JSON 可序列化、内存审计 API（R20） | **同一「撤销合并窗」概念两轨两个答案**；AI 编辑与人手编辑在 mdcore 可分批撤销、B 轨不可分（rpc insert 与键入同栈同粒度，buffer.rs:618-686 无 source 参数） |
| 折叠 | display_map.rs folds（:126-192）+ 围栏/标题区间（mod.rs:1537-1606、4714-4808）+ 按路径落盘恢复（mod.rs:1662-1711）≈ 700+ 行 | **无折叠模块**（rg fold 仅 html.rs:1299 `fold_inline`=HTML 空白折叠转义、fingerprint.rs 测试名——语义无关） | B 轨独有能力；A 轨收敛时需保留或补建 |
| 围栏 | fence.rs（706 行）：``` 与 ~~~、≤3 前导缩进（highlight.rs:166-235 opens_fence）、语言标注（CodeLang::parse）、#205 增量算法 + 三向等价测试 | markdown.rs:20-28 `is_complete_chunk` + :103 起 tree 化：**仅 ``` 前缀翻转奇偶**（v0 自认不查闭合行 info string） | 同一文档 chunk 边界两轨可判不同：如缩进 4 空格的 ```` ``` ```` 或 `~~~` 块——B 轨认为开栏、A 轨不认（markdown.rs:26 `starts_with("```")`）→ 渐进打开的 R10-a 完整性判定在两轨不等价 |
| 字数 | buffer.rs:505-508 `char_count = 全文 chars().count()`——**含空格/换行/md 语法符号**，且每调全文物化 | word_count.rs（541 行）：CJK 精确两段码位（URO+ExtA）、西文词、净文本投影（剥行内标记）、段落/标题/表格分计、Python 参考实现双背书 | **口径完全不同——同一文档两轨字数必然不等**（「标书评审口径」只在 A 轨成立）；B 轻量但语义粗糙且 O(doc)/帧 |

- 规模对比：mdcore 全核 8,786 行 vs mang-shell editor 18,547 行（内核本体 14,003）——**md 编辑语义（undo/查找/围栏/高亮/折叠/光标）在 B 轨整体重写了一遍**。
- **零接触面确认（已确认）**：mang-shell Cargo.toml 无 mdcore 依赖（rg 零命中）；源码内 "mdcore" 仅 3 处品牌字符串（panels.rs:240、shell.rs:916、status_bar.rs:183）。无任何 trait/适配器桥接点。
- **关键反证**：B 轨对 docx **有**桥（EditHook/UndoHook → office-io 引擎，buffer.rs:333-338 / mod.rs:903-916）——证明桥接模式在本仓已被走通，「md 面无桥」不是能力缺口而是架构选择（r42 抄 Zed 设计自研的路径依赖）。
- 双倍维护税具体化：围栏语义已三处实现（fence.rs / highlight.rs opens_fence / mdcore markdown.rs）；#205 的增量算法、#151 的行首态语义、#164 的性能教训——任何围栏语义演进（如 CommonMark 严格闭合）需三处同步改且无共享测试基。

---

## ⑦ 新问题清单（P0-P3 + 置信）

| 级 | 问题 | 证据 | 置信 |
|---|---|---|---|
| P1 | 状态栏每帧同步全文件磁盘读 + git HEAD 读（md 态）——每键/每 notify 一次 `std::fs::read(整个文档)`，大文档下磁盘 I/O 主导帧耗时；网络盘/慢盘下可感知卡顿 | status_bar.rs:79；status_info.rs:209-240；gpui 祖先标脏 window.rs:2328-2342 | 已确认 |
| P1 | 每键 ≥3 次全文物化：render_lines to_plain_text（mod.rs:4227）+ 状态栏 char_count（status_bar.rs:112）+ 属性面板 char_count（panels.rs:214）——7.5MB 文档 ≈ 每键 22MB+ 堆分配 | 上述行号 + buffer.rs:457-459/505-508 | 已确认 |
| P1 | wrap 回填循环 O(可见行×doc) 行表重建（R-b′） | mod.rs:4160-4198 + display_map.rs:324-330/102-106 | 已确认 |
| P2 | 纯光标移动键也触发 reset_wraps 全毁 + 行表全量重建 +（find 开时）全文扫描——移动键本应近零成本 | mod.rs:2542（无条件 after_edit）→ 2179 → display_map.rs:152-157；mod.rs:2224-2226 | 已确认 |
| P2 | inner text::Buffer 历史无界堆积 + undo 双存储（B-5a）——长会话（如小时级连续编辑 10MB 文本量）内存无上界增长 | vendored text.rs:885-886；buffer.rs:198-199 | 已确认（增长量级需外部环境验证） |
| P2 | #164 红线未收口：R-a/R-e/R-f/R-b′ 未修前逐键 <50ms（7.5MB）大概率仍破线；perf_meas.sh debug-only、无 #205 后 release 复测 | ③C-3 全链 | 高度怀疑→需外部环境验证 |
| P2 | Editor 层 fence 增量分发零集成测试（纯层有等价证明，Editor 接线面无回归保护） | rg tests/ 无 refresh_caches；fence.rs:62 注释自认「只测面引用」 | 已确认 |
| P3 | RPC undo/redo 缺 read_only 门禁（与 insert/replace 不对称） | rpc.rs:1152-1174 vs 1189/1221 | 已确认 |
| P3 | IME utf16 换算 O(doc)/键 | mod.rs:4875-4894 | 已确认 |
| P3 | 6×40 字段构造器样板重复（漂移风险） | mod.rs:526-895 | 已确认 |
| P3 | undo 无合并窗/无选区恢复（UX 语义，双轨分叉面） | ④D-3.1/.2 | 已确认 |
| P3 | line_len/line/point_to_zoffset 逐调用行分配（微开销面） | buffer.rs:472-479、1389-1397 | 已确认 |
| — | 修复顺序建议：R-e 缓存 → R-a 移缓存判定位 → R-b′ 批量失效 → R-f char_count 挂 revision 键 → reset_wraps 改脏行级失效（可借 #205 FenceDirty 同款记账模式） | — | 设计偏好 |

---

## ⑧ T7 收敛视角的资产盘点（问题 G：A 方案下哪些留、哪些弃）

### 可保留资产（gpui 耦合层 / mdcore 无对应物 / 桥接模式）

1. **渲染管线全套**：render_editor_area→uniform_list→render_lines→styled_runs_for_line（mod.rs:3711-4555）+ 行 bounds hit-test + 光标/选区/查找命中绘制——mdcore 无渲染面，全量保留。
2. **display_map**（678 行）：fold+wrap 双变换、行表缓存——A 轨无折叠；收敛后 mdcore 需补「折叠语义」时此实现是现成参考或直接保留层。
3. **multi_cursor / mouse / IME EntityInputHandler**：编辑器层交互，与语义核正交。
4. **keymap/commands/palette 分发面**（#217/#218 表驱动 + 三入口单点 exec_command）——动作层与语义层已解耦，换核不动。
5. **fence.rs 增量算法**：FenceCursor::advance 纯函数 + 种子/停机/memmove 技巧——**可反向吸收进 mdcore**（A 轨 fence 面若需性能化，这是仓内已验证的最优解，含三向等价测试）。
6. **EditHook/UndoHook 桥接模式**（buffer.rs:287-338）+ docx_edit_bridge 的落地经验——**这就是 mdcore 桥的现成契约蓝本**：before_edit 拒改门禁 / after_edit 摘要重映射 / before-after_history 行数同步，mdcore 核替换 inner 的同一位置可复用。
7. **perf 门控桩 + 烟雾证据链纪律**（perf.rs + tests/perf_wiring.rs）。
8. Buffer 公开 API 面（41 语义测试锚定的契约，buffer.rs 头注释）——收敛时作为适配层接口保持不变、内部换 mdcore。

### 必弃 / 必替换资产（语义层与 A 轨重复且劣化）

1. **undo 四元组栈**（buffer.rs:115-162、904-1025）→ 换 mdcore history（拿合并窗/源感知/选区快照三件 B 轨缺的能力）。
2. **char_count 字数口径**（buffer.rs:505-508）→ 换 mdcore word_count（标书口径单一真源）——顺手消灭 R-f。
3. **围栏/标题判定**（fence.rs + highlight.rs opens_fence + mod.rs heading_level_of）→ 统一走 mdcore markdown tree 化语义（消灭三处围栏实现漂移）；增量缓存算法仍可作缓存层保留。
4. **Buffer::find_matches 家族**（buffer.rs:1286-1361、buffer_find.rs）→ 可短期保留（查找非 mdcore 面貌），长期看 mdcore 若出查找 op 则并入。
5. **光标/选区/粘性列外壳态**（buffer.rs:1093-1272）→ mdcore anchor/Selection 面（anchor.rs 已有 249 行锚点系统未被 B 轨消费）。

### 收敛形态（设计偏好）

- 换芯路径与 r42 相同形状：`Buffer` 内部 `inner` 从 `zed_text::Buffer` 换成 mdcore Document/History 适配体，公开 API 与 41 项测试契约不动；EditHook 位置改造为 mdcore op 门禁。**最大阻力不是接口而是渲染增量的数据源**：B 轨现靠「全量失效+懒重建」，mdcore op 流（precise ChangeRecord 形态已存在于 doc_events）可天然喂 display_map/fence 增量——恰好是消灭 R-b/R-b′/R-g 的正解。

---

## 结论

1. mang-shell 编辑器内核是**高质量的自研孤岛**：Zed text::Buffer 集成方式正确（无快照误用/无事件误用）、unsafe 零、测试密度仓内第一、#205 修复是教科书级；但其「语义层全自研 + 存储层半弃用」的结构使 Zed 栈的增量事件红利全部弃收，换来自研全量失效重建（R-b 家族）。
2. #164 修复后**每键 O(doc) 并未真正终结**：本轮新增 R-e/R-f/R-b′/R-g 四项（含最重的每帧磁盘读），逐键红线的收口清单从笔记 10 的 4 项扩至 9 项。
3. 双轨税已被四件套逐项实证（undo 粒度、围栏边界判定、字数口径、折叠单边）；B 轨对 office-io 有桥而对 mdcore 无桥，桥接模式本身已在仓内验证可行——T7 若裁 A 方案，可保留面（渲染/display_map/命令分发/交互/桥契约）远大于必弃面（undo/字数/围栏判定/光标态），且 mdcore op 流恰好是 B 轨渲染增量化缺的那块拼图。
