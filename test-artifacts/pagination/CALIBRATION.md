# N-5 分页预估校准报告（#29 任务池 · estimate_pages）

> 日期：2026-10-28 · 引擎 rev = 本提交 · 校准环境：LibreOffice 25.2.3.2（沙箱）+ Noto CJK 字体面
> 模型 = `crates/mdcore/src/pagination.rs`（v2）· 度量 harness = `crates/mdcore-wasm/examples/n1_n5_harness.rs`

## 1. 校准方法（双层实测）

- **结构层**：9 个受控微件（600 段 × 50 字基准 + 逐种类增量），LO 渲染页数差分 → 逐块种类真实行高；
- **聚合层**：40 万字夹具（`test-artifacts/word-count/fixture_400k.md`：400,302 字 / 3,406 段 / 941 标题 / 313 表）双排版档 LO 页数 → 常数收敛。

## 2. 结构层测点（微件差分，2026-10-28 实测）

| 微件 | 内容 | default 实测/估算 | tender 实测/估算 |
|---|---|---|---|
| micro-para | 600 段 × 50 CJK | 28 / 27 页 | 50 / 50 页 |
| micro-code | +600 行 ASCII 代码 | 38 / 38 页 | 72 / 71 页 |
| micro-table | +601 行 × 3 列表 | 41 / 42 页 | 75 / 75 页 |
| micro-list | 600 列表项 × 50 字 | 28 / 27 页 | 50 / 50 页 |
| micro-quote | 600 引用段 × 50 字 | 28 / 27 页 | 50 / 50 页 |
| micro-punct | 600 段 × ~20% 全角标点 | 28 / 27 页 | 50 / 50 页 |

**结构结论**（差分定谳，进常数）：

| 种类 | 行高模型（em） | 依据 |
|---|---|---|
| 正文（段/标题/列表/引用） | `字号 × (0.3115 + 1.15 × 行距倍数)` | 单倍实测 1.46-1.53em、1.5 倍实测 1.99-2.08em → A/B 两常数（LO 比例行距只缩放 ascent/descent 面，lineGap 面不缩放） |
| 代码（等宽） | `字号 × 1.17 × 倍数` | Courier→Liberation Mono 度量矮于 CJK 面（default 差分 +10 页/600 行） |
| 表格行 | 正文行高 + 25 twips | 差分 13-14 页/601 行 ≈ 正文行高 + 边框余量 |
| 列表/引用/标点 | 零额外高度 | 与纯段同页数（缩进在 2 行段落粒度不显影；标点挤压同） |

**排版档参数**：

| 档 | 字号 | 行距 | 版心（twips） | 每行全角 | 每页行数 |
|---|---|---|---|---|---|
| default（引擎缺省） | 10pt | 单倍 | 8504 × 13152 | 42 | 44 |
| tender（中文标书） | 14pt（四号） | 1.5 倍 | 8306 × 13958 | 29 | 24 |

## 3. 聚合层验收（N-5 指标：40 万字 ≤5%）

| 档 | LO 实际页数 | 估算页数 | 偏差 | 判定 |
|---|---|---|---|---|
| default | **330** | 332 | **+0.61%** | ✅ PASS |
| tender | **935** | 912 | **-2.46%** | ✅ PASS |

明细（行消耗分解）：

| 档 | 段落行 | 标题行 | 表格行 | 代码行 |
|---|---|---|---|---|
| default | 11,323 | 941 | 1,985 | 1,095 |
| tender | 18,614+ | 941 | 1,985 | 1,095 |

## 4. 误差带自报（`error_margin_pct`）

- 纯段落型（表格/代码行占比 ≤20%）：±5%；
- 重结构型（>20%）：±10%——单元格折行与等宽字体替换的不确定面放大。

## 5. 已知偏差方向（登记）

- **段落孤行控制**：LO 默认 widow/orphan 2/2 → 段落密集文档页尾浪费 ≈1 行/页（≈2% 偏低估——40 万字档被其他正向偏差吸收，微件纯段档 -3.6% 可见）；
- **Word 实机差**：常数按 LO + Noto CJK 面校准；Word + 中易宋体/仿宋的单倍行高 em 系数略低（≈1.30 vs LO 1.46），实机导出页数会少于本预估——误差带吸收，标书侧以「预估偏保守」为安全方向（宁可高估不漏超页）。

## 6. 复现命令

```bash
cargo build --release -p mdcore-wasm --example n1_n5_harness
python3 test-artifacts/large-doc/run_n1.py          # 40 万字双档验收
# 微件：见 scripts 探针（gen_micro_calib*.py 生成 → harness export → soffice 渲染计数）
```

## 7. API 面（消费方接入）

- 核心：`mdcore::pagination::estimate(&BlockTree, &PaginationParams) -> PageEstimate` / `Core::estimate_pages()`
- wasm：`WasmCore::estimate_pages_json(options_json)`——options 语法与 `export_docx_styled` 同一（typography/page_geometry；以将要导出的同一 options 预估，口径天然对齐）
- 字段：`pages` / `chars_per_line` / `lines_per_page` / `paragraph_lines` / `heading_lines` / `table_lines` / `code_lines` / `error_margin_pct`
