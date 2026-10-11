# mdcore

模块化 Markdown 编辑核心 + web 桥（CodeMirror 6）+ 原生宿主壳（tao+wry+GTK3）。
**私有产品仓。**

## 架构（R10/R22 蓝图，ADR-H1 定谳）

```text
┌─ 宿主壳 shell-native（tao 事件循环 + wry WebView + GTK3 原生控件）─┐
│   mdcore://app 自定义协议 + 注入桥（mdc-inject* 事件族）              │
│   文件生命周期：打开（渐进+进度）/ 编辑 / 保存（md+meta）/ 身份复原    │
├─ web 桥（web/，CodeMirror 6 + wasm-bindgen）─────────────────────┤
│   window.__bridge 契约面；UTF-16↔码点单一换算对（ADR-V2）            │
│   （CM6 只服务 MD 面；PDF 面为独立轻量视图，不经 CM6）              │
├─ PDF 核 mdc-pdf（Rust，第二 Document Model，PDF-b/c/d 三轮完成）    ┤
│   页/大纲/标注三层模型 + Command 边界 + 安全策略（S1-S5） +       │
│   pdfium 引擎/LRU 页缓存/渲染工人（engine-pdfium）+ lopdf 标注    │
│   增量写（engine-lopdf）；mdcore://pdf 协议草案（CONTRACT §6）      │
├─ 编辑核 mdcore（Rust，五 crate workspace）────────────────────────┤
│   ropey 树 + op log + undo（ADR-B2/B4/B5）+ 块身份（ADR-B1）        │
│   revision/anchor（R13/R17 语义锁）+ 渐进打开 + Human+AI 共用状态    │
└──────────────────────────────────────────────────────────────────┘
```

宿主壳 ↔ web 桥 ↔ 核的接口以 **`CONTRACT.md` v1.2** 为准（R23-d 冻结 v1；
破坏性变更走升版纪律：版本常量 + 文档 + 双侧锁测试三处同步）。

## 分支说明

- **main**：产品代码主体（crates/ 五件 + web/ + CONTRACT.md + compliance/）。
  基线 = R23 终态核侧母本快照（2026-10-10 首推；shell-native 当前为蓝图态，
  R22-a..f 宿主壳实体由开发环境按 CONTRACT.md v1 以后续提交合入）。
- **compliance-baseline**：合规基线套件——`COMPLIANCE.md`（姿态/审计终态/常驻
  门禁/遗留登记）+ `compliance_release/`（THIRD-PARTY-NOTICES / ASSET_CHECK /
  TRADEMARK_RULES）。
- **mother/r23-snapshot**：R23 阶段核侧增量参照（单提交快照，保留不动）。

## 构建与门禁

```bash
cargo test --workspace              # 25 套件语义锁（252 用例；R23-e 148 + R24-a 16 + 质量审计 7 + R24-b 12 + R24-c 4 + T1-fix-a 5 + PDF-b 33 + PDF-c 常开 18 + PDF-d 常开 9；2026-10-10 实跑复核 = 245 旧基线 + T1-fix-a 5 mdcore 质量锁 + T1-fix-b 2 pdf 锁 [pdfb_skeleton 32→33 / pdfc_cache_worker 17→18]；默认零引擎依赖，裸环境全绿）
cargo test -p mdc-pdf --features engine-pdfium    # +13 引擎锁 + 6 #[ignore] 性能门（需 libpdfium.so，定位序：MDC_PDFIUM_LIB → 候选清单 → 系统）
cargo test -p mdc-pdf --features engine-pdfium,engine-lopdf    # +17 PDF-d 标注写锁（lopdf 增量写；需 libpdfium.so + spike fixtures）
cargo test -p mdc-pdf --features engine-pdfium --test pdfc_perf -- --ignored --nocapture    # 性能门实测打印（阈值 = 实测×3）
cargo check --target wasm32-unknown-unknown
cd web && npm ci && npm run build   # wasm 构建 + CM6 桥
./compliance/scan.sh                # 产品面合规门禁（每次提交前，PASS 才可合入）
```

R23 新增测试面：`web/tests/ime_cdp.py`（IME CDP 28 checks，需
playwright + chromium）、`web/tests/contract_gate.py`（契约门 16 checks：
v1 面 13 + v1.1 设置面 3）、`crates/mdcore/tests/r23_fingerprint.rs`（15 条）/
`r23_external.rs`（14 条）。R24-a 新增：`crates/shell/tests/r24_settings.rs`
（设置面语义锁 14 条）。

## 收口纪律（自首推起生效）

每轮收口双绿：`bash compliance/scan.sh`（strict 0 / ns 0）+ `cargo test --workspace`
（25 套件全绿）；依赖变更必须再生成 THIRD-PARTY-NOTICES 并重跑 SBOM。

## 状态快照（2026-10-10）

- R15-R21 强化 + R22-a..f（宿主壳实装在开发环境）+ R23-a..e（IME CDP 验收、
  块指纹 ADR-B6、外部修改 ADR-B7、契约冻结、阶段收口）已完成；四路审计全 CLEAN；
  编译产物反编译自检 CLEAN；SBOM 367 项全白名单。
- R24 产品化进行中：**R24-a 设置面核侧完成**（mdshell::settings 四层
  schema + TOML sidecar + 契约 v1.1：__settings / __setSetting /
  mdc-settings-changed）；**质量审计轮完成**（R24-a1 settings 写读形态
  不对称修复 + R24-a2 SetMark 求逆不对称修复，见 gates.md）；**R24-b
  多窗口核侧完成**（mdshell::session：SessionManager / 一窗一档 /
  资源上限 16 / 焦点序 / 会话持久化记录 + Core 脏标记；CONTRACT §5
  会话面，window 面零变更）；**R24-c 打包基建完成**（CARGO_PKG_VERSION
  单源 + packaging/ 四平台模板 + 分发合规核对单；正式定名 = 用户决策
  前置未决，名称参数化不阻塞）。R24-d 壳实体 = 开发环境轮。
- PDF 载体进行中（md-line/pdf_carrier_plan 五轮）：**PDF-a 侦察收口完成**
  （三探针 spike + ADR-P1 双引擎定谳：pdfium non-V8 + lopdf；安全策略表 v1）；
  **PDF-b 骨架完成**（mdc-pdf 第二 Document Model：页/大纲/标注三层模型 +
  Command 边界 + 安全策略 S1-S5 + 协议草案；零引擎依赖，pdfium/lopdf 自
  PDF-c/d 接入；CONTRACT §6 草案，window 面零变化）；**PDF-c 阅读面完成**
  （LRU 页缓存 + 渲染工人（panic 隔离）+ pdfium 引擎实装（feature
  `engine-pdfium`，默认零引擎依赖不变）+ 会话引擎接线 + 性能门实测入
  gates：打开 0.18ms / 翻页命中 5µs / 渲染 p95 1.1ms）；**PDF-d 标注面完成**
  （v0 四标注 lopdf 增量写（engine-lopdf，默认零引擎依赖不变）+ 会话
  flush（S5 先于一切/脏集精确/幂等）+ Acrobat 互通引擎层预证（lopdf
  写 → pdfium 读：四 subtype + /T 全对上；GUI Acrobat 人工验证 = 开发
  环境轮）。PDF-e 收口见计划表。
- 遗留：真机 IME 人工验收（自动化先行已闭环，R23-a 28/28）、定名后商标检索
  （**R24-c 前置阻塞项**）。
- 研究与审计底稿存于私有分析仓（允许域）；本仓仅存产品代码与合规结论。
