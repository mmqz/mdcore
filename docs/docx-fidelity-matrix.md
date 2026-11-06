# DOCX 编辑保真矩阵（#79 T2 · DocxEditor 外科手术式编辑）

> 单一真源随码走：本文件为 docx **编辑往返**保真的显式矩阵（「哪些面保真、
> 哪些拒改、哪些按配置」）。方法论对照 `cleanroom-analysis/office-line/
> xlsx-r1/FIDELITY.md` v2：**实测数字 + 条目级变化全解释 + 无未解释差异**，
> 不根据引擎理论能力认定。可执行锚点 = `crates/office-io/tests/
> docx_roundtrip_matrix.rs`（3 常驻测试 + 1 LO 探针）；真实语料证据链 =
> `scripts/t1_realdoc_verify.py`（POI corpus 18/18，PR #92 附件）。
> 编辑器实现 = `crates/office-io/src/docx_edit.rs`（#79 T1，PR #92）。

## 0. 方法

对每个面：**编辑一个目标段 → 保存 → 三层断言**：

1. **XML 层**（常驻测试）：目标段外 `document.xml` 逐字节恒等（slice 在场
   断言）+ 非目标部件 zip 逐字节恒等（`raw_copy_file` 透传）；
2. **往返层**（常驻测试）：重开编辑器 → `paragraph_text` 落账值/原文值
   与预期逐字等值；
3. **渲染层**（LO）：`soffice --headless --convert-to pdf` 开档 +
   `pdftotext` 新文在场/旧文退场 + 页几何恒等。

夹具两轨：**合成 13 面夹具**（单文件全面覆盖，可复现）+ **真实语料**
（Apache POI corpus 取样 17 件 + 自构混合档 1 件——真实 Word 生产形态）。

## 1. 矩阵（13 面，实测判定）

| # | 面 | 预期（任务书 #90） | 实测判定 | 断言锚点 |
|---|----|------|------|------|
| 1 | 段落文字 | ✅ 完美 | **✅ 完美**（含转义/空白/换行约定往返） | `matrix_thirteen_faces…`（重开等值）+ pit1/pit6/pit7 |
| 2 | 段落样式（Heading1/正文/自定义） | ✅ 完美 | **✅ 完美**（`w:pStyle` slice 逐字节恒等——不动即保真） | 面2 断言（`锚-标题段`/`锚-富格式段` slice） |
| 3 | 行内格式（粗/斜/下画线/色/字号） | ✅ 完美 | **✅ 完美**（`w:rPr` slice 恒等；编辑段的**首含文本 run** 属性保留=坑①，其余合并） | pit1 + 面3 断言 |
| 4 | 编号列表（有序/无序/多级） | ✅ 完美 | **✅ 完美**（`w:numPr` slice 恒等；编辑不重排 numbering 定义部件） | 面4 断言（`锚-列表项`/`锚-子级列表项`）+ ComplexNumberedLists 真实档 |
| 5 | 表格（单元格/合并/边框） | ✅ 完美（不动它） | **✅ 完美**（整 `<w:tbl>` slice 逐字节恒等；gridSpan/vMerge/tblBorders 原样） | 面5 断言 + TestTableColumns/table-alignment 真实档 |
| 6 | 图片（内嵌/位置/大小） | ✅ 完美（不动它） | **✅ 完美**（drawing 元素 slice 恒等；媒体部件 `raw_copy` 逐字节透传） | 面6 断言 + VariousPictures 真实档 |
| 7 | 页眉页脚（三变体） | ✅ 完美 | **✅ 完美**（header1-3/footer1 部件逐字节恒等——编辑面只辖 document.xml） | 面7 断言 + headerFooter/HeaderFooterUnicode 真实档；LO 探针钉 first 变体渲染 |
| 8 | 域代码（页码/目录/交叉引用） | ⚠️ 拒改 | **⚠️ 拒改**（`w:fldChar`/`w:instrText`/`w:fldSimple` 段 typed 拒；读不受影响——缓存结果文本可读） | pit3 + `matrix…`（field/toc 双拒）+ FieldCodes/FldSimple 真实档 |
| 9 | 批注 | ✅ 完美 | **✅ 完美**（comments.xml 部件恒等；正文 `commentRangeStart/End/Reference` slice 恒等） | 面9 断言 + comment.docx 真实档 |
| 10 | 脚注尾注 | ✅ 完美 | **✅ 完美**（footnotes.xml 部件恒等；`w:footnoteReference` run 恒等——非文本子节点不删=坑②） | 面10 断言 + footnotes.docx 真实档 |
| 11 | 修订（w:ins/w:del） | ⚠️ 按配置 | **⚠️ 按配置**（`RevisionPolicy::Refuse` 默认拒 / `Edit`：ins/moveTo 参与首文本位=当前生效文本，del/moveFrom delText 原样不动） | pit4 + `matrix_revision_policy_faces` + delins.docx 双策略真实档 |
| 12 | 文本框（w:txbxContent） | ✅ 完美 | **✅ 完美 + 可编辑**（内层段整 slice 恒等；内层段自身可编辑，`in_textbox` 旗标标记；嵌套链编辑互斥——见 §3-L1） | `matrix_textbox_editable_face` + shapes-with-text 真实档 |
| 13 | 目录（TOC 域） | ⚠️ 需手动更新 | **⚠️ 拒改+缓存原样**（TOC 域段属域族拒改；缓存条目字节原样=显示旧值，直到宿主 Word/LO 更新域——「需手动更新」语义的诚实落地） | `matrix…`（toc 拒改）+ 面13 断言 |

