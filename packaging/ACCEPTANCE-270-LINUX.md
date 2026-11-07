# #270 桌面平台验收——Linux 面报告（ACCEPTANCE-270-LINUX）

> 验收人：Agent-462f（2026-11-07）。对照 issue #270 清单③ Linux 四项。
> 验收对象：main `7178eba`（2026-11-07）上的**当前产品链**——
> mang-shell（GPUI 壳）release 构建 → `packaging/deb/build-deb.sh` /
> `packaging/appimage/build-appimage.sh` 出包产物。
> 报告纪律：每条标注 **通过 / 失败 / 未测**；失败项已单独开 issue（见 §6（#330-#333））；
> 没环境验证的面如实写「未测」，不假装测过。

---

## 0. 验收环境（诚实登记）

```
沙箱：Debian 13 (trixie) x86_64，无 root / 无 sudo / 无 GUI 会话 / 无 IME
CPU 2 核 4GB RAM；Xvfb 代替 X 显示；Vulkan 走 lavapipe 软件 ICD
（mesa-vulkan-drivers 25.0.7 提取树）；Wayland 用 weston 14.0.2（无头）
与 cage 0.2.0（wlroots 无头）双合成器交叉验证。
构建：rustup 1.99.0（rust-toolchain.toml 钉住版）全量 1195 crate 编译通过
（LIBRARY_PATH 注入用户态 sysroot 的 libxkbcommon/-x11 链接面——
本沙箱系统无 -dev 包，apt download + dpkg-deb -x 用户态自愈，见
BUILD-VERIFY.md 同款做法）。
产物：mdcore_0.1.0_amd64.deb（48,462,368 B）+ mdcore-0.1.0-x86_64.AppImage
（84,851,192 B）。
```

**真实安装未执行**：沙箱无 root，`dpkg -i` 不可用。安装面以
`dpkg-deb -c/-I/--fsys-tarfile` 包体检验 + `apt-get install --simulate`
依赖解析推演 + 解包树等效 postinst 三面替代（BUILD-VERIFY §1 同口径），
已在每条中如实标注。

---

## 1. Linux 清单逐条结果

### ① `.deb/.AppImage 安装` —— **通过（带 2 个失败子项，见 §6（#330-#333））**

| 检验面 | 结果 | 证据 |
|---|---|---|
| deb 构建（未改脚本，按可复现命令） | **通过** | `build-deb.sh /path/mang-shell` → `mdcore_0.1.0_amd64.deb`；版本单源 `mang-shell 0.1.0` → control `Version: 0.1.0` 注入正确 |
| deb 包体三面（control/contents/布局） | **通过** | `/usr/bin/mdcore` + `.desktop` + 图标 + `/usr/share/mdcore/dist`（web bundle）齐备；`Installed-Size: 321576` 实测注入；postinst `bash -n` 过 |
| postinst 等效执行 | **通过** | 解包树上 `update-desktop-database -q` 生成 `mimeinfo.cache`：`text/markdown=dev.mdcore.editor.desktop` + `x-scheme-handler/mdcore=…` 两行正确 |
| deb **依赖正确性** | **失败 → issue #330（P0）** | 二进制 ELF NEEDED = `libxcb.so.1`/`libxkbcommon.so.0`/`libxkbcommon-x11.so.0`，control 却声明 `libgtk-3-0, libwebkit2gtk-4.1-0`。`apt-get install --simulate` 实测：**拖入 43 包无关 GTK/WebKit/GStreamer 栈，而实际所需 3 个库 0 个被拉**——干净系统装完即 `libxkbcommon-x11.so.0: cannot open shared object file`（本沙箱以缺该库的系统状态实测复现同一报错） |
| AppImage 构建 | **通过（带 1 个环境卡点）** | 无 GTK dev 的环境 `--plugin gtk` 因无法自检版本硬失败（`DEPLOY_GTK_VERSION=3` 手设后通过）；产物 84.8MB。linuxdeploy 按 ldd 自动布署的恰好是 GPUI 真实依赖（libxkbcommon/libxcb-xkb 等 4 库）——**GTK 插件捆绑的整套 GTK3 运行时（约 1/4 体积）对 GPUI 二进制是死重** → issue #331 |
| AppImage 结构与自足性 | **通过** | `--appimage-extract` 结构完整；**清空 LD_LIBRARY_PATH 直跑 `usr/bin/mdcore`**（RPATH 解析捆绑库）→ X11 窗口创建、Vulkan 适配器枚举均到达（surface 后因测试环境 ICD 相对路径伪失败，换绝对路径 ICD 后全链通过，见 ④） |
| 安装包真机 `dpkg -i` | **未测** | 无 root；已给 CI/真机复跑命令（§5） |
| 升级安装保留设置 | **未测** | 同上（依赖真机） |

