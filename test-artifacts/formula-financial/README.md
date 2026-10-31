# formula-financial —— S3 任务 1 真值对照语料（#110）

**季度财务模型**（16 行 × 7 列，~50 公式）：收入/成本/费用三类别明细 +
SUM 行小计 + SUMIF 类别汇总 + IF 除零防护比率 + Q4 环比。覆盖
`SUM / SUMIF / IF / 算术 / 百分比`——真实财务表形态。

## 文件

| 文件 | 语义 |
|---|---|
| `fin_raw.xlsx` | 手写原始件（数据 + 公式**无缓存值**，2.4KB） |
| `fin_lo.xlsx` | soffice 重算件（LibreOffice 计算写入缓存值 = **真值 oracle**，6.8KB） |
| `gen_fin_fixture.py` | 生成器（重建：`python3 gen_fin_fixture.py`，需 soffice） |

## 用途

`crates/formula/tests/real_xlsx_lo.rs` 双层验证：

1. **恒跑**（无环境依赖）：读 `fin_lo.xlsx` 公式 + LibreOffice 缓存值 →
   mdc-formula 重算 → 逐公式格对照；
2. **环境门**（soffice 在位）：`fin_raw.xlsx` → 即时转换重算 → 再对照
   （链路活性复核——防 fin_lo.xlsx 陈旧）。

真值锚点（Python 独立复核）：Q1 收入合计 165000；Q1 毛利 66000；Q1 毛利率 0.4；全年收入 755500（165000+183000+193500+214000）；全年净利润 213200；Q4 收入环比 ≈ 0.105943152454780。
