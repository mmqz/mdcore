# GUI 框架调研 Spike

> 目标：选最好的桌面方案（混合：原生外壳 + WebView 内容）
> 对应 issue: mmqz/mdcore#58

## 结构

```
probe-gpui/     GPUI 最小可运行（编译/开窗/中文/嵌 WebView）
probe-iced/     iced 最小可运行
probe-hybrid/   混合方案验证
screenshots/    中文渲染截图对比
```

## 评分门槛（硬门槛不过 = 淘汰）

| 维度 | 门槛 |
|---|---|
| WebView 混合可行性 | 必须能嵌 wry/WebView2 |
| 中文排版质量 | 人工看截图 |
| Windows 支持 | 必须能编译+运行 |

## 数据（实测后填）

| 项 | GPUI | iced |
|---|---|---|
| 编译时间 | ? | ? |
| 二进制大小 | ? | ? |
| 10万字滚动 FPS | ? | ? |
| 内存 | ? | ? |
| 浏览器混合 | ? | ? |

## 结论

（Probe 完成后填）