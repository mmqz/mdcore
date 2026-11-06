# mdcore 深化审计补充报告（Phase 2）

- **审计对象**：`github.com/mmqz/mdcore`，main 分支 `6322f86`（Merge PR #218；较上轮基线 `96596d7` 增量 = #217/#218 keymap 子系统，+3,455 行）
- **审计日期**：2026-11-05
- **审计性质**：只读深化轮——上轮六轮审计（见 [final-report.md](final-report.md)）覆盖广度后，本轮做四件事：**①补盲区**（sdk / mdcore-wasm 内部 / web TS 全量 / 合规 / 对标 / mang-shell 编辑器内核 / mang-shell RPC·视图七个面上轮只给概要或笔记缺失）；**②实证化**（web 24 套件首次独立全量运行、P0-3/P0-4 实机复现、P1-18 实测推翻）；**③增量审计**（keymap 新代码 3,455 行独立评估）；**④双轨税量化**（mang-shell 内核 vs mdcore 核的语义四件套逐项对比 + T7 收敛资产盘点）
- **方法**：8 个并行深审子代理（产出笔记 14/15/17/18/19/20/22/23）+ 主审计员实证轮（笔记 21，含 P0-3 scratch 复现工程）+ 主审计员对子代理关键结论抽查 4 处（全部属实：server.py open_file 零校验 / lib.rs:205-209 冻结闭包 / keymap.rs:291-299 跨形态臂只比首键 / pdf_view.ts:1826-1829 属性插值）
- **纪律**：仓库 `git status` 全程干净；构建产物仅落 gitignore 目录（target/、web/pkg*/、web/dist/）；唯一手工干预 = 对 gitignore 产物 `dist/pkg/snippets/` 的补拷根因实验（N-1）；未修改任何被跟踪文件、未合并 main、未建 PR

---

## 0. Executive Summary

**一句话总评：深化轮把上轮七个「概而括之」的盲区（SDK、wasm 内部、web 前端、合规、对标、mang-shell 内核、mang-shell RPC/视图）全部打开了——结论是上轮的顶层判断基本站得住，但细节层发现了 6 项新 P1（含一个「生产构建里 Canvas 静默死亡」的构建链 bug 和两条 MCP 组合逃逸链），推翻了 1 项 P1（wasm 时钟），把「零 CI 的 web 套件」从风险预测变成了已兑现的具体案例，并把 P0-3 从代码直读钉到字节级复现（2.7% 前缀保持率）。**

最重要的七件事：

1. **实证丰收**：上轮 P0-4（web Enter 文本损坏）升级为「本地实机复现」（中段 Enter → 尾文本重复）；上轮 P0-3（PDF forms 全量重写）升级为「字节级复现」（13,097→11,749 字节、前缀逐位相同率 **2.7%**、无 /Prev——「增量修订」声称被量化推翻）；上轮 P1-1 的实证地位点明（t3_reuse_locks 测试即复现演示，在 240 绿测试组内）；上轮 P1-18（wasm 时钟）被实机**推翻**（3 批打字 → 3 条 undo——main.ts:790 mutate() 时钟注入被上轮漏看），降级为 P3 API 缺陷。
2. **构建链新 P1（N-1）**：`web/build.mjs` 不拷 `pkg/snippets/`（ironcalc_base 0.9.1 的 wasm-bindgen inline JS）→ **标准构建产物里 Canvas 视图 boot 失败、`__canvasReady` 恒 false**，empty_state/lifecycle 两套件必红。修复一行。零 CI 使其完全失明。
3. **MCP 安全面新 P1×2**：mang-mcp 的 `open_file`（零校验直通 rpc.rs:959 无沙箱原语）+ `run_command("select_all"/"save")` + `get_state` 组合成**工作区外任意文件读取链**与**原地改写链**——敌意 LLM 可完成越权读写。一处修复（rpc.rs 给 open_file/save_as 套沙箱）同时闭合 P1-9 与本链。
4. **协议盲区关闭**：27/27 命令逐字段对账 SDK 与 rpc.rs **完全一致**——上轮「P3 观察·协议一致性未验证」以通过结论关闭；mang-rpc 评 A、mang-mcp 评 C→B。
5. **keymap 增量质量达标但加深双轨**：#217 五条 review 修复声称**逐条属实**（各带回归测试）；唯一新缺陷 P2（chord 尾键×对域单键冲突漏检 → 双域叠加执行）；2,839 行零 fmt 漂移。但全部动作面在 mang-shell 内闭环、与 mdcore 语义核零接触——**加的是双轨高度，不是门禁失真**。
6. **双轨税首次量化**（笔记 22）：undo 粒度两轨两答案（B 轨无合并窗/无选区快照 vs mdcore 1s coalesce+Source 感知）；围栏边界判定不等价；字数口径完全不同；折叠仅 B 轨有。**T7 收敛资产盘点成表**：渲染管线/display_map/多光标/命令分发/IME/fence 增量算法（可反向吸收进 mdcore）/EditHook 桥契约为可保留资产，undo 四元组/字数口径/围栏判定为必弃面。
7. **#164 残项 4→9 扩容**（笔记 22）：状态栏**每帧同步全文件磁盘读**（status_bar.rs:79）与字数每帧 2×全文物化（status_bar.rs:112/panels.rs:214）为最重新增；纯光标移动也全毁软换行重建全表（mod.rs:4160-4198）。另 mang-shell RPC 面新增两项 P2：Job 队列与每连接 out 通道 unbounded（同 UID DoS 面，rpc.rs:655/739/808）、read_file/list_dir/batch_replace 在主线程 window.update 内做磁盘 I/O（UI 冻结面，笔记 23）。

