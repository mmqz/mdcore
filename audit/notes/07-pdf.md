# 07 · PDF 域深度审计（mdcore-audit main@96596d7，2026-11-04）

> 只读审计。范围：`crates/pdf`（src 25 文件 12,325 LOC + tests 16 文件 10,504 LOC / 242 `#[test]`）
> + `crates/pdf-wasm`（257 LOC）+ 消费面 `mang-shell/src/editor/pdf_doc.rs`（817 LOC，
> tests/render_170_pdf.rs）+ `docs/pdf-{annotate,render-ext,organize,ocr}.md`。
> 证据口径：`文件:行号` 为 HEAD 实测；版本锚 = Cargo.lock（lopdf **0.45.0**、
> pdfium-render **0.9.4**，均 pin，与 ADR-P1 一致）。参照底稿：
> `mang-research-archive/cleanroom-analysis/md-line/pdf-a/ADR-P1-pdf-engine-selection.md`。
> loc-stats 基线：mdc-pdf 25 文件 / 12,325 LOC / 0 unsafe / 13 unwrap / 13 expect / 2 panic。

## 0. 总览事实（一分钟版）

- **架构**：三层分工严格——① Document Model（page/outline/annotation/command/document，
  零依赖常开）；② pdfium 读引擎（engine.rs trait + engine_pdfium.rs，non-V8，
  `FPDF_LoadMemDocument64` 全内存）；③ lopdf 写/结构面（writer_lopdf 增量写 +
  annotate/forms/stamp/organize/links/layers/signature/generate 十模块）。crate 间零 path
  依赖（loc-stats 邻接表：mdc-pdf → 无内部依赖），与 ADR-P1「pdfium 读 + lopdf 写」
  选型**主干落实**。
- **最重的问题**：`forms.rs` 模块文档与 `save()` 文档均声称「增量修订、原字节前缀逐位
  保持、已有数字签名仍有效（PAdES 硬约束）」（forms.rs:30-38、1037-1039），但实现是
  `Document::save_to` **全量重写**（forms.rs:1045-1052）——声称与实现直接相反，且恰好
  落在其目标场景（给已签名的投标函填值）上会**静默摧毁既有签名**（§P0）。
- 次重：stamp.rs 无 S5 加密门、无 S3-L7 输入门（全写面唯一裸奔者）；signature.rs 读面
  无输入字节门；desktop 读路径（mang-shell）完全不接线 S2/S6 安全扫描；CI 只跑
  engine-lopdf 面（windows-test.yml:75-78），**pdfium 全链证据（渲染/工人/看门狗/OCR/
  交叉读）均为本地门禁，CI 零覆盖**。
- **消费面极窄**：mang-shell 只消费 `PdfiumEngine::{page_count,outline,extract_text}` +
  `generate::generate_text_pdf`（冒烟夹具生成，pdf_doc.rs:621-633）。写面（writer/
  annotate/forms/stamp/organize/signature/links/layers/ocr）、worker/cache、security_scan
  **无任何产品消费者**——唯一第二消费者是 pdf-wasm（web 面，pdf.js 渲染 + wasm 模型）。
- 测试密度与质量**显著高于仓均**：语义锁风格（每锁一 test + 编号 + 理由），增量写有
  「前缀逐位保持」双轮断言 + pdfium 交叉读 + wasm 路径复验；xref 链事故（2026-10-31
  autocrlf 污染）有四锁防线 + 夹具金丝雀 + .gitattributes 在位断言。

---

## 1. Q1 阅读与编辑的边界：Document Model / pdfium 渲染 / lopdf 增量写

### 1.1 职责划分（实测代码证据）

| 层 | 模块 | 职责 | 对外依赖 |
|---|---|---|---|
| Document Model（常开） | page.rs / outline.rs / annotation.rs / command.rs / document.rs / security.rs / writer.rs(mock) / cache.rs / worker.rs / render.rs(trait) / protocol.rs | 页几何/Rotate 归一化（page.rs:78-97）、书签树、11 kind 标注模型、六命令 + typed 错误、S2/S3/S5 策略数值、LRU、渲染工人、协议草案 | serde（仅） |
| pdfium 读引擎 | engine.rs:17-37（`DocumentEngine` trait，object-safe）+ engine_pdfium.rs | 页数/几何(MediaBox+Rotate)/大纲/文本抽取 + `Rasterizer`（engine_pdfium.rs:276-351） | pdfium-render 0.9.4（pin，default-features=false，thread_safe） |
| lopdf 写/结构面 | writer_lopdf.rs（增量写）+ annotate/forms/stamp/organize/links/layers/signature + generate + security_scan | 标注落盘、外部标注读、表单读写、盖章、页树手术、链接/图层读、签名结构读、文本→PDF 生成、S2/S6 对象图扫描 | lopdf 0.45.0（pin，default-features=false） |

边界纪律的**类型层证据**：
- `DocumentEngine` trait 只含读面四方法（engine.rs:25-36），写路径刻意不在 trait
  （engine.rs:5-6 注释「写路径是文档级会话面」）；写器独立装配
  `attach_writer`（document.rs:269-271），与引擎**互不为前提**
  （document.rs:262-264「无引擎会话可落盘标注；有引擎会话可不装配写器」）。
- 引擎可 `detach_engine` 移交渲染工人（document.rs:251-255），所有权转移非共享引用。
- 双权威页数在装配面收口（document.rs:236-249，`AttachEngineError::PageCountMismatch`，
  T1 审计 O3 项的落地）。
- pdfium 单线程纪律两层：归属线程主纪律 + pdfium-render `thread_safe` 全局互斥兜底
  （engine_pdfium.rs:22-35）；`PdfiumEngine: Send` 有语义锁见证（pdfc_engine.rs:457）。
- FFI 边界只在引擎模块内（render.rs:6-7「S4.3」）。

### 1.2 ADR-P1 落实度评估

