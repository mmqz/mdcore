# 08 · Canvas 域审计（crates/canvas + mang-shell/canvas_view.rs + mdcore-wasm/canvas.rs）

> 审计对象：mdcore-audit main@96596d7。方法：只读深读一手源码 + 对照
> mang-research-archive/cleanroom-analysis/canvas-line（canvas-a/b/c/e + 06_R2 综合）。
> 引用形式：`文件:行号`（仓库根相对）。LOC 口径与 audit-notes/loc-stats.md 一致：
> canvas src 3,860（session 1,148 / render 2,473 / events 201 / lib 38），
> tests 2,437（7 个集成测试文件），examples 988，wasm 出口 531，GUI 消费面 677。
>
> 一句话定性：**这是一个「证据锚定优先、功能面刻意收窄」的 headless 图模型内核——
> yrs CRDT 真集成（持久化/协作/撤销三合一）、命令回执/事件/指纹三面工程质量高，
> 但渲染侧文本占位、无 z 序、无命中、无导入导出、无版本迁移，功能水位约为
> Excalidraw 的 1/10、tldraw 的 1/20；自研方向本身与研究底稿裁决一致。**

---

## 0. 审计范围与消费面拓扑

```
crates/canvas (mdc-canvas)
  ├─ session.rs  1,148 行：领域模型 + 8 命令 + yrs 载体 + 双撤销栈 + 指纹
  ├─ events.rs     201 行：事件中枢 + updates/touched 双收集器
  ├─ render.rs   2,473 行：几何/变换/Shape 6 图元/SVG 串/扫描线光栅/PNG 编码
  └─ lib.rs         38 行：re-export（公开面零 yrs 类型）
消费面：
  ├─ mang-shell/src/canvas_view.rs (677)：GPUI 显示（路线 A′：全彩位面）
  ├─ mdcore-wasm/src/canvas.rs (531)：WasmCanvas 八命令 JSON 出口 + 事件轮询
  ├─ crates/relay/tests/canvas_harness.rs：SyncReplica 门面接入证明
  ├─ web/src/canvas_view.ts：模型面（WasmCanvas 未入 pkg 时降级显示——i18n cv.modelPending）
  └─ tests/：concurrency / matrix / undo_redo / events / render / golden / evidence_assoc
     examples/：bench_canvas / bench_render / e2e_demo / gen_fixtures / render_demo
```

---

## 1. 数据模型与 yrs 的角色（必答题 1）

### 1.1 模型表示

- **对象三态判别**：`CanvasObject { id, kind: Node|Edge|Group, x/y/w/h, from/to, members, props: BTreeMap<String,String> }`（session.rs:67-82）。Node/Group 用几何（左上角+尺寸，Edge 恒 0）；Edge 用端点引用（节点 ID）；Group 用成员引用集。
- **布局无专门表示**：无自动布局、无 z 序字段、无锚点/绑定/ports。边的路由 = 两端节点矩形中心直线（render.rs:1054-1072），无箭头、无正交路由、无避障（对比 canvas-c drawio 的 ELK/libavoid 全部缺位）。
- **开放属性面**：`props` 为字符串键值（session.rs:81），渲染语义由 props 协议驱动（shape/label/fill/stroke/dash/rotation/opacity/font_size/points/image_data，render.rs:989-1006）。这是「数据极简 + 渲染面解释」的取舍：模型层永不因新增样式而改 schema，代价是类型安全全失（非法值静默回退缺省，render.rs:1002-1003「渲染面永不 panic」）。

### 1.2 yrs 承担什么——是真集成

载体形状：根 `Map("canvas")` → 对象 ID 为键 → 嵌套 Map（`kind/x/y/w/h/from/to/members/props`，session.rs:26-36, 792-806）；props 为嵌套 YMap；**members 为 `Any::Array` 普通数组而非 YArray**（session.rs:1018-1027）——组成员是整值 LWW 替换，无逐成员 CRDT 语义。

yrs 同时承担三件事，全部是真委托而非「仅用数据结构」：

| 职责 | 实现 | 证据 |
|---|---|---|
| 持久化 | `.canvas` = yrs 二进制权威快照（无自有 magic/版本头） | session.rs:556-567；smoke.canvas 头部 `01 48 81 ba...` 即 yrs update 编码 |
| 协作 | state_vector / encode_diff / apply_update + panic 隔离 + 52MB 入向限额 | session.rs:505-554；concurrency.rs:23-119 双向收敛测试 |
| 撤销 | `yrs::undo::UndoManager` ×2（Human/Ai 分栈），**非自研栈** | session.rs:276-277, 328-369 |
| 并发合并语义 | Map 键级 LWW（位置/尺寸高频更新的自然语义） | Cargo.toml:20-21；concurrency.rs:80-119 |

**公开面零 yrs 泄漏**（lib.rs:27-38 re-export 全为自有类型；CanvasError 十变体无 yrs 类型，session.rs:85-117）——crate 边界可独立撤除的纪律成立（Cargo.toml:13-16）。

### 1.3 为何 canvas 要 CRDT（而文档「不要」）——事实澄清 + 判断

