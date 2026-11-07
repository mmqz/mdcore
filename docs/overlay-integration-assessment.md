# overlay 层整合评估（#387 阶段 1）

> 评估基线：origin/main @ `70ab769`（2026-11-07）。产出：本文档。
> 阶段 2+（实施）等 Lead 依据本文裁定路线后另起分批 PR。
> 关联：#239（overlay 立项）/ PR #250（交付）/ #386（批注不落盘 P0）/ #302（PDF 消费倒挂）/ #44（画布）。

## TL;DR（结论先行）

1. **两套体系不是冗余，是两个正交轴**：mdc-pdf 原生标注 = PDF 格式的**权威存储与行业标准互操作**；mdc-overlay = **跨格式语义定位与索引**。真正的选择题不是「二选一」，是「每个格式谁当权威源」。
2. **路线推荐：PDF 走 C（原生为权威 + 派生索引，不写 sidecar）；其余格式（DOCX/XLSX/MD/Canvas）在标注 UI 立项前不接 overlay**——届时 overlay sidecar 就是唯一存储（那些格式没有可丢的原生标注，A/B 之争不存在）。
3. **「双向索引」今天的用户价值 ≈ 零**：产品里没有任何跳转 UI 消费它（实测 grep）。它的价值是**期权**——取决于跳转面（如批注列表面板、画布↔文档跳转）是否立项。故索引应做成**可丢弃的派生缓存**，不为它引入持久化负担。
4. **顺序纪律：#386 先于一切**——批注先能存住（原生路径 `save()` 接线），才谈索引。C 路线与 #386 完全兼容（#386 修的就是原生链）。

---

## 1. 现状

### 1.1 mdc-overlay 提供了什么（逐模块，2428 LOC + 758 LOC 姊妹 crate）

| 模块 | 提供能力 | 成熟度 |
|---|---|---|
| `mark.rs` | `OverlayMark` 四变体（Emphasis / Comment / Freehand / Region）+ `AnnotationId` + 存储单元 `OverlayEntry`（归属文档显式化） | 完整，带模型测试 |
| `anchor.rs` | `SemanticAnchor`（`PrimaryLocator`：PdfSpan{page,line,char_range} / DocxSpan{para,char_range} / XlsxCell / MdBlock / CanvasObj + 降级链 + 内容指纹）与 `GeometricSpan`（坐标空间显式声明）**显式分离** | 完整 |
| `index.rs` | `OverlayIndex` 双向索引（标注 ↔ 定位器；`annotations_at` / `anchor_of` / `render_hints` / `from_entries`） | 完整，纯内存 |
| `store.rs` | `OverlaySidecar` JSON v1（原子写 + schema 版本校验，复用 mdc-evidence 原子写纪律） | 完整，**零调用方** |
| `render.rs` | `RenderHint`（渲染意图——语义样式，视觉细节归 UI 主题） | 完整 |
| `contract.rs` | `NativeAnnotationAdapter` 契约（plan 报告 → 用户确认 → execute 指纹核对；`unsupported/degraded` 显式报告不静默丢弃） | 完整 |
| `geometry.rs` | `CharBox` 格式无关字符几何 + `quads_for_range`（字符区间 → 行带四边形，纯函数） | 完整 |
| `mdc-overlay-pdf/native.rs` | PDF 投影实装：`OverlayEntry`(Emphasis) → `/Highlight` + QuadPoints，plan_with_geometry（pdfium `positioned_chars` 几何注入）→ execute（lopdf 增量写 + 读回全链） | 完整，**零调用方** |
| `mdc-overlay-pdf/view.rs` | `RenderHint` → PDF 页坐标视图四边形（视图渲染面） | 完整，**零调用方** |

结构性纪律（`tests/no_engine_deps.rs` 以 cargo tree 运行时断言钉死）：overlay 不依赖任何文档引擎（仅 mdc-evidence 契约层 + serde）。**这意味着接它进产品不可能反向污染引擎**——接线成本天然被约束在应用层。

### 1.2 产品当前用什么（原生链，核实自 HEAD）

```
mang-shell PDF 高亮（#302 阶段 2·④，已上产品）：
  右键拖框 → add_highlight_from_screen（PageTransform #304 屏幕→用户空间）
           → PdfSession 六命令面（AnnotateAdd）
           → flush() 增量保存（LopdfWriter 三层守卫——原字节前缀逐位保持，
             签名有效 #227/#308 e2e 交叉验证）
           → reload_after_flush（内存字节换新 → RenderKey 渲染刷新 #305）
```

- 模型：`mdc_pdf::annotation::AnnotationKind`——**11 类**（Highlight/Note/FreeText/Ink/Underline/StrikeOut/Rectangle/Ellipse/Line/Arrow/Stamp），比 overlay 的 4 变体宽。
- 互操作：产物是标准 PDF 标注对象，Acrobat/浏览器/任何阅读器可读可编辑。
- 已知缺口（= #386，P0）：`PdfAnnotateController::save(path)` 落盘方法**零调用方**；`Editor::save()` 对只读态假装成功 → **批注关闭即丢**。这是当前真 P0，且它是原生链自己的接线缺口，与 overlay 无关。

