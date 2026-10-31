# PDF-f3 · 渲染补充（links/layers + 渲染注释可见）——能力与保真矩阵

> S2-3（issue #103 任务 3）· feature `engine-lopdf`（+ `engine-pdfium`
> 渲染/交叉面）· 基线 main `c01175a0` · 分支 `feat/pdf-render-ext`

## 1. 交付面

| 模块 | 内容 |
|---|---|
| `links.rs`（新） | `read_links`：/Link 标注 → 目标页（XYZ 直目标/命名目的地[/Dests 字典 + /Names 名称树]）/ URI 外链 / GoToR-Launch 远程登记。`LinkReport` 计数自洽（links = internal + uri + unresolved）|
| `layers.rs`（新） | `read_layers`：/OCProperties /OCGs 清单（/Name UTF-16BE 解码 + /D /ON//OFF 默认可见性）；`set_layer_default_visible`：持久化开关（/ON /OFF 外科手术 + max_id 收口全量保存）|
| `engine_pdfium.rs` | 渲染注释**显式钉定**（`.render_annotations(true)`——缺省即真但显式声明使行为契约进代码面）+ `shared_handle()`（进程单例引用——交叉读面）|
| `security_scan.rs` | `decode_pdf_text` `pub(crate)`（crate 内单一实现纪律——links/layers 复用零复制粘贴）|

## 2. 关键语义与实测

### 链接（issue 验收：能取出所有链接——页码 + 目标）
- **真实语料实测**：LO 转换 docx 超链接产物 **17 件 / 661 条链接 / 345 URI**
  （报告面自洽断言逐件验证）；
- pdfium 交叉：引擎面目标页号与 lopdf 解析一致（包含式断言——pdfium
  将 GoToR 远程目标也解析为页号，不区分远程/本档；我方 Unresolved
  分类为正确语义）。

### 渲染注释可见（issue 验收：渲染时标注可见）
- **像素级钉定**：无标注渲染 vs 有标注渲染的红墨迹像素占比显著上升
  （Ink 粗红笔划三横杠，pdfium /C + /BS /W 外观合成渲染）——
  「渲染时标注可见」的字面证明（pdff3 测试 4）。

### 图层开关（OCG）
- **实施发现：pdfium 遵循 /D 默认态**——实测同一 OCG 夹具，图层 B
  从 OFF 翻 ON 后 pdfium 渲染蓝像素 0.0% → 4.2%（**持久化开关的渲染
  面生效被实测证明**，pdff3 测试 6）；
- **pdfium-render 0.9.4 绑定无 OCG API**（`FPDFDoc_GetOCG*`/
  `FPDF_SetOCGState` 均未封装）——**渲染时即时开关不可达**，走 lopdf
  持久化路径（与 Acrobat 图层面板「默认状态」等价）；即时切换属
  pdfium-render 升级/自建绑定面（登记，不硬造）。

## 3. 验证矩阵（tests/pdff3_render_ext.rs，6 测试全绿）

```
① 手搓夹具四形态链接全解析（XYZ/命名/URI/GoToR）+ 热区 + qpdf
② 真实语料含 /Link 件全量扫（17 件 661 条 345 URI + 报告自洽）
③ OCG 读取（名/可见性）+ 开关往返（翻转/不动/越界拒/无图层容错）+ qpdf
④ [pdfium] 注释渲染可见性（像素占比钉定）
⑤ [pdfium] 链接目标页交叉（引擎面独立确认）
⑥ [pdfium] 图层默认态渲染遵循性（蓝像素 0→4.2% 实测 + 红块恒在）
```

实测：`pdf 双特性 119 → 126 过 / 0 败 / 6 ignored`（新增 7：6 集成 +
1 模块单测；既有零回归）；clippy 0 警告；fmt 0 diff；Cargo.lock 零 diff。

## 4. 已知边界（如实登记）

- **渲染时即时图层切换**：pdfium-render 0.9.4 无 OCG 绑定（上表）——
  持久化开关已验证渲染生效，即时切换留待引擎升级；
- `/A /GoToR` `/Launch` 远程目标：`Unresolved` 登记（合法形态——
  远程跳转的文档级语义属宿主层）；
- 悬空命名目的地：`Unresolved` 登记（外部生成器常见，非错误）；
- 链接热区坐标系 = PDF 用户空间（无 /Rotate 变换——渲染命中测试由
  宿主结合 `PageGeometry::effective_*` 处理，与页模型的单一归一化
  出口纪律一致）。