### ② `.desktop 文件 + MIME 关联` —— **通过（带 3 个失败子项，见 §6（#330-#333））**

| 检验面 | 结果 | 证据 |
|---|---|---|
| `desktop-file-validate`（desktop-file-utils 0.28） | **通过** | 包内 `dev.mdcore.editor.desktop` 0 error 0 warning |
| `.md` → MIME 类型判定（桌面真实路径 gio/GLib） | **通过** | `gio info` 实测 `standard::content-type: text/markdown`（shared-mime-info glob）；注：`file`/`xdg-mime query filetype`（魔数路径）报 `text/plain`，非关联链缺陷 |
| MIME → 应用关联 | **通过** | `xdg-mime query default text/markdown` → `dev.mdcore.editor.desktop`（mimeinfo.cache 生效）；`gio mime text/markdown dev.mdcore.editor.desktop` 设默认成功且 Registered applications 可见（**前提 `/usr/bin/mdcore` 在 PATH——TryExec 命中**；真机安装后恒成立） |
| Desktop Actions（new-window / open-settings） | **失败 → issue #332** | `main.rs` CLI 面只有 `--version/-V/--check-pdfium/--smoke-*`；实测 `--settings`、`--new-window` **被静默忽略**（正常开窗退出 0，无设置面板无新窗）——菜单项点了等于没点 |
| `StartupWMClass=mdcore` 与窗口实况 | **失败 → issue #332** | xprop 实测窗口 **完全不设置 WM_CLASS**（ICCWM 违约；GNOME/KDE 无法把窗口映射到 desktop entry，任务栏图标/单实例归组受损）；`WM_NAME = "mang"` 与 desktop `Name=mdcore` 亦不一致 |
| `.docx`/`.pdf` 关联 | **缺失（跨平台一致性）** | desktop 只声明 `text/markdown;x-scheme-handler/mdcore`；NSIS 侧同样只注册 `.md`（HKCR）。#270 Windows 面要求「双击 .docx/.pdf 用 mang 打开」——两侧都未设计 → 并入 issue #C |
| 图标 | **通过（占位图）** | hicolor 256x256 落位；仍为 `icon-placeholder.png` 确定性生成图（正式资产 checklist C5 未勾，已知项不重复开） |

### ③ `fcitx/ibus 中文输入` —— **未测（沙箱无 IME），周边面 3 项通过**

无 GUI 会话即无输入法 daemon，**候选框定位、中文输入法下快捷键、输入过程**三件在本环境不可验证，不装样子。

可验证的周边面（均为**通过**，作为旁证不替代真机 IME 验收）：

| 旁证面 | 结果 | 证据 |
|---|---|---|
| CJK 字形渲染 | **通过** | 系统在位 Sarasa Mono SC 族 15 face；`mouse_smoke.md`（中日混排语料）全周期渲染无豆腐块，选区快照 stage2-drag 可见高亮 |
| 中文文本编辑管线 | **通过** | 鼠标烟雾拖选断言：`selection 31:11..52:1 (577 chars) [文混排 alignment 检查行，Sarasa]`（577 混排字符选中、自动滚动到 row 52） |
| XIM 代码路径在位 | **通过** | gpui_linux x11 特性链编译进 zed-xim（构建面证明）；无 XIM server 时应用正常启动（不因 IME 缺席崩溃） |

