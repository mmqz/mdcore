# 11 · 横切代码质量审计（unsafe / panic 面 / 内存 / 错误 / 并发 / 体积 / 抽象 / 依赖）

- 审计对象：`/home/z/my-project/mdcore-audit` main@96596d7（只读）。覆盖 16 个 workspace crate + mang-shell（交叉引用级抽查，非其专项）；third_party/zed 为 vendored 闭包不计入。
- 方法：独立重计（python3 静态切分产品码/测试码/文档注释）+ 逐站点源码核验 + 调用路径推演。不重复分域笔记 01~06 的结论，仅引用与交叉验证。
- 与 loc-stats 的口径差：本笔记把 `unwrap/expect/panic!/unreachable!/todo!/unimplemented!` 统称 **panic 站点**，并剔除文档注释里的字面命中（loc-stats 未剔除——office-io 的 "616 unwrap" 里含 doc 注释提及与 doctest 示例，见 §2.1）。

---

## 0. 风险排序总表（横切面）

| # | 级别 | 发现 | 一句话 | 置信 |
|---|---|---|---|---|
| R1 | **P0（继承）** | 敌意文件可达的硬 panic 唯一真实来源 = **docx-rs 依赖层**（4 处真实语料实锤）+ native 消费面无 catch_unwind | 打开即崩的入口在 `docx_doc.rs:171` 只 map_err | 已确认（05 号笔记 P0-1 + 本轮复核消费面） |
| R2 | **P1** | `apply_ops` 每事务整树 clone（R22-b 登记未治理）——逐键 O(全树) 分配，10MB→wasm 425MB 高水位主驱动 | 修复路径明确（§3 给两档方案与工作量） | 已确认（gates.md R22-b + 本轮结构分析） |
| R3 | P2 | 锁中毒策略两极分化且无恢复面：mang-shell rpc 控制面 5 处 `expect("锁中毒")` 级联；wasm `lock().unwrap()` + 无 panic hook（B-4） | 一次 panic 持锁 = 控制面/事件面永久死亡 | 已确认（机制）|
| R4 | P2 | mdcore 错误面系统性落后于 office-io/pdf：Reject/OpError/TreeError/AnchorError 无 Display、`From<OpError>` 降级 Debug 串、`replay_jsonl` 返 String、undo/redo Option 吞错 | 03 号笔记 §7 的横切确认 + 全仓横评定级 | 已确认 |
| R5 | P2 | 体积热点全在最活跃代码上：mang-shell `editor/mod.rs` 5412 行单结构体 + `main.rs` 711 行 main() + `rpc.rs` 3071 行；office-io `docx.rs` 8242 行 | 12 个 >2000 行文件清单见 §6 | 已确认 |
| R6 | P2 | 重复实现量化：XML 文本转义**三源**（office 消毒库未接线 + docx_edit + xlsx_edit 逐字节相同）；fwd_to_* 扫描器 30 LOC 双拷贝；Excel 列字母编解码 **≥8 处 / 5 crate**；collab↔canvas 镜像类型 ~250+ LOC | 无共享 util crate 的结构性税 | 已确认 |
| R7 | P2 | workspace.dependencies 单源纪律只覆盖 serde/serde_json 两项，且 collab:33 / office-io:37 已**违规自写** `serde_json = "1"`；根锁 42 个重名版本几乎全部由**休眠的 shell-native**（wry/jni/ndk 链）拖入 | 活跃 15 crate 的依赖图本身干净 | 已确认 |
| R8 | P3 | 全仓 5 处 unsafe 全在 mang-shell **测试代码**内（native_dialog.rs:248-264，PATH 进程级突变）——与同二进制内 `git_command_surface_on_this_repo`（git_panel.rs:544）存在并行测试互踩窗口 | 产品码 unsafe = 0（全仓）| 已确认（机制），触发概率低 |
| R9 | P3 | edition 漂移：mang-shell 2024 vs 15 crate 2021（unsafe env 语义差异的直接成因）| 工具链 1.99.0 已钉 | 已确认 |
| R10 | P3 | pdf/stamp.rs 自研 PNG 解码器（inflate+unfilter+五色型）直面敌意 PNG 字节——本轮核验其守卫完备，但无 fuzz 门 | 回归敏感面 | 高度怀疑（守卫已核、未 fuzz）|

**总体判断**：这不是"弥散式质量问题"仓库。产品码 panic 站点 148 个（vs 测试码 4,946 个）且逐一带不变量注释；零生产 unsafe；锁/线程模型处处有诚实边界文档。质量问题**集中且大多已被自己登记**：一个上游依赖崩溃面（R1）、一个未治理的内存根因（R2）、双轨与重复实现的结构税（R5/R6）。

---

## 1. unsafe 全景（任务 1）

### 1.1 事实修正：产品码 unsafe = 0（比 loc-stats 口径更强）

loc-stats 记 unsafe=5（mang-shell 5、其余 crate 0）。逐处定位（rg 全仓 + 人工核验）：

