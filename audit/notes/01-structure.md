# 01 · 结构与依赖边界审计（mdcore-audit = mmqz/mdcore main@96596d7，2026-11-04）

> 只读审计。证据口径：`文件:行号` 均为 HEAD 实测；LOC/测试计数引自
> `audit-notes/loc-stats.md`（自动生成）；活跃度 = `git log` 实测；
> 热度文件 `hot-files.txt` = 非合并提交触达次数（实测校准：44/32/29/27/24 对 45/32/29/27/24，口径成立）。
> 参照系：`mang-research-archive/cleanroom-analysis/md-line/boundary-audit`（P1-P6，2026-10-12，基线 03815b6）。

## 0. 总览事实（一分钟版）

- **三个 workspace**：主仓 `[workspace]` 16 成员（root Cargo.toml:3-4，default-members 13，排除 mdcore-wasm/pdf-wasm）；`mang-shell/`（自含 `[workspace]`，Cargo.toml:32）；`third_party/zed/`（32 crate 搬运闭包，自含 workspace + Cargo.lock）。
- **两条并行产品线**：
  - **web 线（CM6）**：`web/`(TS 11.4K LOC + 29 个 py e2e) ↔ `mdcore-wasm`(10.3K LOC) → mdcore 核；宿主壳按文档 = shell-native（tao+wry+GTK3，蓝图态）。
  - **GPUI 线（实际主线 GUI）**：`mang-shell/`（src 33.8K LOC + tests 7.2K，编辑器占 18.5K LOC）→ Zed `text::Buffer` 基座 + `mdc-office-io`/`mdc-pdf`/`mdc-canvas`；**对 mdcore 核 crate 零依赖**（Cargo.toml 全文无 mdcore；src 内 5 处命中全是 UI 字符串，见 §2.4）。
- **治理文档与主线脱锚**：README.md / CONTRACT.md / COMPLIANCE.md / REUSE.md / gates.md（106KB）中 `mang-shell` 出现次数 = **0**（rg 实测）。gates.md 零 GPUI 提及——质量门禁台账止于 R24（2026-10-10 前时代）。
- **GPL**：third_party/zed 219K LOC rs（PROVENANCE 记 230K），其中 GPL-3.0 系 10 crate ≈48.5K LOC（ui 29.8K/theme 6.0K/text 6.7K/rope 4.2K/component 0.5K/icons/syntax_theme/ui_macros/menu/clock）。仓根 LICENSE = GPL-3.0；NOTICES.md 已如实记账"闭源时需替换"。
- 活跃度（10-20 以来提交数）：mang-shell 106 > web 45 > mdcore-wasm 40 > mdcore 15 > shell 6 > shell-native 2 > mdbridge/relay/office 0。

---

## 1. 职责矩阵（16 crate + mang-shell + web）

图例：内部依赖=产品编译边；消费者=生产代码中的真实调用方（dev-dep/测试不计）。

### 1.1 核心与桥