事实澄清：**文档域 mdc-collab 同样用 yrs**（同构不变量集：skip_gc 无条件 / 捕获窗 0 / client_id 53 位域 / 入向限额 + panic 隔离 / 双撤销栈 origin 分栈 / capture_transaction 拒 None-origin，Cargo.toml:16-18，session.rs:1-6）。所以真实差异不是「canvas 要 CRDT、文档不要」，而是**两域各自独立 Doc、不建跨引擎统一 CRDT**（lib.rs:7-8）。

该选型的必要性论证（成立）：
1. AI + Human 经同一命令接口并发写同一画布（lib.rs:1-2），键级 LWW 对拖拽类高频写是自然合并语义；
2. local-first 离线自愈免协议（06_R2 §1.2 D1 建议「统一在 CRDT 侧、yrs 起步」——本实现与研究底稿裁决方向一致）；
3. 对象身份 = Map 键 → 删除撤销同 ID 回归（session.rs 文档 lib.rs:11-14；undo_redo.rs:125-160 钉定），使证据锚获得「删除复原语义连续性」——这是**复用成熟引擎拿不到的性质**（tldraw/Excalidraw 的撤销与本项目 mdc-evidence 指纹体系无耦合）。

成本登记（真实存在）：
- LWW 退化：无意图归因/审计（06_R2 §1.2 反对因素①——需产品层外挂，本仓以 Actor 分栈 + origin 溯源部分缓解）；
- 撤销语义受上游支配：yrs 0.28 map-undo 同会话同键交错撤销 = 逐笔回退而非「移除该 origin 全部写」，已探针钉定并登记为上游询问候选（undo_redo.rs:204-235）；
- 二进制格式不透明 → 版本迁移被动（见 §5）；
- yrs 依赖锁定 0.28（Cargo.toml:33），Yjs 内核 v13/v14 断代风险由「只依赖冻结协议」缓解（06_R2 §1.2 反对因素②），但本仓直接依赖库本体而非仅协议。

---

## 2. 与文档 Core / mdc-evidence 的关系（必答题 2）

### 2.1 与文档域：完全独立，无共用状态

- mdc-canvas 产品依赖仅 `yrs + mdc-evidence`（Cargo.toml:32-34）；mdc-collab 只出现在 dev-dependencies 且仅 e2e_demo 使用（Cargo.toml:36-40）。
- **无任何跨域引用机制**：画布对象不能引用文档块，文档块不能锚定画布对象。两域唯一交汇点在 mdc-evidence 契约层（DocumentRef + EngineKind::Canvas/Doc 并列）。
- e2e_demo.rs（432 行）演示的是**并列协作**而非融合：文档域与画布域各自同步链 + 证据系统分别登记两域指纹（e2e_demo.rs:53-57 两个独立 sync 函数）。

**独立模型是否更合理**：v1 合理。「不建跨引擎统一 CRDT」是显式架构纪律（避免跨引擎事务/合并语义纠缠）。但研究底稿 D2（AFFiNE 两视图投影：几何/z 序/displayMode 作为 block 普通字段，06_R2 §3.2 六条）指向的「画布上的文档卡片」能力目前零基础——CanvasObject 无任何指向文档块的语义位。这是**产品级缺口登记**而非当前错误：若 D2 立项，需要新的关联载体（如 props 约定键或扩展字段），届时 R3 风险（schema 膨胀须配显式版本迁移，06_R2 §3.3）即刻生效。

### 2.2 evidence_assoc 测试测什么

evidence_assoc.rs（198 行）完整钉定「画布作为受证文档」链路：
1. 画布注册为 `DocumentRef(EngineKind::Canvas, canvas_fingerprint())`（evidence_assoc.rs:20-27, 78-79）；
2. 证据锚 = `Locator::CanvasObject { object_id, object_fingerprint }`（evidence_assoc.rs:80-83；locator.rs:132, 147）；
3. **MoveNode 位置漂移 → 指纹失配 → LinkageStatus 降为 Stale（只降不升）**（evidence_assoc.rs:105-129）——「位置即语义」是画布域证据粒度的显式决策（lib.rs:18-21）；
4. 显式 reverify（新指纹 + 新版本引用）恢复 Valid，审计链三事件齐备（evidence_assoc.rs:131-149）；
5. 跨端同步 + AI 并发改写 → 人类端再失效 → AI 端撤销（自己的栈）→ 语义回归可重验（evidence_assoc.rs:151-192）；
6. 三态正交：重验不复位 ReviewStatus（evidence_assoc.rs:193-197）。

指纹器：对象语义字段确定性序列化（几何 4 位小数 / 边端点 / 组员排序 / props 字典序）→ FNV-1a 64，与 mdc-evidence 同指纹器（session.rs:1111-1138）；全画布指纹 = 逐对象指纹拼接再哈希（session.rs:632-640）。读模型确定性（objects() 按 ID 排序，session.rs:604-612）保证指纹确定性。

**这是全仓独一份的能力**：tldraw/Excalidraw 均无「画布对象 → 外部证据系统 → 失效降级」链路；它是自研路线最有分量的差异化论据（见 §8）。

---

## 3. Undo/Redo（必答题 3）

**实现 = yrs UndoManager 原生语义 + 分栈包装，非自研栈**（session.rs:328-369）：

