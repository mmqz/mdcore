# 笔记 21：web e2e 全量实跑 + 关键 P0 实机复现（2026-11-05，HEAD=6322f86）

> 本轮为深化审计的实证轮。此前审计（笔记 16）只实跑了 Rust 引擎面；web 侧 24 个 e2e 套件
> 零 CI、results.json gitignore，从未有独立第三方完整运行记录。本轮从源码构建完整产品
> （wasm×2 + dist）并在真实 Chromium 中全量运行，同时实机复现/推翻两项上轮结论。
>
> 环境：Linux 沙箱 · rustc 1.99.0（按 rust-toolchain.toml 钉住版本）· wasm-bindgen-cli 0.2.129
> （与 Cargo.lock 精确一致）· Node 24 · Playwright Chromium · Python 3.12。
> 纪律：全部构建产物落在 gitignore 目录（target/、web/pkg/、web/pkg_pdf/、web/dist/），
> 仓库 `git status` 全程干净；唯一「手工干预」是对 gitignore 构建产物 dist/pkg/snippets/
> 的补拷（见 N-1 根因实验，改动不触碰任何被跟踪文件）。

---

## 1. 构建链（全部从 HEAD 源码构建成功）

| 步骤 | 命令 | 结果 |
|---|---|---|
| wasm 主核 | `cargo build -p mdcore-wasm --target wasm32-unknown-unknown --release` | ✅ 2m41s，产物 9.4MB |
| JS 胶水 | `wasm-bindgen --target web --out-dir web/pkg`（0.2.129） | ✅ 生成 pkg/mdcore_wasm.{js,d.ts} + **pkg/snippets/ironcalc_base-2500202885b2b58f/inline0.js** |
| pdf wasm | `bash web/build-pdf-wasm.sh`（mdc-pdf-wasm crate） | ✅ 58s，pkg_pdf/ |
| 前端 | `npm install && node build.mjs` | ✅ dist/（main.js 2.0MB + wasm 8.6MB + pkg/ + pkg_pdf/） |

构建注记：`cargo build` 报 mdcore-wasm 5 条 warning（非阻断，与 C2 笔记的死导出/弃用面一致）。

## 2. N-1【新发现 P1·已确认+本地复现】build.mjs 不拷 pkg/snippets/ → 生产构建里 Canvas 视图死亡

**症状**：标准构建（`npm run build:full` 等价链）产物中，页面加载后 `window.__canvasReady` 永不置位；
canvas 面板显示 `canvas panel boot failed: TypeError: Failed to fetch dynamically imported module`；
控制台 404：`/pkg/snippets/ironcalc_base-2500202885b2b58f/inline0.js`。

**根因链**：
1. `mdcore-wasm` 依赖 office-io → formula → **ironcalc_base 0.9.1**（Cargo.lock:1511-1512），
   其 wasm-bindgen inline JS 使 wasm-bindgen 在 `pkg/snippets/` 下生成运行时依赖文件；
2. `pkg/mdcore_wasm.js` 胶水以相对 URL 引用 `./snippets/ironcalc_base-…/inline0.js`（3 处）；
3. `web/build.mjs` 对 pkg 是**逐文件拷贝**（build.mjs:81-82 仅 `mdcore_wasm.js` + `mdcore_wasm_bg.wasm`），
   而 pkg_pdf 是**递归整拷**（build.mjs:62-64 `cpSync('pkg_pdf','dist/pkg_pdf',{recursive:true})`）——
   不对称写法暴露 pkg 拷贝面的假设过时（ironcalc_base 进入 wasm 构建前该假设成立）；
4. main.ts:1741 canvas 装载器运行时 `import('./pkg/mdcore_wasm.js')` → 胶水取 snippets 404 →
   `setupCanvasView` 的 catch 分支吞错（canvas_view.ts:1667-1671 仅写面板文本）→ `__canvasReady` 恒 false。

**影响**：
- web 产品的 Canvas 功能在生产构建路径上**死功能**（面板报 boot failed）；
- 4 个 e2e 套件在标准构建下必红：empty_state、lifecycle_web（`__canvasReady` 超时）——
  canvas_view/concurrent/integration 若干检查同样依赖（本轮实测：补拷前 empty_state/lifecycle 双超时红）；
