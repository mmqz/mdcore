# #270 桌面平台验收 · Linux 沙箱面报告

> 验收人：Agent-ww4y · 日期：2026-11-07 · 分支 `docs/270-linux-acceptance`
> 对象：main@`7178eba` · 载荷 = **主线壳 mang-shell（GPUI/wgpu）**
> 证据目录：`evidence/`（17 件：日志/截图/包体检/哈希/环境清单）
>
> **范围与口径**：本报告只覆盖 issue #270 任务③（Linux）。Windows/macOS
> 需真机（Lead 侧），本环境无法验证（issue「不要假装测过」纪律）。
> 环境边界与桥接方式见 `evidence/env-manifest.md`——**无 root 沙箱**，
> 「安装」= 解包树实跑（Q3-3 同口径升级版）。
>
> **载荷偏离声明**：打包脚本文档口径（BUILD-VERIFY.md §1）以旧壳
> `mdshell-native`（tao+wry+GTK3）为载荷；本报告以**主线产品壳
> mang-shell** 为载荷验收——与 Windows NSIS（#228 后）口径一致。
> 该偏离本身即本报告发现 F1 的实证。

## 一、清单逐项结论（issue #270 任务③原文）

| 清单项 | 结论 | 证据 |
|---|---|---|
| `.deb` 安装 | ✅ 通过（构建+包体+解包树实跑；真实 `dpkg -i` 无 root 未执行，见口径） | `evidence/deb-control.txt`、`deb-contents.txt`、`xvfb-launch-deb-bin-md.log` |
| `.AppImage` 安装 | ✅ 通过（构建⚠️需强制 gtk 版本，见 F4；解包运行+GUI 实跑） | `appimage-build-gtk-autodetect-fail.log`、`appimage-build-forced-gtk3.log`、`xvfb-launch-appimage-docx.log` |
| `.desktop` 文件 | ✅ 通过（desktop-file-validate exit 0；12 键 + 2 动作结构合规） | `packaged-desktop-file.txt`、`mime-and-desktop-validation.txt` |
| MIME 关联 | ✅ 通过（text/markdown 与 x-scheme-handler/mdcore 均解析到本包条目；application/pdf 未被抢注） | `mime-and-desktop-validation.txt` |
| fcitx/ibus 中文输入 | ⛔ **未测**——沙箱无 GUI IME 框架（issue 明示不可用 Playwright 代替，本环境同样不具资格） | — |
| Wayland | ⛔ **未测**——沙箱无 compositor（GPUI wayland 面已链接进二进制，`ldd` 见 libwayland-client/cursor） | — |
| X11 能跑 | ✅ 通过（Xvfb + llvmpipe Vulkan 软渲：裸窗口/.md 编辑器/.pdf 位图视图/.docx 渲染四形态全部实跑成功） | `xvfb-launch-*.log` ×5、`screen-*.png` ×4 |

## 二、验证矩阵（实测命令 → 输出摘要）

### 1. 构建（全部真跑，非转述）

| 步骤 | 命令 | 结果 |
|---|---|---|
| 主线壳 | `cargo build --release`（mang-shell 独立 workspace，CARGO_TARGET_DIR 仓外） | ✅ 35 warnings 0 errors；产物 strip 后 42.5 MB |
| wasm×2 | `cargo build -p mdcore-wasm / -p mdc-pdf-wasm --target wasm32-unknown-unknown --release` + wasm-bindgen 0.2.129 | ✅（⚠️ 脚本硬编码 `target/`，设 CARGO_TARGET_DIR 即断——见 F5） |
| web dist | `npm ci && npm run build`（web/） | ✅ 23 MB（index.html + main.js + pkg/ + pkg_pdf/） |
| deb | `packaging/deb/build-deb.sh <mang-shell>` | ✅ `build/mdcore_0.1.0_amd64.deb` 15,224,884 B |
| AppImage | `packaging/appimage/build-appimage.sh <mang-shell>` | ⚠️ 首跑红（gtk 插件无法自动检测版本）；`DEPLOY_GTK_VERSION=3` 强制后 ✅ `build/mdcore-0.1.0-x86_64.AppImage` 36,915,704 B |