---

## 1. 对上轮终稿的修正（3 项）

| # | 上轮结论 | 本轮修正 | 证据 |
|---|---|---|---|
| C-1 | P1-18「wasm 时钟恒 0 → 全会话击键并成一条 undo 条目（已确认）」 | **产品面影响不成立**：实测三批打字（批间 1.5s）→ 3 次 `__bridge.undo()` 精确分批回退。宿主侧 main.ts:790 `mutate()` 每次变异前注入 `set_clock_ms(Date.now())`（另 main.ts:1157/1590 两处直呼）。**降级 P1 → P3（API 设计缺陷）**：`set_clock_ms` 为冻结常量闭包（mdcore-wasm/src/lib.rs:205-209）且构造器无默认时钟——绕过 mutate() 的消费者（bench.ts:114 路径）仍获恒 0 | 笔记 21 §6 |
| C-2 | 附录 B「未运行：mdcore-wasm（wasm 目标）」 | **148 个测试全部宿主可跑且本地全绿**（117 lib + 31 集成，标准 `#[test]` 无 wasm cfg 门）。「CI 只测 9/17 crate」结论维持（workflows 仅 `cargo check -p mdcore-wasm --target wasm32`，windows-check.yml:52），但登记口径修正为「宿主可跑、CI 未纳入」 | 笔记 21 §8 |
| C-3 | 「fmt 门禁 main 实测 977 行漂移」 | 口径勘误：本轮同命令得 **134 hunk / ~1,900 输出行**，与开放 PR #216 自述「1896 行」吻合——上轮 977 疑为不同计数口径（如仅 diff 行）。漂移分布不变（pdf 106 + canvas 22 hunk 存量债）；**新增 keymap.rs 2,839 行零漂移** | 笔记 21 §7 |

另：上轮 Q6「看起来完成但缺证据的能力」表中「web 24 套件 ≈389 checks 零 CI」的风险预测，本轮兑现为具体案例（§4 P2-27 + N-1）。

---

## 2. 新发现问题清单

> 编号续上轮（上轮至 P1-20/P2-22/P3 若干）。完整证据链见各笔记；本表为裁决级汇总。

### 新 P1（5 项）

