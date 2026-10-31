# PDF-f2 · 标注写回扩展（annotate.rs + 11 kind）——能力与保真矩阵

> S2-2（issue #103 任务 2）· feature `engine-lopdf` · 基线 main `c01175a0`
> 分支 `feat/pdf-annotate`

## 1. 面的构成

| 层 | 文件 | 变更 |
|---|---|---|
| 模型 | `annotation.rs` | `AnnotationKind` +7 变体（Underline/StrikeOut/Rectangle/Ellipse/Line/Arrow/Stamp）+ `Border` 类型 + tag/patch/钳制面扩展 |
| 写面 | `writer_lopdf.rs` | `annot_object` +7 子型臂（Square/Circle/Line/LE/Stamp…）+ `decode_pdf_text`（读面配对）+ `staged_object_id` 访问器 |
| 删除面 | `writer_lopdf.rs` | `remove_annotation`（增量语义：/Annots 摘引用 + 对象 null 重定义） |
| 读取面 | `annotate.rs`（新） | `read_annotations`（11 kind 反向映射 + 宽容跳过计数 + `/M` 日期反解析） |
| 门面 | `annotate.rs` | `PdfAnnotator`：open / add / remove / list / save（issue 草图形状，id = `AnnotId`） |
| 引擎面 | `engine_pdfium.rs` | `shared_handle()`（进程单例引用——交叉读测试复用绑定纪律） |
| 会话面 | `document.rs` | 入列 Border 宽钳制（新 kind 即刻过六命令面——命令面零变更） |

## 2. 11 kind 子型映射（全标准子型，零私有扩展）

| 模型 kind | /Subtype | 载荷面 |
|---|---|---|
| Highlight / Underline / StrikeOut | 同名 | /QuadPoints + /C |
| Note | /Text | /Rect |
| FreeText | /FreeText | /Rect + /Contents + /DA |
| Ink | /Ink | /InkList + /C + /BS |
| Rectangle | /Square | /Rect + /C + /BS |
| Ellipse | /Circle | /Rect（外接框）+ /C + /BS |
| Line | /Line | /L [x1 y1 x2 y2] + /C + /BS |
| Arrow | /Line + /LE [/None /ClosedArrow] | PDF 无原生 /Arrow——标准终止符形态 |
| Stamp | /Stamp | /Rect + /Contents |

## 3. 关键语义

### 读取（`read_annotations`）
- 页升序 + 页内 /Annots 序 → `AnnotId` 文档序单调（从 1）；
- `/Widget` `/Link` `/Popup` = **非标注子型跳过计数**（阅读面消费）；
- 未知/畸形（无 /Subtype、载荷缺字段、非有限坐标）跳过计数——宽容
  不炸 + `ReadReport` 全量登记（无静默）；
- `/T` 作者经 `decode_author` 可逆解码（Ai `" [ai]"` 后缀往返）；
- `/M` 日期串 → epoch **ms**（`days_from_civil` 无 Chrono 确定性日历；
  时区偏移按 UTC 近似——精度面登记）。

### 删除（增量语义）
PDF 增量修订**不能改写字节**——删除 = 新修订页 /Annots 摘该引用 +
标注对象 `null` 重定义（spec 7.5.10 逻辑删除）。产物前缀逐位保持
（既有语义锁不破）；对象字节仍在旧修订（增量物理事实），引用图上
已死——pdfium 交叉测试确认引擎面不可见。全量另存后物理消除。

### 摘除定位双源
外部标注（open 定位表：id → 页/对象号）or 会话新增（写器 `staged`
映射——跨 flush 持久，含已落盘对象）。

## 4. 实施发现

1. **会话新增标注的删除依赖 staged 私有面**：首版 pending_removals 只
   覆盖外部标注——会话新增删除后重开仍见（对象已在产物里）。修复 =
   `staged_object_id` 访问器 + remove 双源定位（实施发现，测试钉死）。
2. **pdfium 类型面拼写**：`PdfPageAnnotationType::Strikeout`（pdfium
   Debug 形，非 StrikeOut）——交叉断言按实测形。
3. **Arrow 互通**：pdfium 类型化为 Line（无 Arrow 变体）——/LE 终止符
   是视觉面，子型仍 Line；读回判据 = /LE 含 ClosedArrow/OpenArrow。

## 5. 验证矩阵（tests/pdff2_annotate.rs，11 测试全绿）

```
① 11 kind 全量往返：add → save → read_annotations → 载荷逐字段相等
   （含 f32 逐位、作者/溯源往返、Ai 后缀可逆）
② 独立夹具读取：测试内裸 lopdf 手搓标注 PDF（不依赖被测写路径）→
   UTF-16BE 作者 / /M 日期 / /C 归一色 / /BS 宽全解析
③ 读取报告计数：/Widget → skipped_non_markup；畸形 → skipped_malformed
④ 删除（会话新增）：前缀逐位保持 + 重开恰剩 + 幂等（重复删 AnnotNotFound）
⑤ 删除（外部既有）：null 重定义路径 + 前缀保持 + 重开恰剩
⑥ 会话命令面：AnnotateAdd（Rectangle 含宽 999 钳 12.0）→ flush →
   读回钳制值落盘——六命令零变更即可用
⑦ wire 兼容：旧 4 kind JSON 反序列化 + 新 kind serde 往返 + 未知
   tag 得 Err 非 panic
⑧ 输入校验：页越界 InvalidPage / 删不存在 AnnotNotFound / list 页
   过滤 / 空载入拒
⑨ Acrobat 互通公共面：/Type /Annot + /F 4 打印位 + /T + /M + 子型谱
   + Arrow /LE + Stamp /Contents（裸 lopdf 独立视角）
⑩ [pdfium] 9 子型独立读回（Highlight/Text/FreeText/Ink/Underline/
   Strikeout/Square/Circle/Stamp + Line×2）+ bounds 可读
⑪ [pdfium] 删除确认：引擎面恰见 1 + 子型正确
```

实测：`pdf 双特性 119 → 132 过 / 0 败 / 6 ignored`（新增 13：11 集成
+ 2 模块单测；既有零回归）；clippy 0 警告；fmt 0 diff；Cargo.lock 零
diff（零新依赖）。

## 6. 已知边界（如实登记）

- **标准图章名 /Name 映射不做**（/Approved /Confidential 等固定名 →
  标准图章图形）——v0 /Contents 文本承载（阅读器回退显示）；映射表
  属产品决策面；
- **Highlight /CA 透明度面不做**（issue 草图提及 opacity——改存量
  Highlight 变体形状会破 wire 兼容；Acrobat 对 /Highlight 默认 multiply
  混合渲染，视觉等效）；
- **/M 时区偏移按 UTC 近似**（无时区库依赖——分钟级偏移量 < 1% 场景
  影响；登记精度面）；
- **Popup 外观（/Popup 子标注、/Open 状态）不建模**——Note 的弹出
  评语窗体属阅读器交互态；
- 删除对象字节的**物理消除**需全量另存（增量语义物理事实——非限制）。