| ADR-P1 决策项 | 落实 | 证据 |
|---|---|---|
| pdfium（pdfium-render 0.9.4 pin）承担渲染/文本/搜索 | ✅ 完整 | Cargo.toml:23-27（=0.9.4、default-features=false、thread_safe）；engine_pdfium.rs 全实装 + pdfc_engine/pdfc_perf/pdfd_writer(交叉读) |
| non-V8 构建（S1.1 JS 不接 / S1.2 XFA 不启用） | ✅ 代码面零 JS 路径 | 全 crate 无任何 JS 执行面；forms.rs:1474-1480 明确「不接 JS」并诚实声明 `/AA /F` 格式校验不做。取件脚本 args.gn 断言在 spike 仓（本仓外，未复验） |
| lopdf 0.45 承担标注增量写（IncrementalDocument，签名友好） | ✅ 主干落实 | writer_lopdf.rs:130/326（`IncrementalDocument::create_from`）；前缀逐位断言 writer_lopdf.rs:299-305 + 测试 pdfd_writer.rs:493-540 |
| 排除 mupdf（AGPL） | ✅ | 全仓零 mupdf 痕迹 |
| qpdf 排除（无 Rust binding 主轨） | ⚠️ 部分 | qpdf 仅作**测试面交叉验证工具**（pdff_organize.rs:10-17，软门）——与 ADR 不冲突，但注意其产物验证依赖外部工具 |

**偏差点（超出 ADR 范围的自研扩张）**：ADR 只裁决「标注增量写」给 lopdf；实际 lopdf 面
已扩张为十个模块（表单/盖章/组织/签名/生成器/链接/图层），其中 **organize/stamp/layers/
flatten 走全量重写**（如实登记，organize.rs:46-49「多段修订……组织操作是结构性重写，
产物全量保存」），**forms 则是「声称增量、实为全量」**（P0，见 §3）。扩张本身有业务理由
（招标工作流），但 ADR 的「签名友好」承诺只在 writer_lopdf 一条路径上成立。

---

## 2. Q2 lopdf 增量写深度分析（writer_lopdf.rs + pdfd_writer/pdfd_session_flush）

### 2.1 增量写机制与合法性

**核心路径**（writer_lopdf.rs:284-332 `save_incremental`）：
1. 零脏短路：`!pending` → 零追加返回（:287-292）；
2. `inc.save_to(&mut out)` 追加修订（lopdf `IncrementalDocument` 写经典 xref 表 +
   `/Prev` 回指 + 新 trailer）；
3. **前缀逐位断言**：`out[..prefix_bytes] == *get_prev_documents_bytes()`，违约 →
   typed Engine 错误、不产出（:299-305）；
4. **产物自检**：重载产物，页树空（xref 链断裂形态）→ 拒绝交付（:314-322，
   2026-10-31 行尾污染事故的纵深防御）；
5. 重建基线：产物 → `IncrementalDocument::create_from(out, reloaded)`，`staged`
   映射（AnnotId→ObjectId）**跨 flush 持久**（:323-326 + :59-62）。

**xref/线性化结构合法性**：
- 追加的修订是合法的经典 xref 节 + `/Prev` 链（lopdf 0.45 语义；pdfd_writer.rs:532-536
  断言两轮 flush 后原前缀仍逐位保持 + :538-539 页数不变可重载）；
- 线性化输入档：organize.rs:34 承认「编辑后线性化本就失效」；对**增量写**路径线性化档
  未经测试（夹具皆非线性化）——spec 允许对线性化档追加修订（阅读器见 `/Prev` 后忽略
  线性化），风险低但零实证（P3）；
- **xref stream（PDF 1.5+ ObjStm）基底档**：增量写路径零测试（pdff_organize 测试无
  xref/ObjStm 匹配，`rg 'xref|ObjStm' pdff_organize.rs` 为空）；from_bytes 的
  `xref_start == 0` 守卫（writer_lopdf.rs:117-123）对 xref-stream 档的行为未实证
  （可能正常放行也可能误拒）——P2 缺口。

**幂等与脏集精确性——验证强度评级：强（四层证据）**：
| 声称 | 会话层（mock） | 真写器层 | 引擎交叉 | wasm 路径 |
|---|---|---|---|---|
| 零脏 flush 零追加 | pdfd_session_flush.rs:239 | pdfd_writer #7 | — | pdf_wasm.rs:189（幂等 +0） |
| 脏集精确（noop edit 不脏） | :278（Tap mock） | #8 编辑重脏 + #12 二轮只写脏（:1018-1055，追加量严格小于首轮 + 重载计数精确） | — | — |
| 写入序 = 脏集升序 | :239 | — | — | — |
| 失败重试安全 | :386（注入失败 → dirty 保留） | 文档面（writer_lopdf.rs:311-313 on-error 状态保持）但**无真写器失败注入测试** | — | — |
| 前缀逐位保持 | — | #2（:493-540 双轮） | #3 pdfium 读回 | pdf_wasm.rs:204-210 |
| S5 先于一切 | :332（先于零脏短路） | #6（加密双防线 + 产物零追加） | — | — |

弱点：真写器层无「save 中途失败 → 产物字节不变」的字节级断言（mock 不可证）；
无三轮以上修订链测试（两轮已证传递性，机制同构，风险低）。

### 2.2 已加密 / 已签名 / 增量历史文件的二次追加行为

- **加密档（仍加密 / 曾加密双形态）**：`from_bytes` 一律 `SecurityDeny`（writer_lopdf.rs:98-104，
  `is_encrypted() || was_encrypted()`）——写入口零放行；会话层 S5 二防线（document.rs:299-303）。
  测试 pdfd_writer #6 + pdfi_xref_chain 无误伤对照。
- **已签名档**：**完全未检测**——from_bytes 不看 /Sig 域、不看 DocMDP。行为面：
  前缀逐位保持 → 原 ByteRange 区间字节不变 → 既有签名**密码学上仍有效**（这正是
  增量写选型理由）。缺口：① 无真实签名档的「追加标注后签名仍验过」往返测试
  （signed_sample.pdf 夹具在仓，只用于 signature 读测试——加一条测试成本极低，P2）；
  ② **无 DocMDP（/Reference transformation /P）感知**——对 DocMDP 锁定的文档追加
  任何修订都是规范违约，Acrobat 会报篡改；本写器静默追加（P2）。