| ID | 模块 | 位置 | 问题 | 影响 | 置信 | 笔记 |
|---|---|---|---|---|---|---|
| P1-21 | web 构建链 | web/build.mjs:81-82（对比 :62-64 pkg_pdf 递归拷） | 不拷 `pkg/snippets/`（ironcalc_base 0.9.1 wasm-bindgen inline JS）→ 胶水 404 → canvas 装载失败 | **标准构建里 Canvas 死功能**；empty_state/lifecycle_web 必红；修复一行；根因实验已完成（补拷即转绿） | 已确认+本地复现 | 21 §2 |
| P1-22 | sdk/mcp | sdk/mcp/mang_mcp/server.py:97-103 → rpc.rs:959,2059-2074 | **组合逃逸·读链**：`open_file(任意绝对路径)`（零校验）→ `run_command("select_all")` → `get_state().selection.text`（≤2000 字符，来自 buffer） | 敌意 LLM 经 MCP 泄工作区外 .md/.txt 内容；单步已确认，链路组合高度怀疑（未实机串测） | 高 | 17 |
| P1-23 | sdk/mcp | rpc.rs:990,mod.rs:1359→1730 | **组合逃逸·写链**：`open_file(工作区外.md)` → `insert_text` → `run_command("save")`（std::fs::write 回原路径） | 越权原地改写可开文件（save_as 不在 11 工具中但 save 经 run_command 可达） | 高 | 17 |
| P1-24 | sdk/mcp | sdk/mcp/README.md:93-95 | 「工作区沙箱」声称只列 4 工具、未警示组合绕过；SDK README 无任何安全段 | 用户按 README 把敌意 LLM 接到 Claude Desktop = 信任传递事故 | 已确认 | 17 |
| P1-25 | mdcore-wasm/web | main.ts:2668→2638-41 · mdcore/word_count.rs:225-244 | 状态栏 500ms 轮询无条件 `word_count_json()+blocks_count()`，word_count 每块 Rope→String 物化，无 revision 守卫 | 10MB 文档**空闲态**每秒 2 次全文档扫描+分配 churn（打字时叠加） | 已确认 | 18 |

### 新 P2（12 项精选）

| ID | 模块 | 位置 | 问题 | 笔记 |
|---|---|---|---|---|
| P2-23 | compliance | compliance/scan.sh | 相对路径 + `2>/dev/null \|\| true` 吞错 → **非仓根调用静默假 PASS**（本地两向复现）；无豁免机制；产品面缺 mang-shell/sdk/scripts 域 | 14 |
| P2-24 | mang-shell keymap | keymap.rs:291-299 | `conflicts()` 跨形态臂只比首键——chord 尾键 × 对域单键碰撞漏检：bind `ctrl+k w`→save 后 `Ctrl+K Ctrl+W` 双域挂起 → **Save+CloseTab 叠加执行**；同域 chord bind 静默杀死同键单键绑定 | 19 |
| P2-25 | web pdf_view | pdf_view.ts:1826-1829 | Ink 标注 `svg.innerHTML` 属性插值，数据源是 wasm 会话 JSON 无数值校验——非数字字段可断 `stroke="` 注 `onload=`。v0 不可达（标注全本地构造），「PDF 既有标注解析」一落地即活化；canvas_view 的 createElementNS 是现成安全范本 | 20 |
| P2-26 | web main.ts | main.ts:537-544,563 | `scanHr` 挂每个 docChange 全文档逐行扫描（代码自认 R19 候选未修）——10MB 档推演 20-40ms/键击，侵蚀打字路径收益 | 20 |
| P2-27 | web 测试 | web/tests/docx_export.py:548-556 | 断言与 `deddf1a`（#196 任务2）有意形状变更脱钩（Rust 断言同提交已更新、web 侧未动）→ **红在 HEAD 且零 CI 失明**；上轮 P1-14 风险的兑现案例 | 21 §4 |
| P2-28 | mdcore-wasm | lib.rs（apply/undo 路径） | panic 隔离不对称：canvas/collab 有 catch_unwind→typed 错误，唯独最核心的 WasmCore apply/undo 零隔离 + 全仓无 panic hook（P2-9 深化） | 18 |
| P2-29 | sdk/python | client.py:393,:883 | 敌意服务端回 `"id":"evil"` → `int(req_id)` 未捕获 ValueError → **reader 线程崩**、错误降级 ConnectionLost（子代理已复现） | 17 |
| P2-30 | mdcore-wasm | 导出面 | **91 个 wasm_bindgen 导出中 16 个死导出（17.6%）**：整套会话持久化机器（snapshot/restore/replay/history_json×2）、`export_anonymous`、自称「AI 路径」的 `replace_text_at_anchor`——零 web 消费 | 18 |
| P2-31 | mang-shell #164 残项 | status_bar.rs:79 · :112 · panels.rs:214 · mod.rs:4160-4198 | 残项从 4 扩至 9：状态栏**每帧同步全文件磁盘读**；字数每帧 2×全文物化；wrap 回填循环 O(可见行×doc)；纯光标移动键也全毁软换行+全表重建 | 22 |
| P2-32 | mang-shell rpc | rpc.rs:655,739,808 | Job 队列与每连接 out 通道均 unbounded → 同 UID 恶意客户端内存耗尽 DoS 面 | 23 |
| P2-33 | mang-shell rpc | rpc.rs（window.update 内） | read_file/list_dir/batch_replace 在主线程 `window.update` 内做磁盘 I/O → 大文件/慢盘 UI 冻结 | 23 |
| P2-34 | mang-shell 内核 | buffer.rs（vendored text.rs:885-886） | undo 双存储：外壳四元组之外 vendored buffer inner 历史无界堆积（注释自称「无害堆积」失准）= O(累计编辑量) 内存 | 22 |