- 失明机理 = P1-14 的预测兑现：web 测试零 CI + results.json gitignore → 无人看见。

**根因实验（决定性）**：手工 `cp -r pkg/snippets dist/pkg/snippets`（仅动 gitignore 产物）后，
empty_state.py 与 lifecycle_web.py 立即转绿（exit 0）。修复 = build.mjs 加一行
`cpSync('pkg/snippets', 'dist/pkg/snippets', { recursive: true })`（或改递归整拷 pkg）。

**置信**：已确认（404 网络证据 + 面板报错文本 + 前后对照实验三重）。

## 3. web e2e 全量记分板（补 snippets 后；标准构建 = 空缺格标 ⚠）

30 个可执行套件全部运行（含 web/ 根目录 4 个）：

| # | 套件 | 结果 | 备注 |
|---|---|---|---|
| 1 | web_smoke.py | ✅ 15/15 | W1-W6/W11-W13；1MB 档（1,033,305 字符/20,448 块）typing p95 **20.3ms**、fallback_sync 332ms、undo 重渲染 154.1ms |
| 2 | tr1_web_e2e.py | ✅ | CRLF 注入→投影 |
| 3 | lifecycle_web.py | ⚠→✅ | **标准构建必红**（__canvasReady 超时，N-1）；补拷后绿 |
| 4 | crash_recovery_web.py | ✅ | |
| 5 | empty_state.py | ⚠→✅ | 同上 N-1 |
| 6 | canvas_view.py | ✅ | 补 snippets 后（canvas 面实际可用） |
| 7 | hostile_ui.py | ✅ | |
| 8 | i18n_ui.py | ✅ | |
| 9 | editor_toolbar.py | ✅ | |
| 10 | concurrent_web.py | ✅ | |
| 11 | integration_journey.py | ✅ | |
| 12 | evidence_panel.py | ✅ | |
| 13 | docx_import.py | ✅ 15/15 | |
| 14 | **docx_export.py** | ❌ **29/30** | `w2_toc_field_emission_and_position` 红——见 §4 |
| 15 | xlsx_import.py | ✅ 12/12 | |
| 16 | xlsx_export.py | ✅ | |
| 17 | xlsx_export_openpyxl.py | ✅ | openpyxl 独立口径对账 |
| 18 | xlsx_grid.py | ✅ | |
| 19 | pdf_view.py | ✅ | 需先 build:wasm:pdf（display-only 时另有降级面） |
| 20 | contract_gate.py | ✅ | R23-d 契约门 |
| 21 | stress_web.py | ✅ | |
| 22 | oversized_web.py | ✅ | |
| 23 | sidecar_faults_web.py | ✅ | |
| 24 | ime_cdp.py | ✅ | CDP 合成 IME 28 checks |
| 25 | tr2a_cross_process.py | ✅ | 8 语料×5 轮×2 面 DETERMINISTIC |
| 26 | tr2b_l1_commonmark.py | ✅ | |
| 27 | k5_recheck.py | ✅ | NO-8-CAP（undo 无 8 条上限） |
| 28 | k45_recheck.py | ✅ | |
| 29 | acceptance_pdf5_ui.py | ✅ | UI 级 PDF 验收 PASS（标注 460 追加 + 下载 %PDF-1.4） |
| 30 | tr2b_l23_external.py | ⛔ 环境门 | 需 LibreOffice 26.8（沙箱无 soffice）——与仓内已知「LO 软门」一致 |

**结论**：web 面真实水位 = 28 绿 / 1 真 红（docx_export stale 断言）/ 1 环境门；另标准构建下
2 套件必红 + Canvas 死功能（N-1）。「零 CI 的 24 套件」首次有了独立完整运行记录。

## 4. docx_export 红的定性：测试断言与有意行为变更脱钩（P2·已确认）

- 失败 check：`w2_toc_field_emission_and_position`（web/tests/docx_export.py:548-556）要求导出
  document.xml 含中文提示段 `（目录域——打开后全选再按 F9 更新生成）`；
- 实测导出（本沙箱逐字节复检）：TOC 五连全部在场且位置正确（begin[dirty]/instrText `TOC \o "1-3"`/
  separate/end + 域居首内容块前），**唯独提示段缺失**；