### 2. deb 包体七面

| 面 | 结果 |
|---|---|
| 版本单源 | ✅ `--version` → `mang-shell 0.1.0` → control `Version: 0.1.0`（脚本注入，无手抄） |
| 文件布局 | ✅ `/usr/bin/mdcore` + `/usr/share/mdcore/dist`（工作台 bundle）+ `.desktop` + 256×256 图标 |
| control 合法性 | ✅ dpkg-deb -I 正常解析，无注释裸进包（Q3-3 修复保持） |
| Installed-Size | ✅ 声明 64152 KB vs 解包实测 64156 KB（构建时点测量，一致） |
| postinst | ✅ `sh -n` 语法过 + 解包根实执行 exit 0（update-desktop-database/update-mime-database 真跑；gtk-update-icon-cache 缺席被 `[ -x ]` 门跳过；权限报错为无 root 模拟的预期现象，真实安装以 root 跑） |
| desktop 条目 | ✅ desktop-file-validate exit 0；Exec/TryExec/Icon/StartupWMClass 全指向 `mdcore` 与布局一致 |
| MIME | ✅ `xdg-mime query default text/markdown` → `dev.mdcore.editor.desktop`；`x-scheme-handler/mdcore` 同；`application/pdf` → libreoffice（未被抢注） |

### 3. X11 运行（Xvfb :99 1280×800×24 + llvmpipe）

| 场景 | 结果 | 关键输出 |
|---|---|---|
| 裸启动 | ✅ 窗口创建存活 | wgpu 选中 `llvmpipe (LLVM 19.1.7) (Vulkan, type=Cpu)`；`x11: no compositor present → server-side decorations` 回退正常 |
| 打开 .md | ✅ | `open: smoke.md` → `tabs: open → 1 开（active 0）` → `smoke: quit reached`，退出码 0；截图文本边缘密度 27.4‰（空白屏 <1‰） |
| 打开 .pdf | ✅ pdfium 全链 | `pdf: 2 页 / 457 字符 / 3 书签 / 0 空文本页（只读）`；位图视图渲染在档 |
| 打开 .docx | ✅（AppImage 形态） | `docx: 26 段（可编辑 14/锁定 0）/ 1 表（4 行）/ 5 标题 / 0 警告`；边缘密度 42.5‰ |
| deb 安装位二进制 | ✅ | 解包树 `/usr/bin/mdcore` 同 .md 场景全过 |
| `--check-pdfium` | ✅ | 六层探针输出完整，退出码合同 0/3 符合（#224 能力在 Linux 包形态下成立） |

### 4. AppImage 运行

| 面 | 结果 |
|---|---|
| `--appimage-extract` 解包 | ✅ AppRun/AppRun.wrapped/apprun-hooks/usr 树完整 |
| AppRun exec 链 | ✅ `AppRun --version` → `mang-shell 0.1.0` |
| GUI 实跑 | ✅ docx 场景全过（同上表） |
| dist bundle | ✅ `usr/share/mdcore/dist/index.html` 在位（Q3-3 修复保持） |
| 体积 | 36.9 MB（旧壳 GTK 形态 100.5 MB——不依赖 GTK 的红利） |

## 三、发现（按严重度）

### F1 🔴 Linux 打包面仍停留在旧壳时代——Depends/描述与 mang-shell 实际运行时相悖