- **增量历史档（多段 /Prev）**：正常装载（xref_start ≠ 0 即过守卫）→ 追加新修订 →
  链延长。pdfd_writer #2 两轮链已证；wasm 测试证「增量档仍合法 PDF 可再扫描」。
- **xref 链不可信档（扫描重建装载）**：入口响亮拒绝（writer_lopdf.rs:105-123）——
  2026-10-31 autocrlf 事故防线，pdfi_xref_chain.rs:125-154 锁死（含「未污染原件不受
  误伤」对照 :151-153）。副作用（如实登记）：xref 损坏的档**不可**经写器「顺手修复」，
  只能拒绝——产品语义偏保守但正确（修复面在 organize 全量重写路径）。

### 2.3 写入破坏原文件的具体风险路径（对象号复用 / 自由条目 / trailer 合并）

| 风险路径 | 实测判定 | 证据 |
|---|---|---|
| 对象号复用冲突 | **已规避**：新对象经 `new_document.add_object` 从解析态 max_id+1 续号，**不复用**原档自由条目号；编辑重写刻意复用**同一**对象号在新修订重定义（spec 7.5.6 后修订覆盖语义，writer_lopdf.rs:265-279） | 代码路径 + pdfd_writer #8（重载无重复标注）+ #12（计数精确） |
| 自由条目（free entry）误写 | **无此面**：删除走 null 重定义（spec 7.5.10 逻辑删除，writer_lopdf.rs:719-721）而非自由链回收——物理字节留在旧修订、引用图上已死；`/Annots` 摘引用双形态（直接数组/间接引用）全覆盖（:678-718） | pdff2_annotate 交叉测试「pdfium 侧不可见」 |
| trailer 合并错误 | **纵深防御在位**：lopdf 负责 `/Prev`/`/Size`/`/Root`；自写守卫是产物重载页树可达性检查（:314-322）+ xref 链入口守卫（:117-123）；事故形态四锁钉定（pdfi_xref_chain.rs:195-248：绕过入口复刻旧版行为 → 产物恰是链断裂形态） | 测试即证据 |
| 页对象克隆漂移 | `opt_clone_object_to_new_document` 幂等（lopdf 语义），挂载点三形态（缺省/直接/间接）显式分支（:191-253），`push_if_absent` 引用查重（:630-637） | pdfd_writer #15（既有 /Annots 挂载不破坏） |
| save 后基线漂移 | 每次保存后从产物**重解析**重建基线（:326）——修订内容恒为本轮脏对象，不重复写已落盘对象；代价 = 每保存一次全量重解析（保存路径，非渲染热路径，模块文档 :13-15 如实登记） | #12 |
| **跨面写器互踩**（真实风险） | PdfAnnotator（增量）与 PdfForm/stamp（全量）若被宿主先后用于同一文件：全量路径会把增量修订链**坍缩重写**（合法但破坏「未坍缩」预期）+ 摧毁签名；crate 内无任何互斥/警告 | forms.rs:1045 / stamp.rs:771 均为 `Document::save_to`（见 P0/P1） |

**结论**：writer_lopdf 单路径的增量写是本仓 PDF 域质量最高的组件（守卫三层 + 四层
测试证据 + 事故复盘防线）；破坏原文件的现实路径不在它自身，而在**旁路写面（forms/
stamp）与它的语义矛盾**（§P0/P1）。

---

## 3. Q3 标注面（annotation.rs / annotate.rs / writer_lopdf.rs）

### 3.1 四（实为十一）subtype 的 PDF 对象与坐标语义

- **对象面正确性：高**。11 kind 全标准子型零私有扩展（docs/pdf-annotate.md §2 矩阵）：
  Highlight/Underline/StrikeOut=同名+`/QuadPoints`+`/C`；Note=`/Text`+`/Rect`；
  FreeText=`/FreeText`+`/Contents`+`/DA`（writer_lopdf.rs:368-373）；Ink=`/InkList`+`/C`+`/BS /W`
  （载荷宽，#45 修复）；Rectangle/Ellipse=`/Square`//`Circle`；Line/Arrow=`/Line`+`/LE`
  （PDF 无原生 /Arrow，标准终止符形态，:458-475）；Stamp=`/Stamp`+`/Contents`（标准名
  映射不做，:476-482 登记回退）。公共面 `/T` origin 编码（writer.rs:62-79）+ `/M` 值时钟
  日期串（writer.rs:91-121，Howard Hinnant 纯整数日历）+ `/F 4` 打印位（:486-494）。
- **坐标语义**：模型即 PDF 用户空间直通，「无 Y 翻转」（writer_lopdf.rs:498-499）——
  正确（PDF annot 坐标本就是用户空间）。QuadPoints 四角序实测 = TL,TR,BL,BR
  （pdfd_writer.rs:220-229 `quad()` 构造 + :408-411 期望序），匹配 Acrobat/pdfium
  de-facto 约定；annotation.rs:26-28 标注为「环序」是**措辞不准**（TL→TR→BL→BR 是 Z 序
  非环序），行为正确（P3 文档瑕疵）。
- **旋转页 / CropBox 偏移 / Bot-top-down 翻转：全部不在 crate 内处理，也没有任何
  转换 API**。`PageGeometry` 只建模 MediaBox+Rotate（page.rs:71-97，effective_* 只做
  宽高交换）；渲染尺寸却取 pdfium 的 `FPDF_GetPageWidthF/HeightF`（CropBox 显示域，
  engine_pdfium.rs:45-48 自己承认「两者在 CropBox ⊂ MediaBox 的文档上可有轻微差异，
  v0 取渲染一致性优先」）。后果：宿主要把屏幕像素（顶左原点）映射到标注用户空间
  （底左原点、可能旋转、CropBox 偏移）时，**crate 无任何设备↔用户空间变换工具**，
  且几何模型（MediaBox）与渲染域（CropBox）不一致——裁剪页/旋转页上的标注落点会
  偏移。当前无消费者，属**潜伏缺陷**（P2，标注面一旦接 UI 必踩）。

