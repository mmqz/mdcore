# Acrobat Reader 功能对照清单（parity matrix）

> issue #229（R5·P1 验收产物）。口径：**Acrobat Reader（免费阅读器）常用
> 功能面 × mdcore 现状**，每项附证据锚点（代码 `文件:行` / 测试函数名——
> 不凭印象）。高频面（打开/渲染/表单/签名/标注/打印）另有自动化验收：
> `crates/pdf/tests/pdfm_acrobat_parity.rs`（7 测试——打印面 #240 起入册，
> 引擎原语面自动化）。
>
> 基线：2026-11-06 @ `main` 4ac0788。范围声明：mdcore = mdc-pdf 引擎 +
> mang-shell 壳 + web 面（pdf.js 数据面）；Acrobat 侧基线 = Reader 常用
> 行为（不含 Pro 专有编辑功能）。

## 总览（12 面）

| # | 面 | Reader 基线 | mdcore 现状 | 证据锚点 | 分级 |
|---|---|---|---|---|---|
| 1 | 打开 | 开 PDF/页数/文档属性 | **对齐**（双引擎装载 + fail-fast 校验） | `document.rs:132` `PdfSession::open`、`engine_pdfium.rs:98`、`writer_lopdf.rs:83`；测试 `parity_open_all_fixtures_*`、`pdfb_skeleton::open_validates…` | — |
| 2 | 渲染 | 位图渲染/缩放 | **对齐**（引擎面；壳侧无位图渲染——文本行模型） | `render.rs:42` `Rasterizer::rasterize`、`engine_pdfium.rs:324`（8192px/64Mpx 钳制）；测试 `parity_render_first_page_bitmap`、`pdfc_engine` 锁 8-10 | 长尾③ |
| 3 | 书签/大纲 | 大纲树导航 | **对齐**（读面） | `outline.rs`、`engine_pdfium.rs:260`；测试 `pdfc_engine::outline_positive_tree…` | 长尾④ |
| 4 | 链接 | 点击跳转/外链 | **部分**（读全；无导航路由、无创建） | `links.rs:70` `read_links`（/Dest/命名树/GoTo/URI）；测试 `pdff3_render_ext::links_*`（17 件语料 661 条） | 长尾① |
| 5 | 表单填写 | 填值/保存/扁平化 | **对齐**（真增量修订 + CJK 外观） | `forms.rs:438/914/1178/1473`；测试 `parity_form_fill_save_reopen`、`pdfg_forms`(11)、`pdfj_forms_adv`(14)、`pdfl_forms_incremental`(7) | — |
| 6 | 签名 | 验签/签署/修订完整性 | **部分**（读 + 结构验证 + 已签名保护；不验签不签署） | `signature.rs:589` `read_signatures`、`forms.rs:1761` `has_signed_signature_fields`；测试 `parity_signature_read_and_signed_probe`、`pdfi_signature`(7) | 长尾⑥ |
| 7 | 盖章 | 图章注释/公章 | **对齐**（注释 Stamp + 内容流图片盖章双路径） | `annotation.rs:198`、`stamp.rs:723` `stamp_image`（PNG/JPEG/透明度）；测试 `pdfh_stamp`(8)、`pdff2_annotate` Stamp 往返 | 长尾⑧ |
| 8 | 标注 | 高亮/批注/注释管理 | **对齐**（11 kind 读/写/删 + 渲染可见 + Acrobat 互通公共面） | `annotate.rs:514` `PdfAnnotator`、`annotation.rs:153`；测试 `parity_annotation_add_persist_remove`、`pdfd_writer`(16)、`pdff2_annotate`(11) | 长尾⑨ |
| 9 | 打印 | 页面设置/缩放/注释打印开关 | **部分**（#240：引擎原语 + 壳系统打印流 + web 打印专用视图；真机打印验收待登记） | 引擎 `print.rs`（DPI/页区间/注释开关）+ `render.rs:55` `rasterize_with_options`；壳 `mang-shell/src/print.rs`（Windows 打印动词/macOS open -p/CUPS lp）+ 菜单「打印…」；web `pdf_view.ts` `printDocument`（150dpi 同源渲染→系统对话框）；测试 `parity_print_render_dpi_range_annotations` | 长尾①（真机验收/设置面板） |
| 10 | 搜索 | 全文搜索/高亮定位 | **对齐**（无 rect 几何命中） | `command.rs:98` `Command::Search`、`document.rs:396`（限额/折叠/摘录）；web `pdf_view.ts`（计数+几何高亮）；测试 `pdfb_skeleton::search_*`(4) | 长尾② |
| 11 | OCR | Reader 无此功能 | **对齐（超 Reader）**（tesseract 外进程，中英混合） | `ocr.rs:107` `ocr_page`（200DPI→PGM→tesseract）；测试 `pdff4_ocr`(5，环境门控) | 长尾⑩ |
| 12 | 文档安全 | 权限位/口令/证书 | **部分**（打开扫描八族 + 限额 + 加密写拒；无解密/权限位模型） | `security.rs`、`security_scan.rs:41` `open_scan`；测试 `pdfe_security`(12)、`pdfb_skeleton::encrypted_assessment_denies_writes…` | 长尾⑤⑦ |

