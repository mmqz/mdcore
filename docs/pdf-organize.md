# PDF-f · 页面组织（organize.rs）——能力与保真矩阵

> S2-1（issue #103 任务 1）· feature `engine-lopdf` · 基线 main `c01175a0`
> 分支 `feat/pdf-organize`

## 1. API 面

```rust
PdfOrganizer::from_bytes(pdf) / from_bytes_with_limits(pdf, limits)  // 安全镜像 LopdfWriter 打开面
  .delete_pages(&[usize])            // 删页（联动书签/链接/表单域手术 + 剪除）
  .insert_blank_page(at, PageSize)   // 插入空白页（A4/Letter/横版/自定义）
  .rotate_page(idx, 90|180|270)      // 叠加式旋转（Acrobat「再转」语义）
  .reorder(&[usize])                 // 恰排列重排（单父组 /Kids 直排；嵌套树物化扁平化）
  .extract(start..end)               // 区间抽取为独立 PDF（= 快照 + 删补集，单代码路径）
  .split_every(n)                    // 每 N 页一块
  .save_to_vec()                     // 全量序列化（xref 重建 + max_id 收口）
PdfOrganizer::merge(&[&[u8]])        // N 文合并（页追加 + 表单/大纲/命名目的地拼接）
  .page_count() / .report() / .assessment()
```

错误面 = `CommandError`（crate 流通货币）：页越界 `InvalidPage`、加密
`SecurityDeny{REASON_ENCRYPTED_WRITE}`、结构/参数面 `Engine`。

## 2. 外科手术语义（每操作）

| 操作 | 触碰面 | 不触碰面 |
|---|---|---|
| delete_pages | 父 /Kids 摘除 + 祖先 /Count 递减；死目标书签子树丢弃；幸存页死目标 /Link 移除；死页 widget 的 /Fields 条目摘除；可达性剪除 | 幸存页对象/内容流/资源（逐字节不变——测试钉定）；对象号零重排 |
| insert_blank_page | 新页对象（显式 /MediaBox + 空 /Resources）+ 宿父 /Kids 定位插入 + 祖先 /Count 递增 | 既有全部对象 |
| rotate_page | 页字典 /Rotate 显式设置（叠加式 `(cur+deg)%360`，现值走继承链） | 祖先共享 /Rotate（兄弟页零波及）；内容流 |
| reorder | 单父组：仅 /Kids 数组序；嵌套树：继承四键物化（引用拷贝）+ 扁平化到根 | 页对象本体（两种路径均零触碰） |
| extract | delete_pages 的全部语义（补集侧） | 快照不变异原组织器 |
| merge | 源对象图重编号注入 + 源页物化挂接 + /Fields 拼接 + 大纲链接链追加/过继 + 命名目的地单侧过继 | 基底对象零重排；基底 catalog 元信息/Info/ID |

## 3. 丢失登记（OrganizeReport——无静默丢）

| 字段 | 语义 |
|---|---|
| `dropped_bookmarks` | 目标页被删的书签（**含整子树展开**——父死子随：子项目标页可能存活，但父项死后悬空挂载不可语义化，保守丢弃并全量计数） |
| `dropped_links` | 幸存页上指向已删页的 /Link（移除而非留悬空目标） |
| `dropped_form_fields` | 被删页 widget 对应的 /AcroForm /Fields 条目 |
| `dropped_named_dests` | 合并冲突（基底赢）+ 名称树不重构面 |
| `dropped_form_appearances` | 合并双方皆带 /DR /DA 时源外观资源弃用（字体坐标系不同，混排必坏） |
| `materialized_pages` | 继承物化触发的页数（引用拷贝，对象本体零复制） |
| `pruned_objects` / `linearized_dropped` | 可达性剪除计数 / 线性化字典随剪除移除（编辑后线性化本就失效） |
| `first_save_growth_pct` | 首轮保存结构重排增长率（对象流解包/经典 xref 回归） |

## 4. 实施发现（工程登记）