### 3.2 Acrobat GUI 实际验证缺口清单

| 项 | 引擎层预证（pdfium） | Acrobat GUI 实证 |
|---|---|---|
| 四 subtype + /T + /Contents 可读 | ✅ pdfd_writer #3（:547-604，含 Ink/Ai 后缀） | ❌ 无（测试注释 :548-550 自己承认「GUI Acrobat 人工验证 = 开发环境轮事项，gates.md 诚实登记，不冒认」） |
| Highlight 视觉位置（quad 渲染） | 部分（pdff3_render_ext `annotations_visible_in_render` 只证「渲染可见」，无坐标级断言） | ❌ |
| FreeText 免 /AP 渲染 | ❌ 未渲染断言（只断言 /DA 字面 = `/Helv 12 Tf 0 g`，pdfd_writer #13）——且**无 /DR 资源字典**，依赖阅读器内置 Helvetica 回退 | ❌ 严格阅读器可能空白 |
| 11 新 kind（pdfb/pdff2 面） | ✅ pdff2_annotate 交叉读（11 kind 全 + Strikeout 拼写锁） | ❌ |
| 删除面（null 重定义） | ✅ pdff2（「pdfium 侧不可见」） | ❌ |
| 打印位 /F 4 打印必带 | 结构断言（:431） | ❌ 无打印验证 |
| Stamp 标准名 /Name | 不做，/Contents 回退（登记） | n/a |

结论：互通证据止步于「pdfium 引擎层预证」，与已知线索一致；**Acrobat/Edge/macOS
预览三级 GUI 矩阵为零**。风险集中在 FreeText 无 /DR 与外观流缺失面。

---

## 4. Q4 cache.rs LRU + worker.rs 渲染工人

### 4.1 cache.rs（264 LOC，常开）

- arena 式双向链表 + HashMap，O(1) get/put，零 unsafe 零依赖（cache.rs:1-26 自述 +
  实现 :127-263）；字节预算精确（saturating 计数）、超大条目自逐、重复 put 原位更新
  不重复计费、驱逐返回键集（调用方可作废 GPU 纹理）——语义完整。
- 测试：7 条 LRU 锁（pdfc_cache_worker.rs:173-316）+ 性能面对比（pdfc_perf
  `perf_sequential_flip_lru_hit_vs_miss`）。
- **生命周期缺口**：cache 是纯结构，未接 PdfSession（宿主自行组合）；渲染
  `render_annotations(true)`（engine_pdfium.rs:330-336）意味着**缓存位图含标注**——
  flush 落盘新标注后缓存即陈旧，而 crate **无任何标注变更→缓存失效 API**。当前无
  消费者，属潜伏面（P3，接 UI 时必须在 flush 后 clear 或以 (page,width,rev) 键控）。

### 4.2 worker.rs（620 LOC，常开）——实现质量高于仓均

- **panic 隔离**：真实 `catch_unwind(AssertUnwindSafe)`（worker.rs:596-597）→
  `CommandError::RenderPanic` 页级错误；诚实边界声明「pdfium C++ 侧段错误/abort
  无法在本层捕获」（:14-16）。测试：`worker_panic_isolated_and_recovers`（:357）。
- **看门狗换代**（PDF-e）：10ms 轮询（:61）、开跑时刻起算（:485-486）、超时 →
  ① Timeout 回执（结果永不采用，迟到回执由 accept() 按 in-flight 落账静默丢弃
  :311-320）② 工人毒化 ③ 工厂换代（字节快照重开，S4.4）④ 在飞作业/未决取消按
  提交序重放（:376-387）。测试四条（:793-1049：超时换代/重放保序/无看门狗慢作业
  完成/工厂 panic 优雅断连 T2-fix）。
- **停机**：drop 发 Shutdown + join **当前代**（:399-405），排队作业静默无回执；
  退役旧代 join 句柄弃置（detach，:363-366）。
- **取消语义**：仅对未开跑作业生效（执行队首前排空命令通道吸收全部取消令牌
  :561-577——否则 FIFO 下取消永不可见的设计洞察）；`last_replied` Option 哨兵修复
  （F1，:606-610）+ 幽灵令牌 watermark 剪枝（:611-620）。
- **线程泄漏面（如实评估）**：退役旧代线程 detach——若敌意 PDF 让 pdfium 永挂 C 调用，
  该线程+其引擎实例+字节快照**永不回收**（模块文档 :26-27 明示接受该权衡「敌意永挂
  C 调用不得悬挂停机」）。**升级面未设防**：反复超时 → 每次换代泄漏一个旧代线程 +
  每代持独立字节快照与 pdfium 文档句柄 → N 次敌意超时 = N 个僵尸线程 + N 份快照，
  **无代数上限、无内存压力释放**（P1）。锁面：BIND_LOCK（绑定串行，毒化不复位
  engine_pdfium.rs:362-364）+ WatchState 短锁纯赋值——无竞争问题。

---

## 5. Q5 安全（security.rs / security_scan.rs + pdfe_security）

### 5.1 策略与扫描实现

- **S2 八族**：OpenAction / AA / LaunchAction / UriAction / EmbeddedFiles / RichMedia /
  JavaScript / NamesJavaScript（security.rs:69-89，serde 名冻结）——**PDF-e 对齐
  S2.2/S2.3 清单**。扫描 = **对象图全扫**（非可达性走查，「不可达的敌意对象同样
  可见」，security_scan.rs:14-16）+ 内联嵌套深度护栏 128（:94-96）+ 一 dict 每族至多
  一 hit + 确定性排序去重（:72-77）。
- **JS/Launch**：动作形态双探测（`/S` 名 + 载荷键，畸形缺 `/S` 也可见，:139-160）。
  **SubmitForm 动作不在八族**（`/S /SubmitForm` 无检测臂）；`/GoToR`/`/ImportData`/
  `/Sound`/`/Movie` 亦无（links.rs 把 GoToR/Launch 登记为 Unresolved 目标但**不告警**）
  ——P2 清单缺口。`/AA` 检测是键在场即报（不区分内部动作类型）——保守方向，可接受。
