# C6 — mang-shell RPC 服务层与文档视图深审（覆盖收尾轮）

- 仓库：`/home/z/my-project/mdcore-audit`（audit 分支，源码 = main@6322f86）
- 范围：`mang-shell/src/rpc.rs`（实测 3071 行）、`editor/{docx_doc,xlsx_doc,pdf_doc}.rs`、`canvas_view.rs`、`shell.rs`、`settings.rs`、`tests/rpc_client.py`（实测 947 行，派单写 921——以实测为准）、`sdk/python/mang_rpc`
- 方式：纯静态逐行阅读。mang-shell 无法在本沙箱编译（GPUI/Xvfb 面缺位）——所有结论以 文件:行号 为锚；置信四档标注（已确认 / 高度怀疑 / 需外部环境验证 / 设计偏好）。
- 铁律遵守：只读，未改动仓库任何文件。

---

## ① 27 命令安全矩阵（命令 × 沙箱 × 路径校验 × 风险）

`ControlCommand` 枚举共 27 个变体（rpc.rs:176-324）：#104 六命令 + #111 九命令（含 subscribe）+ #134-T1 七条 FS + #134-T2 四条 + #134-T3 一条。分派点 `execute_command`（rpc.rs:903-957）。沙箱原语 `resolve_sandboxed`（rpc.rs:1263-1309，canonicalize + 组件级前缀检查，TOCTOU 窄窗已在注释明示接受，rpc.rs:1260-1262）；沙箱根 `workspace_root_of`（rpc.rs:1313-1317，无工作区一律拒）。

| # | 命令 | 沙箱 | 路径校验 | 风险 | 证据（rpc.rs） |
|---|------|------|----------|------|----------------|
| 1 | open_file | ✗ | 仅 `std::path::absolute`（:965） | **高（P1-9 读半）** | :959-988 → shell.rs:957 `open_document_tab` 零边界 |
| 2 | insert_text | n/a（Buffer 面） | 行列钳制（Editor） | 中（与 save 组成写链） | :990-1010 |
| 3 | replace_selection | n/a | 同上 | 中 | :1012-1028 |
| 4 | get_state | n/a | — | 低-中（泄读通道，选区文本 ≤2000 字符，:2063-2066/2096） | :2046-2093 |
| 5 | close_file | ✗（仅 tab 查找，无 FS） | absolute（:1036） | 低 | :1030-1053 |
| 6 | switch_tab | n/a | 索引越界拒（:1061） | 低 | :1055-1069 |
| 7 | set_cursor | n/a | 钳制 | 低 | :1075-1086 |
| 8 | set_selection | n/a | 钳制+排序 | 低 | :1088-1103 |
| 9 | get_selection | n/a | — | 低-中（泄读通道 ≤2000 字符，:1117-1121） | :1105-1135 |
| 10 | undo | n/a | — | 低 | :1152-1166 |
| 11 | redo | n/a | — | 低 | :1168-1182 |
| 12 | save | ✗（原地写 file_path） | 只读态拒（:1189）、未命名拒（:1192） | **高（P1-9 闭链：open_file→insert→save）** | :1184-1211 → mod.rs:1721 `fs::write` |
| 13 | save_as | ✗ | 仅 absolute（:1224） | **高（P1-9 写半——全表最危险单命令）** | :1213-1243 → shell.rs:1141 直写 |
| 14 | scroll_to | n/a | 钳制 | 低 | :1137-1150 |
| 15 | subscribe | n/a | — | 低（无认证面的一部分） | :860-869（dispatcher 特判） |
| 16 | list_dir | ✓（:1358） | 递归上限 5000 条（:1342） | 低 | :1351-1387 |
| 17 | read_file | ✓（:1444） | 1MiB 截断+非 UTF-8 拒（:1345,1447-1455） | 低 | :1438-1463 |
| 18 | write_file | ✓（:1472） | 只读态/行模型态/脏标签三拒（:1475-1495）、父目录须存在（:1498） | 低（沙箱内覆盖写，非原子） | :1465-1518 |
| 19 | create_file | ✓（:1527） | 已存在拒、父目录须存在 | 低 | :1520-1549 |
| 20 | create_dir | ✓（:1557） | 单层、已存在拒 | 低 | :1551-1568 |
| 21 | delete | ✓（:1577） | 根自身拒（:1579-1584）、脏标签拒（:1591-1600） | 中（沙箱内 `remove_dir_all` 不可恢复，:1602） | :1570-1618 |
| 22 | rename | ✓ 双端（:1627-1628） | 不覆盖（:1632）、父目录须存在、标签身份迁移 | 低-中 | :1620-1667 |
| 23 | run_command | ✗ | 名单白名单 18 条（:1679, commands.rs:49） | 中（含 save/select_all——S-1/S-2 链） | :1673-1692 |
| 24 | list_commands | n/a | — | 低 | :1694-1710 |
| 25 | get_outline | n/a | 渲染态拒 | 低 | :1712-1737 |
| 26 | apply_format | n/a | 空/只读/行模型拒 | 低 | :1739-1760 |
| 27 | batch_replace | ✓（:1921，双通道同门） | 200 文件/10k 命中上限（:1847,1851） | 中（disk 通道批量改盘不可 undo） | :1857-2033 |