| 站点 | 位置 | 判定 |
|---|---|---|
| ①-⑤ | `mang-shell/src/native_dialog.rs:248/249/253/259/264` | **全部位于 `#[cfg(test)] mod tests` 的单个测试函数 `show_error_with_controlled_path`（:236 起）内**，内容 = `std::env::set_var/remove_var("PATH", …)` |

- mang-shell 是 edition **2024**（mang-shell/Cargo.toml:18），env 突变在该 edition 下为 unsafe；15 个 workspace crate 均为 2021（crates/*/Cargo.toml:4），故仅此文件出现 unsafe 字面。
- `crates/pdf/src/cache.rs:5` 的 "unsafe" 命中是模块文档自述"零 unsafe"，非代码。
- **引擎域零 unsafe 的声称成立，且可强化为：16 crate + mang-shell 的全部产品代码零 unsafe**。unsafe 只存在于 vendored third_party/zed（gpui_macos/window.rs 147 处等，仓外闭包，另案）。

### 1.2 五处 unsafe 的风险评估（R8，P3）

调用路径：`show_error_with_controlled_path` 为验证 #185 原生对话框送达判定，把进程级 `PATH` 短暂指向空临时目录（:253-264 三次突变 + :248-249 恢复闭包）。风险不在 unsafe 本身而在**测试并行互踩**：

- 同一 lib 测试二进制内，`git_panel.rs:544 git_command_surface_on_this_repo` 断言 `git_available()`（git_panel.rs:52 经 PATH spawn `git`）。若两测试并行且 PATH 窗口（native_dialog.rs:253→268，含 3 次 spawn+wait 与 2 次文件写）覆盖 git spawn 时刻 → `git_available()==false` → 断言 "沙箱有 git" 假失败。
- `tests/docx_bid_e2e.rs:316` 的 soffice spawn 属**另一个测试二进制**（integration test 独立进程），不受影响。
- 缓解已部分在位：恢复闭包在断言前执行（:268）。剩余修复选项：`--test-threads=1` 之外的正统解是 `serial_test` crate 或改用子进程 env 注入（`Command::env("PATH",…)`）。
- 定级 P3：机制已确认，触发窗口毫秒级，后果 = 偶发 flaky。

---

## 2. unwrap/expect/panic 风险分级（任务 2）

### 2.1 独立重计（剔除文档注释/doctest；含 panic!/unreachable!/todo!/unimplemented!）

| crate | 产品码 panic 站点 | 测试码 | 备注 |
|---|---|---|---|
| mdcore | 11 | 474 | 全部不变量守卫（§2.3） |
| mdcore-wasm | 54 | 319 | 20 处在 doc-hidden 夹具构建器（§2.4） |
| office-io | **23** | 1,726 | 全部守卫（§2.2） |
| office | 4 | 23 | xml_control 字符边界不变量 |
| pdf | 16 | 793 | worker/缓存/解码器（§2.5） |
| pdf-wasm | 5 | 21 | 4 处测试面命令构造辅助 + serde_json |
| formula / relay | 0 | 67/50 | 干净 |
| evidence | 2 | 129 | sidecar 守卫 expect |
| workbench | 2 | 282 | 同上 |
| collab | 3 | 341 | events.rs:164 构造期 fail-fast panic! |
| canvas | 2 | 169 | 同构 |
| mdbridge / shell / settings | 1/1/2 | — | 守卫 |
| shell-native | 8 | 5 | bin main 启动期 expect ×7 |
| mang-shell | 14 | 366 | rpc 控制面锁中毒 ×5（R3） |
| **合计** | **148** | **4,946** | **97% 在测试码** |

**"office-io 616 unwrap 中 95% 在测试"的独立复核：保守了。** loc-stats 的 616 含文档注释字面（如 docx.rs:55、:3655 的 "todo!()" 是注释提及 docx-rs 的 instr_text.rs:32；numbering.rs:267-271 五连 unwrap 在 ``` 文档示例内）。剔注释后：src 内 panic 站点 23 个产品 / 1,726 测试 = **1.3% 产品 / 98.7% 测试**。05 号笔记的逐文件拆分（docx.rs 产品 unwrap=2 等）与本次重计吻合。

### 2.2 ① 敌意文件可达类（office-io 解析 / pdf 读 / formula）——top 风险

**结论先行：office-io 自研代码在敌意文档路径上的可达 unwrap ≈ 0；真实崩溃面全在 docx-rs 依赖层（R1，P0 继承）。**

1. **R1（P0，已确认）docx-rs 读路径 4 处 panic + native 无隔离**。触发构造与证据链见 05 号笔记 §5.3（`runprops_word365_props-{b,enum,u}-styles.docx` → docx-rs run_property.rs:110/155 越界；`strict_strict-sample-docx.docx` → width.rs:18 unwrap）。本轮补核**消费面放大器**：mang-shell `docx_doc.rs:171/208` 仅 `map_err` 不 catch_unwind → native 编辑器打开即整进程崩；wasm 面 = trap（但有 t16/t17 自由函数隔离 + JS catch，04 号笔记 §7）。修复 = 读前置预检或 catch_unwind 包装 `read_bytes_gated`（docx.rs:1799 门后直达引擎），**工程量小、收益直接**。
2. **R10（P3，高度怀疑）pdf/stamp.rs 自研 PNG 解码器**（stamp.rs:120-250：手写 chunk 走查 + inflate + 五过滤器去滤波 + 色型归一）。守卫逐项核验**当前完备**：chunk 越界拒（:120-123）、零尺寸拒（:145）、Adam7 拒（:148）、位深限 8/16（:151）、color type 白名单（:158-163）、调色板缺 PLTE 拒（:163）、解压长度下界（:171）；`alpha.as_mut().unwrap()`（:209）由 trns 初始化不变量守卫（:196-200）、`palette.unwrap()`（:234）由 :163 守卫、`unreachable!`（:244）由 :158 穷尽守卫。风险性质 = **无 fuzz 门的自研解码器**（全仓无 PNG fuzz 载荷），建议登记 `cargo-fuzz`/语料门而非重写。
3. pdfium C 侧段错误**诚实声明不可捕获**（worker.rs:13-14 模块文档），进程隔离留给 PDF-e——非本层缺陷。
4. office-io 23 个产品站点逐一核验为守卫不变量：xlsx_edit.rs:963/978（`scanned.as_ref().unwrap()`——惰性扫描已触发）、:1072（peek 后 next）、:1220（刚 collect 的 key remove）；io.rs:1036-1037（非空 Range 的 start/end）；docx_edit.rs:1782（note 手术区间不相交已验）、:2121（is_text 蕴含 Some）；docx.rs:4608/5361（循环保证非空/前置拒）；numbering.rs:1093（空拒前置）。formula：0 站点。**敌意 XLSX 路径（calamine/quick-xml 面）自研零裸 panic**——与 06 号笔记三门结论互证。

### 2.3 ② 用户输入可达类（wasm JSON 面 / web 桥）——top 风险

1. **mdcore-wasm 自身 ≈ 0**。54 个产品站点拆解：xlsx_import.rs:750-810 与 docx_import.rs:802-833 的 expect 簇全部位于 `#[doc(hidden)]` 夹具构建器 `build_sample_xlsx/build_sample_docx`（非 wasm 导出，"产线单一真源"测试夹具，失败=写 API 回归且被 4 组测试即时捕获）；xlsx_grid.rs:105-131 等 12 处 = `serde_json::to_string` 对 String/bool/f64/枚举（实践不可失败）；canvas.rs:239-321 / collab.rs:441-606 / workbench.rs:297-313 同类；docx_import.rs:578 `unreachable!`（TextBox 两趟定点展开，04 号已核）；xlsx_import.rs:343 `expect("ASCII")`（b'A'+col%26 恒 ASCII）。
2. **敌意 JSON 的类型层防线实例**：`PageSpan` 自定义 Deserialize 校验 start≤end（pdf/src/page.rs:118-127）→ pdf-wasm `execute_core`（lib.rs:116-122）对倒序页码区间返回 typed 错误而非 panic；pdf-wasm lib.rs:149 的 `expect("span")` 在**非导出的测试面辅助**（`extract_text_cmd`，"命令构造辅助（测试面用）"）。这是"构造即校验"的正确样板。
3. **残余风险 = 锁中毒级联**（并入 R3）：canvas.rs:511 / collab.rs:644 `events.lock().unwrap()`——单线程 wasm 下仅当先前 panic 在持锁期间展开才毒化；叠加无 `console_error_panic_hook`（B-4，04 号笔记）= 毒化后事件面永久 trap。可达性：需先有一次 panic（≈0）+ 恰在 `take_events_json` 临界区内。
4. mang-shell 14 个产品站点：rpc.rs:581/587/865 `subs.lock().expect("订阅表锁中毒")`（控制面级联，见 §5）；:931 `unreachable!("subscribe 在 dispatcher 侧特判")`（match 穷尽性出口，文档在案）；:1809 正则组 0 恒在；docx_doc.rs:488-489 在 `#[allow(dead_code)]` 测试入口；buffer.rs:192（BufferId 非零恒 Ok）/796（非空已判）；display_map.rs:329（ASCII 列字母）；mod.rs:1864 / xlsx_doc.rs 同类（smoke 模式守卫 unreachable）。

### 2.4 ③ 测试专用类

4,946 个站点全部无害（含根目录夹具、内联 `#[cfg(test)]`、tests/ 目录）。**值得单独点名的好实践**：全仓 expect 消息几乎无一例外是**自文档化不变量**（如 `expect("map 命中槽必有条目（evict 与 map 移除同步）")` pdf/cache.rs:135、`expect("脏标注必在库（v0 无删除命令）")` pdf/document.rs:328）——panic 面审计成本因此大幅降低，这在同类仓库中罕见。

---

## 3. 内存与 Clone（任务 3）

### 3.1 R2（P1）：apply_ops 整树 clone——根因结构与修复方向

**调用路径**：web 快路径每键 → `WasmCore::apply_ops`（mdcore-wasm lib.rs:88-98）→ `Core::apply_ops`（mdcore lib.rs:283）→ `backup = self.tree.clone()`（:297）→ 逐 op 求逆+应用 → 成功后 `ops.clone()` 入历史（:338/:352）。同构站点：undo/redo `apply_raw_batch`（:380）+ **双份 inverses clone**（:908-909）/ 双份 ops clone（:940-941）；replay 每事务 clone（:1015）。

**成本结构**（tree.rs:97-103 + Node :47-56）：`BlockTree = Vec<Option<Node>> + BTreeMap<BlockId,NodeId> + IdAllocator`。Ropey Rope clone 本身近 O(1)（内部分享），真正的每键成本 = **slots arena 深拷贝**（每 Node 的 attrs BTreeMap / children Vec / marks Vec 各一次堆分配）+ id_index BTreeMap 整克隆 → O(全树节点数) 次分配/拷贝，与编辑大小无关。10MB 渐进开档 16 段 × 每段 System 事务 + 逐键 View 事务 = 分配器高水位 425MB（gates.md R22-b 原文登记；R21 先证 -83.8MB 来自 ingest 退出 undo 栈，R22-a Arc 共享仅 Δ-0.6MB 噪音级）。

**修复方向两档（建议 B 先行）**：

- **方案 B（受影响面备份，1-2 天）**：`Kind::Text` 事务（99% 击键 = 单块 ReplaceText）只备份目标 Node 的 `(text, attrs, marks)` 三元组 + 失败回写；`Kind::Structural` 与 System/adopt 路径保留整树备份。零语义风险（回滚面与现行完全一致），直接消灭逐键 O(N)——W4 p95 13.5ms 与高水位同时受益。
- **方案 A（求逆回滚，2-4 天 + ADR）**：失败时逆放已收集的 `inverses_seq` 前缀。前置条件 = `apply_op` 七个 op 改"先全量校验后变异"（当前如 InsertBlock 先 create_node 后 insert_child，半应用态存在）——这正是 gates.md R22-b "回滚正确性需 ADR 先行"的实指。回滚自身失败 = 硬不变量破裂，fail-fast。收益 = 全路径（含 undo/redo/replay）零备份；`replay_jsonl` 还应把 backup 提到循环外。
- 不建议方案 C（CoW/persistent arena 全重构）：收益与 B/A 重叠，扰动 18 模块 + snapshot/external。
- **量化预期**：B 落地后逐键分配 O(1)；wasm 高水位主项转为 log/events/历史条目（R21 已治理 ingest 面）——预期 425MB 显著回落，但需 `__heapTrace` 复测确认（需外部验证）。

### 3.2 其余热路径 clone 扫描（结论：mang-shell 编辑器干净，问题只在 mdcore）

| 路径 | 实测 | 判定 |
|---|---|---|
| mang-shell `display_map.rs` / `highlight.rs` / `brackets.rs` | **0 个 `.clone()`** | 渲染/折叠/高亮热路径零 clone（行数据走借用） |
| mang-shell `buffer.rs`（2583 行） | 13 处：undo 四元组记录（:721/:734/:926/:945/:990/:1004，随编辑量线性、必然）+ `clone_buffer` 显式 API（:261）+ 测试 4 处 | 健康 |
| mang-shell `fence.rs` / `multi_cursor.rs` | 10/1 处，多数在测试 | 健康 |
| 渲染视图 docx_doc(17)/xlsx_doc(1)/pdf_doc(3)/docx_edit_bridge(3) | 装配期 String 化，非每帧 | 健康 |
| mdcore 非 lib 文件 | word_count/pagination/markdown 全 0；op.rs 16 处 = 求逆/物化的必要小拷贝；fingerprint 8 处（匹配面一次性） | 健康 |

**横切结论：全仓系统性 clone 债只有一个——mdcore apply_ops/undo/replay 的整树备份**；mang-shell 自研编辑器栈在内存纪律上反而是优等生。

---

## 4. 错误处理设计横评（任务 4）

### 4.1 各 crate 错误体系分级（统一性 × 可诊断性）

| 梯队 | crate | 形态 | 证据 |
|---|---|---|---|
| **A（模范）** | office-io | 四族 typed 枚举 + 手写 Display + `std::error::Error` + `From` 链：`ReadError`（io.rs:139，门拒绝/容器/解析/部件四变体，reason 内嵌门编号）、`WriteError`（write.rs:162）、`DocxError`（docx.rs:224，Container/Parse/Io/Limit/InvalidInput 五变体）、`XlsxEditError`（xlsx_edit.rs:120，含 `FormulaCell{sheet,cell}` 精确坐标） | 逐文件核验 |
| **B（良好）** | evidence / workbench / collab / canvas / mdbridge / settings / relay / office / formula / pdf（部分） | 均有 typed 枚举 + Display（手写）；pdf 的 CommandError/签名/表单/邮戳有 Display，但 `OpenError`（document.rs:71）/`PageSpanError`（page.rs:101）/`GenerateError`（generate.rs:38）无 Display | rg 清单 |
| **C（落后）** | **mdcore** | `Reject/OpError/TreeError/AnchorError` 全部无 Display/std::error::Error；`From<OpError> for Reject` 把结构化错误降级为 `Invalid(format!("{e:?}"))` Debug 串（lib.rs:111-115）；`replay_jsonl -> Result<Self, String>`（lib.rs:996）；对照同 crate 的 `SnapshotError`/`DocumentIdError` 已做对——内部不一致 | 03 号笔记 §7-1 本轮确认维持 |
| 旁注 | wasm/JS 边界 | `{ok:false, code, message}` 统一形状（web/src/main.ts:1876-1888）——桥面诊断性反而好于核面 | 正面 |

**统一性判断**：仓内**无 thiserror 依赖**（office-io Cargo.toml:50 的命中是供应链注释），全部手写 impl——风格统一但 ~30 个枚举的样板成本可观；错误家族按 crate 各自演化，跨 crate 无共享错误原语（如"容器门拒绝"概念在 office-io 专属）。mdcore 作为被三域镜像的核心（pdf Source 镜像锁、mdbridge 翻译），其错误面落后是**横切放大**的：mdbridge/translate.rs 需自建 BridgeError 映射、pdf 只能镜像五值而无法映射 reject 语义。

### 4.2 吞错点清单（失败原因丢失）

1. **undo()/redo() -> Option\<Receipt\>**（mdcore lib.rs:905/937；wasm lib.rs:101-105）——空栈与"历史树不一致致 apply 失败"同返 None（03 号笔记问题 3，P2 维持）。宿主只能绕道 revision 不前进判 no-op。
2. `take_events` 序列化失败静默降级 `"[]"`（mdcore-wasm lib.rs:116）、`word_count_json` 降级 `"{}"`（:146）、`serialize_reject` 降级 `"invalid"`（:251）——serde_json 对这些类型实践不可失败，属防御性兜底，但事件面若真失败 = 证据断流不可见（与 B-6 同族）。
3. mang-shell `doc_events::publish` 未初始化时**静默 no-op**（doc_events.rs:44 模块文档自述）——首事件丢失设计上有意，可诊断性无。
4. web 桥 `drainEvents/ingestExtend` 吞坏 JSON（04 号笔记 B-6，维持）。

---

## 5. 并发与锁（任务 5）

### 5.1 全仓锁/线程清单

| 单元 | 原语 | 用途与评价 |
|---|---|---|
| **mdcore** | **零**（单写者 `&mut self`）| 最干净；Core 无 Send/Sync 承诺，一致性由 REUSE §5-5 锁定 |
| **pdf/worker.rs** | `Arc<Mutex<WatchState>>` + 每代 2 线程（渲染工人 + 看门狗）+ mpsc 双通道 | **全仓最强并发工程**：catch_unwind panic 隔离（Rust 帧展开、C 侧诚实声明不可捕，:9-14）、看门狗换代（超时→毒化→工厂重建→在飞重放，:15-30）、取消令牌只作用于排队面（:31-35）、回执合同"每 id 至多一条"（:40-44）、drop join 当前代不悬挂（:36-38）。期望值站点（:174/:456/:463 spawn fail-fast、:579 队列不变量） |
| pdf/engine_pdfium.rs | `static PDFIUM: OnceLock`（:76）+ `BIND_LOCK: OnceLock<Mutex<()>>`（:80，:362/:375 绑定串行化）| pdfium 进程级库的正确姿势 |
| mang-shell/rpc.rs | `Subscriptions = Mutex<HashMap<u64, async_channel::Sender>>`（:577）+ async-channel/async-net/futures-lite 单线程 executor | 锁内短临界区（clone+try_send 非阻塞，:574 注释）；**毒化策略 = expect 级联**（R3） |
| mang-shell perf.rs/settings.rs/doc_events.rs | `OnceLock`（MANG_PERF 门/字体/事件总线）| 无风险 |
| collab/canvas | `Arc<Mutex<HashMap<u64, Arc<Mutex<Fn>>>>>`（events.rs:98/presence.rs:109 注册表）+ `Arc<Mutex<Vec<u8>>>` 收集器（events.rs:144）+ canvas `Arc<Mutex<BTreeSet>>`（events.rs:155）| 双层锁刻意设计：注册表层短锁 clone 槽位表，回调在槽位锁内调用——规避"回调持注册表锁"死锁。构造期 `observe_update_v1` 失败 = fail-fast panic!（events.rs:164/canvas :134） |
| mdcore-wasm canvas/collab | `Arc<AtomicU64>` 时钟 + `Arc<Mutex<Vec<String>>>` 事件池 + `.lock().unwrap()`（canvas.rs:511/collab.rs:644）| wasm 单线程下防御性；毒化级联风险并入 R3 |
| relay | 纯状态机无锁（传输无关）| — |

### 5.2 canvas"并发测试"的真实并发度（任务点名）

`crates/canvas/tests/concurrency.rs`（291 行）与 `crates/collab/tests/concurrency.rs`：**零 `thread::spawn`**——全部是同线程内两会话的**确定性交错模拟**（`sync()` 辅助函数 = encode_diff/apply_update 顺序调用，concurrency.rs:16-20；"并发"指两个 Actor 的逻辑并发编辑，:45-74）。这是测 CRDT 收敛的正确姿势（确定性、可复现），但**不测数据竞争**——CanvasSession/CollabSession 本身按单线程所有权建模（`&mut self` execute），真正的跨线程面只有 pdf worker。表述建议：仓内文档若称"并发测试全绿"，其语义覆盖 = 逻辑收敛 + 锁无死锁（间接），不含真并行压力。

### 5.3 R3（P2）：锁中毒策略横评

- mang-shell rpc.rs:581/587/865 `expect("订阅表锁中毒")`：任何一次持锁 panic（广播闭包内）→ 此后**每个 RPC 调用**（27 命令 + doc_changed 推送）都 panic → AI/SDK 控制面死亡至重启。fail-fast 可辩护，但控制面是全 shell 的命脉，建议改 poison-aware 重建（HashMap 可整体重建，无不可恢复状态）。
- mdcore-wasm canvas.rs:511/collab.rs:644 `lock().unwrap()`：同族但后果 = 事件面 trap；叠加无 panic hook（B-4）→ 无诊断。
- pdf worker 的 WatchState 锁**明示"锁内不可 panic，无中毒面"**（worker.rs:104-105）——同一问题三种处置，建议统一为一条策略文档行 + wasm 接 hook（04 号 B-4 行动项维持）。

---

## 6. 模块体积与内聚（任务 6）

### 6.1 >2000 行 src 文件清单（12 个；office-io 无 mod.rs，最大为 docx.rs）

| 文件 | 行数 | 内聚判读 |
|---|---|---|
| office-io/src/**docx.rs** | **8,242** | 读模型+块提取+域状态机+VML 二趟+写 API+zip 后处理+图片/批注/脚注六面混装——**单文件即一个子域**；拆读/写/二趟扫描三件可立即降半 |
| mang-shell/src/editor/**mod.rs** | 5,412 | Editor 上帝结构体：渲染(render_lines 307L/render_find_bar 220L)+键路(dispatch_key_inner 239L)+canvas 拖拽+格式面+右键菜单+测试(:4986+)——**最活跃文件的最大内聚债**（106 commits/2 周的燃烧中心） |
| office-io/src/docx_edit.rs | 3,853 | 手术编辑器（scan_paragraphs 402L 单函数）|
| mdcore-wasm/src/docx_export.rs | 3,423 | #196 样式面活跃 |
| mang-shell/src/rpc.rs | 3,071 | 27 命令+分发+协议+订阅+连接管理单文件 |
| mang-shell/src/editor/buffer.rs | 2,583 | 缓冲+undo+选区（模块文档优秀）|
| canvas/src/render.rs | 2,473 | 渲染模型+黄金例 |
| office-io/src/io.rs | 2,407 | 容器三门（职责单一，体积可忍）|
| mang-shell/src/shell.rs | 2,284 | 壳骨架（render 316L）|
| office-io/src/style.rs | 2,100 | 样式树+主题 |
| mdcore-wasm/src/docx_import.rs | 2,071 | 导入投影 |
| pdf/src/organize.rs | 2,025 | 页面组织 |

### 6.2 超长函数 top5（产品码）

1. `mang-shell/src/main.rs:106 fn main` — **711 行**（启动+面板装配+smoke/perf 模式编排；probe 系列已拆 async fn :825-1231，main 本体仍是巨型序列）。
2. `office-io/src/docx_edit.rs:1183 fn scan_paragraphs` — 402 行（quick-xml 事件树扫描状态机）。
3. `mang-shell/src/shell.rs:1638 fn render` — 316 行。
4. `mang-shell/src/editor/mod.rs:4060 fn render_lines` — 307 行。
5. `office-io/src/xlsx_edit.rs:506 fn scan_sheet` — 261 行。
（测试侧另有 500+ 行用例，不计。）

### 6.3 重复代码模式量化（R6）

1. **mdcore ↔ mang-shell 编辑栈双实现**（01 号 P1-1 的量化补充）：mdcore 8,786 LOC（op/history/anchor/fence/markdown）vs mang-shell editor 18,503 LOC（buffer 自有 undo 四元组栈 buffer.rs:206-207 **无合并窗** vs mdcore history.rs:20 1s coalesce——undo 粒度语义已分叉；fence 状态机 mdcore markdown.rs:20-28 vs mang-shell fence.rs FenceCursor 增量缓存，各自演化的 #205）。**双轨的语义漂移已有实证**：同一"撤销合并窗"概念两轨两个答案。
2. **office-io 内部 docx/xlsx 重复**：`fwd_to_lt/fwd_to_gt/back_to_lt` 三函数 **逐字节相同**（docx_edit.rs:682-715 ≡ xlsx_edit.rs:357-388，~30 LOC）+ `local_name` 等扫描辅具同族；容器三门（io.rs）则**正确共享**——重复在手术层不在门层。
3. **XML 文本转义三源**：`escape_text`（docx_edit ≡ xlsx_edit，逐字节相同）+ `emit_text_payload`（docx_edit.rs:1984，5 实体+控制字符 typed 拒）+ `office/sanitize/xml_entities.rs:27 escape_xml_text`（**零消费者**的"官方"版，05 号 P1-4）。转义是 XML 注入防线——三源漂移属**安全敏感债**。
4. **Excel 列字母 A1 编解码 ≥8 处 / 5 crate**：formula/refs.rs:20（typed 正源，仅单格）；office-io io.rs:1986、xlsx_edit.rs:207、xlsx_chart.rs:166、xlsx_condfmt.rs:165；mdcore-wasm xlsx_import.rs:333；workbench ingest.rs:83；mang-shell xlsx_doc.rs:300。各自 5-15 LOC——无共享 util crate 的典型税（06 号 X 系已记"范围解析各模块自持"）。
5. **collab ↔ canvas 镜像类型**：`Actor` + `wire()` 逐字节相同（collab session.rs:88-98 ≡ canvas session.rs:154-164）；`SessionOptions` 同构（canvas 自注释"与 mdc-collab 同构纪律"）；events.rs 注册表脚手架 53% 相似（`begin()` 100% 同）。合计 ~250+ LOC 刻意镜像（crate 隔离取舍，已在档），代价 = 纪律注释与结构演化需双侧手工同步。

---

## 7. Trait 抽象与泛型质量（任务 7）

**总判：无过度泛化、无无意义动态分发；问题在反方向——平行具体栈的欠抽象（§6.3）。**

| 抽象 | 形态 | 判读 |
|---|---|---|
| **pdf `DocumentEngine`**（engine.rs:17-37）| 3 方法（page_count/page_geometry/outline/extract_text），object-safe 有 S4 理由（:9-10 注释：工人池传递需 `&dyn`）| **好边界**：pdfium 实装 + mock 双实现、feature 门隔离、attach 期双权威一致性校验（document.rs attach_engine，T1-O3 修复溯源）。非过度抽象 |
| pdf `Rasterizer`（render.rs:42）| 1 方法 + `Box<dyn>` 转发 impl（:46-52 满足工人泛型约束）| 最小面，正确 |
| pdf `AnnotationWriter`（writer.rs:39）| 增量写引擎 seam | lopdf 实装门控，职责单一 |
| canvas `CanvasRenderer`（render.rs:887）| `draw` + 默认 `draw_all` | 默认方法为批量化预留，无浪费 |
| **workbench command 层**（command.rs:25-100）| `WorkbenchCommand` 10 变体 + `execute(envelope, cmd, at_ms)` | **非转发壳**：幂等键重放/预期版本乐观并发/审计三语义真实存在（:69-100）；"命令层薄"自述与实现相符 |
| **collab/canvas session** | 平行具体类型（Actor/SessionOptions/错误/事件镜像）| 刻意不做共享 trait（域隔离），代价见 §6.3-5；X1/X2 维持 |
| mdshell `Host`（shell/src/lib.rs:38-42）| 1 方法 `surface_load_markdown` | 薄但有测试 seam 价值；唯一产品实现是蓝图 shell-native——**抽象等待消费者**（与 P2-8 死 crate 群同源） |
| 泛型总体 | mdcore 近零泛型（唯一 trait object = ClockFn）；worker `R: Rasterizer + Send`；`with_table<R>` 等局部泛型 | dyn 使用全部有注释理由（rg dyn 计数 ≤6/文件）|

一处可议：rpc.rs:931 `unreachable!("subscribe 在 dispatcher 侧特判")` ——match 穷尽性出口，文档在案，属可接受的协议内不一致（改两段分发可消除）。

---

## 8. 依赖边界卫生（任务 8）

### 8.1 workspace.dependencies 单源化的实际覆盖（R7）

- 根 Cargo.toml `[workspace.dependencies]` **仅 serde + serde_json 两项**（:19-21，T1 fix-c 注释自认范围）。实际继承 `workspace=true` 的：mdcore/evidence/pdf/settings/shell/workbench（serde 或 serde_json）。
- **违规**：`crates/collab/Cargo.toml:33 serde_json = "1"`、`crates/office-io/Cargo.toml:37 serde_json = "1"`——违反"禁止 crate 内独立写版本号"的仓规（lock 收敛同版，无实际漂移，纪律破口）。mang-shell 独立 workspace 自带 serde "1.0"/serde_json "1.0"（结构性豁免）。
- **serde 之外的版本一致性（de facto 好）**：wasm-bindgen 0.2（两 wasm crate）、yrs 0.28（collab/canvas）、zip 8.6（office-io/workbench）、calamine 0.36（office-io/formula-dev）、lopdf `=0.45.0`（pdf/mang-shell-dev）、pdfium-render `=0.9.4`、ironcalc_base `=0.9.1` 精确钉。除 serde 族外**无一真正多版本**（对照下述 lock 重名全部来自三方传递依赖）。

### 8.2 Cargo.lock 重复版本扫描（两个 lock 各自为政）

- **根锁**：424 包 / 42 重名。溯源（reverse-dep 实测）：windows-sys 0.45.0 ← **jni 0.21**；windows-targets 0.42 族 ← 同链；sha2 0.10.9 ← **wry 0.46.3**；thiserror 1.0.69 ← wry/jni/ndk/webview2-com-sys/glib/gio/cairo；syn 1.0/itoa 0.4/phf 0.8/indexmap 1.9 ← **cssparser/html5ever/kuchikiki**（servo 栈）；rand 0.7/getrandom 0.1 ← phf_generator 0.8。**结论：42 个重名几乎全部由休眠的 shell-native（tao/wry/webview2 蓝图）拖入**——"两个 windows-sys 版本链"声称属实且成因明确。活跃 15 crate 自身的解析图干净；这是 01 号 P2-8 死 crate 群在依赖维度的又一成本。
- **mang-shell 锁**：741 包 / 61 重名（zed gpui 栈固有）：windows 三版（0.57/0.61/0.62）、objc2 双版（mac 面）、aes/cbc/cipher 双版、png 双版（0.17/0.18）、toml 0.8+1.1 并存。**跨锁漂移点**：sha2 0.11.0（根）vs 0.11.1（mang-shell）、toml_edit 版本集不同——双锁 = 两个解析世界，安全相关包（aes/png/getrandom）的旧版滞留值得纳入例行 `cargo audit` 视野（需外部验证：是否有已知 CVE）。
- **office-io 五库边界**（docx-rs + rust_xlsxwriter + calamine + quick-xml + zip）：**判定合理且被 05/06 号笔记双确认**——每库占一个不可替代职责位（DOCX 模型 / XLSX 写 / XLSX 读 / 自研手术层 / 容器门与重打包），Rust 生态无单一库覆盖读+编+写。真实代价是 (a) docx-rs 读层质量（R1）；(b) 同一 XML 解压 ≥3 趟（05 号 P2-2）；(c) quick-xml 与 docx-rs 并行的双解析面。rust_xlsxwriter 在 Cargo.toml 出现两次是 wasm/native 目标条件分化（06 号 Q4 已定谳，非重复引入）。

---

## 9. 行动建议（优先序）

1. **[P0-R1] docx 读入口加 catch_unwind 或 word365 前置预检**（office-io read_bytes_gated 层，1 天内）——把 05 号 P0 从"锁基线"变"已隔离"；mang-shell docx_doc.rs:171 消费面同步受益。
2. **[P1-R2] 方案 B 先行**：Kind::Text 事务受影响面备份（1-2 天）+ `__heapTrace` 复测；方案 A 立项 ADR（validate-before-mutate 七 op 改造）。
3. **[P2-R3] 锁中毒策略统一**：rpc 订阅表改 poison 重建；mdcore-wasm 接 console_error_panic_hook（04 号 B-4 项）。
4. **[P2-R4] mdcore 错误面补课**：Reject/OpError Display/Error 化 + `From<OpError>` 保留结构 + undo/redo Result 化（03 号行动 3 的横切背书）。
5. **[P2-R6] 抽共享 util**：office-io 内 `scan/escape` 辅具下沉 crate 内共享模块（fwd_to_*、escape_text、local_name）；**escape 三源收敛到单一实现并接线 mdc-office 或删除之**（与 01 号 P2-5 同一决策）。
6. **[P2-R7] 补单源**：collab/office-io 的 serde_json 改 workspace=true（5 分钟）；shell-native 依赖链加注释声明"蓝图拖入，激活前须清理旧版链"。
7. **[P3-R8/R9/R10] 测试 PATH 突变改子进程 env 注入；edition 漂移登记于 rust-toolchain 注释；stamp.rs PNG 解码器补 fuzz 门。**
8. **[P3] 体积**：mang-shell editor/mod.rs 与 main.rs 拆分随下一 GPUI 轮顺势做（最活跃文件优先）；office-io docx.rs 读/写面拆文件（纯移动零语义）。

—— 完 ——（本轮零写入仓库；产物仅本笔记）
