# PROVENANCE —— #264 真实 PDF 语料来源登记

> 逐件来源/许可/测试点。机器可读版 = `PROVENANCE.json`（含 sha256/bytes/pages）。
> 重建：`python3 test-artifacts/pdf-corpus/fetch_corpus.py`（幂等；已存在跳过）。

## 许可证

| 来源仓 | 许可证 | 兼容性 |
|---|---|---|
| mozilla/pdf.js | Apache-2.0 | 与 GPL-3.0 主仓分发兼容（Apache-2.0 允许再分发） |
| apache/pdfbox | Apache-2.0 | 同上 |

## 分类目录（件数）

- `annotations/`：13 件
- `bookmarks/`：4 件
- `broken/`：12 件
- `captions/`：2 件
- `encrypted/`：11 件
- `forms/`：11 件
- `mixed/`：17 件
- `multicolumn/`：3 件
- `scanned/`：3 件
- `tables/`：2 件
- **合计 78 件 / 28.3 MB**

## 逐件登记

| 分类 | 文件 | 来源仓 | 大小 | 页数 | 测试点 |
|---|---|---|---|---|---|
| annotations | `annotation-border-styles.pdf` | mozilla/pdf.js | 87KB | 1 | 边框样式族 |
| annotations | `annotation-caret-ink.pdf` | mozilla/pdf.js | 91KB | 1 | 插入符 + 墨迹 |
| annotations | `annotation-fileattachment.pdf` | mozilla/pdf.js | 91KB | 1 | 文件附件注释 |
| annotations | `annotation-freetext.pdf` | mozilla/pdf.js | 84KB | 1 | 自由文本 |
| annotations | `annotation-highlight.pdf` | mozilla/pdf.js | 90KB | 1 | 高亮 |
| annotations | `annotation-line.pdf` | mozilla/pdf.js | 82KB | 1 | 线条 |
| annotations | `annotation-link-text-popup.pdf` | mozilla/pdf.js | 96KB | 1 | 链接 + 弹窗 |
| annotations | `annotation-polyline-polygon.pdf` | mozilla/pdf.js | 86KB | 1 | 折线/多边形 |
| annotations | `annotation-square-circle.pdf` | mozilla/pdf.js | 86KB | 1 | 矩形/椭圆 |
| annotations | `annotation-squiggly.pdf` | mozilla/pdf.js | 91KB | 1 | 波浪线 |
| annotations | `annotation-stamp.pdf` | mozilla/pdf.js | 86KB | 1 | 图章 |
| annotations | `annotation-strikeout.pdf` | mozilla/pdf.js | 89KB | 1 | 删除线 |
| annotations | `annotation-underline.pdf` | mozilla/pdf.js | 89KB | 1 | 下划线 |
| bookmarks | `nested_outline.pdf` | mozilla/pdf.js | 5KB | 9 | 嵌套大纲 |
| bookmarks | `outline_goto_action.pdf` | mozilla/pdf.js | 1KB | 2 | 大纲 + GoTo 动作 |
| bookmarks | `outlines_for_editor.pdf` | mozilla/pdf.js | 2KB | 5 | 编辑器大纲 |
| bookmarks | `outlines_se.pdf` | mozilla/pdf.js | 29KB | 2 | 瑞典语大纲（非 ASCII 标题） |
| broken | `annotation-line-without-appearance-empty-Rect.pdf` | mozilla/pdf.js | 0KB | 1 | 空 Rect 注释 |
| broken | `bad-PageLabels.pdf` | mozilla/pdf.js | 0KB | 1 | 坏 PageLabels 数组 |
| broken | `boundingBox_invalid.pdf` | mozilla/pdf.js | 1KB | 3 | 无效 boundingBox |
| broken | `devicen-none-zero-domain.pdf` | mozilla/pdf.js | 0KB | 1 | DeviceN 零域 |
| broken | `helloworld-bad.pdf` | mozilla/pdf.js | 0KB | 1 | 坏 HelloWorld |
| broken | `issue10438_reduced.pdf` | mozilla/pdf.js | 0KB | 1 | 结构异常裁剪件 |
| broken | `issue10542_reduced.pdf` | mozilla/pdf.js | 0KB | 1 | 结构异常裁剪件 |
| broken | `issue1055r.pdf` | mozilla/pdf.js | 0KB | 1 | 结构异常件 |
| broken | `issue11124.pdf` | mozilla/pdf.js | 0KB | 1 | 零页结构异常 |
| broken | `multiple-filters-length-zero.pdf` | mozilla/pdf.js | 17KB | 1 | 多过滤器零长度流 |
| broken | `zero_descent.pdf` | mozilla/pdf.js | 64KB | 1 | 零下降字体度量 |
| broken | `zerowidthline.pdf` | mozilla/pdf.js | 6KB | 1 | 零宽线 |
| captions | `issue10640.pdf` | mozilla/pdf.js | 49KB | 1 | Open Sans LATEX 文档（figure 引用排版） |
| captions | `issue11045.pdf` | mozilla/pdf.js | 39KB | 1 | 机翼图 Figure 1 图注（X_NP/X_CG 标注） |
| encrypted | `AES128ExposedMeta.pdf` | apache/pdfbox | 18KB | ? | AES-128 加密、元数据可读 |
| encrypted | `AES256ExposedMeta.pdf` | apache/pdfbox | 26KB | ? | AES-256 加密、元数据可读 |
| encrypted | `AESkeylength128.pdf` | apache/pdfbox | 11KB | ? | AES-128 密钥长度探针 |
| encrypted | `AESkeylength256.pdf` | apache/pdfbox | 10KB | ? | AES-256 密钥长度探针 |
| encrypted | `Acroform-PDFBOX-2333.pdf` | apache/pdfbox | 959KB | 1 | 加密 + AcroForm 混合形态 |
| encrypted | `PasswordSample-128bit.pdf` | apache/pdfbox | 8KB | ? | 128bit 密码加密样张 |
| encrypted | `PasswordSample-256bit.pdf` | apache/pdfbox | 7KB | ? | 256bit 密码加密样张 |
| encrypted | `PasswordSample-40bit.pdf` | apache/pdfbox | 8KB | ? | RC4-40bit 密码加密（老式） |
| encrypted | `encrypted-attachment.pdf` | mozilla/pdf.js | 2KB | 1 | 加密容器内嵌附件 |
| encrypted | `preEnc_20141025_105451.pdf` | apache/pdfbox | 40KB | 1 | 真实预加密件（扫描产物） |
| encrypted | `test.pdf` | apache/pdfbox | 12KB | 1 | 加密基线件 |
| forms | `acroform_calculation_order.pdf` | mozilla/pdf.js | 1KB | 1 | 字段计算顺序 CO 数组 |
| forms | `annotation-button-widget.pdf` | mozilla/pdf.js | 23KB | 1 | 按钮控件 AcroForm |
| forms | `annotation-choice-widget.pdf` | mozilla/pdf.js | 89KB | 1 | 选择控件 AcroForm |
| forms | `annotation-text-widget.pdf` | mozilla/pdf.js | 90KB | 1 | 文本控件 AcroForm |
| forms | `annotation-tx.pdf` | mozilla/pdf.js | 11KB | 1 | 文本字段 appearance |
| forms | `annotation-tx2.pdf` | mozilla/pdf.js | 20KB | 1 | 文本字段 appearance 变体 |
| forms | `checkbox-bad-appearance.pdf` | mozilla/pdf.js | 19KB | 1 | 坏 appearance 流的复选框 |
| forms | `file_pdfjs_form.pdf` | mozilla/pdf.js | 8KB | 1 | pdf.js 自产基础表单 |
| forms | `form_two_pages.pdf` | mozilla/pdf.js | 22KB | 2 | 跨两页表单字段 |
| forms | `xfa_filled_imm1344e.pdf` | mozilla/pdf.js | 2953KB | 1 | 真实加拿大移民表单（已填写 XFA，2.9MB） |
| forms | `xfa_issue14315.pdf` | mozilla/pdf.js | 11KB | 1 | XFA 动态表单 |
| mixed | `4PP-Highlighting.pdf` | apache/pdfbox | 2KB | 1 | 双栏高亮渲染 4 页件 |
| mixed | `PDFJS-9279-reduced.pdf` | mozilla/pdf.js | 3017KB | 1 | 裁剪后的真实件（3MB） |
| mixed | `comments.pdf` | mozilla/pdf.js | 1040KB | 14 | tracemonkey 论文注释版（真实评论导出，1MB） |
| mixed | `highlights.pdf` | mozilla/pdf.js | 946KB | 14 | 真实高亮集（946KB） |
| mixed | `issue10084_reduced.pdf` | mozilla/pdf.js | 14KB | 1 | 裁剪后的真实件 |
| mixed | `issue10301.pdf` | mozilla/pdf.js | 30KB | 1 | 希伯来文 RTL 文本件 |
| mixed | `issue10402.pdf` | mozilla/pdf.js | 20KB | 1 | 用户件（裁剪字体形） |
| mixed | `issue1045.pdf` | mozilla/pdf.js | 22KB | 1 | 用户件（裁剪字体形） |
| mixed | `issue10529.pdf` | mozilla/pdf.js | 117KB | 1 | 隐藏文本编码件（真实用户场景） |
| mixed | `issue11144_reduced.pdf` | mozilla/pdf.js | 95KB | 1 | 裁剪后的真实件 |
| mixed | `issue11150_reduced.pdf` | mozilla/pdf.js | 31KB | 1 | 裁剪后的真实件 |
| mixed | `issue19971.pdf` | mozilla/pdf.js | 1688KB | 6 | 用户 bug 附件（1.7MB） |
| mixed | `issue5567.pdf` | mozilla/pdf.js | 2118KB | 1 | 用户 bug 附件（2.1MB 真实文档） |
| mixed | `issue6286.pdf` | mozilla/pdf.js | 1864KB | 1 | 用户 bug 附件（1.9MB） |
| mixed | `issue6961.pdf` | mozilla/pdf.js | 2106KB | 2 | 用户 bug 附件（2.1MB） |
| mixed | `issue8795_reduced.pdf` | mozilla/pdf.js | 4145KB | 1 | 裁剪后的真实件（4.1MB） |
| mixed | `tiger-as-form-xobject.pdf` | apache/pdfbox | 49KB | 1 | Form XObject 嵌套绘制 |
| multicolumn | `freeculture.pdf` | mozilla/pdf.js | 2537KB | 352 | 《Free Culture》整书（Lawrence Lessig，书籍排版，2.5MB） |
| multicolumn | `survey.pdf` | apache/pdfbox | 124KB | 9 | 学术调查问卷（双栏 + 表格） |
| multicolumn | `tracemonkey_with_editable_annotations.pdf` | mozilla/pdf.js | 1184KB | 14 | 学术论文双栏 + 可编辑注释（pdf.js 渲染基准件，1.2MB） |
| scanned | `perf_images_scan_bilevel.pdf` | mozilla/pdf.js | 121KB | 1 | 双级扫描图像 |
| scanned | `perf_images_scan_gray.pdf` | mozilla/pdf.js | 1695KB | 1 | 灰度扫描图像（1.7MB） |
| scanned | `scan-bad.pdf` | mozilla/pdf.js | 29KB | 1 | 坏扫描件 |
| tables | `issue10900.pdf` | mozilla/pdf.js | 33KB | 1 | 四列数值对齐表格（真实数据） |
| tables | `source.pdf` | apache/pdfbox | 226KB | 1 | 捷克语报纸表格布局（mBank 框架表格） |

（生成时间 2026-11-06T10:53:32；URL 模式见 fetch_corpus.py MANIFEST——raw.githubusercontent.com 直链。）