**最危险命令**：`save_as`（任意路径写盘 + 无覆盖确认 + socket 无认证，rpc.rs:1213-1243）——任何同 UID 本地进程可覆写该用户可写的任意文件（如 `~/.bashrc`）。`open_file`（rpc.rs:959）为对偶的任意路径读原语。
**P1-9 之外有无新旁路？——无新增独立原语，结论逐项核实**：
- `read_file` **不是**旁路：走 `resolve_sandboxed`（rpc.rs:1443-1444），冒烟序列实测根外/`..` 全拒（rpc_client.py:560-567；单测 rpc.rs:2356-2369）。
- 「export 到任意路径」不存在：27 命令面无 export 类命令。
- 「复制/移动」= `rename`：双端沙箱 + 不覆盖（rpc.rs:1627-1634）——不是旁路。
- **放大链（非新原语，17-sdk 已记 S-1/S-2，本轮在服务端复核闭合）**：①读链 `open_file(任意 .md/.txt) → run_command("select_all")（commands.rs:42,98）→ get_state/get_selection` 每次泄 2000 字符（rpc.rs:1117-1121）；②写链 `open_file → insert_text → run_command("save")（mod.rs:1359-1361 → save() 原地覆盖）`。两条链的根都是 open_file 无沙箱——P1-9 收口即同时闭合。
- `save`（rpc.rs:1184）本身不引入新路径：file_path 只能源于 open_file/save_as（均无门）或 GUI 对话框（用户意图）——门住两源头即闭合。

---

## ② rpc.rs 服务端工程质量

### ②-1 socket 生命周期（bind → 存活 → 清理）

- 端点解析三级优先：显式 `--rpc-sock` > `$XDG_RUNTIME_DIR`（须绝对路径）> `/tmp/mang-shell-$UID.sock`（无 uid 信息退 `/tmp/mang-shell.sock` 单用户兜底）——`default_socket_path` 纯函数（rpc.rs:435-450）+ `resolve_socket_path` 进程薄壳（:453-461，uid 取 `/proc/self` 属主 :464-467）。`--no-rpc` 显式关闭（main.rs:247,379-380）；默认开（main.rs:383-389）。
- 预备协议 `prepare_socket`（rpc.rs:485-503）：不存在 → Ready；能连通 → Busy（他实例在听，本实例禁 RPC，:636-642）；连不通且是 socket → 删陈旧残留（:501）；**非 socket 文件不动它**（:494-500，防 `--rpc-sock` 误指重要文件被删）——防御性到位。
- 绑定 `UnixListener::bind`（rpc.rs:648）失败非致命（返回 false 应用照跑，:631-654）。`ControlServer` 挂 GPUI Global 防 setup 尾 Drop 提前解除 socket（:605-609,675-677 注释记载的陷阱）；Drop 删文件（:614-618）。
- 事件总线 `init_bus` 单接收端契约（:660-668）：已被占用只损失事件面，命令面照常——失败分层降级而非整体失效。

### ②-2 协议解析健壮性（敌意 JSON）——总体扎实
- 两段式解析：先宽容取 id（Value），再严格反序列化（rpc.rs:416-424）；未知字段忽略、类型错回错误并回显 id（单测 :2193-2211）。serde_json 自带 128 层递归上限，深嵌套 JSON 返回错误不爆栈。行分帧 `LineAssembler` UTF-8 有损解码不炸连接（rpc.rs:529-536）。
- 单行 1MiB 上限（rpc.rs:163），超限回错误并断连（:755-762）；8KiB chunk（:741）逐次排空完整行，缓冲不积压。

### ②-3 错误吞没点（逐一登记）
- rpc.rs:764 读错误静默 break（无日志）——对端异常断不可观测，轻微。
- rpc.rs:781-784 EOF 尾包：响应发进 `reply_tx`（bounded(1)，:782）但接收端 `_reply_rx` 从不读、主循环只排空 `out_rx`（:786-790）——**尾包响应必然丢弃**。对端已半关，功能上可接受，但属未文档化的静默丢弃。
- rpc.rs:833 事件序列化失败 `continue` 丢一条（注释自评不可达）。
- rpc.rs:407 响应序列化失败兜底恒错行。
- rpc.rs:600/878/891 失败路径均有 eprintln 或错误回传——无致命吞没。