- **外部资源加载**：引擎层无网络面（pdfium 非渲染 URL；wasm 面渲染器是 pdf.js，
  外链解析属宿主域——pdf-wasm 未做任何约束声明，P3 登记）。
- **嵌入式文件**：`/Type /EmbeddedFile` 流 + `/EmbeddedFiles` 键双探测（:182-189）。
- **S6 表单探针**：/Fields 路径走查 + seen-set 环防（:294-312「重复/环引用 = None，
  节点至多游走一次——共享子树无指数放大」）+ 字段数上限复用 search cap。
- **S5**：写拒理由码稳定（security.rs:138）；双防线（会话层 document.rs:299 + 引擎层
  writer_lopdf.rs:98）；S2.4「只报告不阻断」语义一致（OpenAssessment 结构化）。
- pdfe_security 12 测试（含深嵌套判别锁 10 号——「文件面深嵌套先被 lopdf 解析器
  MAX_NESTING_DEPTH=100 截断，护栏只对内存构造面可观测」的诚实区分，:86-90）。

### 5.2 恶意对象深度（递归 xref / 循环引用）

- **内联嵌套**：护栏 128 > lopdf 解析器 100——**实际生效的上限是 lopdf 的 100**
  （超深对象在解析层已静默丢弃，扫描看不见——pdfe_security 10 号锁钉定此判别）。
- **字段树环**：seen-set 完整防御（security_scan.rs:301-309 + forms.rs:508
  `!seen.insert(id)` + 64 深度双保险）。
- **书签树环**：organize 侧 10 万步截断（organize.rs:884-886）。
- **xref 链深度：pdfi_xref_chain 只覆盖深度 1**——单次追加修订缺 `/Prev` 的断裂形态
  （:75-87 手工构造）+ CRLF 扫描重建态（:61-71）。**未覆盖**：多级 `/Prev` 长链、
  `/Prev` 循环（A→B→A，lopdf 解析器行为未知）、xref-stream 链、混合链。属测试
  深度缺口（P2）——但入口守卫 + 产物自检双面把「坏链进/坏链出」都挡了，环形链
  若让 lopdf load 挂死则落入 open_budget_ms=3s 的 ❶ 拒绝面（wasm 面恒过——
  security.rs:39-54 零计时占位，诚实登记）。

### 5.3 INPUT_CAP 前置门的绕过面

- **pdf_doc.rs:146-154**：`fs::metadata` 真实尺寸（非声明值），读盘前拒绝——本身
  无「声明小于实际」漏洞；`load_from_bytes` 字节入口**不经 100MB 应用层门**
  （只有引擎 1GiB，pdf_doc.rs:160-164）——不一致但方向保守（P3）。
- **流膨胀（zip 炸弹）**：`decode_output_max_bytes`(1GiB) **只接在渲染目标缓冲**
  （engine_pdfium.rs:301-311）；**内容流 flate2 解压无任何上限挂点**——security.rs:172-178
  自己登记「[v0 边界登记] 升级 lopdf/自研解析层时接线」。100MB 门挡的是盘上字节，
  不挡解压后体积。当前实际暴露面有限（lopdf 解析对象头不解压内容流；pdfium 内部
  有自身配额），但 organize/stamp/forms 的对象操作路径若触发流解压即无界（P2 登记）。
- **安全门不一致清单（真实绕过面）**：
  | 入口 | S3-L7 输入门 | S5 加密门 | 证据 |
  |---|---|---|---|
  | LopdfWriter::from_bytes | ✅ | ✅ 双形态 | writer_lopdf.rs:94-104 |
  | read_annotations / read_links / read_layers / open_scan / PdfOrganizer | ✅ | ✅ 双形态 | annotate.rs:96-104 等 |
  | **PdfForm::read** | ✅ | ⚠️ **仅 is_encrypted**（曾加密档放行） | forms.rs:388-396 |
  | **stamp_image** | ❌ **无输入门** | ❌ **无加密门** | stamp.rs:690-773 |
  | **read_signatures** | ❌ **无输入门** | ⚠️ 仅 is_encrypted | signature.rs:559-564 |
  | PdfiumEngine::open | ✅ | n/a（读面） | engine_pdfium.rs:96-105 |
  | mang-shell load（desktop 读路径） | ✅ 100MB | ❌ **完全不跑 S2/S6 open_scan** | pdf_doc.rs:146-164 |

---

## 6. Q6 forms / stamp / signature / organize / ocr 各面深度与测试真实性