**判定汇总**：✅ 完美 9 / ⚠️ 拒改 2（域、TOC）/ ⚠️ 按配置 1（修订）——
与任务书预期 13/13 逐行一致，零未解释差异。

## 2. 条目级变化全解释（往返后哪些字节会变）

| 变化 | 出现位 | 机械解释 |
|------|--------|------|
| `word/document.xml`（变化） | 全部 | **编辑段本体**：首含文本 run 的首个 `w:t` 换新文（恒带 `xml:space="preserve"`）；后位文本 `w:t` 移除（其 run 残留非 rPr 子节点则保壳=坑②）；无文本段插入新 run；自闭合 `<w:p/>` 转开形。**变更区恒 ≤ 单段内**（不跨段边界——18/18 真实档实测，含 rtl.docx 4246B 多 run 段合并 1124B） |
| 其余全部部件 | 全部 | **逐字节恒等**（`raw_copy_file` 压缩流透传，不解压不重压——`_rels`/headers/footers/comments/footnotes/settings/styles/numbering/media） |
| zip 中央目录时间戳 | 全部 | document.xml 条目经 `start_file` 重写（deflate 同参确定性），其余条目原压缩字节；**10 轮往返尺寸逐字节恒等**（`len(set(sizes))==1`，实测膨胀 0%）——非膨胀，是重写段的确定性副产物 |

## 3. 已知限制（L 系列，显式登记非静默）

- **L1 嵌套段编辑互斥**：外层段与文本框内层段同时待编辑 = typed 拒
  （单趟拼接模型 v1 显式边界——`nested edit` 错误消息）。
- **L2 域/TOC/（Refuse 下）修订/Fallback 复制区段拒改**：读不受影响
  （`paragraph_text` 可读缓存结果/ins 生效文本）。
- **L3 新文本字符面**：XML 1.0 非法控制字符与 U+FFFE/U+FFFF typed 拒；
  `\t`→`<w:tab/>`、`\r\n|\r|\n`→`<w:br/>`（CRLF 合一）；无其他转换。
- **L4 保留节点读回**：保留的 tab/br 特殊节点（坑②「不删」）读回计入
  `paragraph_text`（新文字后随 `\t`/`\n`）——「不删」的诚实后果。