### ②-4 资源泄漏面——无结构性泄漏，两处轻微
- socket 生命周期完整：`ControlServer::Drop` 删文件（rpc.rs:614-618）+ 挂 GPUI Global 防 setup 尾提前 Drop（:675-677）+ 残留探活清理 `prepare_socket`（:485-503，非 socket 文件不动它、活实例判 Busy）。单测覆盖（:2655+）。
- 连接任务 detach（:703），EOF/写错即退（:747,769）；out 通道随任务结束关闭，dispatcher 侧 `let _ = job.reply.send`（:891）失败静默——不阻塞不泄漏。
- 轻微①：订阅表只在下一次广播时清死项（rpc.rs:586-603）——客户端订阅后断连且此后无任何文档事件，sender 残留在表里（P3）。
- 轻微②：accept 无连接数上限（`live` 只计数打日志，rpc.rs:686-695）——fd 耗尽由 `ACCEPT_ERROR_LIMIT`（:167,705-715）兜住服务侧。

### ②-5 DoS 面（新发现，同 UID 前提）
- `Job` 队列为 **unbounded**（rpc.rs:655），`handle_line` 用 `tx.try_send` 永不失败（:808）；每连接 out 通道同为 **unbounded**（:739）。客户端只发不读即可无限堆积内存（每 Job 携带 ≤1MiB 的 content String）。已确认（代码面）；利用需本地 socket 访问权（与 P1-9 同威胁模型）。
- **主线程阻塞 I/O**：dispatcher 串行落主线程执行（rpc.rs:844-878 `cx.update → window.update`），而 `read_file`（1MiB 读盘 :1445）、`list_dir` 递归（5000 条 :1365-1413）、`batch_replace`（200 文件 × 10k 命中 :1857-1909）、`open_file`（docx/pdf 全量解析）都在主线程做磁盘 I/O 与解析——单条慢命令 = UI 冻结。已确认（架构事实）；幅度需实测定级（需外部环境验证）。

### ②-6 并发模型
- 每连接一后台任务 + race 读写交替（rpc.rs:732-791，注释论证半途丢弃安全性：读 future 被抢先 drop 后数据留内核缓冲，下轮照读）。dispatcher 单任务串行保 GPUI 实体正确性（:844-846）；`mark_rpc_source` RAII 区分 user/rpc 事件源（:872）——同一编辑链双源归因。设计自洽，无锁竞争（订阅表 Mutex 短临界区 :576-603，try_send 非阻塞面）。
- 响应与事件同一条 out 出口（:26-28 模块文档）——单写者保序，多客户端互不干扰（rpc_client.py:452-467 双路订阅同版本断言实测）。

### ②-7 端点与权限
- `$XDG_RUNTIME_DIR` → `/tmp/mang-shell-$UID.sock` 回退（rpc.rs:435-450）；全文件无 `set_permissions`/chmod（grep 实证为空）——`UnixListener::bind`（:648）后权限随 umask：umask 022 时 755（他人无写不可连）；umask 000 环境（部分容器/daemon）下他人可连即获得全部 27 命令。已确认（代码面）；实际可达性取决于运行环境 umask（需外部环境验证）。
- 无认证/无加密为 #104/#111 明确边界（rpc.rs:32「本机通信——不做加密/认证/网络」）——威胁模型=同 UID 进程全信，P1-9 的放大器。

---

## ③ 四视图读写链与 panic 扫描

### 分派总口
`document::open`（document.rs:93-161）按扩展名：md/markdown/txt→可编辑文本；docx→`load_editable`（:111）；pdf→`load`（:123）；xlsx→`load`（:135）；canvas→`CanvasView::load`（:147）。构造口 `open_editor`（:180-294）→ `Editor::new_docx_editable/new_docx/new_pdf/new_xlsx/new_canvas`（mod.rs:644/577/717/778/840）。`.xls/.xlsm` 明确「渲染面未接」文案（document.rs:305-307）。

四视图统一骨架（打开→渲染→编辑→保存）：

```text
docx : 磁盘 →docx_doc::load_editable(四道对账门)→ 行模型+LineBlock → Buffer+EditHook(锁行拒改)
       → save_docx: flush回写计划→引擎落账docx字节→tmp+rename原子写
pdf  : 磁盘 →pdf_doc::load(100MB门+PdfiumEngine)→ 页分隔行模型 → Buffer(只读) → 无保存
xlsx : 磁盘 →xlsx_doc::load(office-io读面)→ 网格行模型 → Buffer(只读) → 无保存
canvas: 磁盘 →CanvasView::load(yrs restore)→ 渲染模型+光栅缓存(惰性) → 自由2D位面(只读) → 无保存
```