### 1.3 消费现状（Lead 结论独立复核）

```
$ rg -l "mdc-overlay" --glob Cargo.toml .   # → 仅 crates/overlay/Cargo.toml 自身
$ rg -l "mdc-overlay-pdf" --glob Cargo.toml # → 仅姊妹 crate 自身
$ rg -n "OverlayIndex|mdc_overlay" mang-shell/src/ # → 零命中
```

**核实结论：与 Lead 一致——overlay 双 crate 在产品面零依赖、零代码引用。** mang-shell 中 "overlay" 字样命中全为 UI 概念（confirm 弹层/上下文菜单层）。

### 1.4 能力对照表（两体系真正的差异面）

| 维度 | mdc-pdf 原生标注 | mdc-overlay |
|---|---|---|
| 标注类型 | 11 类（Highlight→Stamp 全谱） | 4 变体（Emphasis/Comment/Freehand/Region） |
| 权威存储 | PDF 文件本体（增量写，签名安全） | 独立 sidecar JSON（默认不碰原文件） |
| 行业互操作 | **Acrobat/任意阅读器可读**（标准对象） | 无（自有格式） |
| 跨格式定位 | 仅 PDF（页/坐标域内） | **五种**（PdfSpan/DocxSpan/XlsxCell/MdBlock/CanvasObj） |
| 双向索引 | 无（只有页→标注的 list） | 有（`OverlayIndex` 标注↔定位器） |
| 非破坏性 | 否（写进文件——增量段） | 是（sidecar 旁挂） |
| 消费状态 | **已上产品**（右键高亮） | 零消费 |

**读法**：第 1-4 行是原生强项，第 5-7 行是 overlay 强项。没有任何一行同时被两边满足——这就是「不是冗余」的实证。

---

## 2. 三条路线的真实代价

### 路线 A：overlay 进产品（统一模型，PDF 高亮改走 sidecar）

| 项 | 评估 |
|---|---|
| 改动面 | mang-shell/Cargo.toml + mdc-overlay 依赖；`pdf_annotate.rs` 整体重写（控制器 ~300 行从 PdfSession 命令流改 OverlayMark 流）；`pdf_pages.rs` 手势与渲染刷新重接；`writer_lopdf` 增量写链路失去产品入口（#227/#304/#305/#308 四个已交付特性降级为无消费者） |
| 风险 | **高**。Acrobat 互操作丢失 = 用户在 mang 里画的标注，Acrobat/浏览器/同事的 Word 全都看不见——对外协作场景的功能性降级；增量写 + 签名保持链路（e2e 钉定）被绕开 |
| 工作量 | 中（1-2 天重写）+ 永久性互操作损失 + 既有 4 个特性 PR 白做 |
| 结论 | **否**。Lead 预判正确：「丢失 Acrobat 互操作是不可接受的降级」。且违反纪律③「不要为了统一而统一」 |

### 路线 B：双轨并存（原生 + Overlay sidecar 双写）

| 项 | 评估 |
|---|---|
| 改动面 | `add_highlight` 后同步构造 `OverlayEntry` 写 sidecar（控制器 +40-60 行）；打开时 sidecar ↔ 原生合并对账（冲突以原生为准，+80-120 行）；sidecar 丢失/损坏重建路径（扫原生 → 重建，+100 行）；一致性测试双份 |
| 风险 | **中**。双写漂移：每新增一个标注入口（未来 Note/Ink 接线）都要记得双写——遗漏即静默分叉；对账代码是长期税 |
| 工作量 | 中（2-3 天 + 持续一致性维护成本） |
| 换来什么 | 统一索引能力（跨格式跳转）——**但当前没有跳转 UI 消费它**（见 §3） |
| 结论 | **暂缓**。为「当前不存在的消费者」支付双写一致性长期税，性价比为负。等第一个真实跨格式跳转需求出现再评估（届时若 PDF 侧已按 C 建了派生索引，B 的增量只剩「非 PDF 格式的 sidecar」——见 §4.2） |

### 路线 C：只做索引层（原生为权威源，索引是派生物）

| 项 | 评估 |
|---|---|
| 改动面 | **零产品改动起步**。索引构建 = 打开时/flush 后 `read_annotations()`（已有 API，lopdf 读面，e2e 在用）→ 内存映射。overlay crate 本体零改动（`OverlayIndex` 本来就是内存索引，`store.rs` sidecar 不用） |
| 风险 | **最低**。索引丢了 = 重扫一次（秒级）；不写任何新文件；原生链不动 |
| 工作量 | 索引本身 ~50-80 行（甚至可先不写——见 §3 的诚实结论）；真正的工作量在**跳转 UI**，而那是独立的产品决策（#389 同族），不应与本 issue 捆绑 |
| 结论 | **推荐**——但按「有消费者才建」的纪律执行（见 §4） |

---

## 3. 产品问题：「双向索引」到底给用户什么价值？（诚实回答）

