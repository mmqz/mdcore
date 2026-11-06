# mdcore 架构审计报告（2026-11-04，已归档）

> **这是一份时间点快照，不是当前状态。**
> 审计基线：main `96596d7`（2026-11-04，Merge PR #214）
> 归档原因：原分支 `audit/architecture-review-2026-11` 除 `audit/` 目录外携带大量过时代码改动，
> 故仅把 `audit/` 文档 cherry-pick 到 main，原分支废弃。

## 方法与规模

```
六轮递进：工程基线 → Core 状态/事务 → 领域专项 → 质量/安全/测试 → 对标 → 裁决
16 个并行深审代理 + 本地真实测试运行 + CI/门禁复核
16 份模块笔记 + 3 份最终报告
```

## 证据等级

| 级别 | 含义 |
|---|---|
| **已确认** | 代码级证据链 + 多代理交叉或本地复现 |
| **高度怀疑** | 代码推理成立但未运行验证 |
| **需外部环境验证** | 需真机/外部软件 |
| **设计偏好** | 主观取舍 |

## ⚠️ 三条「设计决策」——不是缺陷，勿再当问题提

用户已明确裁定以下三项是 1.0 之前的**故意取舍**，审计报告把它列为问题，本归档处更正：

### 决策 1：GPL 依赖是故意的

```
用户原话：「gpui 不考虑gpl，1.0 阶段开始替换」

事实：mang-shell 编译进 10 件 GPL crate / 48,562 LOC
决策：1.0 之前前端优先考虑开放进度和后端，1.0 阶段开始替换
结论：**不是阻塞项，是已排期的技术债**
```

### 决策 2：编辑器双轨是过渡方案

```
用户原话：「编辑器双轨是因为 bid 需要快速能用，所以用最简单的方式得到可用的过渡 ui，
          不是一直维护两套」

事实：mdcore 语义核 ↔ mang-shell Zed text 栈无共享抽象
决策：过渡期接受双轨，1.0 阶段收敛
结论：**不是架构失控，是有终点的过渡**
```

### 决策 3：Overlay 层不做全格式，只先验 PDF

```
见 #239：本轮只做 PDF 一条垂直链路，
跑通后才有依据判断 DOCX/XLSX 怎么接，不提前猜共同抽象
```

## 五个 P0 的当前状态（截至 2026-11-06）

| # | P0 | 状态 |
|---|---|---|
| P0-1 | save→open 身份重编号 + undo 物化 ID 撞号 | ❌ **未修** → 已立 [#244](https://github.com/mmqz/mdcore/issues/244) |
| P0-2 | docx-rs 读路径真实语料 panic | ✅ 已修（PR #236） |
| P0-3 | PDF forms「增量写」实为全量重写 | ✅ 已修（PR #235） |
| P0-4 | web 桥 list_item 内 Enter 文本损坏 | ✅ 已修（PR #231） |
| P0-5 | TSV 剪贴板公式注入 | ✅ 已修（PR #231） |

## 索引

```
final-report.md         主报告（Executive Summary + 架构图 + P0 清单 + 路线图）
final-report-phase2.md  第二轮
final-report-phase3.md  第三轮

notes/01-structure.md             仓库结构
notes/02-core-state.md            Core 状态/事务（含 P0-1 详证 F-1）
notes/03-core-undo-api.md         undo API
notes/04-bridge-host.md           桥/宿主
notes/05-docx.md                  DOCX
notes/06-xlsx-formula.md          XLSX/公式
notes/07-pdf.md                   PDF
notes/08-canvas.md                画布
notes/09-collab-workbench-evidence.md
notes/10-mangshell.md             mang-shell
notes/11-quality.md               质量门禁
notes/12-security.md              安全
notes/13-tests-ci.md              测试/CI
notes/14-compliance.md            合规
notes/15-benchmark.md             对标
notes/16-test-runs-evidence.md    测试运行证据
```

## 使用方式

```
✓ 当「问题清单」用——里面列的缺陷基本都真实存在
✓ 当「证据集」用——每条都带代码位置和实证

✗ 不要当「当前状态」用——审计后已合并多个修复 PR
✗ 不要当「决策依据」用——三条设计决策已被用户裁定，见上
```