### docx（唯一可编辑视图）
- **打开**：`load`（docx_doc.rs:160-174）/`load_editable`（:191-325）：metadata 前置门（:162，LIMIT_INPUT_BYTES_DOCX；可编辑 4× 上限 :193-205）→ 读盘 → 宽面 `read_docx_from_bytes_with_options`（:171/:208）→ 窄面 `DocxEditor::open`（:212）→ 四道对账门（sdt 包表 :234-250、宽窄段落总数 :262-275、不可表示段 :276-290、映射数 :291-302），任一不过降级只读带 notice——降级不静默。
- **渲染**：行模型 + 逐行 span + LineBlock 块归属（convert_inner :505-593）→ `Buffer::from_plain_text` 复用编辑管线（mod.rs:654）。
- **编辑**：EditHook/UndoHook 挂 Buffer 唯一变更入口（mod.rs:903-916 install_docx_hook）；锁定行（表格/域/修订/文本框）由桥逐行拒改（docx_edit_bridge）。
- **保存**：`save` → `save_docx`（mod.rs:1754-1806）：flush 回写计划（:1766）→ 引擎落账出 docx 字节（:1769）→ **tmp+rename 原子写**（:1777-1795，rename 失败退直写）。`save_as` docx 分支（mod.rs:1943-1973）**直写无 tmp+rename**（:1960）——与 save 不对称（P3）。
- **错误面**：全链 `Result<_, String>` map_err；**P0-2 确认**：docx_doc.rs:171（只读装载）与 :208（可编辑装载）处 docx-rs 真实语料 panic（word365 styles index OOB + strict ParseFloat unwrap，终报告已锁基线）；`map_err` 只捕 Err 不捕 panic，向上传播链 = `document::open`（document.rs:184）→ `open_document_tab`（shell.rs:969）→ GUI 菜单/对话框 或 RPC `cmd_open_file`（rpc.rs:975）→ dispatcher `window.update`（rpc.rs:873-878）。GPUI `AsyncApp::update` 不捕 panic（third_party/zed/crates/gpui/src/app/async_context.rs:176 直跑闭包）——**全进程崩溃，未保存编辑丢失**。已确认。
- **新 panic 点扫描**：非测试代码仅 4 处候选——docx_doc.rs:488-489（`editable_from_bytes` 的 expect，`#[allow(dead_code)]` 测试/证据链专用，GUI 不可达）；docx_doc.rs:805/808（`soft_parts[0]`——`split('\n')` 恒 ≥1 元素，逻辑安全）；mod.rs:1868（`unreachable!()`，docx_bid_smoke_edits 烟雾专用，前置 `matches!` 同型守卫）。**视图面无新增可达 panic**。

### pdf（只读）
- **链**：`load`（pdf_doc.rs:146-157）：100MB 应用层前置门（:62,148-154）→ `PdfiumEngine::open`（:161）→ `extract_text`/`outline`（:172-179）→ `convert` 纯函数（:207-278）。渲染复用 Editor 管线（mod.rs:717-772，read_only=true :759）。
- **保存**：无——`save` 早退 true（mod.rs:1722-1724），RPC 侧 `cmd_save` 只读态显式拒（rpc.rs:1189-1191）。
- **panic 面**：pdf_doc.rs:320 `.expect("start 存在则 end 存在")`——start 存在则 rposition 必命中，逻辑安全（仍建议改 ok_or）。其余 expect 全在测试（:624-658）。
- **P1-11 印证**：桌面读路径直调 `PdfiumEngine::open`（pdf_doc.rs:161），全 mang-shell 无 `open_scan` 调用（grep 空）——S2 八族/S6 表单探针/S5 旗标（crates/pdf/src/security_scan.rs:41-58）只在 pdf-wasm 出口（crates/pdf-wasm/src/lib.rs:102）交付。已确认。

### xlsx（只读）
- **链**：`load`（xlsx_doc.rs:92-96）→ `io::read_book_with_options`（office-io 三门+calamine 读面）→ `convert`（:99-258）：表分隔行/列字母行/数据行网格，渲染护栏 1000 行×64 列（:47-49）超限显式提示行（:243-249）。read_only=true（mod.rs:821）。
- **panic 面**：xlsx_doc.rs:311 `String::from_utf8(out).expect("ASCII 列字母")`——构造只压入 `b'A'+n%26`，恒 ASCII，逻辑安全。无其他非测试 panic。
- **保存**：无编辑无保存（同 pdf 口径）。