**现象**：control `Depends: libgtk-3-0 (>= 3.24), libwebkit2gtk-4.1-0, libc6`；
Description 自述 `native shell (tao+wry+GTK3)`。
**实证**：mang-shell 真实运行时 = libxkbcommon / libxkbcommon-x11 / libxcb-xkb / libX11
（X11 路径）+ wgpu 渲染后端（Vulkan 或 GL 4.3+，桌面系统由 Mesa 提供）+
RPC/async 链。linuxdeploy 对二进制 ldd 实际收进的是
`libxcb-xkb.so.1 / libxkbcommon-x11.so.0 / libxkbcommon.so.0 / libXau / libXdmcp`
（`evidence/appimage-build-forced-gtk3.log`）——**无任何 GTK/WebKit 引用**。
**影响**：① 干净系统 `apt install ./mdcore.deb` 会拉进整套无用 GTK3+WebKit2GTK
（数十 MB 依赖面）；② 真正需要的 `libxkbcommon-x11-0`、`libxcb-xkb1` 只被
`libgtk-3-0` 传递性覆盖（gtk3 链 xkbcommon），任一依赖关系变动即裸奔；
Vulkan/GL 运行时零声明。③ 审计 P2-37 已在 #228 修了 NSIS 面（WebView2 面移除，
理由「主线壳 mang-shell 走 GPU 自绘」），**deb/AppImage 面没同步**。
**建议**：Depends 改为 `libxkbcommon0, libxkbcommon-x11-0, libxcb-xkb1, libc6 (>= 2.34)`
（Vulkan/GL 随桌面 Mesa，是否声明 `libvulkan1 | libgl1` 由 Lead 裁定）；
Description 更新为 GPUI 壳口径。→ 已开 issue（见第四节 F1）。

### F2 🔴 Linux 包不捆 pdfium——Windows 捆了，跨平台不对称

**现象**：NSIS 载荷 = `mdcore.exe + pdfium.dll`（#228，BUILD-VERIFY §3）；
deb/AppImage 载荷零 `libpdfium.so`（`deb-contents.txt` / AppDir `usr/lib` 实证 0 件）。
**影响**：干净 Linux 系统装包后打开 PDF 走降级路径（`--check-pdfium` 报未绑定，
PDF 视图不可用）——而同一产品在 Windows 上开箱即用。#270 Windows 清单「双击
.pdf 用 mang 打开」在 Linux 侧对应能力被包装链掐断。
**建议**：deb 载荷加 `usr/lib/mdcore/libpdfium.so` + 包装脚本导出
`MDC_PDFIUM_LIB`（或 AppRun 导出——exe 同目录层已被 #224 定位序支持）；
来源钉 chromium/8086 linux-x64 与 CI 同轨。

### F3 🟡 desktop 动作声明了二进制不认识的 CLI 参数

**现象**：`Actions=new-window;open-settings` → `Exec=mdcore --new-window` /
`mdcore --settings`；mang-shell `main.rs` 参数面只实现
`--version/-V`、`--check-pdfium`、`--smoke-*` 系（源码 grep 实证）。
**影响**：桌面右键菜单两动作会把未知参数传入——实测 `--version` 之外的未知
flag 不报错（按普通启动处理），行为 = 「New Window」开普通窗口、
「Settings」开普通窗口，与动作名承诺不符（不崩溃但功能虚设）。
**建议**：要么实现 `--new-window`（单实例语义下转发已开实例）与 `--settings`
（打开设置面板——设置弹窗 #289 已有），要么从模板删掉这两动作。

### F4 🟡 AppImage 脚本 `--plugin gtk` 对 mang-shell 载荷失效

**现象**：首跑 `linuxdeploy-plugin-gtk: failed to auto-detect GTK version`（红），
需 `DEPLOY_GTK_VERSION=3` 手工强制才出包；且强制后捆绑的 GTK 运行时
（gtk-3.0/gdk-pixbuf/im-* 输入法模块）对 GPUI 壳完全无用（`usr/lib` 实证）。
**根因**：与 F1 同源——脚本为 tao+wry+GTK3 旧壳设计。
**建议**：mang-shell 载荷路径去掉 gtk 插件（省 ~60% 体积与启动时的 gtk hook）。