### 新 P3（精选）

mang-shell 侧：rpc undo/redo 缺 read_only 门禁（rpc.rs:1152 vs :1189 不对称）；docx save_as 直写无原子性（对照 save_docx 的 tmp+rename）；6×40 字段构造器样板（mod.rs:526-895）。web 侧：`__mdcoreEvents` 无上限增长（长会话内存）；设置面板 9 键 6 键无消费方（假旋钮）；面板重开 overlay 泄漏 + 监听器累积；`__inject_chunk` 裸调用无 typed 包裹（异常→宿主悬挂，一行修）；localStorage 污染导出选项恒败；i18n en 破窗。sdk 侧：双客户端 47+47 平行实现 + 仓内第二份 921 行 Python RPC 客户端；发布链期货（pip 安装依赖不存在的 PyPI 包）；测试未接 CI。wasm 侧：canvas/collab 事件缓冲无 cap（后台标签页累积，需环境验证）。合规侧：NOTICES v1.6 停摆细节与 SBOM 三证（见笔记 14，维持上轮 P1-15 定级）。

---

## 3. 增量审计：keymap 子系统（#217/#218，+3,455 行）

**五条 review 修复声称逐条验证（全部属实，各带回归测试）**：

| 提交 | 声称 | 验证 | 证据锚 |
|---|---|---|---|
| 8966e15 | 只读绕过封堵（双层防御第二层） | ✅ L1 物理键白名单（editor/mod.rs:2338）+ L2 动作级门（editor/mod.rs:2356-2363，`Matched && read_only && !readonly_safe()` 整体拒绝）；穷举无残洞 | 笔记 19 §② |
| 3c0fb55 | chord 解绑赢家镜像 dispatch + 单键越界 panic 封堵 | ✅ `chord_unbind_winner`(keymap.rs:1431) 与 `best_continuation` 候选谓词/严格大于/表序逐字同源；`canon.get(1)` 守卫(:1156) | 笔记 19 |
| ee319d1 | 幻影修饰词显式报错 | ✅ 词级匹配不误伤 windows/win；Ctrl+S 失效原例有回归锁(:2213) | 笔记 19 |
| d4eed9e | 纯函数提取+非数组警告 | ✅ parse_keymap_entries(settings.rs:157) 纯函数、3 个单测直接消费 | 笔记 19 |
| 7c42d78 | 死代码 line_of 移除 | ✅ 无残留调用 | 笔记 19 |

**结构质量**：2,839 行中 ~40% 是测试；纯 std 零 GPUI 分层（数据/引擎/UI 接线分离）；默认行为零变化有 36 个逐臂单测+双证据集成测试钉定；fmt 零漂移。