- 双 `UndoManager`：`tracked_origins = {mdc:human}` / `{mdc:ai}`（session.rs:334-335, 160-166）——origin 分栈，Human/Ai 交错互不干扰（undo_redo.rs:164-235：Ai 栈 3 项独立回退，Human 栈 4 项不动）；
- `capture_timeout_millis: 0`：**每事务一栈项，无捕获窗**（session.rs:362）——对比 Yjs 默认 500ms 合并窗，本仓选择显式逐项（与 mdc-collab 同构纪律）；
- `capture_transaction` 拒 None-origin：**远端应用不入本地撤销栈**（session.rs:364）——撤销只回退自己的操作，这是并发编辑下撤销语义的正确姿态；
- 时钟注入面：`SessionOptions.clock_ms`（wasm32 无 std 时间——SystemTime::now() = trap，session.rs:176-181, 336-360；wasm 侧 AtomicU64 + set_clock_ms 推进，mdcore-wasm/src/canvas.rs:352-356, 416-418）；
- 撤销/重做经 collector/touched 包裹后派发 Undo/Redo 事件（session.rs:417-454）。

**并发编辑下的撤销语义（测试钉定）**：
- 删除 → 撤销 → **同 ID 回归 + 指纹复原**（undo_redo.rs:125-160）——Map 键身份稳定，对比文档域 XML 元素复活 = 新 ID 族（域差异被测试显式钉定）；
- **已知上游语义两处**（诚实登记 + 探针防漂移）：
  ① 同会话同键交错写（Human/Ai 轮流 MoveNode 同一节点）的撤销 = **逐笔回退到该笔历史点值**，而非「移除该 origin 的全部写后取另一 origin 末笔」（undo_redo.rs:204-226，注释明确「yrs 0.28 map-undo 交错语义，已登记为已知语义（上游询问候选）」）；跨会话编辑撤销不受影响（evidence_assoc.rs:175-192 证明）；
  ② 对栈先撤后的 pop-until-productive 语义：无产出项被丢弃、一次 undo 消费多项（undo_redo.rs:223-234）。
- 软上限 = **clear_all 粗粒度**：超限清空整栈（非修剪最旧），显式回执 `undo_cleared`（session.rs:480-501；undo_redo.rs:239-285：第三笔超限 → depth 0，数据面不受影响）；
- 快照恢复不带撤销栈（session.rs:371-376；concurrency.rs:166-168）。

**缺口（对照三家引擎）**：无手势聚合原语。Excalidraw 用三态捕获标签（EVENTUALLY 自动聚合拖拽中间态，canvas-a §3）、tldraw 用显式 mark + squashToMark（canvas-b §1）、AFFiNE 用 stash/pop 拖拽事务化（06_R2 §3.1）。本仓捕获窗恒 0 且未提供 mark/squash API——宿主必须自行实现「pointer-up 才提交」的聚合（bench_canvas.rs:18-19 注释把「拖动预览的提交粒度」责任推给宿主侧）。高频拖拽逐笔提交会线性刷爆撤销栈，而兜底手段只有 clear_all 全清。编辑 UI（B 路线）落地前这是 P1。

---

## 4. 坐标系与渲染（必答题 4）

### 4.1 坐标系与变换

- 画布逻辑坐标：y 向下（屏幕系约定），`rotate` 弧度、顺时针为正（render.rs:705-706，测试 render.rs:268 `rotate_is_clockwise_in_screen_coords` 钉定）；
- `Transform = T·R·S` 绕 origin（默认形状支点=包围盒中心，render.rs:699-793）；`Affine` 2×3 矩阵（render.rs:630-697）；变换折进形状几何时线宽按 scale_factor 各向同性放大（render.rs:795-879）；
- 对象→图元：`shape_from_object`（render.rs:1007-1179）props 驱动，Group 不画、悬空边跳过不 panic（render.rs:1053, 1060-1061）——并发删除留下的悬空边在读模型合法（session.rs:100-104 注释），渲染面容错。

### 4.2 命中测试

**不存在几何命中测试**。模型级 `find_object` = Map 键 O(1) 查找（bench_canvas.rs:173-188 以 find 为「命中查找代理」，诚实标注「模型级」）。无点选、无框选、无 OBB/逆变换——交互编辑（B 路线）的几何前置完全缺位。对比：Konva color-key 像素命中 O(1)、Fabric O(N) 倒序线性扫 + OBB、tldraw Geometry2d（canvas-e §4.3 对照表）。

### 4.3 双后端与渲染质量

