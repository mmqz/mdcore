# 引擎仓（mdcore）增量复核 T0 —— 三阶段审计销账 + R7 跨仓漂移

- 复核执行：Agent-qkm4（两仓审计职责——标书仓已完成 T0/T0b，本轮转引擎仓）
- 日期：2026-11-06（UTC+8）
- 复核对象：mmqz/mdcore main HEAD `c4baff927fec788f0a1a5b23cad0525fc21694db`（2026-11-06 17:46 +0800）
- 核验方式：git grep / rg 提交对象级与工作区只读核验；未改任何源码/测试/配置；token 仅用于克隆/查询/推送，不入交付物
- 输入登记册：仓内归档三阶段审计（`audit/final-report{,-phase2,-phase3}.md`，基线 96596d7→6322f86）+ 团队 R 轮次 issue 登记册（【R0·P0】~【R11·P2】）

---

## 一、P0 销账（5/5 全部已修复，本轮逐一验证）

| P0 | 修复载体 | HEAD 证据锚点 | 验证 |
|---|---|---|---|
| P0-1 块身份撞号 | #244（R0）+ #220/#221 | `crates/mdcore/src/tree.rs:84`（register 撞号守卫）+ `snapshot.rs:58-207`（重复 ID 拒收/validate）+ History 格式版本闸门 | ✅ |
| P0-2 docx-rs panic | #226（R2） | `mang-shell/src/editor/docx_doc.rs:174`（catch_unwind→typed 错误）+ `crates/office-io/src/docx_panic_guard.rs`（预检防御+度量归一化） | ✅ |
| P0-3 forms 假增量 | #227（R3） | `crates/pdf/src/forms.rs:30-40/1163`（真增量修订 spec 7.5.6 + 前缀逐位不变 + flatten 拒签 + 签名在场探针） | ✅ |
| P0-4 Enter 文本损坏 | #225（R1） | `web/src/main.ts:876-883`（快路径 textChars 增量维护） | ✅ |
| P0-5 TSV 公式注入 | #225（R1） | `web/src/xlsx_grid.ts`（onCopy 出口接 guard_csv_cell） | ✅ |

## 二、P1/P2 销账矩阵（按 HEAD 存活状态）

### 2.1 已修复/已关闭（有载体）

| 项 | 载体 |
|---|---|
| P1-27（ADR-B5 不实声称） | #220 勘误（be545df）+ #221 版本闸门 |
| P1-10（DOCX 多节 sectPr 丢失） | #257（R6）中段节 sectPr 全量承载（sdt 读面是否一并收口，建议按 gates/257.md 验收对照复核） |
| P2-37（NSIS 三断链） | #228（版本注入机制 + installer CI 工作流） |
| P2-24（keymap chord×chord 尾键漏检） | `keymap.rs:290-299` conflicts() 双键比较已入（chord×单键仍仅比首键——注释自述为保守策略，残留面待 dispatch 语义复核） |
| P1-14（CI 门禁，部分） | #216 fmt 复位 + #210/#223 pdfium 真环境 CI + #228 installer；**在途**：#262 语料门（PR #273）、#264 PDF 语料（PR #274） |
| P1-15（合规，部分） | A3 行修复复位（#228 docs）+ #247 许可证路线图（已关）；SBOM/NOTICES 重建仍欠 |
| P1-4（GPL） | 用户裁定为设计决策（1.0 阶段替换）→ #247 路线图落地 |
| P1-3（编辑器双轨） | 用户裁定为过渡方案（1.0 收敛）——非缺陷 |
| P1-18（wasm 时钟） | phase2 C-1 降级 P3（产品面影响被实测推翻） |

### 2.2 存活且已有 issue 跟踪

| 项 | issue |
|---|---|
| 语料门静默空转（第 19 条假绿） | #262（PR #273 在途） |
| PDF 真实语料零存量 | #264（PR #274 在途）；XLSX 语料 #265 |
| 打印管线缺失 | #240 |
| LaTeX 数学承载 | #261（PR #263 在途） |
| 多级列表编号深化 | #266；DOCX 样式树 #267 / 分栏 #268 / 主题色 #269 |
| 桌面平台验收 | #270；PDF 文字编辑债 #271 |

### 2.3 存活且**零跟踪**（本轮验证锚点 + 新建 issue）