### F5 ⚪（工程便利项）wasm 构建脚本硬编码 `target/` 与 AGENTS.md §11 冲突

**现象**：`web/build-wasm.sh` / `build-pdf-wasm.sh` 第 2 步按
`target/wasm32-unknown-unknown/release/...` 定位工件；一旦按 AGENTS.md §11
设 `CARGO_TARGET_DIR`（仓外 target 纪律）即 `No such file or directory`。
**影响**：低（不设变量即可用），但与全仓磁盘纪律相悖，本报告构建时即被绊。
**建议**：`cargo metadata` 或 `$(cargo build ... --message-format)` 定位工件。

### 观察（非缺陷，登记在案）

- **Name=mdcore 而非 mang**：checklist A4 既定约定（定名/商标流程未完成前
  以自研命名域工作名出内测包）——与 Windows NSIS `DisplayName=mang` 的差异
  属已登记的待定名项，不算走样。
- **.docx/.pdf/.xlsx 无 MIME 关联**：desktop 条目只注册 text/markdown（与
  NSIS 行为一致）。#270 Windows 清单期望「双击 .docx/.pdf 用 mang 打开」——
  是否扩 MIME 注册（`application/vnd.openxmlformats-officedocument.*` 等）属
  产品决策，待 Lead 裁定后同步两平台。
- **GenericName=Markdown Editor**：产品已是四格式编辑器（md/docx/pdf/xlsx），
  文案滞后，随定名批次顺改。

## 四、缺陷 issue 登记

| 发现 | issue |
|---|---|
| F1+F4（同根因：打包链旧壳残留——Depends/描述/gtk 插件） | 单独开 issue（Linux 打包面切 mang-shell） |
| F2（pdfium 未捆绑） | 单独开 issue（跨平台不对称） |
| F3（desktop 动作虚设） | 单独开 issue（CLI/desktop 合同对齐） |

## 五、未测清单（诚实面）

| 项 | 原因 |
|---|---|
| fcitx/ibus 中文输入 | 沙箱无 GUI IME 框架——**issue 明示不可代替**，须真机 |
| Wayland 会话 | 无 compositor（二进制 wayland 链接面在，行为未验证） |
| 真实 `dpkg -i` / 干净容器安装 | 无 root / 无 docker（Q3-3 同阻塞，命令已备档 BUILD-VERIFY §1） |
| 真实 GPU 桌面（含高 DPI/多显示器） | Xvfb+llvmpipe 为软渲模拟；窗口管理器行为（装饰/拖动/快捷键冲突）未覆盖 |
| 升级安装（旧→新保留设置） | 无 root 无法多版本装卸对照 |
| mdshell-native 旧壳载荷 | GTK3+WebKit2GTK 开发栈无 root 不可装；旧壳已被 NSIS 面淘汰出 Windows 分发，建议 Lead 裁定 deb 文档口径后一并处理 |
| AppImage FUSE 直跑 | 沙箱无 FUSE（`--appimage-extract` 解包形态代验） |

## 六、结论

**Linux 沙箱可测面 100% 完成且全绿**：主线壳 mang-shell 在 Linux
可构建、可打包（deb/AppImage）、可安装（解包树口径）、可在 X11 启动并
完成 md/pdf/docx 三格式打开-渲染-退出全链；desktop/MIME 注册符合规范。
**四个包装链发现（F1-F4）全部指向同一根因**——Linux 打包模板停留在
mdshell-native（tao+wry+GTK3）时代，而产品壳已在 #228 换代为 mang-shell
（GPUI/wgpu）：Windows 分发面已跟上，Linux 面没跟。修复量级小
（control 依赖行 + 去 gtk 插件 + 补 pdfium 载荷），已在第四节开 issue 跟踪。

IME/Wayland/真机桌面行为仍须 Lead 真机收口（#270 本 issue 不因本报告关闭）。