**今天的用户价值：零。** 实测依据：mang-shell 全部导航面 = md 编辑器 goto-line（`editor/mod.rs`）+ 标题树面板（`panels.rs` 的 `tg-outline`）+ 文件树。**没有任何「点 X 跳到 Y」的 UI 存在**——索引建了也没人消费，用户完全看不见。

**期权价值（若跳转面立项）**，按性价比排序：

1. **批注列表面板**（PDF 内）：列出全部标注（页码/类型/摘要）→ 点击滚动到对应页。**注意：这个功能直接消费原生 `list()`/`read_annotations()` 就能做，不需要 overlay**——它恰好证明 C 路线「索引按需派生」的正确性。
2. **画布 ↔ 文档跳转**（用户原话愿景，#44）：点画布上的引用对象 → 打开文档定位段落；点段落 → 高亮画布对象。这是 overlay `CanvasObj`/`DocxSpan` 定位器的主场——**但前提是画布先有引用语义的对象**（当前 Stroke 只是笔迹，无引用载荷）。此为 1.0 后场景。
3. **全局搜索命中标注内容**（跨文档）：远期。

**判断**：索引是期权不是资产。若 1.0 路线图里没有跳转面，overlay 应继续休眠（孤岛不可耻，休眠的通用层比错误的接线便宜）；若有，C 形态的派生索引随时可建（小时级）。

---

## 4. 推荐与设计

### 4.1 推荐：分格式裁定的 C（「C-PDF + 按-需-其余」）

```
PDF    ：路线 C——原生标注 = 唯一权威源；索引 = 打开/flush 后重扫的内存派生物；
         不写 sidecar、不接 overlay 依赖。#386 修的正是这条链（先修数据不丢）。
DOCX   ：无原生标注写面（OOXML comments 未建模）→ 标注 UI 立项时，overlay
         sidecar 就是唯一选择（没有可丢的互操作，A/B 之争不存在）。
XLSX   ：同 DOCX。
MD     ：同 DOCX（mdcore 块 ID 寻址天然匹配 MdBlock 定位器）。
Canvas ：对象已在 CRDT session 内有身份与快照——批注/引用挂对象属性即可，
         overlay 的 CanvasObj 定位器在跨文档跳转出现时才有用。
```

**为什么不现在就给非 PDF 格式接 overlay**：那些格式当前连标注 UI 都没有（#389 的 XLSX 连网格视图都还没建）——先建 UI 再定存储，顺序不能反。overlay crate 的零引擎依赖设计保证它随时可接，**不需要预先占位**。

### 4.2 若按 C 实施 PDF 索引：具体设计

- **索引键**：原生 `AnnotId`（标注身份）+ `DocumentRef`（mdc-evidence：doc_id/引擎/指纹——标签页打开时建立，指纹防文档错配）。
- **存哪里**：**仅内存**（`OverlayIndex` 或更薄的 `HashMap<page, Vec<AnnotId>>`）。生命周期 = 标签页。**不落盘**——派生物落盘就是 B，而 B 已论证暂缓。
- **丢了怎么重建**：`read_annotations(bytes, limits)`（lopdf 纯读，无渲染参与；加密档已有 S5 拒绝语义）——打开时一次、每次 flush 后一次（增量后全量重扫）。
- **用户怎么感知**：第一阶段唯一可感知面 = **批注列表面板**（右 dock）：`页 N · 高亮 · "…"摘要` 列表 → 点击 → `PdfPagesView::scroll_to_page(page)`（**API 已存在**，`pdf_pages.rs:256`，面板直连即可）→ 落地成本主要在列表 UI 本身。画布对象跳转 = 等画布有引用语义对象后再议。
- **overlay crate 的角色**：上述最小索引**可以不引 overlay**（原生 list 已够列表面板用）；overlay 在 PDF 侧的真实入口是「Emphasis 语义需要投影回原生文件」的场景（如画布标注导出 PDF 时经 `PdfNativeAdapter` 落成标准 Highlight）——那是导出功能立项时的事。

### 4.3 实施顺序建议（供 Lead 排期）

```
① #386    批注落盘（P0，数据安全——原生链自身缺口，与路线裁定无关）
② （可选）批注列表面板 = 第一个真实索引消费者（消费原生 list，零 overlay 依赖）
③ 非 PDF 格式标注 UI 立项时（#389 XLSX 网格是前置）→ 接 overlay sidecar 作唯一存储
④ 画布引用对象出现 → CanvasObj/DocxSpan 跨文档跳转（overlay 定位器主场）
```

## 5. 验收对照（#387 阶段 1 验收条目 → 证据位置）

| 验收要求 | 本文位置 |
|---|---|
| 1. 评估文档写清三条路线的真实代价（不是泛泛而谈） | §2（每路线：改动面列到文件/特性 PR 级、风险、工作量、结论） |
| 2. 明确回答「双向索引给用户什么价值」 | §3（今天=零[实测依据]；期权价值三项按性价比排序） |
| 3. 推荐一条路线 + 理由 | §4.1（分格式裁定：PDF=C，其余按需；理由=正交轴分析 §1.4 + 期权性质 §3） |
| 4. 如果推荐 C，给出索引设计细节 | §4.2（键/存储/重建/用户感知四问全答） |
