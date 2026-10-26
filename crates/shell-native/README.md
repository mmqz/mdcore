# mdshell-native — 原生宿主壳（Linux）

tao + wry + GTK3/WebKitGTK 栈（ADR-H1 定谳：Slint 因 GPL 证排除）。

## 平台面（issue #4）

**GUI 面 = Linux-only（架构定谳）**。wry/tao/gtk 三依赖已挂
`[target.'cfg(target_os = "linux")'.dependencies]` 门：

- **Linux**：完整构建与运行（本文件其余节）；
- **其他目标（Windows/macOS）**：**仅保编译**——`native` 模块与
  GUI 路径不进编译，bin 为 stub（打印一条说明后退出）；
  `cargo check --workspace --target x86_64-pc-windows-msvc` 全绿
  可验证（cfg 分支有 check 覆盖——不再是「写到哪算哪」）。

正式支持平台矩阵的单一真源见仓库根 `COMPLIANCE.md` §平台矩阵。

## 无 GTK 环境跑 workspace 测试

`gobject-sys`/`glib-sys`/`gdk-sys` 的 build script 需要
pkg-config 找到 GTK 系统库——**无 GTK 机器（含 Windows/macOS/
最小 CI runner）上 `cargo test --workspace` 必然在此失败**，与被测
语义无关。排除跑法（CI 与新机器的标准命令）：

```bash
cargo test --workspace --exclude mdshell-native
```

GUI 壳自身的 4 个纯逻辑测试（blueprint/js_escape/percent_decode/
serve_dist）需 GTK 环境：`cargo test -p mdshell-native`。

## Linux 系统库清单（Debian/Ubuntu 系）

编译需要（dev 面）：

```
libwebkit2gtk-4.1-dev   # wry 的 WebView 底座
libgtk-3-dev            # tao 窗口 + GTK3 绑定
libglib2.0-dev          # gobject-sys/glib-sys build script
libgdk-pixbuf-2.0-dev   # gdk-sys
libsoup-3.0-dev         # WebKitGTK 网络栈（webkit2gtk-4.1 传递）
libjavascriptcoregtk-4.1-dev
```

运行时由 dev 包的 Depends 自动带入；最小运行面 =
`libwebkit2gtk-4.1-0` + `libgtk-3-0` + `libglib2.0-0`。

## 无 root 沙箱自愈（s5a-env 先例）

无 sudo 环境用 `apt download` + `dpkg -x` 提取树 + pkg-config
叠加（不遮蔽系统值）：

```bash
source /home/z/my-project/optroot/env.sh   # 提取树在场时
cargo check -p mdshell-native
```

提取树构建法与闭包清单见分析仓 s5a-env 轮登记（optroot 2.3G 中
必需件 = root/ 提取树 561M + env.sh + libexecpath shim）。

## examples

- `xvfb_smoke.rs`：Xvfb 虚拟显示下的可退出冒烟（CI 形态——
  实机人工验收协议见 web/README §6）。