- **架构**：`CanvasRenderer` trait（draw/draw_all，render.rs:887-896）+ `CanvasRendering` 渲染模型（shapes + background，render.rs:905-985）——「数据层自持 + 渲染层可换」的架构位已预留（与研究底稿 R2 §2.4 横贯结论同构），当前两个后端：
- **SVG 后端**（零依赖字符串拼接，render.rs:1377-1585）：数字规范化 `num()`（4 位小数去尾零、负零归一，render.rs:1303-1315）、XML 文本/属性转义（render.rs:1317-1348）、viewBox 含描边外扩（`canvas_bounds` vs `canvas_bounds_geometry` 分离——render.rs:1620-1624 注释记录了「PNG 视口必须用几何包围盒否则粗描边消失」的踩坑教训）。**SVG 文本出真字形**（浏览器渲染 `<text>`，render.rs:1484-1505）。
- **光栅后端**（自研扫描线，render.rs:1752-2235）：解析覆盖率积分（sample_rows 纵向逐行解析而非数采样点，render.rs:1796-1826——注释记录「4px 黑描边画出 80/255 灰」的修复）、胶囊线光栅 + 端帽圆补（render.rs:1879-1936）、闭区间端点补偿（1px 线不淡，render.rs:1827-1874）、贝塞尔自适应展平（FLATTEN_TOLERANCE 0.1，render.rs:6-7）。像素工程质量注释密度高（每处踩坑都有归因），质量对得起零依赖实现。
- **两处诚实登记的降级**：
  ① **光栅文本 = 带基线的空描边框**（render.rs:2197-2221：「光栅后端不做字形……空框一眼能看出是占位。要真字形请走 SVG」）；golden_data.rs:70 同口径「文字（SVG 出真字形；PNG 出已登记的占位框）」；
  ② 光栅图像 = 虚线占位框（render.rs:2222-2232），image_data 十六进制协议仅在 SVG 后端真正内嵌 base64（render.rs:1507-1530）。
- **GUI 显示路径直接受害**：mang-shell 路线 A′ 用 RasterRenderer 出全彩位面（canvas_view.rs:14-26）——因 GPUI `paint_svg` 是单色蒙版通道（canvas_view.rs:4-13 源码有据的路线否决，工程判断优秀）。结果是 **.canvas 在编辑器里文字标签显示为空框**：smoke 夹具 3 个文本节点（输入/处理/输出，canvas_view.rs:474-491）上屏即占位框。显示面只读的 v1 里这是最直观的产品质量缺口（P0，见 §10）。

### 4.4 golden.svg 字节级比较的跨平台脆弱性

- golden.rs:62-71 `golden_svg_matches_fixture` 字节级断言；golden.png 同为字节级（golden.rs:74-80）；
- **CI 已知跳过**：windows-test.yml:83-88 `cargo test -p mdc-canvas -- --skip golden_svg_matches_fixture`，理由注释：夹具含三角函数结果（旋转矩形 sqrt(2) 分量如 145.4437/31.1127），「Windows 与 Linux 的浮点格式化末位有差异，字节比较必然失败」，并登记「跨平台应改容差比较（待后续任务）」；
- 根因判读（本审计推断）：Rust 自身 float 格式化跨平台确定；真正的漂移源是 **libm sin/cos 的末位 ULP 差异**（MSVC vs glibc）→ 经 `num()` 4 位小数量化后末位翻转。即脆弱性来自「三角派生几何 + 高精度数字输出」的组合，而非格式化本身。缓解路径：数字输出降位（2 位小数，Excalidraw 同款，canvas-a §4）、或 golden 比较改解析后容差。当前状态 = **SVG 渲染回归在 Windows CI 上是盲区**（golden.png 整数像素字节不受 libm 影响，未跳过，是唯一跨平台护栏）；
- 交叉验证（加分项）：PyMuPDF 光栅化 golden.svg 与自产 PNG 逐像素比对，阈值 12%（golden.rs:95-169），缺 python/PyMuPDF 打日志跳过（不阻塞）——「两条独立实现互证」的思路正确，但跳过面较宽。

### 4.5 性能面（bench 实测口径）

- **bench_render.rs**：1,000 图元（250 矩形 + 250 椭圆 + 250 线 + 150 路径 + 100 文字）@1024×768，出口 SVG 与 PNG 分别计时，门槛 100ms（issue #125 验收），**只报告不失败**（bench_render.rs:124-125 注释：「基准在 CI 因机器差异而红，会让人开始忽略它」——诚实但无自动护栏）；
- **bench_canvas.rs**：模型面七场景（load/move/find/readmodel/fingerprint/sync/delete），1,000 基准 + 10,000 压力双档，LCG 确定性伪随机 + p50/p95/p99 + VmHWM 峰值 RSS（bench_canvas.rs:16-25）；边界诚实登记：「帧表现/拖动跟手性 = 渲染器面，待宿主原型补测；本基准为其提供模型侧下界」；
- GUI 侧缓存纪律（canvas_view.rs）：光栅缓存键 =（内容版本 rev，光栅缩放 bits，位面尺寸）（canvas_view.rs:314-351）——**平移只挪绘制四边形不重光栅、缩放/内容变更才重建**（测试钉定 canvas_view.rs:594-626）；护栏 4096 边 / 8M 像素（canvas_view.rs:74-75）。

---

## 5. 持久化与版本迁移（必答题 5）