### canvas（只读显示 + 交互视口）
- **链**：`CanvasView::load`（canvas_view.rs:144-152）→ `CanvasSession::restore`（空文件=空会话）→ `CanvasRendering::from_session` → RasterRenderer 惰性光栅缓存（:127-139，护栏 4096 边/8M 像素 :74-75）→ `render_canvas_area` 全彩位面。视口缩放/平移纯绘制期变换（pan 不重光栅）。
- **保存**：无（v1 只读；会话命令通路 execute/undo/redo 已备但 `#[allow(dead_code)]`——canvas_view.rs:208-237，RPC 未接线）。
- **panic 面**：无 unwrap/expect/panic（grep 空）。

---

## ④ Python 双客户端重复度量化

**形态对照**：

| 维度 | mang-shell/tests/rpc_client.py（947 行） | sdk/python/mang_rpc（client 1183 + protocol 864 + errors 118 + init 145） |
|---|---|---|
| 定位 | 冒烟断言脚本（rpc_smoke.sh 消费） | 产品客户端（pip 包 `mang-rpc`） |
| 命令面 | 单命令模式 13 条（build_payload :83-122：#104 六条 + FS 七条）；烟雾序列覆盖全 27（--smoke/:125、--smoke2/:320、--smoke-fs/:516、--smoke-cmd、--smoke-batch） | `COMMANDS` frozenset 27/27 全集（protocol.py:101-134）；Client 27 个类型化方法 + 泛型 `call`（client.py:540-707,531） |
| 连接模型 | `send()` 每命令新连接（:61-80，TIMEOUT 5s）；`SubClient` 长连接 + reader 线程（:234-317） | 持久连接 + 后台 reader + **自动重连**（client.py:425）+ **自动重订阅**（:454）+ AsyncClient 全镜像（:746+，21 个 async 命令方法） |
| 线协议实现 | 手写 JSON-lines 分帧/id 回显/ok-error 分派/`type=="doc_changed"` 事件判别（:276-279） | 同语义重复实现：`_is_event`（client.py:158）、`_decode_line`（:142）——**协议判别逻辑逐行重复** |
| 端点解析 | docstring 口径（:5-6） | `default_socket_path`（client.py:102）——重复实现 |
| 结果形态 | 裸 dict + assert | dataclass 类型化结果 + `errors.py` 错误分类 |

**量化结论**：命令名集合交集 27/27 = **100% 重叠**；线协议语义（分帧/id/ok|error/事件判别/端点解析）为**第二份平行实现**（约 150-200 行等价逻辑重复）。差异面完全互补：SDK 独有 = 类型化结果/重连/重订阅/async/错误分类（~1600 行）；rpc_client.py 独有 = 六套冒烟断言序列（~600 行，含夹具复原 :334-337/501-503、断连注销 :492-498）。**判定**：不是「复制粘贴分叉」而是「测试脚本与产品客户端各自演化」，但协议判别逻辑的双实现意味着协议变更时需改两处（无共享测试钉定对方的分帧行为）——P3 观察，置信已确认。

**方法面对照表（27 命令 × 两客户端）**——`单发` = build_payload 单命令模式支持（rpc_client.py:83-122）；`序列` = 冒烟序列内裸 dict 直发；`同步/异步` = SDK Client/AsyncClient 类型化方法（client.py:540-707 / 1009+）：

| 命令 | rpc_client.py 单发 | rpc_client.py 序列 | SDK 同步 | SDK 异步 |
|---|---|---|---|---|
| open_file | ✓(:97) | ✓(:171,361,588…) | ✓(:540) | ✓(:1009) |
| insert_text | ✓(:88) | ✓(:157,391…) | ✓(:544) | ✓(:1013) |
| replace_selection | ✓(:95) | ✓ | ✓(:550) | ✓ |
| get_state | ✓(:86) | ✓(:148…) | ✓(:554) | ✓(:1025) |
| close_file | ✓(:99) | ✓(:189,442…) | ✓(:558) | ✓(:1029) |
| switch_tab | ✓(:101) | ✓(:178) | ✓(:562) | ✓ |
| set_cursor | ✗ | ✓(:369) | ✓(:568) | ✓ |
| set_selection | ✗ | ✓(:379) | ✓(:572) | ✓ |
| get_selection | ✗ | ✓(:382,392) | ✓(:580) | ✓ |
| undo | ✗ | ✓(:387,407…) | ✓(:584) | ✓ |
| redo | ✗ | ✓(:422) | ✓(:588) | ✓ |
| save | ✗ | ✓(:434) | ✓(:592) | ✓ |
| save_as | ✗ | ✓(:439) | ✓(:596) | ✓ |
| scroll_to | ✗ | ✓(:446) | ✓(:600) | ✓ |
| subscribe | ✗ | ✓(SubClient:356) | ✓(:604，返回事件迭代器) | ✓ |
| list_dir | ✓(:105) | ✓(:541) | ✓(:638) | ✓ |
| read_file | ✓(:107) | ✓(:555,580…) | ✓(:645) | ✓ |
| write_file | ✓(:109) | ✓(:563,578…) | ✓(:649) | ✓ |
| create_file | ✓(:111) | ✓(:571) | ✓(:655) | ✓ |
| create_dir | ✓(:116) | ✓(:570) | ✓(:662) | ✓ |
| delete | ✓(:118) | ✓(:566,610…) | ✓(:666) | ✓ |
| rename | ✓(:120) | ✓(:610+) | ✓(:670) | ✓ |
| run_command | ✗ | ✓(smoke-cmd 序列) | ✓(:676) | ✓ |
| list_commands | ✗ | ✓(smoke-cmd) | ✓(:683) | ✓ |
| get_outline | ✗ | ✓(smoke-cmd) | ✓(:687) | ✓ |
| apply_format | ✗ | ✓(smoke-cmd) | ✓(:691) | ✓ |
| batch_replace | ✗ | ✓(smoke-batch) | ✓(:701) | ✓ |