- 根因：`deddf1a`（#196 任务2，PR #207）把 wasm 侧 TOC 发射改为「有目录项时发真实目录项+
  PAGEREF 域+书签，提示段仅在零收录退化形发射」（crates/mdcore-wasm/src/docx_export.rs 同提交
  更新了 Rust 断言：texts 期望从提示串改为 `["","章一\t","","章一","正文段。"]`）；
- **web/tests/docx_export.py 未随动**（该文件最后触碰 = 943a05b i18n 六件套，早于 deddf1a）；
- 零 CI（P1-14）使这处转红不可见——上轮终稿「24 套件 ≈389 checks 零 CI」的风险预测在本轮兑现为
  具体案例。修复 = 测试期望改对齐新发射形状（或按产品语义裁决提示段应否保留——那是产品问题不是测试问题）。

## 5. P0-4 实机复现（升级：代码级确认 → 已确认+本地复现）

**中段切分案例（文本重复损坏，预测症状兑现）**：
```
S0 注入        core='- ab'                          DOM='- ab'
S1 光标 a|b 打X core='- aXb'                         DOM='- aXb'
S2 光标 X|b Enter core='- aXb\n- b'  blocks=2        DOM='- aXb¶- b'
S3 新行打 Z     core='- aXb\n- Zb'                   DOM='- aXb¶- Zb'
```
Enter 后光标后的 `b` **同时留在原块和出现在新块**（正确语义应为 `- aX` + `- b`）——
中段文本重复＝数据损坏。与上轮机理判断一致：快路径编辑后 `BlockMap.byId.textChars` 陈旧
（仍记 "ab" 2 字符），handleEnter 以之为 ReplaceText 区间端点 → 切分区间错位。

**行尾 Enter 案例（同根因的另一面，补充登记）**：
```
'- abX' 行尾 Enter → core 完全无事件无块变更（Enter 被吞）
DOM 多一空行 → 下一键 Y 物化为【普通段落】而非列表延续项
```
视图/核心瞬时分叉 + 块类型物化错误（应为新 `- ` 列表项，实为 paragraph）。
复现脚本：`/home/z/my-project/scripts/probe_p04_{enter,forensic,mid}.py`（序列化步骤见脚本）。

## 6. P1-18 实测推翻（降级 P1 → P3）

上轮结论：「wasm 时钟恒 0 → 合并窗失效 → 全会话击键并成一条 undo 条目（已确认）」。

**实测**：三批打字（AA/BB/CC，批间 1.5s ≫ 1000ms 合并窗）→ **3 次 `__bridge.undo()` 分别回退
各批，精确回到基线**。合并窗按真实时间工作，粒度正常。

**修正依据**：main.ts:790 `mutate()` 包装器在**每次变异前**注入
`this.core.set_clock_ms(BigInt(Math.max(1, Date.now())))`（另见 main.ts:1157、1590 两处直呼）。
上轮判断漏看了宿主侧的时钟注入链。

**降级后残存问题（P3·API 设计缺陷）**：`set_clock_ms` 是「冻结常量闭包」
（crates/mdcore-wasm/src/lib.rs:205-209——每次调用生成捕获固定值的新闭包），且构造器不注入
默认时钟：**绕过 mutate() 的消费者（如 bench.ts:114 路径）仍会拿到恒 0 时间戳**。建议 API 改为
内部持有时钟闭包 + set 注入（或文档显著标注「每次变异前必须调」）。

## 7. 其余门禁复跑（新 HEAD 6322f86）

| 门 | 上轮（96596d7） | 本轮（6322f86） | 备注 |
|---|---|---|---|
| `cargo fmt --all -- --check` | 977 行漂移（口径=输出行） | **134 hunk / ~1,900 输出行**；pdf 106 + canvas 22 + office-io 3 + mdcore-wasm 2 + mdcore 1 | 与开放 PR #216 自述「1896 行」吻合；**新增 keymap.rs 2,839 行零漂移**（新代码纪律好，存量债未清） |
| `compliance/scan.sh` | exit 1（5 命中） | **仍 FAIL（5 命中）** | 命中全部在 packaging/ 的「零命中自证清单」（checklist.md:20/README.md:44 引用词本身）——假阳性门红灯，见笔记 14 |
| 开放 PR | #218/#216 | #216（fmt 复位，未合） | #218 已合入 |