| 面 | 深度 | 测试真实性 | 备注 |
|---|---|---|---|
| **forms.rs**（1,760 LOC） | **完整**：域收集双源（/Fields 树 + 页 /Annots 野控件，:448-483）/ /Parent 继承链（:581-650）/ 五域型模型/填值（跨页同名域合并，:544-568）/ 校验（诚实边界：`/AA /F` JS 不验且明说，:1462-1540）/ 外观（Helv 或 CIDFont Type0）/ 扁平化（拒做乱码） | 真实（真夹具 form_bid_letter.pdf；跨页合并/MaxLen 字节口径/CID 外观/缺字形响亮报错/幂等）。**但 save 的「增量」声称无任何前缀测试**（`rg prefix pdfg_forms.rs` 零命中）——因为实现根本不是增量（P0） | 「字典三形态」纪律落地好（dict_text :771-777 + 回归锁 :1729-1745）；复选框 /V 名字对象坑、/SubFilter 名字坑皆有锁 |
| **stamp.rs**（798 LOC） | **完整但旁路**：自写 PNG 解码（8/16bit、五过滤器、调色板+tRNS）+ 自写 zlib inflate（stored/fixed/dynamic Huffman，:331-554）+ JPEG 原样 DCTDecode + SMask 间接引用（MuPDF 实测踩坑教训，:584-588） | 真实（透明 PNG 解码、内容保持、透明度乘进 alpha）；qpdf/MuPDF 交叉在研究仓 | 无 S5/S3-L7 门（P1）；全量保存（未声称增量——诚实）；16-bit 降采样/隔行拒绝如实体面 |
| **signature.rs**（813 LOC） | **结构验证器，非密码验证器**：手写 DER TLV（定长、拒 BER 不定长）+ CMS SignedData 遍历 + X.509 摘要 + sid→证书序列号匹配 | **真结构**：夹具 = pyhanko 生成的真 PKCS#7/DER + 自签 X.509（pdfi_signature.rs:3-12 诚实登记「证书自签；证书链验证本就不在 v1 范围」）；ByteRange 只验「偶数个 + 递增」（:67-74），**不验摘要与实际字节一致**、不验时间戳 | 「结构完整 ≠ 签名有效」在类型注释里三次声明（:63-66）；`strip_trailing_zeros` 处理 /Contents 填充（:751-754） |
| **organize.rs**（2,025 LOC） | **完整**：页树手术（删/插/旋/重排/抽/拆/合）+ 联动（书签子树丢弃/失效链接/表单域同步/继承物化/可达性剪除/max_id 收口 :507-527）+ OrganizeReport 全量丢失登记 | 真实且**最强交叉**：qpdf --check 结构验证 + poppler 页数 + pdfium 文本/大纲交叉（软门：工具缺失跳过并打印——与「不静默 skip」纪律的显式折衷，pdff_organize.rs:10-11）；22 测试 | 全量重写**如实登记**（organize.rs:46-49）；线性化/对象流边界登记（:33-34）；签名档「不验签，调用方慎用」（:54-55） |
| **ocr.rs**（195 LOC） | **完整**：pdfium 光栅化 → 灰度 PGM（BT.601）→ tesseract 子进程（--psm 3） | 环境门真实（`ocr_available` 探针：二进制 --version + --list-langs；pdff4 测试按探针 SKIP 并打印——pdff4_ocr.rs:50-176）；断言取「关键词出现」非全文相等（OCR 非确定性，如实登记 ocr.rs:33-35） | 环境旋钮 MDC_OCR_BIN/MDC_OCR_TESSDATA；200 DPI 等效宽钳 200..4000 |

**pdfi_signature 是否真签名验证：否——是结构验证**（模块第一段就声明，测试夹具
声明自签）。**pdff4_ocr 环境门条件：tesseract 二进制 `--version` 成功 + 语言包清单
含目标语言（chi_sim 经 MDC_OCR_TESSDATA 指向仓内 test-artifacts/ocr-tessdata/）**，
缺件 SKIP（测试面）/ typed Engine（产品面）。

---

## 7. Q7 generate.rs 自研生成器 + pdf-wasm 宿主集成

### 7.1 generate.rs（621 LOC）

- **用途**：① R1「纯文本 → A4 PDF」（#29 方案 a）——CJK Type0/CID 自建面（lopdf
  font_embedding 只支持单字节简单字体，CJK 必须手工构建，lib.rs:96-99）；②
  **表单中文外观/扁平化的字体引擎复用出口**（`font_gid`/`font_object`，forms.rs:1242/
  1255/1349/1396 消费）——这是 forms 面中文能力的唯一支撑；③ mang-shell 冒烟夹具
  生成器（pdf_doc.rs:621-633，`gen_smoke_pdf` → render_170 测试消费）。
- **质量**：最小 TTF 解析（cmap fmt4/12 + maxp/head/hhea，:230-330）+ Type0 字体字典
  全内联构造（:101-146）+ ToUnicode CMap（rank 序，提取召回 100% 的 spike 实证，
  :1-9）+ 降级显式化（skipped_chars 计数）。确定性输出（同输入逐字节同输出，:148）。
  **R1 边界如实**：A4 固定几何、全角近似排版（DW=1000）、拉丁比例宽/hmtx 属 R2 登记。
  零字体入库（部署方注入 TTF）。**定位合理**——printpdf/genpdf 类库的 CJK 嵌入质量
  同样要自己拼 Type0，此实现有 spike 实证与子集字体回归锁，是「必须自研」名单内。

### 7.2 pdf-wasm（257 LOC + 251 测试 LOC）

- **能力面**：`pdf_open_scan`（S2/S5/S6 全扫 JSON）+ `WasmPdfSession`（构造/时钟/
  六命令 JSON/`attach_writer`/`flush_annotations`/`product_bytes`）——**模型层闭环
  完整**，含增量写（前缀 + 幂等 + 产物再扫描合法，pdf_wasm.rs:189-224）。
- **架构定谳诚实**：「渲染分离、真相不分离」——pdf.js 纯显示，wasm 会话是模型
  单一真相（lib.rs:1-14）；worker/cache/pdfium 在 wasm 面**不消费**（无线程——
  S4 架构在 wasm 塌缩被列为 A 路线否决理由之一，如实登记）。
- **集成深度限制**：测试是 `*_core` 纯内核原生测试（pdf_wasm.rs:1-3「JsValue-free
  ——原生 cargo test 面」）；**wasm-bindgen JS 绑定面只有编译检查，无浏览器端
  wasm-bindgen-test 实测**；渲染管线的宿主（pdf.js ↔ wasm 会话）双向接线在 web/
  侧（本轮范围外）。P3 登记。

---

## 8. Q8 与成熟引擎对标水位

| 能力面 | PDFium（经 pdfium-render） | 本仓自研/lopdf 面 | 评价 |
|---|---|---|---|
| 渲染/光栅化 | ✅ 全承（BGRA 低层路径、S3-L2 钳制、FPDF_ANNOT 显式钉定 engine_pdfium.rs:323-336） | 无自研 | 正确——不自研渲染 |
| 文本抽取/搜索 | ✅ 抽取全承 | 搜索收集器自研（document.rs:621-663，Unicode 折叠非重叠——质量好） | 合理（pdfium 无跨页搜索 API） |
| 大纲/几何 | ✅ 全承 | — | 正确 |
| 标注写（增量） | pdfium 写面弱（ADR-P1 理由成立） | **lopdf IncrementalDocument + 三层守卫** | 必须自研 ✓ 质量高 |
| 页树手术 | qpdf 强但无 Rust binding（ADR 排除记录） | organize 2,025 LOC + qpdf/poppler/pdfium 三交叉 | 合理替代，交叉验证到位 |
| 表单读写 | pdfium-render 0.9.4 form 面弱（NeedAppearances 依赖） | forms.rs 完整 | 有市场理由（PDF.js 表单写也自研）**但 save 语义失实（P0）** |
| PNG/JPEG 图像 | pdfium 面「零 image」纪律 | 自写 PNG 解码 + 自写 inflate（~550 LOC） | ⚠️ 自研动机=依赖纪律，但 inflate 是高危面（见下） |
| 签名 | — | 手写 DER/CMS（结构级） | 诚实限界；密码验证应交 rcrypto 系（未来） |
| OCR | 渲染面 | 外部 tesseract | 正确（不重造 leptonica） |
| 安全扫描 | — | security_scan 8 族 | 独有价值（PDFium 不给对象级危险族告警） |