合计：单命令 CLI 面 13/27（rpc_client.py:122 未支持即 SystemExit）；序列覆盖 27/27；SDK 双端 27/27。形状差异主轴：rpc_client.py 的 `send()` 每命令新建连接（:61-80）无会话状态；SDK 持久连接 + id 序号 + 重连后 `resubscribe`（client.py:454）——「无状态脚本 vs 有状态客户端」。

---

## ⑤ GUI 的 XLSX 消费路径实证

- **GUI 编辑/导出：两者都不存在。** mang-shell 对 xlsx 的全部消费 = 只读查看器：document.rs:135-144 → `xlsx_doc::load`（xlsx_doc.rs:92-96，office-io **读面** `io::read_book_with_options`）→ `convert` 行模型 → `Editor::new_xlsx`（mod.rs:778，read_only=true :821）。`cmd_save` 只读态拒（rpc.rs:1189-1191）；`save()` 早退（mod.rs:1722-1724）；GUI 无任何 xlsx 写出/导出菜单或命令。
- **XlsxEditor（1,806 行高保真部件透传编辑器）在 GUI 零消费**：全 mang-shell 目录 grep `XlsxEditor` 零命中（命中仅 office-io 自身测试/源码与审计笔记）。已确认。
- **P1-5 精化表述**：「消费倒挂」的 rebuild 有损路径不在 GUI——产品唯一编辑面是 mdcore-wasm `export_with_edits`（crates/mdcore-wasm/src/xlsx_grid.rs:37,232-233，rebuild 通道）。GUI 侧的倒挂形态是**更彻底的零消费**：既不走 XlsxEditor（保真）也不走 rebuild（有损）——没有编辑面。GUI 消费的是 calamine 读面（显示值 + 公式缓存），与 P1-5 的写面问题正交。
- 调用链钉死：`open_file/.xlsx` → rpc.rs:959 → shell.rs:957 `open_document_tab` → document.rs:135 `xlsx_doc::load` → xlsx_doc.rs:93 `io::read_book_with_options`（compat:true）→ :99 `convert` → mod.rs:778 `new_xlsx`。终态：只读渲染。置信已确认。

---

## ⑥ 视图层与引擎层边界卫生

**总体判定：干净（未见绕过引擎公开 API 的状态操纵）。** 逐对核查：
- docx：视图经 `DocxPort` trait 窄面操作 `DocxEditor`（docx_doc.rs:400-472：set_text/save/table_cells/sdt_paragraphs/fields/comments 全是 trait 方法），引擎内部零直触。
- pdf：只用 `DocumentEngine`/`PdfiumEngine` 公开面（pdf_doc.rs:161-179）。
- xlsx：只用 office-io 读面公开 API（xlsx_doc.rs:39,93）。
- canvas：只用 `CanvasSession` 公开命令面（canvas_view.rs:213-237）。
- RPC 层：cmd_* 全部走 ShellApp/Editor 公开方法（tab_*/editor().update 的 insert_text_at/apply_batch_replacements 等，mod.rs:1047-2146 均为 pub fn）——未发现视图直捅引擎内部。
- **唯一内部状态直触**（同 crate 内、设计内）：editor/mod.rs:930-947 `remap_docx_rows_after_batch` 直接 `e.bridge.rows_mut()` 平移行归属表——桥是 shell 自有结构（docx_edit_bridge），非引擎层；属「Editor↔Bridge 单一所有者」的既定设计。设计偏好档。

---

## ⑦ 新问题清单（P0-P3 + 置信）