- `.canvas` = `authoritative_snapshot()` 的 yrs 二进制 wire（session.rs:556-567），**无自有 magic、无版本头、无 schema 描述**（smoke.canvas 首字节即 yrs update 编码）；
- 空文件 = 空会话（新文件语义，canvas_view.rs:146-150）；破损快照显式报错不静默白屏（canvas_view.rs:668-676 测试钉定）；快照恢复后撤销栈空、指纹同一、可继续编辑与同步（concurrency.rs:121-186）；
- 维护面：`maintenance_gc`（清双栈 + 显式 gc + wire 字节数对比，session.rs:571-592；concurrency.rs:188-226）；
- **版本迁移策略 = 缺失**。模型演进（新增对象字段/props 协议变更）无任何迁移机制：旧快照读入时未知字段被 `read_object` 静默丢弃（session.rs:1066-1095：`num()` 缺字段回 0.0、str_field 回 None、props 只收字符串值）——「缺字段默认值回填」事实上形成前向兼容（同 Excalidraw restore 归一化路线，canvas-a §4），但这是**无意识的兼容**而非设计：无 schemaVersion 字段可判定新旧，无法做有损告警。yrs 库本身升级（0.28 → 未来）的 wire 兼容性风险同样被动承接。对照：tldraw 三层显式迁移链 + schema 随行文件（canvas-b §1）；研究底稿 R3 风险原样命中（06_R2 §3.3：「画布 props 进模型后必须配显式 schema version + 迁移器」）。**props 协议目前是渲染面的隐式契约（render.rs:989-1006），协议本身漂移无版本位**——当前影响小（缺键回退缺省），随 props 键语义增殖会变成真实债。
- 会话态（视口 zoom/pan）不入 `.canvas`（无 tldraw session-scope 概念，canvas-b §1 scope 三分对照缺位）。

---

## 6. 导入导出（必答题 6）

- **出口**：`export_svg` / `export_png`（render.rs:1550, 2450）——自有图元集，非任何第三方格式；PNG 导出护栏 65536 边 / 64M 像素（render.rs:8-9）；
- **excalidraw / tldraw / drawio 导入导出：零**。全仓 `crates/**` 对 `excalidraw|tldraw|drawio|mxgraph|.tldr|.excalidraw` 的 grep 无一命中（本审计执行，含 mdcore-wasm 与 office 域）。mermaid→图 管线（Excalidraw 的 AI 生成主链，canvas-a §7）同样零。
- **判定：能力缺口，且未见显式范围外声明**——Cargo.toml/模块文档/测试均未登记「不做导入导出」的决策（对比「显示面只读」「B 路线未接」均有登记）。研究底稿已备齐三家的格式深读材料（canvas-a §4 场景 JSON 结构、canvas-b §1 .tldr schema、canvas-c drawio XML 往返），实现时的净室输入就绪。对「AI 工作台画布」定位而言，至少 .excalidraw（MIT、纯 JSON）读取是低成本高回报的第一步。

---

## 7. 大型画布性能（必答题 7）

**结论：无视口剔除、无空间索引、全量重渲染（模型层全量快照 + 渲染层全量重光栅）。**

- 模型读面：`objects()` 每次全量遍历根 Map + 逐对象深读 + ID 排序（session.rs:604-612）——无增量读模型（事件 touched 已给出受触对象 ID，events.rs:152-198，但 `objects()` 不消费它）；
- 渲染模型重建：`CanvasRendering::from_objects` 全量重建；`shape_from_object` 的边端点查找 = `all.iter().find(...)` 线性扫（render.rs:1055-1061）——**每条边 O(N)，全模型 O(N·E)**；万级节点 + 千级边时每帧（每次 rev+1）重建成本显著；
- 光栅：全画布一次性光栅化（canvas_view.rs:377-389 遍历全部 shapes），无脏区、无瓦片、无 LOD；仅有的优化是**整幅位面缓存 + 相机四边形移动**（= 研究底稿「吞吐三招」中「相机合成器卸载」的粗粒度版，06_R2 §2.2）；Excalidraw 的 per-element 位图缓存 / tldraw 的 R-tree culling + 置换缓存 / Penpot 的脏瓦片均无对应物；
- 撤销栈无上限默认（SessionOptions.undo_stack_limit = None，session.rs:201）——长会话内存单调增长（yrs skip_gc=true 加剧 tombstone 留存，这是 mdc-collab 同款取舍）；
- 实测水位：bench_canvas 到 10,000 节点（模型面）；bench_render 到 1,000 图元 @1024×768。**万级 GUI 帧率未测**（诚实登记）。研究底稿推断 Excalidraw 千级舒适/万级天花板（canvas-a §8）、tldraw 万级（canvas-b §8）——当前架构形态与 Excalidraw 同级（全量重绘路线），但因「全幅重光栅（无元素级缓存）」在连续缩放交互下更粗糙：每次 zoom 步进都重建整幅位面（canvas_view.rs:606-621 测试钉定「缩放重光栅」）。

---

## 8. 对照 canvas-line 研究底稿：水位与自研 vs 复用裁决（必答题 8）

### 8.1 对标水位表