| 单元 | 实际职责 | 公开 API 概貌 | 内部依赖（rg 验证） | 生产消费者 | src/test LOC · #[test] | 完成度 vs 声称 |
|---|---|---|---|---|---|---|
| **crates/mdcore** (`mdcore`) | MD 块核：ropey 树 + 18 模块（tree/op/history/anchor/revision/snapshot/fingerprint/external/pagination/word_count/html/markdown…） | Core{pub tree/history/log}+apply_ops 十原语+undo/redo+snapshot/渐进打开+FileStamp/plan_reload | 无内部依赖；外部 ropey/serde/sha2 | mdcore-wasm、mdshell、mdbridge | 8,786/7,322 · 240 | README"R23 终态"基本属实；仍活跃（word_count/pagination 轮，10-01/11-01）。**注意：GPUI 主线 GUI 不用它** |
| **crates/mdcore-wasm** (`mdcore-wasm`) | wasm 绑定 + **事实上已成 Office/web 应用层**：docx_import/export、xlsx_import/export/grid、collab、canvas、workbench 六面 | WasmCore 23+ 导出 + 每域独立导出（#196 样式面等） | workbench/canvas/evidence/mdcore/office-io/collab | web/（唯一） | 10,296/2,013 · 148 | 活跃（11-03）；REUSE.md"经 wasm 桥消费 office"路径属实。62 unwrap/167 expect（panic 面）|
| **crates/shell** (`mdshell`) | R10 宿主契约层：DocumentSession/SessionManager/settings re-export | open/save/progressive + Host trait | mdcore、mdc-settings | **仅 shell-native**（蓝图） | 480/1,474 · 44 | headless 可验；#43 i18n 后仍动（10-28）；无产品二进制消费 |
| **crates/shell-native** (`mdshell-native`) | tao+wry+GTK3 宿主壳（ADR-H1） | INJECT_SNIPPET 常量 + cfg 门 stub | mdcore、mdshell；linux-only wry/tao/gtk | **无**（bin，未发布） | 568/0 · 4 | 蓝图+编译级；10-26 平台门轮后停滞。README 仍把它当"桌面壳" |
| **web/** | CM6 桥（main.ts 3.2K 行）+ pdf_view/xlsx_grid/canvas_view/evidence_panel/collab_channel/i18n | window.__bridge/__settings/__inject 族（CONTRACT §3/§4） | mdcore-wasm(+pkg_pdf=mdc-pdf-wasm)（JS 边） | 浏览器/（声称的）shell-native | TS 11,448；py 测试 10,891 · 29 套件 | 活跃（10-29 xlsx 大修轮）；README"19 web e2e 套件"已漂移（现 29 个 py） |

### 1.2 引擎域（Office/PDF/Canvas/协作/证据）

| 单元 | 实际职责 | 内部依赖 | 生产消费者 | src/test LOC · #[test] | 状态 |
|---|---|---|---|---|---|
| **crates/office** (`mdc-office`) | 敌意输出消毒六面（csv_formula/xml_control/xml_entities/html_escape/field_whitelist/url_policy） | 无（纯 std） | **零**（office-io 仅 doc 注释引用，无 Cargo 边——write.rs:61"写入前必须过 mdc-office 消毒链"） | 905/908 · 78 | 10-13 后停滞；**安全层未接线**（§6 问题 5） |
| **crates/office-io** (`mdc-office-io`) | XLSX 读/写/原位编辑 + DOCX 块序读/写 + **DOCX 外科手术编辑 docx_edit** + 容器三门 | mdc-formula | mdcore-wasm、mang-shell、workbench | 22,814/11,821 · 350 | 最重引擎；活跃（11-03 #196）。616 unwrap——IO 边 panic 面大 |
| **crates/formula** (`mdc-formula`) | ironcalc_base=0.9.1 包装：求值/重算/依赖提取/限额 | 无内部 | office-io | 1,185/725 · 27 | 10-31 落地；选型记录完备 |
| **crates/pdf** (`mdc-pdf`) | 第二 Document Model：25 模块（页/大纲/标注/表单 forms/签名 signature/OCR/organize/stamp/generate/render/worker/security…） | 无内部 | mdc-pdf-wasm、mang-shell | 12,325/10,504 · 242 | 活跃（11-04）；已远超 README"PDF-b/c/d 三轮"叙述（docs/pdf-*.md 六篇在档） |
| **crates/pdf-wasm** (`mdc-pdf-wasm`) | mdc-pdf 的 lopdf 写回 wasm 出口（pdf.js 渲染分离真相不分离） | mdc-pdf[engine-lopdf] | web（pkg_pdf，可选构建） | 257/251 · 5 | 10-23 后稳定 |
| **crates/evidence** (`mdc-evidence`) | 证据系统基础层：三态正交/sidecar/指纹 FNV-1a/定位器 | 无 | canvas、workbench、mdcore-wasm | 3,907/内联 · 53 | 10-21；契约层定位达成 |
| **crates/workbench** (`mdc-workbench`) | headless 编排器：八步闭环（XLSX→登记→关联→计算→报告→失效→审核→导出） | evidence、office-io | mdcore-wasm | 2,195/2,845 · 31 | 10-27；dev-dep 含 mdbridge/mdcore/collab（组合测试） |
| **crates/collab** (`mdc-collab`) | yrs 协作引擎：文档域，双 UndoManager/捕获窗 0/限额 | 无（yrs+serde_json） | mdbridge、relay、mdcore-wasm | 3,234/3,275 · 60 | 10-27（presence flaky 修复） |
| **crates/canvas** (`mdc-canvas`) | 画布域 yrs 引擎 + 渲染黄金例 | evidence | mdcore-wasm、mang-shell | 3,860/2,437 · 59 | 11-01 活跃 |
| **crates/mdbridge** (`mdc-mdbridge`) | mdcore op 流 ↔ mdc-collab 命令流互译（应用层组合点） | mdcore、collab | **零生产消费者**（仅 workbench dev-dep/测试/示例） | 1,037/903 · 21 | 10-22 后停滞；集成样板 |
| **crates/relay** (`mdc-relay`) | 服务端中继状态机（传输无关） | collab | **零生产消费者** | 226/598 · 9 | 10-22 后停滞；蓝图 |
| **crates/settings** (`mdc-settings`) | 域无关四层 schema + TOML sidecar | 无 | **仅 mdshell**（→shell-native 链，不入任何产品二进制） | 958/28 · 2 | 10-28 #43；web 侧运行时实为 TS 镜像（main.ts:1451） |

### 1.3 mang-shell（GPUI 主线外壳）

- **规模**：src 33,780 LOC（43 文件）+ tests 6,240-7,187 LOC（14 文件）· 563 #[test]；其中 `src/editor/` 13 模块 **18,503 LOC（55%）**。
- **职责**：#63 Lead 布局骨架（title_bar/activity_bar/panels/shell/status_bar）+ 文件树/git 面板/命令面板/菜单栏/设置面板/上下文菜单 + **自研编辑器**（buffer/highlight/fence/brackets/display_map/multi_cursor/mouse/find_bar/commands）+ 文档视图（docx_doc/docx_edit_bridge/pdf_doc/xlsx_doc/canvas_view）+ **RPC 控制面**（rpc.rs：AF_UNIX JSON-lines，27 命令 + doc_changed 订阅推送，"AI 操作运行中的 UI"地基）+ 自有 settings（JSON `~/.config/mang/settings.json`）+ 冒烟/性能模式（--smoke-*）。
- **依赖**：mdc-office-io、mdc-pdf[engine-pdfium,engine-lopdf]、mdc-canvas + Zed 栈（gpui/gpui_platform/theme/ui/zlog/text）+ image/smallvec。**无 mdcore、无 mdc-collab/evidence/workbench/mdbridge/relay/mdshell/mdc-settings/mdc-office**。
- **状态**：全仓最活跃目录（10-20 以来 106 提交；HEAD 309a24a 即本目录）；CI 有专属 job（windows-test.yml:105-145）。

### 1.4 辅助面（非任务清单但影响结构判断）

- `sdk/python`（mang-rpc）+ `sdk/mcp`：mang-shell RPC 的官方 Python/MCP 客户端——**第三条 AI 接入通道**（独立于 mdcore Source::Ai 与 web __bridge）。
- `packaging/`（deb/AppImage/NSIS/Info.plist 模板，10-28 构建验证过）、`compliance/`（scan.sh denylist 门禁）、`test-artifacts/`（语料+TEST-ROUND-REPORT 敌意测试轮台账）、`scripts/`（夹具生成 + zed_port.py 搬运器 + p1 补丁）、`docs/`（pdf/docx 专题 11 篇）。

---

## 2. 真实依赖图与边界判定

### 2.1 产品编译边邻接表（Cargo.toml + rg use 双重验证）

```text
mdcore            → (叶)
mdc-office        → (叶)                        [零消费者]
mdc-office-io     → mdc-formula
mdc-formula       → (叶)
mdc-pdf           → (叶)
mdc-evidence      → (叶)
mdc-collab        → (叶)
mdc-settings      → (叶)                        [仅下述链消费]
mdshell           → mdcore, mdc-settings
mdshell-native    → mdcore, mdshell             [bin，无下游]
mdc-mdbridge      → mdcore, mdc-collab          [零生产消费者]
mdc-relay         → mdc-collab                  [零生产消费者]
mdc-workbench     → mdc-evidence, mdc-office-io
mdc-canvas        → mdc-evidence
mdcore-wasm       → mdcore, workbench, canvas, evidence, office-io, collab
mdc-pdf-wasm      → mdc-pdf
mang-shell        → office-io, pdf, canvas, (zed: gpui/theme/ui/text/zlog/gpui_platform)
web(TS)           → mdcore-wasm(JS 边), mdc-pdf-wasm(可选)
sdk/python        → mang-shell RPC socket
```

dev-only 边：workbench→{mdbridge,mdcore,collab}；canvas→collab；relay→canvas；mang-shell→{gpui(test-support),lopdf,flate2}。

### 2.2 Mermaid 依赖图（→ = 产品编译边；⇢ = dev/测试边；⊘ = 文档声称但无代码边）

```mermaid
graph TD
    subgraph 主线GUI["GPUI 线（实际产品 GUI）"]
        MS[mang-shell 33.8K<br/>GPL-3.0]
        ZED[(third_party/zed 219K LOC<br/>10 crate GPL)]
    end
    subgraph web线["web 线（CM6 桥）"]
        WEB[web/ TS 11.4K]
        WASM[mdcore-wasm 10.3K]
        PWASM[mdc-pdf-wasm]
    end
    subgraph 蓝图["宿主蓝图（停滞）"]
        SN[mdshell-native 568]
        SH[mdshell 480]
        SET[mdc-settings]
    end
    CORE[mdcore 8.8K<br/>块核/op/undo/anchor]
    subgraph Office域
        OFF[mdc-office 905<br/>消毒六面 ⊘未接线]
        OIO[mdc-office-io 22.8K]
        FRM[mdc-formula]
    end
    PDF[mdc-pdf 12.3K]
    subgraph 证据协作
        EV[mdc-evidence]
        WB[mdc-workbench]
        CV[mdc-canvas]
        CO[mdc-collab]
        MB[mdc-mdbridge ⊘零消费者]
        RL[mdc-relay ⊘零消费者]
    end
    SDK[sdk/python+MCP]

    MS --> OIO; MS --> PDF; MS --> CV; MS --- ZED
    WEB --> WASM; WEB --> PWASM
    WASM --> CORE; WASM --> WB; WASM --> CV; WASM --> EV; WASM --> OIO; WASM --> CO
    PWASM --> PDF
    SN --> SH; SH --> CORE; SH --> SET
    OIO --> FRM; OIO -.⊘文档声称.-> OFF
    WB --> EV; WB --> OIO; WB ⇢ MB
    CV --> EV; CV ⇢ CO
    MB --> CORE; MB --> CO
    RL --> CO
    SDK -.RPC socket.-> MS
    style MS fill:#f96
    style CORE fill:#6f9
    style ZED fill:#fc9
```

### 2.3 边界判定（逐项）

1. **Core 反向污染**：mdcore 是纯叶（依赖仅 ropey/serde/sha2），无反向污染。✅ 三域纪律（office/pdf 对 mdcore 零 path 依赖）在 Cargo 层全部成立（逐一核对 16 份 Cargo.toml）。
2. **循环依赖**：Cargo 结构层无环（含 dev 边也无环）。✅
3. **间接强耦合**：mdcore-wasm 一端聚合 6 个域 crate，是"上帝出口"——但这与 web 单 bundle 形态相称，属设计选择。⚠ 可接受。
4. **为拆而拆 / 纯抽象层 / 死代码**（核心发现）：
   - `mdc-office`（消毒）：905 LOC + 908 测试，**零生产消费者**，office-io 只在 doc 注释里说"必须过"（office-io/src/write.rs:61、lib.rs:87）而无 Cargo 边——安全防线仅存在于文档语气中。
   - `mdc-mdbridge`（1,037）：mdcore↔collab 桥，零生产消费者（rg 全仓：仅自身测试 + workbench dev-dep/examples）。
   - `mdc-relay`（226）：零生产消费者。
   - `mdshell`+`mdc-settings`+`mdshell-native`（480+958+568）：互相消费成链，链尾是未发布 bin；CONTRACT §4 设置面的真实运行时是 web/main.ts 的 TS 实现（main.ts:1451 默认值、1468 校验），Rust 链只承担契约锁测试。
5. **职责重复**（最大结构问题，详见 §6 问题 1/2）：
   - **编辑器双轨**：mdcore（ropey/op/undo/anchor，8.8K）+ CM6 桥 vs mang-shell 编辑器（Zed text::Buffer + 自研 undo/选区/folding/fence/brackets/多光标，18.5K）。MD 领域语义（标题折叠、围栏、词选中、字数）在两轨各写一份。
   - **DOCX 双管线**：mdcore-wasm docx_import/export（块模型重建，#196 样式面活跃）vs office-io::docx_edit（外科手术 XML 编辑）+ mang-shell docx_edit_bridge（Buffer↔DocxEditor）。rg 实测 mdcore-wasm 零引用 docx_edit——两模型无共享。
   - **XLSX/PDF/Canvas 双表面**：web 侧 xlsx_grid/pdf_view/canvas_view.ts vs mang-shell 侧 xlsx_doc/pdf_doc/canvas_view.rs（共享底层引擎 office-io/pdf/canvas，表面层各写）。
   - **设置三栈**：mdc-settings（TOML 四层）↔ web TS 镜像（契约锁钉住，可接受）↔ mang-shell settings.rs（JSON、另一套键、另一路径，无契约关联）。
   - **AI 通道三条**：mdcore Source::Ai + ByAnchor（web 线）vs mang-shell RPC socket + doc_events 订阅总线（GPUI 线）vs sdk/python+MCP。溯源语义（Source::Ai 事件流）与 RPC 事件（source="user"）互不相通。

### 2.4 mang-shell ↔ mdcore 关系（线索①核实，**成立且比预期更强**）

- `mang-shell/Cargo.toml`（全文）：依赖 mdc-office-io/mdc-pdf/mdc-canvas/zed 栈，**无 mdcore**。src 内 "mdcore" 5 处命中均为 UI 字符串（panels.rs:240 "mdcore 0.1.0"、shell.rs:914 "mang-shell {}（mdcore 桌面外壳）"、status_bar.rs:183、file_tree.rs:546/548 路径样例）。
- 文本存储：`mang-shell/src/editor/buffer.rs:1-51` —— "r42 阶段 A 换芯：存储层 Zed `text::Buffer`（rope 持文 + 事务历史 + 锚点基座）"，`use text as zed_text`（:51）；undo/选区/粘性列是**外壳自有四元组实现**（:36-40），刻意不用 text::Buffer 内建 undo；`col` 按 char 索引、`\r\n` 直通——与 mdcore 的 char-op/事件流语义无任何共享代码。
- 结论：**编辑器双轨已确认**。mdcore 核只服务 web 线；实际 GUI 用一套平行实现。两者唯一共享物 = office-io/pdf/canvas 三个引擎 crate + 品牌 strings。

### 2.5 三条宿主路径现状（线索②，git log 佐证）

| 路径 | 形态 | 最后活跃 | 判定 |
|---|---|---|---|
| **web/ + mdcore-wasm**（CM6 桥） | 实体、全功能、29 e2e 套件 | 2026-10-29（45 commits/2 周） | **活跃**（xlsx/样式轮） |
| **shell-native（tao+wry+GTK3）** | 蓝图+编译级（README 自认 ⚠️/❓） | 2026-10-26（平台门轮，2 commits） | **停滞**；CONTRACT §2 仍称"产品仓 R22-d/e 实装" |
| **mang-shell（GPUI）** | 实体、33.8K LOC、CI 专属 job、SDK 生态 | 2026-11-04（106 commits/2 周） | **实际主线**；README/CONTRACT 零提及 |

---

## 3. 治理文档声称 vs HEAD 现实（差异清单）

| # | 文档声称（位置） | 现实（证据） | 定级 |
|---|---|---|---|
| D-1 | README:3 架构 = "web 桥（CM6）+ 原生宿主壳（tao+wry+GTK3）"；README:18-37 架构图顶层是 shell-native | 实际 GUI = mang-shell（GPUI），README 全文 0 次提及；shell-native 568 LOC 蓝图 | P1 |
| D-2 | README:33 "编辑核 mdcore（Rust，五 crate workspace）"；README:44 "main：产品代码主体（crates/ 五件…）" | workspace = 16 成员（root Cargo.toml:3）；另有 mang-shell/third_party 两个独立 workspace | P3 |
| D-3 | README:10 "cargo test --workspace 850+ 用例" vs README:55 "语义锁全绿 361 用例" | 自相矛盾且双双过时：default-members #[test] 合计 ≈1,220（含 mang-shell 1,936）；"361"是 2026-10-13 口径（COMPLIANCE.md:34 同样残留） | P3 |
| D-4 | README:61-62 "./compliance/scan.sh 每次提交前 PASS 才可合入" | CI 仅 2 个 Windows workflow（windows-check/windows-test），scan.sh、web e2e（29 套件）、Linux 基线均不在 CI——门禁全靠人工纪律 | P2 |
| D-5 | README:10 "19 web e2e 套件全绿" | web/tests 现有 29 个 py 套件 | P3 |
| D-6 | README:4 "私有产品仓" + COMPLIANCE.md:8 "GPL/AGPL 零容忍（闭源最严）" | 仓根 LICENSE=GPL-3.0；NOTICES.md"整体 GPL-3.0-or-later，闭源时需替换 10 crate（≈48.5K LOC，含编辑器基座 text/rope 与 UI 全家）"；COMPLIANCE 审计终态日期 2026-10-10 早于 zed 搬运（10-29/30） | P1 |
| D-7 | CONTRACT §2 "宿主壳（tao+wry+GTK3，产品仓 R22-d/e 实装）"；web/package.json:4 "Same bundle the host shell (tao+wry+GTK3) loads" | shell-native 停滞于编译级；没有任何宿主在加载 dist/（wry 路径未实装） | P1 |
| D-8 | gates.md（106KB，"第一阶段硬 Gate 清单"） | 零 GPUI/mang-shell 提及；质量台账未覆盖当前主线（mang-shell 的门禁证据散在 PR 描述/TEST-ROUND-REPORT/scripts） | P2 |
| D-9 | CONTRACT.md:130 `editor.undo_coalesce_ms` 默认 1000、范围 1..=10000（暗示可调）；web 设置面板 main.ts:3196 暴露"撤销合并窗口(ms)"滑钮；TEST-ROUND-REPORT TR-6 称"undo_coalesce_ms 可调" | 引擎恒用常量 `COALESCE_MS=1000`（history.rs:20；should_coalesce :91-98 直读常量）；全仓 rg 无任何消费者把该设置传入 Core——**用户可调旋钮是死的**；REUSE.md:29 自认"登记保留键、当前不驱动引擎行为" | P2 |
| D-10 | README 平台矩阵 Linux"✅ 全量测试基线" | Linux 无 CI（全部 CI=Windows）；基线只在开发者本机复现 | P2（与 D-4 同源） |

---

## 4. 游离临时文件与实验代码清单（均已 git 跟踪）

| 文件/目录 | 抽样判性 | 处置建议 |
|---|---|---|
| 根目录 `_dm_add.txt/_helpers.txt/_settings_add.txt/_purefns.txt/_foldtests.txt/_foldtests2.txt/_mod_fold_api.txt/_settingstests.txt/_persist.txt`（9 件） | 均为 #132-3 折叠轮的**代码粘贴残片**（API 片段/纯函数/测试体），内容已集成进 mang-shell（display_map.rs:160 fold_heads 已在位） | 删除（P3 卫生） |
| 根目录 `.t.log`（13.9KB） | 构建告警日志（"unexpected cfg condition name: ztracing"——顺带暴露 vendored 代码 cfg 噪音） | 删除 + .gitignore（P3） |
| `spike/gui-framework-eval/probe-gpui/` | **只剩一个 178KB 的 Cargo.lock**（git ls-files 唯一文件）——探针源码从未提交，锁文件成孤儿 | 删除；GPUI 调研结论应入 docs（P3） |
| `web/` 根 `k5_recheck.py/k45_recheck.py/tr2a_cross_process.py/tr2b_l1_commonmark.py/tr2b_l23_external.py/acceptance_pdf5_ui.py` | 测试轮脚本散在 web 根（正式套件在 web/tests/） | 归档进 tests/ 或删（P3） |
| `scripts/` | 混装：夹具生成器 + `zed_port.py`（搬运器，**关键基建**）+ `p1_gpui_windows_zoom_toggle.patch`（vendored 源码补丁重放件） | zed_port/patch 必须保留（PROVENANCE 依赖）；其余可分层（P3） |
| `third_party/zed/` | 32 crate / 219K LOC rs；上游 commit 1663af4（2026-10-29）；"源码零修改"但存在 1 个登记补丁（gpui_windows zoom toggle）；GPL 10 件记账在案 | 见 §6 问题 3（P1 合规/替换成本） |
| 仓外依赖 | `crates/pdf/src/engine_pdfium.rs:147` 编译期候选路径 `../../../cleanroom-analysis/md-line/pdf-a/spike/bin/lib`——**产品仓测试载荷取件位在仓外研究档案**；本审计克隆中该路径不可达（同级目录名不同），pdfium feature 测试只能靠 MDC_PDFIUM_LIB env | 环境耦合登记（P3，文档已自认"门禁环境路径"） |

---

## 5. boundary-audit P1-P6（2026-10-12，基线 03815b6）逐条核验 @ HEAD 96596d7

| 历史结论 | HEAD 状态 | 证据 |
|---|---|---|
| Q3：mdc-pdf 与 mdcore 零编译依赖 | **仍成立** | pdf/Cargo.toml 无 mdcore |
| 缺口1：Source 五值镜像锁单向（mdcore 加第六值 → pdf 静默漂移） | **已修复**：双向锁落地——`source_mirrors_mdcore_five_values`（pdfb_skeleton.rs:1132）+ `source_mirror_bidirectional_exact_count`（:1180，mdcore 侧变体计数恰 5 + pdf 侧 const fn 穷尽锁） | 改进 ✅ |
| 缺口2：settings.undo_coalesce_ms 死设置 | **仍成立且恶化**：现在 web UI 面板向用户暴露该旋钮（main.ts:3196），契约面校验范围（CONTRACT:130），但引擎不消费（history.rs:20 常量）——从"死键"变"活 UI 假旋钮" | §6 问题 6 |
| 缺口3/P3-G3：wasm source 非法字符串静默兜底 View | **已修复**：`parse_source_str` typed reject 五合法值（mdcore-wasm/src/lib.rs:256-276，注释明言"修复审计 P3 G3/U2"） | 改进 ✅ |
| 缺口3/P3-G5：undo 栈无上限 | **仍成立**：history.rs 无 cap/limit/max（rg 零命中） | P3 维持 |
| 缺口3/P3-G4：save→open 结构性编辑 ID 漂移/撞号未测 | **未加锁**：mdcore/tests/ 19 文件无新增对应往返专项（r24_quality_fixes 非 ID 面） | 维持 |
| P1 §6.5 / R1 旁路警示：Core `pub tree/history/log` 直访敞开 | **仍成立**（lib.rs:138-140） | 维持 |
| A1：take_events 拉取式单消费者、无推送/多订阅 | **mdcore 侧仍成立**（lib.rs:267 drain）；mang-shell 侧另起炉灶自建 doc_events 总线 + RPC 订阅（rpc.rs/doc_events.rs）——缺口未回补核，而是被第二轨绕开 | 维持+分化 |
| A2：无 get_block 族 API | **仍成立**（rg `pub fn get_block` 零命中） | 维持 |
| X1/X2：op 集/undo 容器跨域不可复用 | 仍成立；实际演化走了预期路径：collab/canvas 各自 op+命令面（与 mdcore op 语义对齐但独立） | 一致 |
| P5 F-1：__wasmMemory/__heapTrace 文档说恒在、实际渐进专属 | **已修复**：CONTRACT.md:72-74 改记"渐进路径专属"；main.ts:1553/1570 渐进分支安装 | 改进 ✅ |
| P5 F-2：CONTRACT §3 版本号残留 | **已修复**：§3:70 = 1.4，main.ts:1708 = 1.4 | 改进 ✅ |
| P5 F-3/F-4（__view 锁面不全 / __bridge 运行时宽于声明） | 未复核（低风险文档级，维持原判） | — |
| P6 B-1..B-10 MD 面性能基准 | 大部分仍空位：mdcore/tests 仅 r14_gates/r23_fingerprint 含 Instant 计时（println 无阈值断言）；wasm 侧 heap 口径有 R20-R22 台账 | 维持 |
| P4 §3：六项协同能力缺失（多端同步/OT/CRDT/广播/离线/自动冲突） | **部分被 mdc-collab/mdc-canvas/mdc-relay 覆盖**（yrs CRDT/中继/presence），但这些在 web 线仅经 mdcore-wasm 出口、GPUI 线完全未接 | 演化 |
| 总评 | P1-P6 的**核内**结论几乎全部保持成立或被正式修复（修复处有审计溯源注释——工程纪律好）；但**审计边界外**长出了 mang-shell 第二轨，使"mdcore = 编辑核心"的前提在主线 GUI 上失效 | — |

---

## 6. 问题清单（P0-P3）

> 置信级别：已确认 = 本轮代码/git 实证；高度怀疑 = 证据充分但未运行验证；需外部验证 = 需构建/运行或用户裁决；设计偏好 = 非缺陷。

**P0（数据丢失/安全/核心状态破坏）：本轮结构审计范围内未发现。**（行为级问题如 TR-1 CR 镜像不属本轮结构面，留后续轮。）

### P1-1 编辑器双轨：mdcore 核被实际 GUI 弃用，两套 MD 编辑栈并行演进
- 证据：`mang-shell/Cargo.toml`（无 mdcore 依赖；text 依赖 :74）；`mang-shell/src/editor/buffer.rs:1-51`（Zed text::Buffer 换芯自述）；mang-shell/src 仅 5 处 "mdcore" 字符串（panels.rs:240 等）；mdcore 消费者全景 = mdcore-wasm/mdshell/mdbridge，无一在 GPUI 线。
- 调用链：用户 → mang-shell Editor → Buffer(Zed text) → 自研 undo/折叠/fence/brackets/highlight；另一轨 用户 → web CM6 → mdcore-wasm → mdcore op/log/anchor。
- 影响：MD 语义两份实现漂移（undo 合并窗、字数口径、折叠规则、锚点/身份模型互不相通）；AI 溯源、协作（collab 经 mdbridge 只认 mdcore op 流）、证据定位（evidence 无 GPUI 接入）在主线 GUI 全部缺席；双倍维护成本（web 40 commits vs mang-shell 106 commits 并行燃烧）。
- 置信：**已确认**。裁决属用户（收敛到单轨 or 承认双轨分工）。

### P1-2 治理文档与产品主线脱锚（README/CONTRACT/COMPLIANCE/gates 零提及 mang-shell）
- 证据：§3 D-1/D-7/D-8（rg 计数 0）；gates.md 无 GPUI 字样；COMPLIANCE 审计终态 2026-10-10 早于 zed 搬运 19 天。
- 影响：契约面（CONTRACT §1-§4 描述 mdcore/CM6/shell-native 体系）对真实产品只有历史解释力；新人/外部审计按 README 会得出错误架构图；门禁台账（gates）不覆盖 33.8K LOC 主线。
- 置信：**已确认**。

### P1-3 GPL 闭源阻塞：合规姿态自相矛盾
- 证据：COMPLIANCE.md:8（GPL 零容忍，ADR-C1 闭源最严）↔ NOTICES.md（整体 GPL-3.0，闭源需替换 10 crate ≈48.5K LOC：ui/theme/text/rope/component/icons/menu/syntax_theme/ui_macros/clock）↔ README:4（私有产品仓）；issue #7 闭源决策挂起。
- 调用链：mang-shell bin → 静态链接 GPL text/rope（编辑缓冲）+ ui/theme（全部控件）→ 分发即触发 GPL 源码披露义务。
- 影响：闭源商业目标当前不可达；替换成本最高的恰是编辑基座（text::Buffer）与 UI 全家（ui/theme）——与 P1-1 双轨叠加时，"换掉 Zed 栈"与"收敛编辑核"是同一个决策的两面。
- 置信：**已确认**（仓内文档自证）；商业影响需外部验证（发行计划）。

### P1-4 DOCX 双编辑模型无共享抽象
- 证据：mdcore-wasm/src/{docx_import,docx_export}.rs（块模型重建，#196 活跃）↔ crates/office-io/src/{docx,docx_edit}.rs（外科手术）↔ mang-shell/src/editor/docx_edit_bridge.rs:9（唯一消费 docx_edit）；rg 实测 mdcore-wasm 零引用 docx_edit。
- 影响：同一保真问题（样式丢失 vs 结构重建）两套答案（docs/docx-fidelity-matrix.md 只覆盖一侧）；行为分歧将随两轨各自演进而扩大。
- 置信：**已确认**。

### P2-5 mdc-office 消毒层零接线：安全姿态=文档语气
- 证据：`crates/office-io/src/write.rs:61`"写入前**必须**过 mdc-office 消毒链"——但 office-io Cargo.toml 无 mdc-office 边（无法调用）；全仓 rg `mdc_office::` 生产命中 = 0（仅 doc 注释 + 自身测试）；url_policy/csv_formula/field_whitelist 全部无消费者。
- 调用链（应有的）：write/docx 出口 → sanitize 六面 → 引擎；现实：出口直连引擎（rust_xlsxwriter/docx-rs 内建转义兜底 XML 面，但 URL scheme 白名单/CSV 公式注入/域指令白名单**无任何防线**）。
- 影响：README 架构图把"消毒六面"画进 Office 域防线；实际 TR-5 已实测 javascript: 字面可进 docx 文本层（docx 无执行面，暂非漏洞，但 HTML/CSV 出口一旦出现即裸奔）。
- 置信：**已确认**（未接线事实）；实际可利用性**需外部验证**（取决于未来出口面）。

### P2-6 undo_coalesce_ms 假旋钮（含 TR-6 文档失实）
- 证据：history.rs:20（常量 1000）+ :91-98（直读）；settings/src/lib.rs:98/363/617（存取链齐备）；CONTRACT.md:130；web/src/main.ts:3196（UI 暴露）+1468（校验）；TEST-ROUND-REPORT.md TR-6"可调"失实。
- 影响：用户改设置 → 快照变更 + 事件派发 → 引擎行为零变化——静默欺骗面；契约文档背书了不存在的行为。
- 置信：**已确认**。

### P2-7 CI 结构性缺口：Linux 基线/compliance scan/web e2e 全不在 CI
- 证据：`.github/workflows/`仅 windows-check.yml + windows-test.yml（后者已含引擎 crate 测试 + mang-shell job——好）；README:55/61-62 声称的收口双绿为人工流程。
- 影响："Linux 全量测试基线"无守护；scan.sh denylist（品牌词合规门）靠自觉；web 29 套件漂移无人拦。
- 置信：**已确认**。

### P2-8 死/休眠 crate 群（约 3.2K src LOC + 各自测试）
- 证据：§2.1 消费者普查——mdc-office(905)/mdc-mdbridge(1,037)/mdc-relay(226)/mdshell(480)+mdc-settings(958)+mdshell-native(568) 无产品二进制可达路径；git 活跃度 0（10-13~10-28 后零提交，shell-native 2 提交为平台门）。
- 影响：维护面/心智负担；"16 crate"清单里近 1/3 是蓝图或样板；version 单源纪律反使它们持续被 workspace 检查拖着走。
- 置信：**已确认**（是否"该删"属设计偏好）。

### P3-9 设置三栈分裂（mang-shell settings 与契约面无关联）
- 证据：mang-shell/src/settings.rs:9-19（JSON `~/.config/mang/settings.json`）↔ mdc-settings（TOML 四层）↔ web TS 镜像。键域/格式/路径互不相干。
- 置信：**已确认**；定级 P3（单机产品可忍，跨端同步时变 P2）。

### P3-10 游离文件/孤儿锁/仓外取件位（§4 全表）
- 置信：**已确认**。

### P3-11 Core 封装敞开 + undo 无上限 + take_events 单消费者（历史审计维持项）
- 证据：lib.rs:138-140 / history.rs 无 cap / lib.rs:267。
- 置信：**已确认**（均为登记过的已知边界）。

### P3-12 panic 面统计（office-io 616 unwrap、mdcore-wasm 62 unwrap/167 expect、evidence 125 unwrap）
- 证据：loc-stats 列；属 IO 边/敌意输入面的潜在 panic 路径，需行为审计轮确认可达性。
- 置信：**高度怀疑**（统计级，未逐条溯源）。

---

## 7. "哪些自研值得继续"速判（供主审计员）

- **值得继续（有差异化价值 + 质量纪律好）**：mdcore 核（op/undo/anchor/外部重载语义，240 测试 + 审计修复溯源）、mdc-pdf（25 模块引擎域 + 双引擎 feature 门）、office-io 的三门容器安全 + docx_edit 手术路径、mdc-evidence 三态契约、mdc-collab/canvas 的 yrs 隔离不变量、mdc-formula 包装纪律。
- **需要裁决（双轨核心）**：mang-shell 编辑器 18.5K LOC vs mdcore 8.8K——收敛方案取决于 GPL/闭源与产品主线两个未决项（issue #7）。
- **建议收缩**：mdc-mdbridge/mdc-relay（无消费者，等真实协作宿主再激活）、mdshell/mdshell-native（若确认 mang-shell 为主线则应把 CONTRACT §2 改写为历史档案）、mdc-office（要么给 office-io 加边接线，要么降级为"未来出口面库"并在 README 撤下防线图）。
- **文档动作（低成本高收益）**：README/CONTRACT/COMPLIANCE 增补 mang-shell 章节 + 平台矩阵改写 + 用例数刷新 + GPL 姿态统一口径；gates.md 补 GPUI 线门禁索引或明示截止日期。

—— 完 ——（本轮零写入仓库；产物仅本笔记文件）