1. **lopdf `Document::max_id` 陈旧面（本批最重要发现）**：写出器从
   `max_id` 派生 trailer /Size 与 xref 规模，而结构手术必使其失真——
   删页成孔 → /Size 虚高；合并注入 → /Size 虚低。qpdf --check 对后者
   出警告 "reported number of objects is not one plus the highest
   object number"。**修复**：`save_to_vec` 前按对象表真实最大号收口
   （`organize.rs::save_to_vec` 文档）。任何在 lopdf 上做结构手术的
   后续面（S2-2 标注写回若走全量路径）都需此收口。
2. **lopdf `Processor::delete_pages` 不可用**：其 `delete_object` 会
   删除被删页的**全部**引用——包括大纲目的地——即静默砸书签。本
   模块自实现 /Kids 手术（引用摘除仅限页树 + 显式书签联动）。
3. **书签子树语义**：Acrobat 对删页的书签处理无公开规范；本实现取
   「父死子随」保守面（全量登记）。与「子项晋级」的取舍已登记在
   `drop_dead_bookmarks` 文档——若产品面需要晋级语义，改动点单一。
4. **lopdf 保存确定性**：同输入 → 同输出逐字节（roundtrip 测试 ×10
   钉定）——为「10 次不膨胀」提供强于体积断言的稳定性保证。
5. **名称树合并不做**（双方都带 /Names /Dests 时基底赢 + 计数）：
   排序树重构（spec 7.9.6 键序不变量）风险/收益比不成立——v0 如实
   登记丢弃数，单侧存在时完整过继零损失。

## 5. 验证矩阵（tests/pdff_organize.rs，21 测试全绿）

```
本地断言（engine-lopdf，16 项）
  删页：页数 + 幸存页内容流逐字节不变 + 校验矩阵（越界/全删/空/重复）
  书签：三场景（p0/p2/p0+p1 删）+ 子树语义 + 全灭摘除 + 目标引用保真
  旋转：叠加数学（90+90=180/270+90=0）+ 兄弟零波及 + 非法角拒绝
  重排：内容流逐页对位（[2,0,1]/逆序/恒等）+ 非恰排列三拒 + 失败零损伤
  插入：位置/尺寸（A4/Letter/横版）+ 原页零损伤 + 原生页几何
  抽取：独立可开 + 区间外书签丢弃 + 原组织器状态不变
  拆分：块数/页数守恒 + n=0 拒
  合并：页数=各和 + 内容流逐页对位 + 书签拼接/过继 + 单文档=纯往返
  往返：3 fixtures + 5 真实语料件 ×10 轮 < 2% + 逐字节确定性
  语料扫：30 件全操作（删/旋/排/抽/合）每产物 qpdf --check
  大件：1390 页 → 删 10 页 1380 + 抽 300 页独立件
  加密：qpdf 现场生成 256 位加密档 → from_bytes/merge 双拒（S5）
  报告：计数器跨操作累加精确性
  评估：S6 表单探针随组织器携带

交叉验证面
  qpdf --check     全部产物（结构层）
  poppler pdfinfo  页数（独立工具）
  pdftoppm         旋转视觉（渲染视口宽高交换）
  pdfium（engine-pdfium，5 项）：删页后重开页数 + 幸存页文本逐页相等
    + 旋转几何（视口交换 + 旋转位）+ 书签重映射 outline 独立确认
    + 合并/大件重开
```

实测（本机，Rust 1.99.0 / pdfium chromium-8086 / poppler 25.03）：
`pdf 全套双特性 144 过 / 0 败 / 6 ignored`；默认特性 71 过 / 0 败；
clippy 0 警告；fmt 0 diff；Cargo.lock 零 diff（无新依赖）。

## 6. 已知边界（不做什么——如实登记）

- 内容流编辑（页内绘制指令）——组织边界外，标注面归 PDF-d 增量写；
- 多段修订（incremental update）——组织是结构性重写，全量保存；
- 加密档任何结构操作（含先解密再组织）——S5 双形态拒；
- 双方都带名称树的命名目的地合并（见 §4.5）；
- 签名档：结构重写使增量签名必然失效——不验签不阻断，调用方慎用；
- `crates/pdf/examples/q31_acceptance.rs` 默认特性 E0432 为 **main
  既有基线**（example 消费 engine-lopdf 门后 API，净仓验证过——
  非本分支引入），与 office-io examples wasm32 基线红同族。