**真机 IME 验收移交**：需 fcitx5/ibus 会话的 GUI 真机（Lead 或任一有桌面环境的代理）。

### ④ `Wayland / X11 都能跑` —— **双通过（各带 1 个缺陷发现）**

**X11：通过。** Xvfb 1600x1000 下 deb 二进制 `--smoke-sec 8` + 真实 md 文档：
exit 0 + `smoke: quit reached`；状态栏 `recent: 1 文件`；RPC 域套接字就位；
1280x800 窗口树在位（`xwininfo`）；窗口截图 1280x800/17,185 色/101:200 行有内容
（非空屏）；GPU 走 lavapipe（`Selected GPU adapter: "llvmpipe (LLVM 19.1.7,
256 bits)" (Vulkan)`）。

**鼠标交互五阶段（AppImage 产物）：通过。** `--smoke-mouse` 全序列
（单击→拖选→双击→三击→滚轮→出界拖拽自动滚动）exit 0，31 条事件日志；
四阶段定时截图落盘且 stage1↔stage2 内容差异 bbox (414,202)-(1042,370)
（真实选区高亮，非重放静态帧）。

**Wayland：通过（cage 正向 + weston 异常面）。**
- 正向：cage 0.2.0（wlroots，WLR_BACKENDS=headless）下 AppImage `--smoke-sec 16`：
  **exit 0** + `smoke: quit reached`——wl_seat 绑定、xdg toplevel、Vulkan 适配器
  选择、编辑器全周期在真实 Wayland 协议栈上走通。
- 异常面（**失败 → issue #333**）：weston headless/kiosk（无输入设备、
  `wayland-info` 实测 globals **不含 wl_seat**）下启动 →
  **panic exit 101**（`gpui_linux platform.rs:381 Wayland compositor does not
  provide wl_seat`）。对照 GPU 失败路径已有的优雅降级（exit 70 + 双语提示 +
  对话框兜底 #185），seat 缺失应是同类降级而不是裸 panic。真桌面合成器
  恒有 seat，故定 P3；但 kiosk/嵌入场景会踩中。

---

## 2. 与旧壳（mdshell-native）验收基线的差异说明

BUILD-VERIFY.md（2026-10-28）的 deb/AppImage 验证对象是**旧 tao+wry+GTK3 壳**
（`mdshell-native`，产物 `target/release/mdcore`，deb 2.4MB）。本轮是**GPUI 壳
（mang-shell）首次走完 Linux 打包链**，出包产物 48MB（deb）/ 84.8MB（AppImage），
体积与依赖面的变化全部源于换壳，缺陷清单见 §6（#330-#333）。

## 3. 沙箱验收方法论（可复现命令）

```
构建链（全部未改脚本）：
  mang-shell: CARGO_TARGET_DIR=… cargo build --release --locked
  wasm+web:   web/build-wasm.sh && web/build-pdf-wasm.sh && (cd web && node build.mjs)
  deb:        packaging/deb/build-deb.sh <mang-shell 二进制>
  appimage:   DEPLOY_GTK_VERSION=3 APPIMAGE_EXTRACT_AND_RUN=1 \
              packaging/appimage/build-appimage.sh <二进制>
依赖推演：    apt-get install --simulate --no-install-recommends ./mdcore_0.1.0_amd64.deb
desktop 链：  desktop-file-validate → update-desktop-database（解包树）→
              gio mime / xdg-mime（XDG_DATA_DIRS 指向解包树）
X11 启动：    Xvfb :N -screen 0 1600x1000x24 + VK_DRIVER_FILES=<lavapipe ICD 绝对路径> \
              + --smoke-sec N [FILE]（判定 exit 0 + "smoke: quit reached"）
鼠标五阶段：  --smoke-mouse（AppImage 产物，31 事件断言 + 定时截图）
Wayland：     cage：WLR_BACKENDS=headhead WLR_RENDERER=pixman cage -- AppRun --smoke-sec N
              weston：--backend=headless-backend.so --shell=kiosk-shell.so
              （模块路径用 WESTON_MODULE_MAP 映射用户态 sysroot）
截图链：      xwd → xwdtopnm → pnmtopng（sysroot netpbm；须带其 LD_LIBRARY_PATH）
```