## 8. mdcore-wasm 148 测试宿主实跑（对上轮附录 B 的修正）

上轮登记「未运行：mdcore-wasm（wasm 目标）」。本轮发现其 148 个测试全部是**宿主可跑**的
标准 `#[test]`（无 `cfg(target_arch)` 门）：

| 套件 | 结果 |
|---|---|
| src/lib.rs 单测 | 117/117 ✅（1.26s） |
| canvas_wasm | 7/7 ✅ |
| collab_wasm | 8/8 ✅ |
| external_corpus | 1/1 ✅ |
| health_check_docx | 2/2 ✅（31.55s——P0-2 基线锁在场且绿） |
| health_check_xlsx | 1/1 ✅（4.55s） |
| n3_word_count_wasm | 2/2 ✅ |
| workbench_wasm | 5/5 ✅ |
| xlsx_grid | 5/5 ✅ |
| **合计** | **148/148 ✅** |

CI 事实不变：`.github/workflows/` 仅有 `cargo check -p mdcore-wasm --target wasm32`（windows-check.yml:52），
**148 个可跑测试不在任何 CI**——「CI 只测 9/17 crate」结论维持，但「未运行」的登记修正为
「宿主可跑且本地全绿、CI 未纳入」。另注：wasm32 真运行时（wasm-bindgen-test）在本轮环境不可用
（runner 0.2.129 无 --node 模式），与仓内状态一致（该面从未配置）。

## 9. 原始工件

- 全量运行日志：`audit/notes/test-runs/web-e2e.log`
- smoke/pdf/docx_export 结果 JSON：`audit/notes/test-runs/web-results/`
- 复现脚本（沙箱路径，仅记录）：`scripts/probe_p04_{enter,forensic,mid}.py`、`probe_p118_clock.py`、`probe_ready.py`、`probe_toc.py`、`run_web_tests.sh`

---

## 10. 补充：P0-1/P0-3 的实证状态（2026-11-05 第二批）

**P0-1（save→open 身份重编号 + undo 物化 ID 撞号）——实机证据已在场**：
`t3_reuse_locks.rs::save_open_undo_rematerialization_id_collision_boundary`
（crates/mdcore/tests/t3_reuse_locks.rs:145-186）本身就是逐步复现演示：
`p1/p2/p3/p4` → 插 MID(id=5) → 删 p3 → save_document → open_document（重解析先序 [1,2,3,4]，
p2 顶占 ID 3）→ undo 删除 → 树内 `vec![1,2,3,3,4]` **重复 BlockId 3 断言绿**。
该测试在上轮 240 个 mdcore 通过测试中（笔记 16），即 P0-1 从第一轮起就具备「测试锁定的本地复现」
证据——本轮补充点明这一证据等级，并确认其在当前 HEAD 测试组内持续在场。

**P0-3（PDF forms「增量写」实为全量重写）——本轮字节级实证（scratch 工程，仓外）**：
方法：scratch crate（path 依赖 mdc-pdf engine-lopdf，仓库零改动）读真实投标函夹具
`crates/pdf/fixtures/form_bid_letter.pdf`（13,097 字节，8 域）→ `set_field("project_name",…)`
→ `save()` → 逐字节比对：

| 指标 | 实测 | 增量语义要求 |
|---|---|---|
| 输出尺寸 | 11,749 字节（比原文件**小** 1,348） | ≥ 原尺寸（追加） |
| 原字节前缀保持 | **否**——首个差异在偏移 10（头注释行即重写） | 严格前缀 |
| 前 N 字节逐位相同率 | **2.7%** | ≈100% |
| 尾部 /Prev（增量 xref 链） | 无 | 有 |
| %PDF 头计数 | 1（与原相同，无意义） | 1 |

结论：`forms.rs:1038`「增量修订——原字节前缀逐位保持」的文档声称被字节级证据推翻；
对「填已签名投标函」场景的直接含义：签署字节区间被整体重写 → 签名摘要输入变更 →
签名必然失效（P0-3 从「代码直读+零测试」升级为「本地复现+量化」）。
复现工程：`/home/z/my-project/scratch-p03`（Cargo.toml path 依赖 + main.rs 全步骤）。
