# 05 · DOCX 读写链路深度审计（mdcore-audit main@96596d7，2026-11-04）

> 只读审计。范围：`crates/office`（sanitize 六面）+ `crates/office-io` DOCX 面
> （docx.rs / docx_edit.rs / write.rs / rebuild.rs / style.rs / numbering.rs / io.rs）
> + mang-shell 消费面（docx_doc.rs / docx_edit_bridge.rs 及 5 个测试）。
> 证据口径：`文件:行号` 为 HEAD 实测；版本锚 = Cargo.lock（docx-rs 0.4.22 /
> zip 8.6.0 / quick-xml 0.41.0）。参照底稿：`mang-research-archive/
> cleanroom-analysis/office-line/`（ADR-O1、docx-r1 四件）、`ai-bid-research/`
> （18 仓调研）。loc-stats 基线：office-io src 22,814 LOC / 616 unwrap / 350 #[test]。

## 0. 总览事实（一分钟版）

- **规模**：DOCX 专属引擎代码 ≈ **15,812 LOC**（docx.rs 8,242 + docx_edit.rs 3,853
  + style.rs 2,100 + numbering.rs 1,617），共享容器门 io.rs 2,407；write.rs(1,199)/
  rebuild.rs(796) **为 XLSX 专属，DOCX 无对应物**。测试：内联 123 + 集成 106
  （DOCX 面），另有 mdcore-wasm health_check_docx 双语料门（630 真实档）与
  mang-shell 5 个消费测试。
- **三条路径、三种模型**（读写**不共用**模型——见 §1）：读=docx-rs 模型→自有
  扁平模型；新建=自有 RunSpec→docx-rs builder；编辑=quick-xml 字节手术（零模型）。
- **「部件透传保真」在 DOCX 侧 = DocxEditor**（PR #92/#79，早于 xlsx PR #193）：
  非目标部件 `raw_copy_file` 逐字节透传（docx_edit.rs:2275-2280），编辑段外
  document.xml 零接触（apply_all_edits 单趟拼接，docx_edit.rs:2044-2173）。
- **最重的问题**：docx-rs 0.4.22 读路径在**真实语料**上 4 处 panic + word365
  属性族 22/23 解析失败（已锁基线但生产可达，§P0）；w:sdt 内容结构性丢弃（§P1）；
  sanitize 六面**零产品接线**（§Q6）；style/numbering corpus 门因语料目录改版
  **静默空转**（§P1）。
- **差分测试**：LibreOffice 有真实调用但全部软门（缺件 SKIP / `#[ignore]`）；
  **Word / WPS 零真实验证**——结论成立（§Q4）。

---

## 1. Q1 · 读写模型对称性与「部件透传保真」机制

### 1.1 三条路径、三套模型：读写不对称（已确认）

| 路径 | 入口 | 模型 | 引擎 |
|---|---|---|---|
| 读（模型级） | `read_docx*`（docx.rs:1685-1726） | `DocxDocument{blocks/headers/footers/images/comments/footnotes/endnotes}`（docx.rs:1660-1678）——**扁平文本模型** | docx-rs `read_docx`（docx.rs:1799） |
| 写（新建） | `DocxWriter`（docx.rs:4262） | `RunSpec/TableSpec/FieldSpec/TocEntry/StyleDef/NumberingDef`（docx.rs:3854-4233 等） | docx-rs builder（save_to_vec→`Docx::build().pack()`，docx.rs:5146-5150） |
| 编辑（外科手术） | `DocxEditor`（docx_edit.rs:247） | **无模型**——原文坐标系字节 span + quick-xml 事件树（ElNode，docx_edit.rs:1789） | 自研（quick-xml + zip raw_copy） |

- 读模型与写模型**类型不共享**：`RunData`（text/bold/italic/hyperlink/revision_inserted，
  docx.rs:1426-1436）≠ `RunSpec`（text/bold/italic/mono/strike/underline/link/anchor/
  footnote/field_*/char_styled，docx.rs:3854）。读→写回放是**消费方手写映射**——
  唯一示范在测试 `r4t3_roundtrip_read_modify_write`（docx_roundtrip.rs:1977-2200），
  非产品 API。XLSX 侧有产品化 `rebuild_book`（rebuild.rs:1-60），**DOCX 侧没有**。
- 因此「读-改-写复杂文档」在 DOCX 有两条互斥路：模型重建（大丢集，§3）或
  字节手术（近零丢集，但只能改文本/域显示值/批注脚注文本）。
- 消费面（mang-shell docx_doc.rs:160-229 / mdcore-wasm docx_import.rs:760）全部
  走读面 + 编辑面组合；导出面（docx_export）只走新建写面。

### 1.2 「部件透传保真」（PR #193 同族机制，DOCX 侧实现）

DOCX 侧机制 = `repack_parts`（docx_edit.rs:2242-2290）：

1. **非整包 zip 透传，是逐部件重建**：遍历原 zip 条目（`by_index_raw`），
   非目标部件 `writer.raw_copy_file(entry)`——**原压缩字节流原样搬运**（不解压
   不重压，字节级保留，含压缩方法/CRC）；目标部件（word/document.xml +
   可选 footnotes.xml/comments.xml）`start_file` + 重 deflate。