**独立判断**：**质量达标，非「失真地基加高」型**（Windows CI 实跑 test-shell 且 PR 期三次编译打回证明门禁咬合）——但它**确是双轨上的继续加高**：全部动作面在 mang-shell 内闭环、与 mdcore 语义核零接触。修 P2-24（conflicts 补尾键比较）+ 命令面板标签随换绑（palette_view.rs:83 静态 default_key）后可评 A 级。

---

## 4. web 产品路径健康度（实证记分板）

首次独立完整运行 30 个可执行套件（上轮零 CI 的「389 checks」从预测变为记录）：

- **28 绿 / 1 真 红 / 1 环境门**（tr2b_l23_external 需 LibreOffice）；
- 真红 = docx_export 29/30（stale 断言，P2-27）；
- **标准构建下另 2 套件必红 + Canvas 死功能**（N-1/P1-21，补 snippets 后转绿——含 canvas_view.py 本体）；
- 性能锚点：1MB 档（20,448 块）typing p95 **20.3ms** / fallback_sync 332ms / undo 重渲染 154ms；
- acceptance_pdf5_ui **UI 级 PASS**（标注 460 追加 + 下载 %PDF-1.4）——PDF 标注 UI 链路实际可用；
- XSS 裁决：**当前无可利用 XSS**（全库 4 处 innerHTML 均静态/安全；敌意文本全走 textContent）；唯一实质风险是 pdf_view 的潜伏注入点（P2-25，前置阻断项）。

## 5. 模块裁决更新（仅列变化项；其余维持上轮裁决表）

| 模块 | 上轮 | 本轮 | 变化原因 |
|---|---|---|---|
| sdk/python（mang-rpc） | P3 观察 | **A（保持扩展）** | 27/27 协议对账全过、测试半真（传输层真）、负向用例密度高；附 P2-29 与 CI 接线小整改 |
| sdk/mcp（mang-mcp） | P3 观察 | **C→B（暂缓对外推广）** | 组合逃逸双链（P1-22/23）+ 误导文档（P1-24）——在 rpc.rs 沙箱收口或 MCP 层自建白名单前不应引导用户配置 |
| mdcore-wasm（内部） | （随桥评 B） | **B-** | 边界层纪律严整（91 入口 typed 错误全覆盖、无畸形 JSON→panic 路径、630 件敌意语料硬门）；接线完整度弱（时钟靠宿主自觉、16 死导出、panic 隔离不对称、word_count 轮询税） |
| web/ 前端 | B | **B（维持，风险细化）** | 无新 P0/P1 代码缺陷（N-1 是构建链）；XSS 放行；潜伏注入点与性能税入 P2 |
| mang-shell keymap（新增面） | — | **B+（一修可 A）** | §3 |
| mang-shell 编辑器内核 | （概览级） | **B（高地基·债在残项）** | 架构正确（组合式外壳/零拷贝 snapshot/321 模块内测试/unsafe=0）；#205 fence 增量算法高质量可反向吸收；但 9 项残项未收口 + undo 双层无合并窗 |
| mang-shell RPC/视图面 | （随 GUI） | **B-（P1-9 修复面已锄定）** | 27 命令 8 沙箱无新独立旁路；P0-2 双入口确认（shell.rs:969+rpc.rs:975）；XLSX 视图纯只读（XlsxEditor 零消费更彻底）；P1-9 最小修复 30-60 行断三链 |

## 6. 路线图增量（对上轮 T1-T10 的影响）

