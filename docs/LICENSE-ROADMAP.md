# 许可证路线图与 GPL 清零进度

> 决策依据：issue #247（2026-11-06 用户裁定）。
> 决策原文：「那就先用 gpl，然后后期都改完依赖后换严格商业协议」。
> 本文件 = 阶段二（GPL 清零 → PolyForm Strict）的跟踪清单，**阶段一不改任何许可**。

## 分阶段决策

```
阶段一（现在）：GPL-3.0-or-later
  理由：mang-shell 静态链接 10 件 GPL crate，法律上只能 GPL
        且用户已裁定「1.0 阶段开始替换 GPUI」

阶段二（1.0）：PolyForm Strict 1.0.0 + 独立第三方清单
  前置条件：GPUI 依赖全部替换掉，二进制内零 GPL
  届时 LICENSE 才能合法改成严格商业协议
```

**法律事实（硬约束，不是偏好）**：GPL-3.0 要求衍生作品整体以 GPL-3.0 分发、
不得附加额外限制；PolyForm Strict 的限制 = 额外限制。「PolyForm Strict +
含 GPL 二进制」法律上不成立——所以现在改任何严格协议都不行，必须等替换完成。

## 污染范围（2026-11-06 普查）

```
mdcore 主 workspace（16 crate，6.7 万行 src）
  408 包一手普查 → 0 GPL ✅ 干净

mang-shell（GPUI 壳，33.8K LOC）
  静态链接 10 件 GPL-3.0-or-later / 48,562 LOC ❌ 污染
```

## 当前阻塞项（10 件 GPL crate）

LOC 按 `third_party/zed/PROVENANCE.md` 记账口径（r39 闭包）：

| crate | LOC | 用途 | 替换方案 | 状态 |
|---|---:|---|---|---|
| ui | 29,844 | GPUI 组件库 | 待定 | ❌ |
| theme | 5,957 | 主题系统 | 待定 | ❌ |
| text | 6,746 | 文本缓冲（rope + 锚点，text::Buffer） | 待定 | ❌ |
| rope | 4,166 | Rope 数据结构 | 可换 ropey (MIT) | ❌ |
| component | 530 | 组件框架 | 待定 | ❌ |
| clock | 338 | 时钟抽象 | 可自研 | ❌ |
| icons | 360 | 图标 | 可换 lucide (ISC) | ❌ |
| syntax_theme | 345 | 语法高亮主题 | 待定 | ❌ |
| ui_macros | 239 | 宏 | 待定 | ❌ |
| menu | 37 | 菜单栏 | 待定 | ❌ |
| **合计** | **48,562** | | | **0/10** |

## 关键洞察（1.0 规划必读，避免误判工作量）

```
gpui 核心本身是 Apache-2.0（102,096 LOC）
污染全部来自上层 crate：ui / theme / text / rope / menu / ...

→ 阶段二可能不需要「重写整个 UI 框架」
   只需要替换 10 个上层 crate（48,562 LOC）
   gpui 核心保留

这大幅降低 1.0 阶段的工作量。
```

搬运闭包总账（PROVENANCE.md r39 口径）：32 crate / 230,756 LOC，
其中 GPL 系 48,562 / Apache 系 182,194——即 **79% 的闭包代码（按行数）无需动**。

## 替换策略（待 1.0 阶段细化）

```
选项 A：整个 GPUI 换掉（写自己的 GPU UI 框架）
选项 B：只换 GPL crate，保留 Apache-2.0 的 gpui 核心
  查证：gpui 本身是 Apache-2.0 ✅
        污染来自 ui/theme/text/rope/menu 等上层
  → 选项 B 工作量小得多，可行性高
```

## 阶段二前置检查清单（替换完成时逐项验证）

```
[ ] cargo tree 全量：0 个 GPL/AGPL 依赖
[ ] 二进制反编译自检：无 GPL 上游符号
[ ] third_party/zed 目录已完全移除或改为白名单许可
[ ] THIRD-PARTY-NOTICES.md 与 cargo metadata 逐条对齐
[ ] 法务复核（如果走正式发行）
```

## 阶段一纪律（现在生效）

```
✗ 不改 LICENSE 正文（保持 GPL-3.0 全文）
✗ 不改各 crate 的 Cargo.toml license 字段（保持 GPL-3.0-or-later）
✗ 不加 PolyForm 文件（会引起混淆）
✗ 不动 mang-shell 的依赖
```

## 进度记录

| 日期 | 事件 |
|---|---|
| 2026-11-06 | 用户裁定分阶段路线；本文件建立（#247）。0/10。 |