> 状态口径：**对齐** = 用户旅程闭环可自动化验收；**部分** = 数据面/读面在
> 而关键交互面缺；**缺失** = 0%。

## 高频面自动化验收（issue 任务 3）

`crates/pdf/tests/pdfm_acrobat_parity.rs`（双特性 `engine-lopdf,engine-pdfium`）：

| 高频面 | 测试 | 旅程断言 |
|---|---|---|
| 打开 | `parity_open_all_fixtures_pdfium_loads_and_counts` / `parity_open_all_fixtures_lopdf_writer_parses` | 7 夹具（普通/书签/表单×3/签名/扫描）双路装载 + 页数锚 |
| 渲染 | `parity_render_first_page_bitmap` | 300→388 等比 + w*h*4 合同 + 非全零 + 白底主导 |
| 表单 | `parity_form_fill_save_reopen` | 填中文值 → 增量保存（前缀逐位）→ 重开可见 |
| 签名 | `parity_signature_read_and_signed_probe` | 读签名（结构/签名人）+ 已签名探针阴阳双态 |
| 标注 | `parity_annotation_add_persist_remove` | 增→存（增量）→重开可见→删→重开零残留 |
| 打印 | `parity_print_render_dpi_range_annotations` | 150dpi 换算锚 + 页区间闭区间 + 注释开关红墨迹翻转（引擎原语面） |

## 缺口清单（分级入册——不静默）

### P1（缺陷级，已开 issue 跟进）

| 缺口 | issue | 说明 |
|---|---|---|
| ① 打印管线 0% → 已交付最小闭环（#240，2026-11-06） | #240 | 引擎：`print.rs` 打印渲染原语（DPI 驱动 + 页区间 + 注释开关）；壳：方案 C 系统打印流（GPUI 无打印 API——Windows 打印动词 / macOS open -p / CUPS lp 参数映射）；web：打印专用视图（150dpi 同源渲染 → 系统对话框）。遗留：真机打印人工验收、壳面设置面板（页区间/份数 UI）、打印预览（当前 = 屏显即预览） |

### 长尾登记（按性价比排序，暂不开单——排期总纲滚动评估）

| # | 缺口 | 级 | 说明与锚点 |
|---|---|---|---|
| ① | 链接点击导航未接宿主 | P2 | 数据面就绪（`links.rs` 读全 /Dest/命名树/URI），壳/web 未接 click→page 路由与外链策略——接线即可闭环 |
| ② | 搜索无 rect 几何命中 | P2 | `SearchHit` 仅页+摘录（`command.rs:50` 注明扩展点）；pdfium SearchEx 未封装——UI 定位高亮需几何 |
| ③ | 壳侧无位图渲染 | P2 | mang-shell `pdf_doc.rs` 走文本行模型；web 面有完整 canvas 渲染（`pdf_view.ts` LRU 64MB）——壳侧渲染待 #68 阅读面 |
| ④ | 书签编辑/命名目标 | P3 | 仅页目标读面（`outline.rs`）；web `buildOutlineJson` 恒空树（`pdf_view.ts:192` 登记） |
| ⑤ | 加密档解密/权限位模型 | P3 | 曾解密档只读放行写拒（S5 策略）；无口令输入 UI、无 /Encrypt P 位遵从 |
| ⑥ | 验签/签署 | P3 | 结构验签在（`signature.rs`）；密码学校验（链/时间戳/摘要复算）与签署域不在 v1——pdfi_signature 头注如实登记 |
| ⑦ | /Info/XMP 元数据 | P3 | 仅 id/文件名/页数；Acrobat 有完整属性面板 |
| ⑧ | /Name 标准图章图形/动态图章/骑缝章 | P4 | `/Approved` 等标准图形映射不做（`docs/pdf-annotate.md` 边界）；公章内容流盖章已在 |
| ⑨ | 注释透明度 /Popup/回复流/列表面板 | P4 | 11 kind 公共面在；Acrobat 的注释管理（排序/筛选/导出）不在 |
| ⑩ | OCR 识别结果写回可搜索 PDF | P4 | 当前只出文本（`ocr.rs`）；无扫描页自动检测 |

## 维护纪律

- 本清单为 **#229 验收基线**：新增 PDF 能力或缺口修复时同步更新对应行
  （证据锚点必须可点开复核——`文件:行` 或测试函数名）；
- 长尾项滚动评估：排期总纲（#230）每轮按 ①-⑩ 顺序重估性价比，升级为
  issue 时在本表回填编号并从长尾表移除；
- 高频面测试与清单同步：`pdfm_acrobat_parity.rs` 的新增/变更测试必须在
  §高频面表登记。
