# pdf-organize-corpus —— S2-1 页面组织真实语料

**真实 PDF 验证语料**（issue #103 任务 1 验收要求：真实文档 + 多页 + 书签 + 表单）。

## 纪律（对齐 `test-artifacts/large-file/` 先例）

- **生成器 + 取件脚本 + 清单入库；PDF 二进制不入库**（`*.pdf` 见根
  .gitignore——主语料 92 件约 37MB，其中 300 页级压测件 31MB）。
- 测试面跨环境安全：`tests/pdff_organize.rs` 的语料门 = **未取件时
  跳过并打印**（清单件全缺 = 未取件；`MDC_PDF_ORG_CORPUS` 可覆盖
  路径）；fixtures 级验证（`crates/pdf/fixtures/`，仓内随包）不依赖
  本目录。
- 门语义（Lead 评审 2026-11-06 定案）：目录在任何 clone 里都在场
  （README/清单已提交），取件信号看**清单件在场性**——任一清单件
  在盘但递归收集 0 件 = 收集逻辑坏 → 红；清单件全缺 = 未取件 →
  响亮 skip。

## 语料构成（provenance 见 `MANIFEST.json`）

| 段 | 来源 | 件数 | 写出器 |
|---|---|---|---|
| `poi__*.pdf` | POI 真实 docx 语料（`health-check/corpus/poi-docx/` 抽样 40）→ soffice 转换 | 40 | LibreOffice |
| `docx4j__*.pdf` | docx4j 真实 docx 语料（`health-check/corpus/docx4j/` 抽样 40）→ soffice 转换 | 51 | LibreOffice |
| `large__manual300.pdf` | 全池真实件 ×10 轮 `pdfunite` 拼接（**1390 页**压测件） | 1 | poppler |

覆盖面（实测统计）：加密 0 件；带书签（LO 标题样式驱动大纲）19 件
（最深 25 项）；表单域/字体子集/资源树随 LO 写出器原生结构；
`large__manual300` 含 poppler 拼接的 offset-0 对象等**真实怪癖**。

| 段 | 来源 | 件数 | 写出器 |
|---|---|---|---|
| `realworld/*.pdf` | pdf.js / PDFBox 测试仓（Apache-2.0，与 #264
  pdf-corpus 同源）——多页真实件轻量子集 | 8 | 多写出器 |

`realworld/` 是 CI 通道（Lead 方案 A）：`windows-test.yml` 每次跑
`Test mdc-pdf` 前取件，sweep（≥5 件全操作）与 10× 往返抽样在云端
真跑；`large__manual300` 压测件不随 CI（31MB + 组装耗时），
`large_manual_delete_extract` / `manual300_pdfium_reopen` 按在场性
自跳过。

## 重建 / 取件

```bash
# 轻取件（CI 同款——多页真实件 8 件，约 11MB，幂等）
python3 test-artifacts/pdf-organize-corpus/fetch_corpus.py

# 全量重建（本地——LO 转换 92 件 + manual300 拼接）
python3 test-artifacts/pdf-organize-corpus/build_corpus.py --sample 40
# 依赖：soffice（转换）+ pdfunite/pdfinfo（poppler-utils，拼接与登记）
```

生成器确定性：排序 + 等距抽样（无随机源）——同语料源重建结果一致。
取件幂等：已存在文件跳过（`kept`）；单源漂移会显式 `[MISS]`，
取件不足 5 件退出码 1（sweep 下限对齐）。

## 为什么不用下载的「真实招标文件」

沙盒出网下载外站文件不可复核（来源完整性/许可面）。本语料的
「真实」= 真实文档内容（POI/docx4j 语料库，与 office-io S1 轮验证
同源）× 真实工业写出器结构（LibreOffice/poppler 的 xref/页树/资源/
字体子集布局）——对页面组织手术的结构验证面，这比单一来源的下载件
覆盖更广（92 件 × 多写出器）。若后续需要 Acrobat/Word 直出件的
补充锚点，登记为语料增强项（不阻塞本任务验收）。