| 项 | HEAD 锚点 | 新 issue |
|---|---|---|
| P1-9/22/23/24：RPC/MCP 沙箱逃逸双链 + SDK README 误导 → **#275** | `mang-shell/src/rpc.rs:959-965`（cmd_open_file `std::path::absolute` 直通 open_document_tab，无 resolve_sandboxed；#134 沙箱只覆盖 FS 命令族）；`sdk/mcp/README.md:93`（仅列 4 工具沙箱，无组合逃逸警示） | ✅ 新建 |
| P1-7（残余五面）：消毒六面 + url_policy 零产品接线 → **#276** | `url_policy` 仅存在于 `crates/office/src/sanitize/`（自测试消费）；TSV 面 #225 已接；docx `javascript:` 超链接→GFM 直转等五面仍未接线 | ✅ 新建 |
| P1-11：stamp.rs 写面安全门缺失 → **#277** | `crates/pdf/src/stamp.rs` 零 `is_encrypted/S5/S3` 命中——对加密档盖章静默损坏；#227 只修了 forms 面 | ✅ 新建 |
| P1-8：markdown 深度门缺失 → **#278** | `crates/mdcore/src/markdown.rs` 无深度上限（`html.rs:256` 有 256 门——同仓双标）；深嵌套敌意 md → blocks_json 等递归消费者栈溢出 | ✅ 新建 |
| P1-12：pdf worker 换代线程无界累积 → **#279** | `crates/pdf/src/worker.rs:363`（旧代 join 弃置=detach；永挂 C 调用线程不退出）+ 每代字节快照重开 | ✅ 新建 |
| P1-19 + P1-25：wasm 桥双税 → **#280** | `crates/mdcore/src/lib.rs:320`（每事务整树 clone）+ `web/src/main.ts:2647`（word_count_json 每 500ms 无 revision 守卫） | ✅ 新建 |
| P1-17/P2-31：#164 残项群 → **#281** | `mang-shell/src/status_bar.rs:79`（每帧 `e.file_path().map(std::fs::read)` 全文件磁盘读——编码/换行探测）等 9 项残项 | ✅ 新建 |
| P2-25 + P1-21 + P2-29：web/sdk 边界三小件 → **#282** | `web/src/pdf_view.ts:1828`（svg.innerHTML 属性插值——潜伏注入）；`web/build.mjs` 无 pkg/snippets 拷贝（ironcalc_base 仍在 Cargo.lock 依赖图——需一轮标准构建复核）；`sdk/python/mang_rpc/client.py:393`（int(req_id) 无 ValueError 守卫，except 仅 OSError） | ✅ 新建 |
| P1-5/6：XLSX 消费倒挂 + 无重算 → **#283** | `git grep XlsxEditor HEAD` 仅命中 `crates/office-io/src/xlsx_edit.rs` 自身——零产品消费（mang-shell XLSX 视图纯只读）；产品路径有损 rebuild；P1-6 无重算面本轮未深查（phase1 登记维持） | ✅ 新建 |

### 2.4 开放但**不新建 issue**（理由在档）

| 项 | 理由 |
|---|---|
| P1-2（anchor 偏移映射） | `anchor.rs:11` 代码内诚实登记「属 R9 增强，v0 以 expect 保护」——已知限制在档，属能力路线图非缺陷登记 |
| P1-16（canvas 光栅无字形） | `render.rs:2234` 诚实登记「要真字形请走 SVG」——#161 能力矩阵域 |
| P1-13（collab 平行宇宙） | 与用户裁定「双轨过渡」同族——1.0 收敛决策域 |
| P1-20（vendored Zed 无上游跟踪） | #247 许可证路线图关联（1.0 替换窗口一并处置） |
| P1-1（marks 双轨） | op.rs 已见 adjust_marks 代数（:144-201）；「结构 op 全链 marks」是否收口本轮未深查——标待复核，不轻率登记 |
| P1-26（研究仓 Adobe/Typora 合规暴露） | 属 mang-research-archive 仓治理（T-R 路线），非 mdcore 本仓范围 |
| P2-27（docx_export stale 断言） | 未复核（web 测试面，随 #262 语料门批次一并看） |

## 三、R7 跨仓一致性：锁点 f4328eb → HEAD c4baff9（167 提交）对 bid 的价值

bid-engine 锁定三件于 `f4328eb`（2026-11-03 时代）；其后引擎侧 167 提交含多项**bid 消费面直接相关**的修复/能力，全部未被 bid 消费：

| 引擎演进 | 载体 | 对 bid（ai-bid-research）的相关性 |
|---|---|---|
| docx-rs 读层 panic 预检防御 + 度量归一化 | #226 | **高**——bid 的 docx 解析健壮性直接受益（P0-2 同族风险在 bid 侧同样存在） |
| zip-exotica 防线（EOCD 守恒门 + 压缩比观察门） | #233 | **高**——bid 处理外部敌意招标文件（zip 容器）的防线 |
| .doc OLE2 魔数识别 + 转换指引 | #234 | **高**——bid #270（.doc 导入两层设计）的引擎侧对应能力已就位 |
| 多节 sectPr 全量承载（中段节页眉页脚） | #257 | **中**——bid 的 docx 读保真（多节招标文件） |
| mdc-pdf 表格结构化提取（单元格+列序+包围盒） | #232 | **高（含语义重复风险）**——bid #266 在 bid-parse 自研了 PDF 布局面语义表（商务要求 0→13）；引擎侧现已有原生表格提取——**双实现/语义漂移风险，建议 Lead 裁定对账统一**（单一真源原则） |
| 块身份/undo/History 版本闸门 | #244/#220/#221 | 低——bid 仅消费 pagination/word_count，不触块核 |
| forms 真增量写 | #227 | 低——bid 不经引擎写 PDF |
| web 桥双 P0 | #225 | 无——bid 不消费 web 桥 |

**建议（供 Lead 裁定，不擅动）**：
1. 锁点升级窗口：独立 PR `chore(dep): bump mdcore to c4baff9`（或等 #292/#300 合入后的稳定点），按 bid-engine Cargo.toml 自身纪律「升级 rev 走独立 PR 并重跑基线差分」执行；
2. #232（引擎表格提取）与 bid #266（自研布局面语义表）做一次语义对账：抽取口径、列序、合并格语义是否一致，裁定单一真源归属（引擎侧一次做对 vs bid 侧专路）；
3. R-23（MDCORE_REV 常量错报，bid 仓 #327）在升级时一并消解（常量从 Cargo.toml 单一真源生成）。

## 四、复核边界（如实登记）

- 本轮为只读静态核验 + issue/commit 对账；**未跑测试**（引擎全量测试在归档审计中已三轮实证 1,241/1,311 全绿，本轮未重复）；
- P1-1（marks 全链）、P1-6（XLSX 重算）、P2-27（web stale 断言）、P1-10 之 sdt 面未深查——已在矩阵标注；
- 新建 issue 共 9 个：#275–#283（§2.3，链接见 GitHub），全部为「存活 + 零跟踪」项；已有跟踪项不重复登记；
- 三条用户裁定（GPL 故意保留 / 双轨过渡 / Overlay 只做 PDF）不作为问题登记。
