# pdf-organize-corpus —— S2-1 页面组织真实语料

**真实 PDF 验证语料**（issue #103 任务 1 验收要求：真实文档 + 多页 + 书签 + 表单）。

## 纪律（对齐 `test-artifacts/large-file/` 先例）

- **生成器 + 清单入库；PDF 二进制不入库**（`*.pdf` 见根 .gitignore——
  本目录 92 件约 37MB，其中 300 页级压测件 31MB）。
- 测试面跨环境安全：`tests/pdff_organize.rs` 的语料门 = 目录缺失时
  **跳过并打印**（`MDC_PDF_ORG_CORPUS` 可覆盖路径）；fixtures 级验证
  （`crates/pdf/fixtures/`，仓内随包）不依赖本目录。

## 语料构成（provenance 见 `MANIFEST.json`）

| 段 | 来源 | 件数 | 写出器 |
|---|---|---|---|
| `poi__*.pdf` | POI 真实 docx 语料（`health-check/corpus/poi-docx/` 抽样 40）→ soffice 转换 | 40 | LibreOffice |
| `docx4j__*.pdf` | docx4j 真实 docx 语料（`health-check/corpus/docx4j/` 抽样 40）→ soffice 转换 | 51 | LibreOffice |
| `large__manual300.pdf` | 全池真实件 ×10 轮 `pdfunite` 拼接（**1390 页**压测件） | 1 | poppler |

覆盖面（实测统计）：加密 0 件；带书签（LO 标题样式驱动大纲）19 件
（最深 25 项）；表单域/字体子集/资源树随 LO 写出器原生结构；
`large__manual300` 含 poppler 拼接的 offset-0 对象等**真实怪癖**。

## 重建

```bash
python3 test-artifacts/pdf-organize-corpus/build_corpus.py --sample 40
# 依赖：soffice（转换）+ pdfunite/pdfinfo（poppler-utils，拼接与登记）
```

生成器确定性：排序 + 等距抽样（无随机源）——同语料源重建结果一致。

## 为什么不用下载的「真实招标文件」

沙盒出网下载外站文件不可复核（来源完整性/许可面）。本语料的
「真实」= 真实文档内容（POI/docx4j 语料库，与 office-io S1 轮验证
同源）× 真实工业写出器结构（LibreOffice/poppler 的 xref/页树/资源/
字体子集布局）——对页面组织手术的结构验证面，这比单一来源的下载件
覆盖更广（92 件 × 多写出器）。若后续需要 Acrobat/Word 直出件的
补充锚点，登记为语料增强项（不阻塞本任务验收）。
