# pdf-corpus —— #264 真实 PDF 语料（78 件 / 28.3MB）

**真实 PDF 验证语料**（issue #264：PDF 真实语料零存量——8 件合成件
撑不起 Acrobat 1:1 目标）。来源 = pdf.js / PDFBox 两个 Apache-2.0
仓的测试语料（真实世界捐赠件：bug 附件 / 书籍 / 学术论文 / 加密
样张），逐件 provenance 见 `PROVENANCE.md`。

## 纪律（对齐 `test-artifacts/pdf-organize-corpus/` 先例）

- **取件脚本 + PROVENANCE + README 入库；PDF 二进制不入库**
  （`*.pdf` 见根 .gitignore）。
- 取件：`python3 test-artifacts/pdf-corpus/fetch_corpus.py`
  （幂等：已存在跳过；依赖 curl；pdfinfo 缺席时 pages 记 null
  不阻断；取件 <70 件退出码 1）。**CI 同款**：windows-test.yml
  在 Test mdc-pdf 前跑本脚本——行为分布锁在云端真跑。
- 单件 ≤5MB；来源全部 Apache-2.0（与 GPL-3.0 主仓分发兼容）。
- 语料门 = `crates/pdf/tests/pdf_corpus.rs`（feature
  `engine-lopdf`）：#262 三态防御（Lead 评审 2026-11-06 修正：
  **清单件全缺 = 未取件 skip** / PROVENANCE 登记件在盘但收集
  0 件 = 收集逻辑坏红 / 缩水红）+ 每类数量下限 + 全件
  catch_unwind 不 panic + 行为分布锁。

## 分类目录

| 目录 | 件数 | 覆盖面 |
|---|---|---|
| `encrypted/` | 11 | AES-128/256、RC4-40bit 密码、加密+表单混合（PDFBox 加密全家族 + pdf.js 加密附件） |
| `forms/` | 11 | AcroForm 控件（按钮/选择/文本）、计算顺序、XFA（含真实加拿大移民表单 2.9MB） |
| `annotations/` | 13 | 高亮/下划线/删除线/波浪线/图章/墨迹/自由文本/附件/链接/折线 13 面 |
| `bookmarks/` | 4 | 嵌套大纲、GoTo 动作、非 ASCII 标题（瑞典语） |
| `scanned/` | 3 | 双级/灰度扫描、坏扫描件 |
| `multicolumn/` | 3 | 学术双栏（tracemonkey 基准件）、《Free Culture》整书 352 页、调查问卷 |
| `captions/` | 2 | 机翼图图注（X_NP/X_CG）、LATEX 图表引用排版 |
| `tables/` | 2 | 四列数值对齐表、捷克语报纸表格布局 |
| `mixed/` | 17 | 真实用户 bug 附件（issue*）、真实评论/高亮导出、Form XObject |
| `broken/` | 12 | 零页、坏 PageLabels、零宽线、零长度流、空 Rect 等结构异常 |

## 实测行为基线（2026-11-06，语料门首跑）

```
78 件：ok 68 / SecurityDeny 8（S5 加密拒绝实证）/
       Engine 拒 2（零页 spec 7.7.4）/ panic 0
max_pages = 352（freeculture.pdf——长文档覆盖锚点）
```