2. **编辑段外 document.xml 零接触**：`apply_all_edits` 按段 span 升序单趟拼接
   （docx_edit.rs:2044-2173），段间字节直拷；同段手术只动 `w:t` 节点（保留
   首个含文本 run 的 `w:rPr`，其余文本 run 合并——「坑①」，docx_edit.rs:1937-1982）。
3. **零编辑 = document.xml 原样**（matrix_revision_policy_faces 断言，
   docx_roundtrip_matrix.rs:426-431）；**10 轮往返零膨胀**（docx-fidelity-matrix.md
   §2：`len(set(sizes))==1`）。
4. 已验证面：编辑保真矩阵 13 面（docs/docx-fidelity-matrix.md §1：✅9/⚠️拒改2/
   ⚠️按配置1）+ 真实语料 18/18（POI，PR #92 证据链）+ mang-shell 桥三硬性质
   （docx_edit_bridge.rs:14-33：不编辑→字节级恒等 / 改一段→非目标部件逐字节
   不变 / 非目标段落逐字节不变）。

**结论**：DOCX 侧「部件透传」= 未修改部件**字节级**保留（raw_copy_file），
修改部件仅 document.xml/footnotes/comments 三件重压缩；中央目录时间戳等 zip
元数据有确定性副产物（已实测登记）。该机制真实、测试锚扎实——是全链路最强的
保真资产。

---

## 2. Q2 · 能力矩阵核实（逐项实现深度 + 代码证据）

图例：🟢 完整（语义+结构+测试锚）/ 🟡 部分（有实质实现但有明确缺口）/ 🔴 桩或缺失。
「读」= 模型级读面；「写」= DocxWriter 新建面；「编」= DocxEditor 手术面。