| 维度 | 本仓 mdc-canvas | Excalidraw（canvas-a） | tldraw（canvas-b） | 研究底稿建议位 |
|---|---|---|---|---|
| 数据模型 | 3 对象类型 + 开放 string props | 12 类型判别联合 + 版本三元组 + 软删除 | 13 shape + shape util 注册表 + scope 三分 | shape util 注册表（P0 移植） |
| z 序 | **无**（渲染序 = ID 字典序） | fractional index（CC0 vendored） | fractional index + FixIndexKeys 迁移 | fractional index（R4 风险登记） |
| 协同 | yrs YATA（键级 LWW + 独立 Doc） | 元素级 LWW + nonce 平局（下限方案） | 服务器定序 + git 式 rebase | **统一 CRDT 侧、yrs 起步（D1 建议一致✓）** |
| 撤销 | yrs UndoManager 双 origin 栈，无手势聚合 | 快照 diff + 三态捕获（EVENTUALLY 手势合并） | mark + squash diff 双栈 | mark 显式化 + diff 自动 squash 折中 |
| 渲染 | SVG 串 + 扫描线光栅双后端，文本占位 | 双 canvas + per-element 位图缓存，真字形 | DOM/SVG 保留 + 命令式 CSS + R-tree culling | 数据自持 + 渲染可换（**trait 已备✓**） |
| 命中测试 | 无 | 线性扫 AABB（自标 bottleneck） | Geometry2d + R-tree | 几何命中为主 |
| 文本 | SVG 真字形 / 光栅占位框 | 自制排版（CJK/emoji 规则 + 宽度缓存） | 真 DOM + 隐藏测量池 | — |
| 版本迁移 | **无**（缺字段回填式被动兼容） | restore 归一化（无 schemaVersion） | 三层显式迁移链 + schema 随行 | 显式 version + 迁移器（R3） |
| 格式进出 | 仅自产 SVG/PNG | .excalidraw JSON + PNG/SVG 回嵌 + 加密 | .tldr + IndexedDB + schema 协商 | — |
| AI 面 | 8 命令受控集 + Actor 双源 + 回执 | 无头契约 + mermaid 管线 | exec + focused proxy + updateModelContext + checkpoint 四件套 | 四件套为直接模板 |
| 测试 | 59 #[test] + 双基准 + e2e + 跨实现交叉验证 | ~2,000 用例 + 覆盖率门禁 + size-limit | （canvas-b §10） | — |

**功能水位定量**：命令 8 个（Excalidraw 96 action、tldraw 工具状态机 18 叶子）；图元 6 种（Excalidraw 12 元素类型、tldraw 13 shape）。约 1/10 与 1/20 的量级判断成立——但见下：功能数量不是裁决变量。

### 8.2 「不能仅凭功能数量决定自研还是复用」——实质性论据

**自研的五个硬论据（本仓现状 + 底稿裁决互证）**：

1. **数据模型所有权 = 证据链所有权**。mdc-evidence 的 `Locator::CanvasObject` 指纹锚、「位置漂移即 Stale」「删除撤销同 ID 复原指纹连续」（§2.2）是本产品独有的语义契约。嵌入 Excalidraw/tldraw 则对象身份/撤销/序列化全归引擎，指纹与撤销复原语义不可达——这套能力**只能长在自持模型上**（tldraw 的 shape util 注册表思想正是为此，canvas-b §13-1）。
2. **Rust 单一真源**。mdcore-wasm 四表面同构（WasmCanvas 与 WasmCore/WasmBook/WasmWorkbench 同模式，mdcore-wasm/src/canvas.rs:3-9）；tldraw/Excalidraw 均无 Rust headless 形态（React 组件依赖使 headless 嵌入不可行——canvas-a §12「关键判断」、canvas-b §12 同结论）。嵌入意味着核心状态离开 Rust 域，wasm 桥、relay SyncReplica（canvas_harness.rs:15-34）、yrs 协作面全部重做。
3. **离线/嵌入约束**。产品定位 local-first 免协议离线自愈（06_R2 D1 问③）；tldraw 协同需 Durable Object 服务器定序（canvas-b §5），Excalidraw 协同是「整元素覆盖 + 20s 全量重播」的薄层（canvas-a §5）——都不能满足「AI + Human 同画布离线并发」。
4. **许可**。tldraw = source-available，npm 依赖即触发商业链（ECDSA 验签 + 生产熔断 + 水印 + 遥测回传部署 URL），直接依赖 = 排除（canvas-b §11、06_R2 §7 表 7 行）；Excalidraw MIT 但 React 绑定（§8.1 引证）。渲染基座 Konva/Fabric/PixiJS = 3×MIT（canvas-e §0.2 逐仓核验）**可作可替换后端候选**——注意这与自研内核不冲突，恰是 CanvasRenderer trait 的备选实现。
5. **AI 写入通道**。8 命令受控集 + Actor 双源分栈撤销 + 逐事务 wire 事件（updates 可独立转发）= 「agent 走同一 op 通道 + 幂等命令 + 审计」的 06_R2 §1.2 问③形态，Excalidraw/tldraw 均不提供 origin 分栈撤销。

**复用方的真实论据（须诚实登记）**：若产品目标其实是「通用手绘白板」而非「结构化图形 + 证据锚定的 AI 工作台」，则 Excalidraw（MIT）数据原语 + 宿主壳是数量级更快路径；文本真字形、笔迹、选区/句柄、导入导出、双 canvas 性能工程全是现成的。**裁决因此取决于产品定位而非功能数**——本仓所有文档（lib.rs:1-2「图形状协作共享状态 + 人类与 AI 经同一命令接口」、Cargo.toml:7-30）一致指向后者，自研方向正确。