**「自己实现但明显不如成熟引擎」清单**：
1. **stamp.rs 自写 zlib inflate**（:331-554）——对比 flate2/miniz_oxide 数十年模糊
   测试；自写版无 fuzzing 证据，坏 PNG 的鲁棒性只覆盖「提前结束/越界」类错误
   （png 炸弹/恶意 Huffman 构造未证）。动机（「lopdf 的 flate2 未在本 crate 直接
   声明」）成立但解法次优——**直接声明 flate2 依赖即可**，不值得 550 行高危解析器。
2. **forms/stamp 的 `Document::save_to` 全量重写**——对比 qpdf 的增量写能力；至少
   forms 的声称与实现互相矛盾（P0）。
3. **signature.rs 手写 DER**——限界诚实、fixture 级验证可过，但 OIDs/edge 形态
   覆盖窄（signerInfos 只取第一个、signedAttrs 不展开）。
4. **generate.rs 全角近似排版**（DW=1000）——R2 已登记，非隐藏缺陷。
**必须自研的（判断成立）**：增量写标注（lopdf 是现实最优选）、会话/命令/安全策略层
（无现成库承担此语义）、Type0/CID 中文面（生态缺口真实）、安全扫描（产品差异点）。

---

## 9. P0–P3 分级（含置信级别 + 证据链）

### P0-1 · forms.rs「增量写」声称与实现相反——签名静默失效面【置信：高】
- **证据链**：forms.rs:30-38（模块文档：「增量修订（spec 7.5.6）…产物前缀逐位不变 →
  已有的数字签名仍然有效…这是 PAdES 的硬约束」）→ forms.rs:1037-1039（`save()` 文档
  重复声称「增量修订——原字节前缀逐位保持」）→ **forms.rs:1045-1052 实现**：
  `Document::with_version("1.5")` 替换 + `doc.save_to(&mut out)` = lopdf 普通 Document
  **全量重写**（非 `IncrementalDocument`）→ 无任何前缀断言（pdfg_forms/pdfj_forms_adv
  `rg 'prefix|前缀'` 零命中；`save_is_idempotent_when_nothing_changed` 只测零改动短路）。
- **后果**：给已签名投标函填值（模块自述目标场景，forms.rs:5-7）→ 全档重写 → 既有
  签名 ByteRange 失配 → **签名静默失效**；同时摧毁线性化/未引用对象/修订历史。
- **建议**：改走 `IncrementalDocument`（同仓 writer_lopdf.rs 已有成熟范式与三层守卫可
  直接复用），或至少删除失实文档并在返回值/报告里显式声明「全量重写——将使既有签名
  失效」。

### P1-1 · stamp.rs 无 S5 加密门、无 S3-L7 输入门【置信：高】
- stamp.rs:690-773：`Document::load_mem` 直入，无 `Limits`、无 `is_encrypted`/`was_encrypted`
  检查——对加密档盖章 = 静默产出（lopdf 以未解密形态装载后全量保存，产物结构性损坏或
  内容错乱）。全写面唯一裸奔入口（对照表见 §5.3）。同为写面，writer_lopdf/organize/
  annotate 四门全开。

### P1-2 · mang-shell desktop 读路径完全不接线 S2/S6 安全扫描【置信：高】
- pdf_doc.rs:146-164 只走 `PdfiumEngine::open`（S3-L7/S3-L4）；`rg 'open_scan|
  security_scan' mang-shell/src` 零命中。SECURITY_POLICY S2.4 的「打开评估先行，宿主
  决定去留」在桌面端**零交付**——JS/Launch/OpenAction/嵌入文件文档打开时无任何告警面
  （执行面确实不存在——non-V8，但警告合同本身缺失）。wasm 路径有（pdf-wasm
  `pdf_open_scan`）。

### P1-3 · worker 换代线程/内存无上界累积【置信：中高】
- worker.rs:363-371：退役代 join 弃置（detach）+ 工厂闭包持字节快照与 pdfium 文档；
  敌意文档反复触发 20s 看门狗 → 每代泄漏一个僵尸线程 + 一份快照 + 一个 pdfium
  document。模块文档 :26-27 明示接受「单次」权衡，但**无代数上限/无压力释放**。
  建议加代数上限或老代资源释放入口。

### P1-4 · PdfForm::read 加密判定单形态（was_encrypted 放行）【置信：高】
- forms.rs:394（仅 `is_encrypted()`）vs writer_lopdf.rs:98 / annotate.rs:100 /
  organize.rs:187（双形态）。曾加密档（空口令可解）可被填值+全量保存——S5「加密档
  零写」纪律破口；与 P0-1 叠加（全量保存产出明文新档，原加密语义被静默剥除）。

### P2-1 · 已签名档的增量写往返无测试 + 无 DocMDP 感知【置信：高】
- writer_lopdf 前缀保持是签名友好的**机制**，但无「signed_sample.pdf + 追加标注 →
  ByteRange 区间不变 → 签名仍有效」端到端断言（夹具已在仓，pdfi_signature.rs:23）；
  DocMDP（/TransformParams /P 锁定）完全未检测——对锁定档追加修订 Acrobat 判篡改。

