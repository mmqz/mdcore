# PDF-f4 · OCR 接入（ocr.rs）——能力与保真矩阵

> S2-4（issue #103 任务 4）· feature `engine-pdfium` + tesseract 环境门
> · 基线 main `c01175a0` · 分支 `feat/pdf-ocr`

## 1. API 面

```rust
ocr_page(pdf: &[u8], page: usize, lang: &str) -> Result<String, CommandError>
ocr_available() -> OcrEnv { binary_ok, languages, has_lang() }
```

## 2. 管线（零新依赖纪律）

```text
pdfium 光栅化（200 DPI 等效宽——pt/72×200，[200,4000] 钳制）
  → BGRA → 灰度 PGM（P5：Y = 0.299R+0.587G+0.114B——tesseract/
    leptonica 原生输入格式：零 image crate、零 zlib、零 PNG 编码）
  → tesseract <pgm> stdout -l <lang> --psm 3
  → 识别文本
```

环境旋钮：`MDC_OCR_BIN`（二进制路径）/ `MDC_OCR_TESSDATA`（语言包目录
——本仓 `test-artifacts/ocr-tessdata/` 携带 chi_sim，注入由测试
`setup_tessdata` 自动发现）。

## 3. 环境门（issue 指令「沙盒缺就 SKIP——参照 bid-parse」）

- **产品 API**：二进制缺失/语言包缺失/识别失败 → typed `Engine`（含
  stderr 摘要——不静默）；页越界 → `InvalidPage`；
- **测试面**：探针门控——缺件跳过并打印（英文面与中文面独立门控：
  chi_sim 缺失只跳过中文面）；
- 沙盒实况：tesseract 5.5.0（无 root 装 apt 包）→ chi_sim 从
  tessdata_fast 官方仓取件（SHA256 pin——`fetch_tessdata.sh` 版本闭环）。

## 4. 扫描件夹具（真实扫描语义）

`crates/pdf/fixtures/scanned_mixed.pdf`（248KB，生成器
`gen_scanned_fixture.py` 随包）：

- PIL + **NotoSansSC 真字体**渲染中英混排招标文件样张（12 行中英
  交错）→ JPEG quality 88（扫描件有损压缩面）→ `/DCTDecode` 嵌入
  （手写最小 xref——独立于被测写路径的夹具纪律）；
- **零文本层**（`pdftotext` 提取为空——「扫描件」的字面验证）；
- 实施发现（夹具生成期）：`/Resources << /XObject << /Im0 … >> >>`
  需双层嵌套（/XObject 的值是名字→引用字典）——首版单层使
  pdfium 渲染全白（Do 静默找不到资源）；qpdf 的
  `/QPDFFake1` 警告即此症状。

## 5. 验证矩阵（tests/pdff4_ocr.rs，5 测试全绿）

```
① 扫描件 eng OCR：英文锚点 Specification/Requirements 识别
② 扫描件中英混合（chi_sim+eng）：「招标文件」「验收标准/总体要求」
   + 英文锚点——issue 验收「OCR 出文本（中英混合）」的字面证明
   （实测输出：招标文件技术规格书 / Bid Document Technical
   Specification / 第一章 总体要求 / …）
③ 真文本 PDF（small.pdf）同一管线 OCR：'Spike' 识别——渲染→识别
   全链健康度
④ 探针语义：binary_ok/languages/has_lang
⑤ typed 错误面：缺失语言包 → Engine；页越界 → InvalidPage
```

断言口径：OCR 非确定性（版本/字体/分辨率敏感）——**关键词条级**
断言非全文相等（登记于 ocr.rs 模块文档）。

实测：`pdf 双特性 119 → 124 过 / 0 败 / 6 ignored`（新增 5；既有零
回归）；clippy 0 警告；fmt 0 diff；Cargo.lock 零 diff。

## 6. 已知边界（如实登记）

- `--psm 3`（全页自动分割）固定——版面分析旋钮不暴露（v0 单页扫描件
  语义；复杂版面属产品迭代）；
- 渲染宽 200 DPI 等效固定（质量/速度平衡实测锚点）——调参面留产品
  迭代；
- 识别非确定性——调用方不应全文比对（断言口径同测试面）；
- 多页批量 = 调用方循环（单页 API 面与 issue 草图一致——批处理/
  进度回调属宿主层）。
