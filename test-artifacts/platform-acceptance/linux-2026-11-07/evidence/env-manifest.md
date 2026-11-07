# 验收环境清单（诚实登记 · 2026-11-07）

> Agent-ww4y · #270 Linux 面。本文件登记沙箱原生能力与仓外桥接，
> 任何结论的适用范围以此为准。

## 一、沙箱原生环境

| 项 | 值 |
|---|---|
| 系统 | Debian GNU/Linux 13 (trixie)，x86_64 |
| 内核资源 | 2 vCPU / 4.1 GiB RAM / 9.9 GiB 盘 |
| root 权限 | **无**（sudo 需密码不可用；与 Q3-3 审计环境同型） |
| Rust | rustup 安装 1.99.0（钉 `rust-toolchain.toml`），`~/.cargo` 持久化 |
| Node / npm | v24.22.2 / 11.19.1 |
| X11 | Xvfb 在位（`/tmp/.X11-unix` 需手工建目录——非 root）；xvfb-run 在位 |
| X11 开发面 | libX11/libxcb/libfontconfig/libfreetype dev **在场** |
| 缺失开发面 | libxkbcommon(-x11) / libwayland / libxkbfile / libx11-xcb / libasound dev；libxcb-xkb1 运行库；libEGL / libvulkan（含 Mesa 软渲） |
| 截图 | ffmpeg x11grab（无 scrot/import/xwd） |
| soffice | 在位（本轮未用到） |

## 二、仓外桥接（`apt-get download` + `dpkg-deb -x` 本地提取，零 root）

Q3-3 先例（BUILD-VERIFY.md「仓外 optroot 提取树」）同模式。

| 桥接层 | 内容 | 用途 |
|---|---|---|
| `/home/z/optroot/sysroot` | libxkbcommon-dev/0、libxkbcommon-x11-dev/0、libwayland-dev、libxkbfile-dev、libx11-xcb-dev、libasound2-dev、libxcb-xkb1（共 9 deb） | mang-shell 链接 + 运行时（`LIBRARY_PATH`/`LD_LIBRARY_PATH`/`CPATH`） |
| `/home/z/optroot/mesa` | libegl1、libegl-mesa0、libgl1-mesa-dri（swrast/llvmpipe）、libvulkan1、mesa-vulkan-drivers（lavapipe ICD） | wgpu 渲染后端软渲（`VK_DRIVER_FILES=lvp_icd.json`） |
| `/home/z/optroot/pdfium` | chromium/8086 linux-x64 `libpdfium.so`（bblanchon/pdfium-binaries，与 CI #223 同钉扎轨） | PDF 引擎运行时 |
| `/home/z/optroot/tools` | desktop-file-utils、xdg-utils、shared-mime-info（3 deb） | .desktop 校验 + MIME 查询 |
| wasm-bindgen CLI | 0.2.129 官方预编译 x86_64-musl（与 Cargo.lock 钉扎一致） | web bundle 构建 |

## 三、验证口径

- 「安装」= `dpkg-deb -x` 解包到本地根 + 解包树内 `/usr/bin/mdcore` 实跑 +
  `XDG_DATA_DIRS` 指向解包根跑 xdg-mime 查询 + postinst 在解包根实执行。
  **真实 `dpkg -i` 到系统路径未执行**（无 root）——与 Q3-3「干净容器安装测试」
  同一阻塞，命令已在 BUILD-VERIFY.md §1 备档。
- 「X11 能跑」= Xvfb 虚拟帧缓冲 + llvmpipe（Vulkan type=Cpu）软件渲染。
  非真实桌面会话：无窗口管理器（应用日志如实出现
  `no compositor present, falling back to server-side window decorations`）、
  无真实 GPU。**真实 GPU 桌面上的行为未验证**。
- Wayland / fcitx / ibus：**未测**（沙箱无 compositor、无 IME 框架）。