### P2-2 · xref-stream / ObjStm / 线性化基底档在增量写路径零实证【置信：中】
- 测试夹具全为经典 xref（`rg 'xref|ObjStm|lineariz' pdff_organize.rs` 零命中）；
  writer_lopdf.rs:117 xref_start 守卫对 xref-stream 档行为未知（误拒或放行后产物
  形态未证）；pdfi_xref_chain 深度=1（无 /Prev 环、无深链）。

### P2-3 · S2 族缺 SubmitForm/GoToR/ImportData 等动作族；内容流解压无界【置信：高】
- security_scan.rs:117-194 无 SubmitForm 臂；security.rs:172-178 自认 flate2 无挂点
  （zip 炸弹面只在渲染目标缓冲收口）。INPUT_CAP（100MB/1GiB）只计盘上字节。

### P2-4 · 标注坐标变换面缺失 + 几何双源不一致【置信：高】
- 无屏幕↔用户空间变换 API（旋转/CropBox/Y 翻转零设施）；PageGeometry=MediaBox
  （engine_pdfium.rs:192-210）vs 渲染尺寸=CropBox 显示域（:288-291，模块自认差异
  :45-48）。接 UI 标注放置时裁剪/旋转页会偏移。

### P2-5 · FreeText 无 /DR；表单外观 /N 字典键 "Off"；填值渲染可见性无引擎断言【置信：中】
- writer_lopdf.rs:372 只有 /DA 无 /DR（严格阅读器可能不渲染 FreeText）；forms.rs:1107-1111
  外观 /N dict 键 = `"Off"`（文本域常态键应为空名/单流形态）——pdfium 可渲染，
  Acrobat 未证；pdfg/pdfj 全部结构断言，无「填值后渲染可见」的 pdfium 像素级检查。

### P2-6 · PdfForm::write_field_dict 流对象静默替换【置信：中】
- forms.rs:991-995：`get_dictionary(id).cloned().unwrap_or_default()`——目标对象为
  Stream 时以空字典起步，保存后原流内容静默丢失（畸形档边缘）。

### P3-1 · CI 只跑 engine-lopdf 面，pdfium 全链零 CI 覆盖【置信：高】
- windows-test.yml:75-78（「pdfium 面需要动态库，CI 上没有，只开 lopdf」）——渲染/
  工人/看门狗/OCR/交叉读/增量写主语义锁全部只在本地门禁跑。回退面 = 环境合同
  panic-not-skip（pdfd_writer.rs:5-8）保证缺失即红，但 CI 红不了它没跑的东西。

### P3-2 · 杂项（如实登记级）
- annotation.rs:26-28「环序」措辞 vs 实写 TL,TR,BL,BR（行为对、文档不准）；
- render_annotations(true) 渲染含标注 → cache 无失效 API（flush 后位图陈旧）；
- load_from_bytes 绕过应用层 100MB（引擎 1GiB 兜底）；read_signatures 无输入门（与
  P1 清单同源，读面危害低并入）；worker recv_timeout 超时与死亡不区分（自述 v0 最小面）；
- pdf-wasm JS 绑定面无浏览器实测；Stamp /Name 映射、` [ai]` 作者名歧义均已自登记。

---

## 10. 能力矩阵（总）

| 能力 | 实现深度 | 消费者 | 测试 | 对标成熟度 |
|---|---|---|---|---|
| 读：页/大纲/文本 | 完整（pdfium） | mang-shell ✅ wasm(pdf.js) | pdfc_engine 15 锁 | =PDFium |
| 渲染 | 完整（BGRA 低层） | **无产品消费者** | pdfc_engine/perf + pdff3 | =PDFium |
| 搜索 | 完整（自研收集器） | 无（wasm 经命令面可及） | pdfb | ≥PDF.js |
| 标注增量写 | **完整+三层守卫** | wasm ✅ | pdfd 15 锁 + 交叉 + wasm | ≈qpdf（可用面） |
| 标注读/删 | 完整（11 kind） | 无 | pdff2 11 锁 + 交叉 | 良 |
| 表单读写 | 完整但**save 失实（P0）** | 无 | 真夹具结构级 | <Acrobat/qpdf |
| 盖章 | 完整（自写 PNG/zlib） | 无 | 真夹具 | 良（inflate 风险） |
| 页面组织 | 完整 | 无 | 22 锁 + 三工具交叉 | ≈qpdf 可用面 |
| 签名 | 结构级读取 | 无 | 真 DER 夹具 | <pyhanko/openssl |
| OCR | 完整（tesseract 外包） | 无 | 环境门 + 关键词锚 | 合理 |
| 生成 | R1（文本→A4/CJK） | mang-shell 夹具 + forms 字体面 | spike 回归锁 | 生态缺口自研合理 |
| 安全 | S2 八族+S3+S5+S6 | **仅 wasm 路径** | pdfe 12 锁 | 独有价值 |
| LRU/工人/看门狗 | 完整 | 无 | 21 锁 | 良 |

**一句话结论（Q8 判断）**：「pdfium 读 + lopdf 写」双引擎选型与分工**成立且执行
纪律良好**；真正的问题不在选型，而在 **lopdf 面扩张出的三个旁路写面（forms/stamp/
organize）安全门与语义承诺不一致**，以及**整个 PDF-d/e 能力面至今没有桌面产品消费者**
（质量靠测试债维持）。

## 11. 建议行动序（按杠杆）

1. **P0-1**：forms.rs `save` 改 `IncrementalDocument`（复用 writer_lopdf 范式）或
   改文档 + 显式签名失效警告——半小时级改动，消除「投标函填值毁签名」的业务事故面。
2. **P1-1/P1-4**：stamp/signature/forms 三个入口补齐 S3-L7 + S5 双形态门（纯复制
   writer_lopdf.rs:94-104 模板）。
3. **P1-2**：mang-shell `load_from_bytes` 接 `open_scan`（S2.4 评估先行——桌面端
   交付警告合同）。
4. **P2-1**：加一条 signed_sample.pdf 增量写往返签名有效性测试（夹具已在仓）。
5. **P1-3**：worker 代数上限（如 8 代后显式报 Engine 死亡）。
6. **P3-1**：CI 加 pdfium 动态库缓存步骤（bblanchon releases 固定 SHA），让 242 测试
   的主链进 CI。