取证产物（本报告引用的全部日志/截图）在本机
`/home/z/my-project/acceptance/`：`app_launch.log`、`wininfo.txt`、
`wmclass.txt`、`window_all_props.txt`、`screenshot_x11_window.png`、
`mouse-smoke/stage{1..5}-*.png`、`app_wayland*.log`、`wayland_globals.txt`、
`appimage_build.log` 等；分支内不含（体积/非代码资产），PR 合并后按需补传。

## 4. 平台间状态

| 平台 | 状态 | 说明 |
|---|---|---|
| Linux | 本报告 | 四项中三项实测通过（一子项未测=真机 IME），缺陷 4 件开 issue |
| Windows | **未验证（Lead 真机）** | Lead 有 Win11；NSIS 面代码审阅补充：安装器只注册 `.md`（HKCR）→ #270 的「双击 .docx/.pdf」在 Windows 同样未设计；pdfium.dll 有捆绑（与 deb 不对称） |
| macOS | **无法验证** | 无环境（packaging/macos/Info.plist.tpl 在位，无 CI 构建面）；不装样子 |

## 5. 移交与建议

1. **真机 IME**：fcitx5/ibus 候选框定位 + 中文输入 + 快捷键不吞——需 GUI 真机（Lead）。
2. **真机安装**：干净容器 `docker run --rm -v $PWD/build:/b ubuntu:latest sh -c 'apt update && apt install -y /b/mdcore_0.1.0_amd64.deb && timeout 5 mdcore --version'`（修完 #330 的 Depends 后才有意义）。
3. 升级保留设置 / 卸载干净：deb 侧无配置文件迁移设计（settings.json 落盘于何路径需产品定），建议并入 #A 修复一并定义。

## 6. 缺陷 issue 清单（本轮新开）

| Issue | 标题 | 级别 | 一句话根因 |
|---|---|---|---|
| #330 | deb Depends 面向已退役的 GTK3 壳——GPUI 二进制装完即缺库 | **P0** | `control.tpl` 声明 libgtk-3-0/libwebkit2gtk-4.1-0；实际 NEEDED 是 libxkbcommon0/libxkbcommon-x11-0/libxcb1（apt --simulate：43 包无关依赖、0 个命中） |
| #331 | 打包链三处旧壳残留：web/dist 硬依赖 + GTK 插件死重 + pdfium 不对称 | P1 | mang-shell 不消费 web/dist（源码零引用）却硬失败；gtk 插件捆 ~20MB 无用运行时且无 GTK dev 环境构建必败；NSIS 捆 pdfium.dll 而 deb 不捆 libpdfium.so（Linux PDF 面装后 `--check-pdfium` 实测 exit 3） |
| #332 | .desktop 三缺陷：Actions 假参数 + 窗口无 WM_CLASS + docx/pdf 未关联 | P2 | `--new-window/--settings` 未实现被静默忽略；窗口不设 WM_CLASS（StartupWMClass=mdcore 落空）；双平台都只注册 .md |
| #333 | Wayland 无 seat 时 panic 101 而非优雅降级 | P3 | `gpui_linux platform.rs:381` unwrap；对照 GPU 失败路径（exit 70+双语提示）应同类降级 |

另：release 二进制带 `debug = "limited"` 直接入包（306MB/安装面 314MB），
建议 #A/#B 修复时一并 `strip` 或出包专用 profile——并入 #B，不单开。