**裁决一句话**：维持自研内核（数据模型 + 命令 + 协同 + 证据面），渲染层按 CanvasRenderer trait 保留可换（必要时以 MIT 基座补位），把 tldraw/Excalidraw 当净室设计输入而非依赖——与 06_R2 §2.3 选型矩阵（「自研 store + shape util 注册表 + 几何命中为主」+ 基座备选 Fabric > Konva）逐条吻合。

### 8.3 应从底稿吸收而尚未吸收的设计（自研方向的补课清单）

1. shape util 注册表思想（canvas-b §13-1 P0）：当前 shape 类型判别硬编码在 shape_from_object 的 match（render.rs:1052-1170），props 协议无注册面；
2. agent 四件套（canvas-b §13-3 P0）：③结构化状态回推、④checkpoint 寻址缺位（wasm 面只有命令 + 事件轮询）；
3. mark + squash 手势聚合（canvas-b §13-4 / canvas-a §13-3）；
4. fractional index z 序（CC0 可 vendor，canvas-a §13-8、R4）；
5. SVG 白名单清洗器思路（canvas-b §13-7）——当前 export_svg 的 xml_escape 只转义 &<>"'，无 URI/动画面清洗（导入面为零时风险低，导入能力一旦落地即为硬门）；
6. 显式 schema version + 迁移器（R3）。

---

## 9. 能力矩阵（当前交付面）

| 能力 | 引擎面（mdc-canvas） | GUI 面（mang-shell） | wasm 面（mdcore-wasm） | 成熟度 |
|---|---|---|---|---|
| 命令执行（8 命令） | ✅ 受控集 + 校验 + 回执 | ⚠️ 通路在、无编辑 UI（dead_code 登记 canvas_view.rs:208-252） | ✅ flat JSON + typed 错误码 | 引擎完备 |
| 撤销/重做（双栈） | ✅ origin 分栈 + 软上限 | ⚠️ 同上 | ✅ | 引擎完备；无手势聚合 |
| 并发协同 | ✅ SV/diff/apply + panic 隔离 + 限额 | ❌（无网络接线） | ✅（updates base64 可转发） | 引擎完备；relay 仅 harness 证明 |
| 事件订阅 | ✅ 一入口一事件 + touched 对象 ID | ❌（rev 计数自管） | ✅ take_events_json 轮询 | 完备 |
| 证据关联 | ✅ 对象/全画布指纹 + 三态降级 | ❌ | ✅ fingerprint 出口 | 完备（差异化能力） |
| SVG 导出 | ✅ 零依赖、真字形、转义 | ❌（仅引擎出口） | ❌（wasm 未暴露 export_svg） | 可用 |
| PNG 导出 | ✅ 自研光栅 + zlib stored | ⚠️（内部位面用） | ❌ | 可用；体积大、文本占位 |
| 视口交互 | — | ✅ 缩放/平移/缓存/护栏（缩放锚点=视图中心） | — | v1 良好 |
| 几何命中 | ❌ | ❌ | ❌ | 零 |
| z 序/图层 | ❌ | ❌ | ❌ | 零 |
| 文本编辑/富文本 | ❌ | ❌ | ❌ | 零 |
| 导入（excalidraw/tldraw/drawio/mermaid） | ❌ | ❌ | ❌ | 零 |
| 版本迁移 | ❌（被动回填） | — | — | 缺位 |

---

## 10. P0-P3 分级（置信级别 + 证据链）

### P0（产品阻断：当前交付面上用户可感知）

| # | 问题 | 置信 | 证据链 |
|---|---|---|---|
| P0-1 | **GUI 文本渲染为占位空框**：.canvas 在 mang-shell（路线 A′ 光栅路径）中所有文本节点显示为带基线描边框，标签不可读；SVG 真字形仅在导出面可达 | 高 | render.rs:2197-2221（诚实登记）→ canvas_view.rs:14-26（A′ 路线：GPUI paint_svg 单色蒙版否决）→ canvas_view.rs:377-389（rasterize 全量 RasterRenderer）→ golden_data.rs:70（「PNG 出已登记的占位框」）；smoke 夹具含 3 文本节点 canvas_view.rs:474-491 |
| P0-2 | **SVG 渲染回归在 Windows CI 盲区**：golden_svg 字节级比较被 CI 显式跳过，无容差替代护栏 | 高 | windows-test.yml:83-88（--skip + 理由注释 + 「待后续任务」）；golden.rs:62-71（字节级断言）；根因 = libm sin/cos 末位 ULP（旋转矩形 sqrt(2) 分量 145.4437/31.1127）经 num() 4 位量化的末位翻转（render.rs:1303-1315） |

### P1（显著缺口：B 路线/规模化前置）