- **T3（P0-3 修复）**：验收标准已有字节级量化基线（前缀逐位相同率 2.7%→应≥99.9%、尾部 /Prev 在场、输出尺寸单调不减）——scratch 复现工程可直接改造成回归测试；若选择降级声称+拒写已签档路线，`form_signed.pdf`/`signed_sample.pdf` 夹具已在仓。
- **T4（P0-4 修复）**：验收标准已有现成实机复现脚本与逐步序列（笔记 21 §5）——修复后应跑中段/行尾双案例 + 「Enter 前后全文比对」e2e。
- **T5（门禁止血）**：新增两个一行级修复项——build.mjs 补 snippets 拷贝（P1-21）、docx_export 断言对齐 #196 任务2 形状（P2-27）；fmt 复位后建议 CI 加 hunk 数上限防口径再漂。
- **T7（双轨收敛）**：**收敛资产盘点表已成**（笔记 22 §⑦）——渲染管线/display_map/多光标/命令分发/IME/fence 增量算法（可反向吸收进 mdcore）/EditHook 桥契约为可保留资产；undo 四元组/字数口径/围栏判定为必弃面。docx 的 office-io 桥证明桥模式可行，为收敛提供现成蓝本。决策会输入齐备。
- **T8（消费面接线）**：P1-9 的 rpc.rs 沙箱收口**现在必须同时闭合 MCP 双链**（一处改动断三链：RPC 直连/MCP 读链/MCP 写链——笔记 23 已锄定 30-60 行最小修复面）——优先级提升。
- **P1-17（#164 收口）**：残项清单从 4 项扩至 9 项（P2-31，含状态栏每帧磁盘读两处）——收口范围需按新清单重排。
- **新增高 ROI 小修**（建议插入 T5 批次）：word_count revision 守卫（约 3 行）；`__inject_chunk` 补 ingestExtend（一行，P2-8 精确化）；`__inject_chunk` 裸调用包 try/catch（一行）；wasm panic hook + apply 隔离（一改三收益）；rpc 队列加上限（P2-32）。
- **P1-18 撤出 P1 清单**（降级 P3）：undo 粒度坍塌不成立；残存 set_clock_ms API 缺陷随 wasm 面整改。

## 7. 本轮不可验证项（如实登记）

mang-shell 编译与测试（GPUI 系统依赖，沙箱不可装）；wasm32 真运行时测试（wasm-bindgen-test-runner 0.2.129 无 node 模式，仓内亦从未配置）；真 IME（合成+CDP 已绿，真机协议未执行）；LibreOffice 差分（tr2b_l23_external 环境门）；MCP 组合逃逸链的端到端串测（需真实 LLM 客户端与工作区外文件布局）。

---

## 附录：本轮笔记索引（audit/notes/）

| # | 文件 | 内容 | 来源 |
|---|---|---|---|
| 14 | compliance.md | 合规体系深审（补上轮缺失 slot）：scan.sh 假 PASS 缺陷/许可拓扑五层/SBOM 三证/GPL 面重建/闭源最小路径 | 子代理 D1 |
| 15 | benchmark.md | 竞品对标档案（补上轮缺失 slot）：六线决策档案/12 条决策对账/Typora 与 Acrobat 启示/五分类增量复核 | 子代理 D2 |
| 17 | sdk.md | mang-rpc + mang-mcp 深审：27/27 协议对账/MCP 逃逸双链/测试可信度/裁决 | 子代理 C1 |
| 18 | wasm-internals.md | mdcore-wasm 内部全量：91 API 面与消费对账/JSON 边界评级/内存模式/16 死导出 | 子代理 C2 |
| 19 | keymap-delta.md | #217/#218 增量审计：五声称验证/匹配引擎深读/冲突检测漏检/增量健康度 | 子代理 C3 |
| 20 | web-frontend.md | web TS 全量：XSS 面审计表/IME/undo 接线/竞态/测试覆盖缺口 | 子代理 C4 |
| 21 | web-e2e-runs.md | 实证轮：构建链/30 套件记分板/N-1 根因实验/P0-4 复现/P1-18 推翻/P0-3 字节级复现（2.7% 前缀率）/门禁复跑 | 主审计员 |
| 22 | editor-kernel.md | mang-shell 编辑器内核深审：架构图/Zed buffer 集成/#164·#205 残项 9 项/undo 双层/双轨税量化/T7 收敛资产盘点 | 子代理 C5 |
| 23 | rpc-views.md | mang-shell RPC·视图深审：27 命令安全矩阵/服务端质量/四视图链/双客户端量化/XLSX 零消费实证/P1-9 最小修复面 | 子代理 C6 |
| — | test-runs/web-e2e.log + web-results/ | 原始运行日志与结果 JSON | 自动化产出 |

*本报告由只读深化审计产生；所有修复建议均未实施。审计分支：`audit/architecture-review-2026-11`（仅新增 audit/ 文档；本轮将分支基线同步至 main@6322f86，未触碰 main）。*