| 编号 | 级别 | 问题 | 证据 | 置信 |
|---|---|---|---|---|
| P0-2（复核确认） | P0 | docx 读路径 docx-rs 真实语料 panic，GUI/RPC 双入口均无 catch_unwind → 全进程崩（GPUI update 不捕 panic） | docx_doc.rs:171,208；shell.rs:969；rpc.rs:975,873-878；gpui async_context.rs:176 | 已确认 |
| P1-9（复核确认） | P1 | open_file/save_as 任意路径读写 + socket 无认证无显式 chmod | rpc.rs:959,1213,648 | 已确认 |
| P1-11（GUI 面复核） | P1 | 桌面 pdf 读路径不接 S2/S6 open_scan | pdf_doc.rs:161；security_scan.rs:41；pdf-wasm/lib.rs:102 | 已确认 |
| N-1（新） | P2 | unbounded Job 队列 + 每连接 unbounded out 通道：同 UID 进程只发不读可内存耗尽（DoS） | rpc.rs:655,739,808 | 已确认（代码）；利用需本地 socket |
| N-2（新） | P2 | 主线程阻塞 I/O：read_file/list_dir/batch_replace/open_file 解析全在 window.update 内串行——慢命令冻结 UI | rpc.rs:873-878,1445,1365-1413,1857 | 已确认（事实）；幅度需实测定级（需外部环境验证） |
| N-3（新） | P3 | docx save_as 直写无 tmp+rename（与 save_docx 原子写不对称）；write_file/create_file 亦非原子 | mod.rs:1960 vs 1777-1787；rpc.rs:1506,1543 | 已确认 |
| N-4（新） | P3 | 订阅表死项仅在下次广播时清理；空闲期残留 sender | rpc.rs:586-603 | 已确认（逻辑，影响微小） |
| N-5（新） | P3 | EOF 尾包响应必然丢弃（reply_tx 无消费者）——未文档化 | rpc.rs:781-790 | 已确认 |
| N-6（新观察） | P3 | Python 双客户端线协议判别逻辑双实现，协议演进需双改 | rpc_client.py:276 vs client.py:158 | 已确认 |
| N-7（登记） | P3 | delete 不可恢复（无回收站）——#134 已声明取舍 | rpc.rs:123,1602 | 设计偏好 |
| N-8（登记） | P3 | /tmp 回退端点 umask 依赖：umask 000 环境他人可连获全 27 命令 | rpc.rs:648（全文件无 chmod） | 代码已确认；可达性需外部环境验证 |
| P2-21（复核确认） | P2 | 设置三栈分裂：mang-shell JSON（settings.rs:25-49，`$XDG_CONFIG_HOME/mang/settings.json`）vs mdc-settings TOML sidecar（crates/settings/src/lib.rs:1-40，schema_version/四域）vs mdshell re-export（crates/shell/src/lib.rs:13）；mang-shell 不依赖 mdc-settings（Cargo.toml 实证）。undo_coalesce_ms 假旋钮复核：引擎恒用常量 `COALESCE_MS=1000`（crates/mdcore/src/history.rs:20），设置面值域 1-10000（crates/settings/src/lib.rs:58）+ web UI 旋钮（web/src/main.ts:3196）从不进引擎 | 上列行号 | 已确认 |

---

## ⑧ P1-9 最小修复面

**辅助已存在，零新建**：
- `resolve_sandboxed`（rpc.rs:1263-1309，**pub**，单测覆盖逃逸/符号链接/组件前缀 ：2334-2369）；
- `workspace_root_of`（rpc.rs:1313-1317，无工作区即拒，错误文案现成）；
- 8 条 FS 命令已是标准接入样板（1358/1444/1472/1527/1557/1577/1627-1628/1921）。

**接入点恰好 2 处命令体**：
1. `cmd_open_file`（rpc.rs:959-988）：在 :965 `absolute` 处替换为 `let root = workspace_root_of(shell, cx)?; let path = resolve_sandboxed(&root, Path::new(path))?;`（返回即 canonical 绝对路径，后续 tab 匹配/打开逻辑零改动）。策略与 FS 命令对齐：无工作区 → 拒（与 rpc.rs:1315 现有文案一致）。
2. `cmd_save_as`（rpc.rs:1213-1243）：:1224 同款两行；`resolve_sandboxed` 产物直接喂 `save_active_as`（canonical 路径成为新 tab 身份，语义不变）。

**派生闭合（无需另改）**：`save`（rpc.rs:1184）与 `run_command("save")`（mod.rs:1359-1361）的 file_path 源头只剩「已门禁的 open_file/save_as」与「GUI 对话框（用户意图）」——S-1 读链（open_file→select_all→get_state）与 S-2 写链（open_file→insert→save）同批闭合，MCP 双链（17-sdk S-1/S-2，server.py:97-103）也由服务端单点修复一并断链。