- **L5 mc:AlternateContent 双分支**：Fallback 复制区段拒改（防双分支
  不一致）；编辑 mc:Choice 分支条目。
- **L6 合成夹具的 LO 可载性**：comments/footnotes/settings 部件需挂
  `document.xml.rels` 关系条目（LO 经关系解析部件——bisect 实测定谳），
  夹具已按真实形态接线（rId13-15）。

## 4. 定性结论

1. **组合语义 = 修改（in-place surgical edit），非重建**：与 xlsx
   读-改-重建（FIDELITY v2 §4.1「不存在未触及字节保持的修改路径」）相反，
   docx 编辑面存在**未触及字节保持**路径——非目标部件 `raw_copy` 逐字节
   透传 + 目标段外 `document.xml` 零接触（18/18 真实档 + 3 常驻测试 +
   LO 探针实测）。**样式/元数据/表格/图片/页眉页脚在编辑往返中零丢失**
   （xlsx 路径的必然丢失集在此路径为空）。
2. **拒改面是安全设计不是缺陷**：域代码段编辑会造成缓存结果与指令
  不一致；修订编辑需明确策略（`Refuse` 默认宁缺毋错）——typed 错误
  消息含原因，调用方可诊断。
3. **编辑面的模型边界**：编辑辖 `word/document.xml` 段落（body/表格内/
   文本框内扁平索引）；页眉页脚/脚注/批注内容不进编辑面（原样保留=
   该面保真）；`w:sectPr`/numbering 定义/styles 定义同辖外（不动=保真——
   多节文档手术编辑后节结构不变，#257 起由 `tests/docx_sections.rs::
   edit_preserves_sections` 回归钉住）。

## 5. 复现

```bash
# 常驻矩阵测试（3 个）
cargo test -p mdc-office-io --test docx_roundtrip_matrix
# LO 探针（需 soffice + pdftotext）
cargo test -p mdc-office-io --test docx_roundtrip_matrix -- --ignored --nocapture
# 真实语料 18/18（编辑→XML diff→LO→10 轮零膨胀）
python3 scripts/t1_realdoc_verify.py   # 沙箱侧脚本，仓外复现
# 编辑驱动器（任意 docx）
cargo run -p mdc-office-io --example docx_edit_probe -- list <in.docx>
MDC_EDIT_POLICY=edit cargo run -p mdc-office-io --example docx_edit_probe -- edit <in.docx> <idx> <新文本> <out.docx>
# 多节分节符读写面（#257）
cargo test -p mdc-office-io --test docx_sections
```

## 6. 多节分节符读写面（#257，2026-11-06 增补）

编辑矩阵之外的**节级**读写面（审计 P1-1：多节文档中段节页眉/页脚
静默丢——`extract_headers_footers` 只读末节 `section_property`）：

| 面 | 判定 | 锚点 |
|---|---|---|
| 节读取（几何/槽位/块区间） | ✅ `DocxDocument::sections`（恒 ≥1 条，中段节六槽位引擎第二趟自解析） | `tests/docx_sections.rs`（docx4j `headers_footers_sections_multiple`：中段节 3 页眉 + 3 页脚恢复；POI `Headers.docx`：3 节各归位） |
| 节写入（封面/正文异构） | ✅ `DocxWriter::section_break`（SectionBreakSpec：几何/titlePg/起始页码/本节六槽位）+ `page_number_start`（末节） | 同上 `section_break_write_read_roundtrip`（双节回环 + OOXML 直检） |
| 编辑幸存 | ✅ sectPr 辖外不动=保真（回归钉住） | 同上 `edit_preserves_sections` |

末节行为零变化：文档级 `DocxDocument::headers`/`footers` 语义 = 末节
（health-check 基线零漂移——`corpus_docx4j_mid_section_slots_recovered`
双口径断言：文档级仅末节 / `sections[0]` 中段节全量）。
