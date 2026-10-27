# TR-2A · 跨进程确定性报告（issue #11 §3）

> 驱动：`web/tr2a_cross_process.py` · 结果：`web/tests/results/cross_process_determinism.json`
> 进程模型：每轮 = 独立 chromium OS 进程（`chromium.launch()` 冷启动，wasm
> 实例随页面重建——单进程内缓存/全局态被彻底排除）。
> 覆盖：解析面（`__inject(md)` → `__md()` 投影 SHA-256）+ 导出面
> （`__exportDocx()` 字节 SHA-256）。OCR 面属标书仓（bid-parse）——
> 引擎仓无 OCR 面，诚实登记不伪装；导出面与标书共享同一 wasm 包。

## 语料（8 件——跨层形态覆盖）

headings / lists_mixed / table / code_fence / unicode（emoji+ZWJ+CJK 扩展 B+零宽+bidi）/ long_doc（400 段）/ nested_quotes / inline_marks

## 结果表（3 次独立进程固定序 + 2 次乱序）

| 轮次 | 进程 | 语料序 | 结果 |
|---|---|---|---|
| A | 进程 1（冷启） | 固定序 | 基线 digest 集 |
| B | 进程 2（冷启） | 固定序 | 与 A **逐位一致**（8 语料 × 双面） |
| C | 进程 3（冷启） | 固定序 | 与 A **逐位一致** |
| D | 进程 4（冷启） | 乱序（seed 20261027） | 与 A **逐位一致**——输出与语料序无关 |
| E | 进程 5（冷启） | 乱序（seed 99887766） | 与 A **逐位一致** |

**判定：DETERMINISTIC**——8 语料 × 5 轮 × 2 面 = **80 组 digest 比对零失配**
（解析投影 SHA-256 与导出字节 SHA-256 全部跨进程、跨序稳定）。

## 结论

1. 引擎解析与导出在跨进程（冷 wasm 实例）与语料乱序两个扰动轴上**完全
   确定性**——#11 §3 所指「进程内缓存/全局态掩盖不确定性」风险**实证排除**；
2. 未发现任何不一致——无需缓存定位（OCR/字体/zlib 三项缓存嫌疑面中，
   OCR 不在本仓、字体与 zlib 经导出 digest 稳定间接排除）；
3. 复现：`python3 web/tr2a_cross_process.py`（约 40s，5 轮冷启动浏览器）。
