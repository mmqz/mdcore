# R-1 大文件边界实测报告（#22 任务池 · R 组）

> 度量对象：mdcore main @ `9d890f9`（web dist 全量重建：mdcore_wasm_bg.wasm 6,090,592B 同源产线）
> 环境：Linux x86_64 沙箱（Playwright chromium headless / 2C / 9.9G 盘）
> 日期：2026-10-27 · 夹具与度量脚本全部入库可复现（本目录），大二进制不入库
> （.gitignore——生成命令见各生成器头注）

---

## 总览（#22 R-1 指标表逐项）

| 指标 | 50MB DOCX | 100MB XLSX | 500 页 PDF | 阈值 | 判定 |
|---|---|---|---|---|---|
| 文件字节（实测） | 50,096,362 | 101,096,230 | 423,379 | — | — |
| 导入/打开耗时 | **4,891 ms** | **11,607 ms** | open 42 ms + 首渲 256 ms | 记录 | ✅ 有数 |
| 峰值内存（渲染器 RSS） | **888.0 MB**（Δ+746.3） | **728.8 MB**（Δ+588.3） | **222.7 MB**（Δ+81.5） | < 1 GB | ✅ 全过（docx 余量 11%） |
| 是否崩溃 | 否（ok=True，0 pageerror） | 否 | 否 | 必须不崩 | ✅ |
| UI 是否卡死 | 阻塞 4,892 ms（同步导入期） | 阻塞 11,608 ms | 42 ms（无感） | 导入期间可响应 | ⚠️ 见「发现 F-2」 |

**结论：R-1 三档全部有实测数；崩溃零；内存全过阈值（docx 档贴近上限——见 F-1）；UI 阻塞 = 同步 wasm 调用的架构事实（F-2，非本档缺陷——1MB/10MB 档同样阻塞只是时长短）。**

---

## 夹具（全部确定性可复现）

| 夹具 | 生成器 | 参数 | 产物 |
|---|---|---|---|
| `large-50mb.docx` | `crates/office-io/examples/gen_large_fixture.rs`（引擎写面产线——顺带 50MB 量级压测 DocxWriter：117,371 段生成 3.1s） | `docx <out> 50000000` | 50,096,362 B（输入上限 52MB[50MB 声明总量 + 2MB 结构边距]内） |
| `large-100mb.xlsx` | 同上（`add_sheet_streaming` 流式表——1M 格 = 写面上限 `MAX_CELLS`，56s 生成） | `xlsx <out> 250` | 101,096,230 B（16 表 × 250 行 × 250 列；抗压缩确定性伪随机填充——真实语料 zip 后仅 ~9MB 达不到边界档） |
| `large-500p.pdf` | `gen_large_pdf.py`（reportlab——引擎外产线：PDF 面度量对象是 pdf.js 视图非引擎写路径） | 默认 500 页 | 423,379 B / 500 物理页（PyMuPDF 复核 `page_count=500`） |

度量：`run_r1.py`（fetch 字节与导入分开计时；RSS = 渲染器进程合计——含 wasm 线性内存，比 `performance.memory`[仅 V8 JS 堆] 贴近峰值口径；rAF 旗标测事件循环恢复）。

## 明细数据（results-r1.json 原文摘要）

```
docx-50mb   bytes=50096362  fetchMs=139  importMs=4891  rssΔ=+746.3MB  rssAfter=888.0MB
            blocks=117371（headings=1 / paragraphs=117370）  rafDelayMs=4  pageerrors=[]
xlsx-100mb  bytes=101096230 fetchMs=228  importMs=11607 rssΔ=+588.3MB  rssAfter=728.8MB
            blocks=32（16 表 × [heading+table]）  stats: sheets=16 tables=16 rows=800
            rows_dropped=3200（渲染窗 50 行/表前缀截断——既有语义，越限计数显式）
pdf-500p    bytes=423379    fetchMs=5    openMs=42    rssΔ=+81.5MB   rssAfter=222.7MB
            pages=500（首渲后复核）  firstRenderMs=256  pageerrors=[]
```

## 发现（登记，含处置建议）

- **F-1 · docx 50MB 档内存贴近阈值（888MB / 1GB，余量 11%）**：
  线性外推不可靠的实证——#22 评论此前外推 52MB ≈ 439MB 堆，实测 888MB
  （外推低估 ~2×：docx-rs 模型对象开销 + zip 全量 inflate 双份峰值未计入
  线性模型）。处置建议：①导入大档走流式/分批（office-io 读面已全借用
  化 r4-t2，但 docx-rs 模型解析仍整体驻留）；②>30MB 档预警提示用户。
  非阻塞（阈值内）——登记为后续优化候选。
- **F-2 · 同步导入阻塞主线程（阻塞时长 = 导入时长）**：
  `__importDocx`/`__importXlsx` 为同步 wasm 调用——导入期间 UI 恒阻塞
  （rAF 在导入完成后 4ms 内恢复 = 事件循环健康，非死锁）。1MB 档
  ~166ms 无感、10MB 档 ~2.5s 可感知、50/100MB 档 4.9s/11.6s 明显卡。
  处置建议：Web Worker 化导入（wasm 无 DOM 依赖，worker 迁移面小）——
  登记为产品优化项，不属 R-1 度量缺陷。
- **F-3 · 环境对照**：本机（Linux 沙箱）50MB docx 4.9s，显著快于
  #22 评论的 Windows 环境外推（12.8s）——硬件差异 + 其外推基于 10MB
  档线性放大。两环境各自成立，跨环境数值不可直接比。

## 复现（仓库根）

```
cargo run -p mdc-office-io --example gen_large_fixture -- docx test-artifacts/large-file/large-50mb.docx 50000000
cargo run -p mdc-office-io --example gen_large_fixture -- xlsx test-artifacts/large-file/large-100mb.xlsx 250
python3 test-artifacts/large-file/gen_large_pdf.py
cd web && npm run build:full && cd ..   # wasm + dist
python3 test-artifacts/large-file/run_r1.py
```

（生成 ≈ 1.5 min + 度量 ≈ 1 min；磁盘峰值 ~750MB——大档导入内存
回落由 GC 决定，连续度量间渲染器进程随 page.close() 回收。）
