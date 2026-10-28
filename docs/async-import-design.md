# P2-5 · 同步导入阻塞评估与异步化设计（issue #29 A9 / R-1 F-2 立项）

> 状态：**评估轮（只设计，不实现）**——issue #29 P2-5 原文约束。
> 事实来源：R-1 大文件实测（50.1MB docx / 101MB xlsx / 500 页 PDF）
> + 本轮代码级核查（核查对象 = main @ 4a84bd5）。

## 1. 问题陈述（R-1 实测锚）

`__importDocx` / `__importXlsx` 是**同步 wasm 调用**（主线程）：

| 档 | 耗时 | 期间 UI |
|---|---|---|
| 50.1MB docx（117,371 段） | 4,891ms | **完全冻结**（rAF 旗标：导入完成后 4ms 恢复——事件循环健康，纯阻塞非死锁） |
| 101MB xlsx（1M 格） | 11,607ms | 同上 |

R-1 的 F-2 登记：架构事实（同步 wasm）而非 bug；本文件评估解除路径。

## 2. DOM 依赖核查（方案 A 可行性的代码级证据）

**结论：wasm 导入链零 DOM/浏览器 API 依赖，Web Worker 可行。**

| 核查项 | 结果 |
|---|---|
| `crates/mdcore-wasm/Cargo.toml` | **无 `js-sys` / `web-sys` 依赖**（仅 wasm-bindgen 桥本体） |
| `import_docx(bytes) -> Result<String, JsValue>` | 纯计算：字节进 → JSON 字符串出；`JsValue` 仅作错误映射载体，Worker 内语义不变 |
| `import_xlsx` | 同上 |
| `office-io` 读面 | 纯 Rust（zip/OPC 门 + docx-rs/calamine），无浏览器面 |
| 模块加载 | `import init, { import_docx } from '../pkg/mdcore_wasm.js'`——ESM 模块 worker（`new Worker(url, {type:'module'})`）可直接加载同一产物 |

**唯一主线程耦合在 TS 侧**（`web/src/main.ts`）：`__inject(markdown)` 重建核与 CM6、状态行/警告面板渲染——这些本来就是导入**完成后**的轻量动作（毫秒级），不是阻塞源。

## 3. 方案对比

### 方案 A：专职导入 Worker（推荐主路线，双轨回落）

```
主线程                                worker(import-worker.ts)
  fileInput.change                       init: import init + import_docx/import_xlsx
    │                                    ← {type:'ready'}
    ├─ {type:'import', format,           (transferable ArrayBuffer 零拷贝)
    │   bytes}  ──────────────────────→  wasm import_docx(bytes)
    │  [UI 可响应；可发 {type:'cancel'}]       │
    │                                    {type:'result', ok, markdown, stats}
    └─ __inject(markdown) + 状态行/面板 ←────┘
```

- **迁移面**（全在 web 侧，wasm 零改动）：
  1. 新文件 `web/src/import-worker.ts`（~80 行：init 握手 + 分发 docx/xlsx + 回传）；
  2. `web/src/main.ts` 导入 handler：同步调用改 worker 往返（~40 行改动）；
  3. **最大摩擦点——`window.__importDocx` 同步桥契约**：现形状
     `(bytes) => {ok, markdown, stats} | {ok:false, ...}` 被 web 测试面
     （docx_import.py 等 15+ 套件）与 CONTRACT window 面依赖。
     **对策：双轨**——worker 路径为主，`window.__importDocx` 保留为
     同步回落桥（worker 不可用/初始化失败时走原路径）。既有测试零
     改动（回落路径恒可用），worker 路径新增独立 e2e。
  4. 新增 e2e：worker 路径导入（成功/失败/取消三态）。
- **收益**：主线程零阻塞（4.9s 冻结 → 0）；可取消（`worker.terminate()`）；
  进度反馈成为可能（office-io 若后续加分阶段回调）。
- **成本/风险**：
  - wasm 双实例内存（worker 导入实例 + 主线程编辑器实例）——50MB 档
    worker 峰值 ~888MB（R-1 实测单实例值），导入完成即 terminate 释放；
    主线程实例不受影响。移动端 Safari 的 module worker 兼容（iOS 15+ OK）。
  - worker 内 panic = 整个 worker 死 → 回落路径自动接管（双轨价值）。

### 方案 B：分片异步（主线程内切片 yield）

**不可行（本轮评估否决）**：
- wasm 调用是单一同步边界——进入 `import_docx` 后无法让出事件循环，
  「切片」要求 office-io 读面重构为分阶段回调（docx-rs 模型整体驻留
  的既有架构与之冲突，r4-t2 已登记流式化边界）；
- 引擎侧大改（读面 API 面目全非）换「阻塞时间减少」而非「阻塞消除」，
  投入产出远劣于 A。

### 方案 C：保持现状 + UX 缓解

- 导入前按文件大小弹确认（>10MB 提示「导入期间界面将暂停数秒」）+
  状态行预置「导入中…」。
- 零实现成本，但不解决本质（用户仍冻结 4.9-11.6s）；作为 A 落地前的
  **即时止血**可单独小 PR（~15 行），与 A 不冲突。

## 4. 建议

**A（双轨）为主，C 作过渡止血，B 否决。**

实施估算（供排期，非本轮）：
- R1：import-worker.ts + main.ts 双轨接线 + worker e2e 三态（1 轮）；
- R2：取消面（大文件导入中的取消按钮 + terminate + 回落重试）+
  内存面验证（50MB 档双实例 RSS 实测入 R-1 报告附录）（0.5 轮）。

## 5. 验收口径（实施轮用）

1. 50MB docx 导入期间：rAF 持续触发（UI 可交互）、状态行有「导入中」态；
2. 导入结果与同步路径**逐字节一致**（markdown + stats JSON 双比对）；
3. worker 失败注入（terminate）→ 回落路径自动完成导入；
4. 既有 15+ web 套件零改动全绿（双轨保兼容的直接证据）。