| # | 能力 | 读 | 写 | 编 | 判定与证据 |
|---|---|---|---|---|---|
| 1 | 样式表定义（basedOn 链/next/w:default/docDefaults/主题色） | 🟢 | 🟡 | 🟢(不动=保真) | 读=自有 quick-xml 解析层 style.rs:1-115（引擎四缺失定谳：w:default/next 不读、Name/BasedOn 值私有、Theme 只解析 FontScheme）→ `StyleTree.chain`（环 visited 截断/悬空截断，style.rs:289-309）+ `effective_format`（docDefaults 铺底→链逐层 Some 覆盖，style.rs:313-324）+ `ThemeColors` 12 槽 + shade/tint 数学（ECMA 数值例钉定）。204 件真实语料首轮实测（style_corpus.rs:1-24）。写=`define_style/styled_paragraph`（style.rs:1268/1450），link 放松不合并。 |
| 2 | 段落样式（pStyle 语义） | 🟢 | 🟢 | 🟢 | 读=style ID 透传（docx.rs:3037）+ 标题层级四链单一真源解析（outlineLvl 直接格式>pStyle 前缀>样式名 heading/紧凑/中文形>样式 outlineLvl，docx.rs:2131+1991-2012；真 Word 中文版 styleId="1"-"9" 夹具钉定 tr13）。写=Heading1-9 定义+视觉面（docx.rs:4315-4342）。 |
| 3 | 字符样式（rStyle） | 🔴 | 🟡 | 🟢 | **读模型无承载**：`RunData` 无 style 字段（docx.rs:1426-1436）——run 级 rStyle 引用丢弃（styles.xml 定义可读但正文引用断链）。写=`RunSpec::char_styled`+预置 Strong/Emphasis（docx.rs:4343-4358）。 |
| 4 | 样式继承落到内容（有效格式） | 🔴 | — | 🟢 | `effective_format` 存在（style.rs:313）但**零产品消费者**（grep 全仓仅 office-io 自测试）——导入渲染（mang-shell/wasm）不解析有效格式，行内样式只有 bold/italic presence。 |
| 5 | 嵌套表格 | 🟢 | 🔴 | 🟡 | 读=递归提取 depth≤16、内层表独立成块+`nested` 标记（docx.rs:3219-3257，#39：旧静默丢整块已修）；写=TableSpec 单元格纯文本（docx.rs:4182-4233）**无嵌套发射面**；编=嵌套表段落可改但 `table_cells` 只收最外层表（docx_edit.rs:619-660，显式登记）。 |
| 6 | 表格合并（gridSpan/vMerge） | 🟢 | 🟢 | 🟢(不动) | 读=栅格展开+merges 矩形清单（docx.rs:3186-3255）；**读取通道=serde_json 序列化 hack**（`TableCellProperty` 字段私有无 getter，docx.rs:3237-3244——版本钉死 0.4.22，升级即静默退化，代码内已登记）。写=table_advanced gridSpan/vMerge（docx.rs:4884）。真 Word 件 word-merged.docx 锚定。 |
| 7 | 图片（a:blip） | 🟡 | 🟡 | 🟢 | 读=**容器级清单**（[Content_Types]+word/media/* 枚举+.rels 联接+run 级 r:embed 计数，docx.rs:996-1121/1125-1400）——**块序内图片位置不入块模型（#170 已知缺口登记）**；消费面图片不入行流（docx_doc.rs:107-109，props.images 计数 + 空段占行）。写=PNG-only 内联图显式尺寸（docx.rs:4802，`Pic::new` 转码需 +23 crate 特性不启用）。 |
| 8 | 页眉/页脚 | 🟡 | 🟢 | 🟢(不动) | 读=六槽位中实际挂接的（docx.rs:3505-3656，变体透传；first/even 激活语义归消费方）；**只读 `docx.document.section_property` 单节（docx.rs:3513）——多节文档中段节的页眉/页脚静默丢失**（引擎模型只挂末节）。写=六方法+自动 titlePg/evenAndOddHeaders+原位替换保 rid（docx.rs:4723-4783/5595-5654）。 |
| 9 | 分节符/节属性 | 🔴 | 🟡 | 🟢(不动) | 读模型**无 sectPr 承载**（页尺寸/边距/分栏/分节符均不入 DocxDocument——PageGeometry 是写面类型 docx.rs:3713）；块序无节边界。写=单节 page_geometry（docx.rs:5049）。 |
| 10 | 页码域（PAGE/NUMPAGES） | 🟡 | 🟢 | 🟡 | 读=FieldMachine 表现型剥离（volatile 域缓存结果不入正文+fields_dropped 计数，docx.rs:2053-2094/2098-2130）；指令文本不入正文。写=fldChar 三连（docx.rs:4016-4033）。编=显示值可改（结果区 w:t 手术，域保持活——`set_field_value`，docx_edit.rs:479-541，docx_fields.rs 域活证伪法验证）。 |
| 11 | 超链接 | 🟢 | 🟡 | 🟢 | 读=四变体（External/Internal/Anchor/Unresolved）+自有 OPC rels 联接（docx.rs:1895-1956——引擎只透传 r:id 的缺口自补）；**表格单元格内目标丢弃**（TableData 扁平面，登记）。写=外链+锚点（docx.rs:3959/3981）；**包内 Internal 目标写面无承载**（roundtrip 测试登记静默丢）。 |
| 12 | TOC / PAGEREF（#206） | 🔴 | 🟢 | 🟡 | 写=`toc_entries`（域头 begin[dirty]+instr+separate + 项段 hyperlink(anchor)+tab+PAGEREF 五连 + toc_N 书签避让，docx.rs:4523-4720）+`field_pageref`（docx.rs:4077）——目录可点击跳转、打开提示 F9 更新。读=TOC 属内容型域缓存结果保留为**普通文本**（结构不暴露）；**sdt 包裹的 TOC 整体结构性丢弃（见 P1-2）**。编=TOC 跨段域只读（docx_edit.rs:524-532）。 |
| 13 | 批注（tr19） | 🟢 | 🔴 | 🟢 | 读=comments.xml+锚关联（commentRangeStart→块索引，孤儿批注 None，docx.rs:2669-2755）+批注内容块同构提取；真 Word 批注栈（commentsExtended）主件读取（tr19_comments_notes.rs:6-9）。编=`set_comment_text`（docx_edit.rs:564-571，作者/日期/锚零接触）。**写面无批注创建 API**。 |
| 14 | 修订（tr17） | 🟢 | 🔴 | 🟡 | 读=w:ins 当前生效文本+run 级 revision_inserted 标记；w:del delText 不提取+逐段 RevisionDelete 警告；**MoveFrom/MoveTo 不提取（幽灵文本，登记）**（docx.rs:3075-3103）。编=Refuse 默认拒/Edit 放行（ins 参与、del 原样，docx_edit.rs:175-183/395-402）。写面无修订发射。 |
| 15 | 文本框（tr18） | 🟢 | 🔴 | 🟢 | 读=双形态：DrawingML wps:txbx 模型面（docx.rs:2843-2860）+ VML v:textbox 文档级第二趟（scan_vml_textbox_regions 自研字节扫描+buffer_position 双端补偿，docx.rs:2230-2339）+ mc:Fallback 防双取；内层块与正文同构（样式/域/修订通道全承载）。编=内层段可编辑（in_textbox 旗标）。**写面无文本框创建 API**。 |
| 16 | 脚注/尾注 | 🟢 | 🟡 | 🟢 | 读=引擎从不读 footnotes.xml（CAPABILITY_NOTES §5）→**文档级第二趟补提取**（parse_notes_part，docx.rs:1850-1869/2555-2668；分隔符条目滤除）+段落级引用位标注（scan_note_references，docx.rs:2398）。写=RunSpec::footnote（docx.rs:3999）。编=set_footnote_text（多段脚注残余空段登记）。 |
| 17 | 编号语义（numPr→numFmt 全链） | 🟢 | 🟡 | 🟢(不动) | 读=NumberingIndex（num 桥→abstractNum→lvlOverride，docx.rs:2903-2933）+kind/format+悬空严格拒/兼容 BrokenReference（carry_numbering docx.rs:2947-3009）；numbering.rs 另有**完整定义面解析+序号计算器**（lvlRestart/isLgl/startOverride Word 语义，numbering.rs:274-338）——但零产品消费（§P2-4）。写=固定 100/101 两抽象（非 decimal/bullet 原生格式降级、format 字段可诊断，docx.rs:1444-1459）+define_numbering 自定义级（numbering.rs:986）。 |
| 18 | 缩进（w:ind） | 🟡 | 🔴 | 🟢(不动) | 读=left/right/firstLine/hanging twips 透传（docx.rs:3013-3022）；**Chars 字符单位变体不承载**（登记）。写=无任意 ind 面（仅列表/引用间接几何）。 |
| 19 | 行内格式读取（bold/italic/rFonts/u/strike） | 🟡 | 🟢 | 🟢 | bold/italic **presence 语义**——`w:val="false"` 显式关闭不可区分（引擎 Bold.val/Italic.val 私有，docx.rs:3170-3171+模块文档登记）；rFonts/strike/underline 值私有不承载。写=mono/strike/underline 三位（docx.rs:5336-5340）。 |
| 20 | 表格首行跨页表头（tblHeader） | — | 🟢 | — | 引擎 TableRowProperty 无此面→**save 漏斗 zip 后处理注入**（mark_table_header_rows，docx.rs:5875-6031；n4_table_header.rs）。 |

**汇总**：🟢 11 / 🟡 7 / 🔴 6（按上表 20 面计）。整体画像：**读面「文本提取」
强（表格/批注/脚注/文本框/修订/域语义均有真实现+真件锚），「格式与结构语义」
弱（字符样式/有效格式/节属性/TOC 结构/sdt）**；写面是「新建生成器」
（能力面=标书导出所需子集，非通用编辑器写面）；编辑面窄而深（文本/域值/
批注脚注四类手术，其余拒改=fail-closed）。

---

## 3. Q3 · 往返损失面（读→改→写复杂 Word 文档会丢什么）

### 3.1 路径 A：模型重建（read_docx → DocxWriter，消费方映射）

矩阵测试范围（docx_roundtrip_matrix.rs，513 行）：**只测 DocxEditor 手术面**
（13 面：样式/行内格式/编号/表格合并边框/图片/页眉三变体+页脚/域/批注/脚注/
修订/文本框/TOC/超链接——每面「编辑目标段外逐字节恒等」断言）。模型重建路径
只有 docx_roundtrip.rs:1977-2200 一个**测试示范**（非产品 API），其登记丢集：

| 丢失面 | 证据 |
|---|---|
| 图片（不回放——块序无图片块） | docx_roundtrip.rs:2064-2067 注释「登记丢失成员」 |
| 脚注（内容面读回了但写回不回放） | 同上；写面 RunSpec::footnote 存在但重建映射不产 |
| 文本框 | `Block::TextBox(_) => {}`（docx_roundtrip.rs:2148-2150） |
| 原样式表（styles.xml 整体） | DocxWriter::new 只产 Heading1-9+Strong/Emphasis（docx.rs:4315-4358）；define_style 需消费方逐条重建 |
| 表格结构（边框/底纹/列宽/合并） | table() 纯文本栅格（docx.rs:4825）；table_advanced 需显式重声明 |
| 编号定义（自定义 numFmt/lvlText/glyphs） | 写面 100/101 降级（docx.rs:1444-1459） |
| 节属性/页面几何 | 写面单节 page_geometry（docx.rs:5049），多节不可表达 |
| 批注/修订/书签定义 | 写面无对应 API（书签 API 有：heading_bookmarked/bookmark_paragraph） |
| 行内字符样式/等宽/下画线之外的面 | RunSpec 子集 |
| 内部超链接（Internal 目标） | roundtrip 登记静默丢（docx_roundtrip.rs:2094-2101） |

### 3.2 矩阵没测的结构（读面直接丢弃/忽略——Word 复杂档真实存在）

- **SmartArt / DrawingML 图表 / 形状**：`RunChild::Drawing/Shape 面外跳过`
  （docx.rs:3164-3165）；图片仅容器级清单。
- **OLE 对象**（docx4j 语料 OLE_5 在场仅测不崩）：同上跳过。
- **OMML 数学公式**：ParagraphChild/RunChild 无 m:oMath 分支→`_ => {}`
  （docx.rs:2888/3165）。
- **内容控件 w:sdt**：docx-rs 解析为 StructuredDataTag，extract_blocks `_ => {}`
  **结构性丢弃**（docx.rs:2887-2888；#156：标书 TOC 域包 sdt→宽面 609≠窄面 611
  段，mang-shell 靠 in_sdt 旗标对账降级，docx_edit.rs:204-221）。
- **宏（.docm/vbaProject.bin）**：无扩展名门——二进制部件过门（非 .xml 不做
  结构校验）后模型忽略；写面永不产出。
- **自定义 XML（customXml/*）**：过门后忽略。
- **glossaryDocument / themes 的完整面**：theme 只取 clrScheme 12 槽（style.rs）。
- **多节 sectPr**：§2-8（中段节页眉页脚丢）。
- **MoveFrom/MoveTo 移动修订**：不提取（docx.rs:3104-3107）。
- **mc:AlternateContent 非 txbxContent 的分支**：Fallback 复制区读面跳过
  （防双取）——语义=Choice 分支优先（合理但属有损选择）。

### 3.3 路径 B：DocxEditor 手术（近零损失，窄面）

丢集 = 显式拒改面（域段/Refuse 修订/Fallback/跨段域/嵌套域/退化域）+ 编辑段内
多 run 合并（首 run rPr 保留、其余并入）+ 表格/页眉页脚内容不可编辑（原样=保真）。
L1-L6 限制登记完整（docx-fidelity-matrix.md §3）。**这条路径的诚实度是全链最高**：
每个拒改都 typed + 原因字符串。

---

## 4. Q4 · 差分测试矩阵现状

| 门 | 外部软件? | 现状 | 证据 |
|---|---|---|---|
| external_corpus（M-1 L3，POI 6 件） | **否** | 内部重读 + BASELINE「下降即红」；Crash 夹具零 panic | external_corpus.rs:63-156 |
| corpus_harness（17 件） | 否 | xlsx 敌意件三分归类 | corpus_harness.rs:1-13 |
| health_check_docx（POI 129 + docx4j 501） | 否（纯内部六维） | **catch_unwind 包裹**；4 panic 锁基线+新增即红；word365 族 err 锁基线 | mdcore-wasm/tests/health_check_docx.rs:160-208/435-458；BASELINE.json hc_docx4j{ok:477,err:20,panic:4} |
| g1/g2_lo_probe | **声称 LO 实开，实际测试体只产夹具** | `w.save_to_path("/tmp/g1/toc.docx")` 后即返回——soffice 调用不在测试内（LO 验证为仓外手动轮） | g1_lo_probe.rs:7-19；g2_lo_probe.rs:9-43 |
| g3_probe | 否 | zip/docProps 清单巡检 | g3_probe.rs:8-43 |
| docx_roundtrip_matrix lo_probe | **是**（soffice+pdftotext） | `#[ignore]` 显式手动跑 | docx_roundtrip_matrix.rs:472-513 |
| docx_fields / docx_notes C 组 / docx_table_advanced / style_corpus / numbering_corpus LO 门 | 是 | **软门**：`which soffice` 失败→SKIP 不判红 | docx_fields.rs:13-17；style_corpus.rs:324-336 |
| mang-shell docx_bid_e2e C 组 | 是 | 软门（soffice 缺→ENV_MISSING 证据行） | docx_bid_e2e.rs:34-36 |
| style_corpus / numbering_corpus 204 件真实语料 | 内部 | **当前静默空转**：语料目录已改为 calamine/docx4j/poi-docx 三子目录，`corpus_files()` 只读顶层（style_corpus.rs:40-53）→ 顶层 0 个 .docx → skip（实测 `find -maxdepth 1 -name '*.docx'|wc -l`=0） | §P1-3 |

**判定**：
1. LibreOffice 差分**存在且真实**（soffice→pdf→pdftotext 断言编辑文本在场/域重算
   证伪/页几何恒等），但**没有一个 LO 门是硬门**（全部缺件即 SKIP 或 `#[ignore]`）
   ——CI 无 LO 时 DOCX 面零外部验证。
2. **Word / WPS 完全没有真实验证——结论成立**：全仓无任何 Word/WPS 自动化调用；
   Word 侧仅「真 Word 验收登记 Lead 真机」的欠条（docx_table_advanced.rs:13-14）
   与真 Word COM 生成夹具的**内部重读**；WPS 侧只有 XML 形态分析（tr31 的四实现面
   共识里 WPS 不在列——LO 渲染/LO 导入/pandoc/python-docx）。
3. 真实差分的最强资产是 **630 件真实语料的内部六维门**（崩溃率/召回/结构/往返/
   幂等）——但它对照的 GT 是自家模型（gt_text），非外部渲染器输出；「渲染级」
   对比只有 field-code-semantics.md 的 LO 25.2 一次性人工证据链。

---

## 5. Q5 · 第三方库分工与 unwrap 分布

### 5.1 分工（Cargo.toml:31-61 + Cargo.lock）

| 库 | 承担 | 备注 |
|---|---|---|
| **docx-rs 0.4.22** | 读=模型解析（read_docx 全树）；写=builder+打包（Document/Paragraph/Run/Header/Footer/Pic/Style/Numbering/InstrText 全发射面） | default-features=false（关 image 特性）；「自有模型」只是 docx-rs 模型之上的**提取投影** |
| **quick-xml 0.41** | 自研面全部承载：rels/Content_Types 解析（docx.rs:366-908）、VML 第二趟扫描（2230-2339）、脚注/批注部件扫描（2398-2668）、编辑器段落/表格/域扫描（docx_edit.rs:1183-1587）、styles.xml/theme1.xml（style.rs）、numbering.xml（numbering.rs）、门 3 结构校验（io.rs:378-428） | GeneralRef 透传不展开（十亿笑声免疫） |
| **zip 8.6.0** | 容器三门 + raw_copy 透传 + 写面打包 | zip-rs/zip2 活跃线（ADR-O1 O1-5） |
| **serde_json** | **TableCellProperty 序列化读通道**（gridSpan/vMerge——docx.rs:3244，字段私有的 workaround） | 版本钉死风险在档 |

**自研面清单**（≈15.8K LOC）：OPC 关系层+联接、Content_Types、媒体清单、域状态机、
块提取、样式树+主题色、编号定义+序号计算器、**DocxEditor 整个手术编辑器**、
tblHeader zip 后处理、匿名化、容器门。**判断**：自研部分全部是 docx-rs 的
补面/绕面（值私有、不读 footnotes、无 sdt、无 tblHeader），没有一处是「本可
用库而自研」的浪费；但也没有一处能替换 docx-rs 的模型层。

### 5.2 616 unwrap 分布（实测拆分）

| 文件 | 总数 | 产品代码（`#[cfg(test)]` 前） | 测试代码 |
|---|---|---|---|
| docx.rs | 124 | **2**（4608 `names.last().unwrap()`——loop 内 push 后 break 保证非空；5361 `chars().next().unwrap()`——5344 is_empty 前置拒） | 122 |
| docx_edit.rs | 168 | 0 | 168 |
| style.rs | 47 | 0 | 47 |
| numbering.rs | 80 | 1（1093 `levels.last().unwrap()`——1002 空拒前置守卫；另 5 处在 `///` 文档注释里） | 74 |
| io.rs | 48 | 0 | 48 |
| write.rs/xlsx_* | 117 | 9（xlsx_edit 内部不变量） | 108 |

**结论**：616 中 ≈95% 在测试代码；**DOCX 文件解析路径（敌意文档可达）上
可达的 unwrap ≈ 0**。expect 同理（io.rs:1036-1037 非空区间不变量、
docx_edit.rs:1782/2121 调用面已证 Some、style.rs:1351 前置过滤）。
**真正的 panic 面不在自研代码，在 docx-rs 依赖层**（下 P0）。

### 5.3 最危险的具体 panic（依赖层，全部真实语料实测在档）

| # | 触发构造 | panic 签名 | 证据 |
|---|---|---|---|
| 1-3 | `runprops_word365_props-{b,enum,u}-styles.docx`（docx4j 真实件：word365 run 属性形态进 styles.xml） | `index out of bounds: the len is 0 but the index is 0`（docx-rs run_property.rs:110/155） | results/docx4j.json per_file.panic_msg；REPORT.md:186 |
| 4 | `strict_strict-sample-docx.docx`（OOXML strict 转换件） | `should read width.: ParseFloatError { kind: Invalid }`（docx-rs width.rs:18 unwrap） | 同上 |
| 5（族） | word365 非规范属性形态（ST_OnOff={TRUE,empty,space,yes}/ST_Color=case,hex,names/枚举 missing）——**OOXML 规范允许、真实世界存在** | 非 panic 但 `err:docx_parse` 拒（22/23 件失败） | REPORT.md:193；BASELINE hc_docx4j err:20 |
| 6（潜在） | 敌意畸形件 | 4 处 panic 均**先过了容器三门**（门 3 只验结构不验语义）→直达 docx-rs | read_bytes_gated:1734-1799 |

**产品可达性**：mang-shell `load/load_editable` 只 `map_err` 不 catch_unwind
（docx_doc.rs:171-172/208-209）→ **native 编辑器打开此类文档即崩溃**；
wasm 面=trap（REPORT.md E1 已自认）。POI 批 129 件零 panic、docx4j 批 501 件
抓 4 处——语料多样性的直接价值，但「锁基线」≠「已修复」。

---

## 6. Q6 · sanitize 六面与 DOCX 导入防线分层

### 6.1 sanitize 六面（crates/office，纯 std 零依赖）

六面=csv_formula/xml_control/xml_entities/html_escape/field_whitelist/url_policy
（office/src/lib.rs:13-28）。**实测：全仓（mang-shell、mdcore-wasm、workbench）
零产品调用点**——`rg 'mdc_office::'` 仅命中 office 自身与 office-io 文档注释；
workspace 无任何 crate 依赖 mdc-office（Cargo.toml 依赖边实测）。即：**消毒链
是「文档在档、责任声明在案、接线为零」的悬空资产**。DOCX 导入路径的实际防线
全部在 office-io 自身（下表）；重放/再输出时（如 docx_edit 写 w:t）由**本地
等效物**承担（emit_text_payload：5 实体转义+XML1.0 非法控制字符 typed 拒，
docx_edit.rs:1984-2042；escape_text 同）——与 xml_entities/xml_control 语义
重叠但**是独立第二实现**（双源漂移风险）。

### 6.2 DOCX 导入实际防线（分层定位）

| 威胁 | 防线层 | 证据 | 判定 |
|---|---|---|---|
| zip bomb（解压炸弹） | 门 0 输入字节上限 52MB（docx.rs:1696-1725）→ 门 1 声明值预扫（总量/单条目/条目数 10k，docx 族 50MB——io.rs:100/215-219）→ 门 2 尺寸诚实性（实际>声明即拒，io.rs:360-366/662-781） | **完整**（三道，两模式恒等） |
| XML 嵌套炸弹 | 门 3 深度上限 1000（io.rs:396-398）+ 自有面嵌套表 16（docx.rs:3219）+ VML 扫描迭代式 | **完整** |
| XXE / 外部实体 | quick-xml GeneralRef 透传不解析（io.rs:371-374 注释）；docx-rs 同为 quick-xml 系；**语料实证**：POI `ExternalEntityInText.docx` → `err:docx_parse` 正确拒绝（REPORT.md 批 3） | **完整**（拒绝型而非解析型） |
| 路径穿越（media 部件名/rels 目标） | 门 1 `enclosed_name().is_none()` 即拒（io.rs:246-251）+ 自有 `normalize_part_path` 词法归一化越根检出（docx.rs:891-908；1076-1089 图片关系、3468-3483 页眉页脚） | **完整**（且因永不解包落盘，穿越无放大面） |
| 控制字符 | 门 3 扫描登记 ControlChars 警告（两模式恒登记，io.rs:265-270） | 读侧容忍+可见性（重输出靠消费方消毒——即 §6.1 的悬空面） |
| 关系洪水/词汇表 | LIMIT_RELATIONSHIPS=10k 恒等两模式 + ECMA-376 Part 2 词汇表严格拒/兼容警告（docx.rs:286/366-501） | **完整** |
| 域指令注入（写面） | 无需（DocxWriter 域=内置枚举 FieldSpec，无自由指令串入口）；field_whitelist 六面留给未来 LLM 面 | 适龄 |

**判定**：容器/实体/穿越/炸弹四类在**引擎读路径**有完整 Typed 防线且实测扎实
（POI clusterfuzz 件全拒）；缺口是**重放/导出侧的消毒链未接线**（消费方责任
声明了但没人做）与 sanitize 双实现漂移。

---

## 7. Q7 · 与成熟 OOXML 库对比的水位

参照：ai-bid-research/office-e（Rust 生态选型：docx-rs=「唯一成熟 Rust 件」）、
office-f（渲染级读侧评估：「Rust 侧渲染级读侧必须自研」）、ADR-O1（O1-6
渲染级空位登记）。

| 维度 | docx4j / POI（JVM，语料源） | python-docx | docx-rs 全量 | 本仓自研面 |
|---|---|---|---|---|
| 模型完备度 | 全 OOXML 对象树+未知部件保留 | 段落/run/表格级 | 模型全树但**值面私有**（styles/numbering/borders 不可读出） | 扁平文本模型+专项补面（rels/styles/numbering/notes/textbox） |
| 读侧健壮性 | 工业级 | 良好（只读 w:t 面故稳） | **4 panic+word365 族 22 件拒**（本审计 P0） | 门前置+自有层零 panic；但 docx-rs 段无 catch |
| 保存保真 | 全模型重序列化（未改部分也会重写 XML） | 同左 | 同左 | **未触部件字节级透传（raw_copy）——超过三个成熟库** |
| 编辑能力 | 全模型编辑 | 段落/run 级 | builder 级（无编辑） | 窄面手术（文本/域值/批注脚注），域/修订 fail-closed |
| 渲染/分页 | 有（docx4j PDF via FOP） | 无 | 无 | 无（明确面外，ADR-O1 O1-6 空位维持） |

**水位判断**（置信：已确认）：
- **导入面** ≈ 「python-docx 加强版」：文本/表格/批注/脚注/修订/域语义提取
  超过 python-docx（其不解析 footnotes/comments 语义、不剥离 volatile 域），
  但远低于 docx4j/POI 的结构完备度（sdt/SmartArt/chart/OLE/节属性全缺）。
- **编辑面** 是**独有资产**：Rust 生态（docx-rs/umya 均无）没有字节级部件透传
  编辑器；与 docx4j「模型编辑+全量重序列化」相比，未触部件保真严格更优。
  对「标书澄清件微改」场景这是正确架构。
- **新建写面** ≈ docx-rs 的包装（能力受限于选择性暴露），低于 docx4j 模板装配。
- **必须自研**（换库无收益）：DocxEditor 手术、rels/Content_Types 层、VML
  第二趟、域状态机、style/numbering 解析（引擎值私有是硬约束）、容器门。
- **应换库/升版评估**：docx-rs 读层——4 个 panic 位点与 ST_OnOff/ST_Color
  容忍度属**上游缺陷**，修复面=升版评估/上游 issue/fork 补丁/读前置预检四选一
  （REPORT.md 建议在档但**未立项**）；渲染级读侧维持 ADR-O1 O1-6 空位裁决
  （先 JS 参照件过渡）。
- office-e 底稿结论「模型级读写开源件齐备（选型问题）、渲染级空位（自研问题）」
  与本审计实测一致；**未预见的是 docx-rs 读层的真实语料崩溃面**（2026-10-29
  批 4 才发现，晚于选型 17 天）。

---

## 8. P0-P3 分级汇总（含置信与证据链）

### P0（敌意/真实文档可达的崩溃或数据级错误）

| # | 问题 | 置信 | 证据链 | 建议修复面 |
|---|---|---|---|---|
| P0-1 | **docx-rs 读路径 4 处 panic（真实语料实锤）+ word365 属性族 22/23 解析拒**；native 消费面（mang-shell docx_doc.rs:171/208 只 map_err）无 catch_unwind → 打开即崩；wasm=trap | 已确认 | BASELINE.json hc_docx4j{panic:4}；results/docx4j.json panic_msg 逐条；REPORT.md:182-193；read_bytes_gated docx.rs:1799（门后直达引擎）；docx_doc.rs:171-172 | 读前置预检（word365 形态特征扫描→typed 拒）或 catch_unwind 包装读入口；同时立项 docx-rs 升版评估 |

### P1（核心正确性/能力声称与实现不符）

| # | 问题 | 置信 | 证据链 |
|---|---|---|---|
| P1-1 | **多节文档只提取末节 sectPr 的页眉/页脚**——中段节（`w:p/w:pPr/w:sectPr`）页眉/页脚/奇偶槽静默丢失；读模型无任何节属性承载 | 已确认（代码路径） | extract_headers_footers 只读 `docx.document.section_property`（docx.rs:3513）；引擎模型单节锚定 CAPABILITY_NOTES §挂接；模块文档「节属性面外」登记 |
| P1-2 | **w:sdt 内容结构性丢弃**：Word 惯例把 TOC 包在 sdt——真 Word 档目录**整块不可读**（宽面）；#156 只解决编辑对账盲区，未解决读面内容丢失 | 已确认 | extract_blocks `_ => {}`（docx.rs:2887-2888）；docx_edit.rs:204-221（盲区旗标自述）；docx_bid_e2e.rs:22-27（sdt 包 TOC 实测 2 段盲区） |
| P1-3 | **style_corpus/numbering_corpus 真实语料门静默空转**：语料目录 10-29 起改为三子目录（poi-docx/docx4j/calamine），`corpus_files()` 只读顶层 → 顶层 0 docx → skip；204 件风格/编号断言自 10-31 后未再真实执行 | 已确认（文件系统实测） | style_corpus.rs:40-53（非递归 read_dir）；`find test-artifacts/health-check/corpus -maxdepth 1 -name '*.docx'` = 0 |
| P1-4 | **sanitize 六面零产品接线**（含 DOCX 重放消毒）；docx_edit 的转义是独立第二实现（语义重叠、双源漂移） | 已确认 | rg `mdc_office::` 全仓仅自引用；workspace Cargo.toml 无 mdc-office 依赖边；docx_edit.rs:1984-2042 |

### P2（明显技术债/结构性限制）

| # | 问题 | 置信 | 证据链 |
|---|---|---|---|
| P2-1 | gridSpan/vMerge 经 serde_json 序列化通道读取——docx-rs 升版即**静默**丢失合并面（无编译期保护） | 已确认（代码自认） | docx.rs:3237-3244 |
| P2-2 | 读路径同一 XML 内容解压 ≥3 趟（verify_gates 全量→rels/document/notes 趟→docx_rs 内部再解压）——50MB 档约 3× 解压成本 | 已确认 | docx.rs:1742/1746-1796/1799 |
| P2-3 | LO 差分门全部软门/ignore——CI 无 LO 时 DOCX 零外部验证；g1/g2 头注释声称「LO 实开探针」而测试体不含 soffice 调用（注释与实现不符） | 已确认 | §4 表；g1_lo_probe.rs:1-19 |
| P2-4 | 引擎已有能力未被消费：NumberingTracker（Word 编号语义）零产品消费——mang-shell 用自家简化计数（「有序列表计数不重置」自认）；StyleTree::effective_format 零消费（行内样式只剩 bold/italic） | 已确认 | grep NumberingTracker/read_docx_styles 仅 office-io 测试；docx_doc.rs:24-27 |
| P2-5 | 读模型格式语义缺口族：rStyle 不入 RunData、bold/italic presence 语义（`w:val="false"` 误报）、Chars 缩进变体、单元格内超链接目标、MoveFrom/MoveTo | 已确认（登记在档） | docx.rs:1426-1436/3170-3171/3013-3022/3186-3190/3104-3107 |
| P2-6 | 写面无批注/文本框/修订/嵌套表/内部链接/多节发射面（读有的写没有——读写不对称的另一半） | 已确认 | §2 矩阵 🔴 项 |

### P3（建议）

- P3-1 g1/g2/g3 探针或删或补 soffice 断言（防止「注释声称>实现」继续漂移）。
- P3-2 50MB 输入上限对超大标书（含高清扫描图）可能误拒——考虑媒体部件
  单独额度或消费方配置（r4-t2 设计裁决点，登记即可）。
- P3-3 numbering 写面降级（100/101 抽象）已有 format 诊断字段——消费方比对
  面未建。
- P3-4 LO 软门至少一处转硬门（CI 装 LO 的 nightly job），否则差分矩阵名存实亡。
- P3-5 `read_docx` 路径版全量入内存（≤52MB 上界内）——大档峰值内存=2×文件，
  已做 4× 可编辑门限降级（docx_doc.rs:193-205），可接受但登记。

---

## 9. 结论

1. **架构方向正确且执行诚实**：三条路径分工清晰（读=提取、写=生成、编=手术），
   每个缺口都有登记而非静默；「未触部件字节级透传」是超过 docx4j/python-docx
   的独有能力，测试锚（13 面矩阵+18 真实档+桥三性质）扎实。
2. **读面是「文本提取器」不是「文档模型」**：格式/结构/节/sdt 语义缺失决定了
   「读-改-重建」路径不可用于保真场景——这一点仓库自己定谳（FIDELITY/fidelity-matrix
   「组合语义=修改非重建」），与 xlsx 侧结论对偶成立。
3. **当前最大风险不在自研代码而在依赖与门禁**：docx-rs 4 panic+word365 族
   （P0）、语料门空转（P1-3）、LO 软门（P2-3）——三件都已在仓内文档/基线中
   「可见」，但修复均未立项。
4. **投入判断**：DOCX 自研面（手术编辑器+补面层）值得继续——它在 Rust 生态
   无替代品且质量纪律罕见；但应把「继续」定义为**补 P0/P1 + 接线既有资产**
   （catch_unwind 读入口、sdt 读面、多节 sectPr、语料门修复、sanitize 接线），
   而非扩写面——扩写（SmartArt/chart/OLE/渲染）按 ADR-O1 O1-6 裁决仍应外置。
