# GPUI Probe 结果（Lead 亲测）

> 机器：Windows 10.0.26100 · MSVC BuildTools 2022 · Rust（cargo 路径 C:\Users\yezi6\.cargo）
> 日期：2026-10-29

## 结论：**编得过、跑得起来、窗口句柄有效**

| 项 | 结果 |
|---|---|
| 依赖数量 | 109 个（含 4 个 windows crate，Windows 支持完整） |
| 首次编译 | ~4 分钟（含 100+ 依赖） |
| 增量编译 | **6.68 秒** |
| 二进制大小 | **10.7 MB**（release） |
| target 目录 | 1.7 GB（首次） |
| 运行 | 进程常驻，`MainWindowHandle=6162822`（**非零 = 窗口已创建**） |
| 响应 | `Responding=True`，CPU 1.28s |

## 已验证的代码

完整三栏布局 + 标题栏 + 状态栏，**编译零错误**：

```rust
Application::new().run(move |cx: &mut App| {
    let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
    cx.open_window(
        WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
        |_, cx| cx.new(|_| ProbeApp { status: ... }),
    ).unwrap();
    cx.activate(true);
});
```

## API 要点（0.2.2 实测）

| 项 | 正确写法 | 坑 |
|---|---|---|
| 入口 | `Application::new().run(\|cx\|)` | 不是 `application()` |
| 闭包 | **必须 `move`** | 否则 E0373 lifetime 错误 |
| 窗口 | `cx.open_window(WindowOptions{..}, \|_,cx\| cx.new(\|_\| App))` | |
| 布局 | `div().flex().flex_col().child(...)` | 类 Tailwind，很好上手 |

## 未能验证的部分

**窗口截图失败**——不是 GPUI 的问题：

```
以计划任务方式启动 -> 进程跑在非前台桌面会话
MainWindowHandle 非零（窗口已创建）
但 CopyFromScreen 只能截到当前交互桌面（浏览器）
-> 需要真实交互式会话才能看到窗口
```

**这条要在你自己的桌面上手动验证**：

```powershell
cd E:\Code\ai\md\audit\mdcore\spike\gui-framework-eval\probe-gpui
.\target\release\probe-gpui.exe
```

双击或命令行都能直接跑，应该看到 1200×800 窗口：标题栏「标书文档引擎 — GPUI Probe」+ 三栏 + 状态栏。

## 待验证（下一步）

| # | 项 | 重要性 |
|---|---|---|
| 1 | **中文渲染质量**（需人工看） | 🔴 硬门槛 |
| 2 | **嵌 wry WebView**（混合路线关键） | 🔴 硬门槛 |
| 3 | 10 万字滚动帧率 | 中 |
| 4 | iced 对比 Probe | 中 |