**配套改动**：① 单测补 open_file/save_as 越界拒绝用例（rpc.rs tests 尾部）；② `docs/rpc-protocol.md` 同步（rpc.rs:42-43 声明同源义务）；③ 若需更严：给 Editor 加「rpc 打开」旗标以拒绝对 GUI 打开但沙箱外文件的 RPC save——属增强非必需。**估计 diff：rpc.rs 单文件 30-60 行**（两条命令体各 ~5 行 + 测试 + 文档）。

---

## ⑨ 附录 A：会话/设置面与窗口生命周期（P2-21 证据面）

### A-1 设置三栈分裂（键面清单）

| 栈 | 存储 | 键面 | 消费方 |
|---|---|---|---|
| mang-shell/src/settings.rs | `$XDG_CONFIG_HOME/mang/settings.json`（JSON，:25-49） | workspace / theme / font_size / indent_unit / panel_widths / folds（每文件 200 条、全局 64 文件，:215-217）/ find_history / find_regex / keymap | mang-shell GUI 自身（设置面板 settings_panel.rs 为 UI 壳） |
| crates/settings（mdc-settings） | TOML sidecar + 原子写 tmp+rename（lib.rs:22,726） | schema_version + editor{undo_coalesce_ms, progressive_open_threshold_bytes} / ai / storage{meta_sidecar} / appearance{theme,font_size_px,lang} / docx 导出预设（bounds :57-82） | mdcore-wasm/web（CONTRACT §4）+ mdshell |
| crates/shell（mdshell） | 无自有存储——`pub use mdc_settings as settings` re-export（lib.rs:13）+ Host trait / OpenProgress / session.rs（263 行） | 同 mdc-settings | headless 宿主契约层（shell-native 因 GPL/沙箱不可编译不入 workspace） |

- mang-shell 的 Cargo.toml 依赖面（office-io/pdf/gpui…）**不含 mdc-settings/mdshell**——两栈零共享、零互认，GUI 用户在 web 面配的 `editor.undo_coalesce_ms` 对桌面壳完全不可见，反之亦然。
- 假旋钮闭环证据：设置面值域 1-10000ms（crates/settings/src/lib.rs:58）、web UI 暴露（web/src/main.ts:3196）、引擎恒用常量 `pub const COALESCE_MS: u64 = 1000`（crates/mdcore/src/history.rs:20）——旋钮全程未接线。已确认（P2-21 复核成立）。
- mang-shell 侧容错纪律本身良好：读写失败静默跳过（settings.rs:16-17 设计声明）、keymap 非数组警告不静默（:161-166，#218 review 修正）、渐进扩展契约有单测（:449-462）。

### A-2 shell.rs 窗口/会话生命周期

- **构造**：`ShellApp::new`（shell.rs:257-299）：启动文档→首标签（合成身份 :273-282）、工作区记忆恢复（:263-270）、右键菜单回调注入（:287-292）、`on_should_close` 脏确认钩子（:293-295 四后端接线）。
- **打开**：`open_document`（:948-952）→ `open_document_tab`（:957-1009）：绝对化身份（:963）、已开激活（:966-967）、`document::open_editor` 分派（:969）、Unsupported/Failed 走提示横幅不白屏（:1000-1007）、文件树高亮同步（:991-996）。
- **关闭**：`close_active_tab`（:1029-1058）脏标签弹确认框（ConfirmState :1045-1051，模态接管焦点）；`resolve_confirm`（:1086-1125）保存后关/直接关/取消三分支，保存失败不关（:1094-1100）。
- **退出**：确认框 Quit 分支逐标签 save 再 `cx.quit`（:1105-1116）；`Act::Quit` 直 `cx.quit` 不经脏确认（:908-911 注释自认遗留，建议另开 issue）。
- **RPC 磁盘权威面三方法**：`reload_tab`（:1177-1186）/`tab_can_reload`（:1190-1194）/`apply_tab_renames`（:1206-1219+）——为 #134 write_file/rename 服务，主线程内聚。
- 单窗口单实例模型：socket Busy 即禁第二实例 RPC（rpc.rs:636-642）；无多窗/多会话面——P2-21「单窗口无多开」复核成立。

---

## 收束判定

RPC 层工程纪律高于仓均（分帧上限/探活清理/原子写样板/四道 docx 对账门/降级不静默），安全面短板集中在**两个无门原语（P1-9）与无认证端点**——且修复面已就绪（辅助函数 + 8 处样板）。四视图读写链错误面诚实（String 错误贯穿、只读门禁三口径一致），唯一 P0 是依赖层 docx-rs panic 的无兜底传播（P0-2）。XLSX 在 GUI 的形态是「零消费」而非「有损消费」——P1-5 的修复落点在 wasm 编辑面接线，与 GUI 无关。新增工程问题以 unbounded 通道与主线程 I/O 为最重（N-1/N-2），均不阻断发布但应进下轮整改清单。