| # | 问题 | 置信 | 证据链 |
|---|---|---|---|
| P1-1 | **撤销无手势聚合原语**：捕获窗恒 0、无 mark/squash API；高频拖拽逐笔刷栈，兜底只有 clear_all 全清；Excalidraw/tldraw/AFFiNE 三家均有显式方案 | 高 | session.rs:362（capture_timeout_millis: 0）；undo_redo.rs:239-285（超限全清回执）；bench_canvas.rs:18-19（提交粒度责任推宿主）；对照 canvas-a §3 / canvas-b §1 / 06_R2 §3.1 |
| P1-2 | **无 z 序模型**：渲染序 = ID 字典序（对象创建序的代理），无重排命令、无 fractional index；「图形状模型」缺基础分层语义 | 高 | session.rs:604-612（objects() ID 排序）；CanvasCommand 全集无 z/reorder（session.rs:209-251）；render.rs:976-984（from_objects 按此序入 shapes）；对照 canvas-a §1 / canvas-b §1（两家均 fractional index） |
| P1-3 | **无几何命中测试**：交互编辑（B 路线）的几何前置零基础；模型级 find_object 只是键查找代理 | 高 | render.rs 全文无 hit/pick/intersect（grep 证）；bench_canvas.rs:173-188（「模型级命中代理」自认） |
| P1-4 | **导入导出零且未登记范围外**：excalidraw/tldraw/drawio/mermaid 全无；生态孤岛 | 高 | crates/** grep 零命中（本审计执行）；Cargo.toml/模块文档无「不做」决策登记 |
| P1-5 | **版本迁移缺位**：.canvas 无版本头、props 协议无版本位；模型演进依赖「缺字段回填」的被动兼容；yrs 升级 wire 兼容性被动承接 | 中-高 | session.rs:556-567（快照无自有头）；session.rs:1066-1095（read_object 静默回填/丢弃）；render.rs:989-1006（props 协议隐式契约）；对照 canvas-b §1（tldraw 显式迁移链）+ 06_R2 R3 |
| P1-6 | **PNG 体积失控**：zlib stored 不压缩块——golden.png 278KB @240×290（≈9.6 字节/像素）；大画布导出与 GUI 位面内存放大 | 高 | render.rs:2376-2396（zlib_store「不压缩，但结构合法」）；fixtures 目录实测 278,778B |

### P2（结构性债：规模化/语义边角）

| # | 问题 | 置信 | 证据链 |
|---|---|---|---|
| P2-1 | 全量重渲染 + O(N·E) 渲染模型重建：边端点线性扫、objects() 无增量消费 touched、全幅重光栅无脏区/瓦片/元素缓存；万级 GUI 未测 | 中 | render.rs:1055-1061（all.iter().find）；session.rs:604-612；canvas_view.rs:377-389；events.rs:152-198（touched 已有但未被读模型消费）；bench 登记口径 bench_canvas.rs:16-25 |
| P2-2 | members = Any::Array 整值替换（非 YArray）：并发组成员变更 = 整组 LWW 覆盖 | 中 | session.rs:1018-1027（In::Any(Any::Array)）；无逐成员合并测试 |
| P2-3 | 文本宽度 0.6em/char 估计：CJK 全角（≈1.0em）低估 ~40% → SVG viewBox/占位框与实际渲染偏差 | 高 | render.rs:1685（shape_bounds）、render.rs:2206（光栅占位）——chars().count() × size × 0.6 |
| P2-4 | 撤销栈默认无上限 + skip_gc 无条件：长会话内存单调增长（同 mdc-collab 取舍，但画布拖拽写频更高） | 中 | session.rs:199-201（undo_stack_limit: None）；session.rs:299（skip_gc = true） |
| P2-5 | 跨实现交叉验证跳过面宽（缺 python/PyMuPDF 即静默跳过）；基准只报告不失败——两处护栏均非硬门 | 中 | golden.rs:145-158；bench_render.rs:124-125 |

### P3（登记级：低风险/良好实践确认）

| # | 项 | 置信 | 证据链 |
|---|---|---|---|
| P3-1 | 指纹几何 4 位小数截断：<1e-4 漂移不触发证据 Stale（「位置即语义」的精度下界） | 高 | session.rs:1119（`{:.4}`） |
| P3-2 | 良好实践确认：next_id 撞号跳段环（同槽 reload 复活面，session.rs:392-409 + concurrency.rs:231-290 测试）；读写事务遮蔽死锁规避（session.rs:942-950，p18 纪律注释）；空画布 1×1 退化（render.rs:1609）；GPUI svg() 单色蒙版的源码级路线否决（canvas_view.rs:4-13） | 高 | 各处一手注释与测试 |
| P3-3 | render.rs 无模块级文档（其余三文件均有工程文档头）；wasm 未暴露 export_svg/export_png | 高 | render.rs:1（直接 use 起始）；mdcore-wasm/src/canvas.rs 方法清单 |
| P3-4 | yrs 0.28 锁定 + Yjs 内核代际风险由依赖库本体（非仅协议）承接 | 中 | Cargo.toml:33；06_R2 §1.2 反对因素② |

---

## 11. 总评

工程纪律面（不变量集同构、公开面零 yrs 泄漏、诚实登记文化、踩坑归因注释、夹具进仓纪律、探针钉定上游语义）是**全仓样板级**；差异化能力（证据三态关联 + 删除复原指纹连续 + origin 分栈撤销 + touched 深观察事件）是任何成熟引擎都给不了的资产。但交付面上它是「模型内核强、呈现与交互弱」的偏科生：文本占位、零命中、零 z 序、零导入、零迁移——这五件事在编辑 UI（B 路线）落地前都不是 bug，落地那天全部变成 P0。研究底稿的裁决（自研 store + shape util + 渲染可换 + yrs 协同）与仓内实现方向一致，**方向对，水位低，补课清单明确**。
