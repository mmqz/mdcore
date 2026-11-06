# Q3-3 构建可行性验证报告（BUILD-VERIFY）

> 范围：**只做技术验证，不发布**（checklist A1/A2 定名与商标流程未完成，
> `{{APP_NAME}}`/mang 命名面零触碰——CI 内测包按既有约定以 `mdcore`
> 工作名出）。验证环境：Debian 13 沙箱（x86_64，无 docker / 无 sudo /
> 无 makensis；GTK3+WebKit2GTK 4.1 开发栈来自仓外 optroot 提取树）。
> 日期：2026-10-28 · 版本单源 `mdcore::VERSION` = **0.1.0**（#29 任务 3）
>
> **2026-11-06 更新（#228 R4·P0 打包链修复）**：§3 Windows NSIS 由
> 「阻塞登记」更新为「链路就位」（本节内含三断链处置与实机复核清单）；
> §4 macOS 构建路径已由 #228 第二支（feat/packaging-macos）落地（脚本
> 就位，实体构建待 Lead/macOS 复核）。

---

## 1. Linux deb ✅（构建 + 包体检验 + GUI 启动全过）

**命令**（可复现）：

```bash
cargo build -p mdshell-native --release --locked   # 壳二进制（tao+wry+GTK3）
cd web && ./build-wasm.sh && ./build-pdf-wasm.sh && npm run build  # 完整 bundle
cd .. && packaging/deb/build-deb.sh target/release/mdcore
```

**产物**：`build/mdcore_0.1.0_amd64.deb`（2,516,700 B，2.4 MiB）
SHA256 `fb7bd0ee165c14d57c3b0f84dfff957c3de9c3c81589059d3a3bc46c983b1dae`

| 检验面 | 结果 |
|---|---|
| 版本单源 | `--version` → `mdcore 0.1.0`（注入 control，无手抄）✅ |
| Depends 声明 | `libgtk-3-0 (>= 3.24), libwebkit2gtk-4.1-0, libc6 (>= 2.34)` ✅ |
| 文件布局 | `/usr/bin/mdcore` + `/usr/share/mdcore/dist`（含 pkg_pdf）+ `.desktop` + 图标 ✅ |
| postinst | `bash -n` 语法过 ✅ |
| Installed-Size | 构建脚本实测注入（非手抄）✅ |
| **GUI 启动** | Xvfb 下窗口存活 ≥8s（GTK + wry webview + 包内 bundle 全链）✅ |

**启动验证的环境口径**（诚实登记）：沙箱系统无 webkit2gtk 运行时，
以 optroot 库 + spawn 路径垫片（webkit 助手路径编译为系统绝对路径，
沙箱 /usr 不可写）桥接。**正式安装环境无此问题**——助手随
`libwebkit2gtk-4.1-0`（Depends 已声明）落位在编译路径上。

**验证发现并修复的缺陷**（本轮修复，见 git log）：
1. control.tpl 的 EULA 注释缩进在 Description 块内 = 合法续行文本，
   `grep -v '^#'` 剥不掉 → **注释裸进包**（已移到字段区外）；
2. desktop 模板的内部工作备注（定名过程注记）随包分发（已加
   `sed '/^#/d'` 剥离）。

**阻塞项**：干净容器安装测试（`docker run ubuntu` + `dpkg -i`）——
**沙箱无 docker、无 sudo**，无法执行。包体本身三面已验（control /
contents / stage 解包运行）；建议 CI 或任一有 docker 的环境补跑：
`docker run --rm -v $PWD/build:/b ubuntu:latest sh -c 'apt update && apt install -y /b/mdcore_0.1.0_amd64.deb && timeout 5 mdcore --version'`

---

## 2. Linux AppImage ✅（构建 + 启动过——修复三个阻断级缺陷后）

**命令**：

```bash
mkdir -p build/_tools && cd build/_tools    # 工具下载（APPIMAGE_EXTRACT_AND_RUN=1 免 FUSE）
curl -sSfL -O https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
curl -sSfL -O https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
curl -sSfL -o linuxdeploy-plugin-gtk https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh
chmod +x * && cd ../.. && PATH="$PWD/build/_tools:$PATH" APPIMAGE_EXTRACT_AND_RUN=1 \
  packaging/appimage/build-appimage.sh target/release/mdcore
```

**工具版本钉**（实测，2026-10-28）：
- linuxdeploy `continuous` = 1-alpha git **07333c6**（build 369，2026-10-01）——
  脚本旧钉 `1-alpha-20250106` 的 release 资产已 **404**（已更新钉注释）
- appimagetool `continuous` = git **854e19e**（build 313，2026-10-04）
- linuxdeploy-plugin-gtk `master` 脚本（运行时自取二进制）

**产物**：`build/mdcore-0.1.0-x86_64.AppImage`（105,388,536 B，100.5 MiB——
gtk 插件捆绑完整 GTK/WebKit 运行时所致）
SHA256 `b71580d61aa871aadc75d1d5554a4e8e1db7b29240ae4adb4f3a6d34bfce86d5`

| 检验面 | 结果 |
|---|---|
| `--appimage-extract` 解包 | 结构完整（AppRun/AppRun.wrapped/usr 树/198 .so）✅ |
| AppRun exec 路径 | `usr/bin/mdcore`（占位符已注入）✅ |
| **GUI 启动** | 解包后 `./AppRun` 窗口存活 ≥11s（bundle+GTK+WebKit 全链）✅ |

**验证发现并修复的缺陷**（本轮修复，前两个为**任何机器上都启动失败**的
阻断级）：
1. **AppRun 的 `{{APP_EXEC}}` 占位符不替换**（原脚本 install 直拷模板；
   linuxdeploy 把它裹成 AppRun.wrapped → exec 字面量 ENOENT）；
2. **web/dist bundle 完全不装**（壳的三候选解析序在 AppImage 布局下
   全落空 → 启动即 exit 2；现镜像 deb 布局 + AppRun 导出 `MDCORE_DIST`）；
3. 图标占位资产缺失（linuxdeploy `--icon-file` 硬失败；补确定性生成的
   `icon-placeholder.png`——正式资产仍走 ASSET_CHECK / checklist C5）。

**已知限制（登记，非本轮修）**：
- **WebKit 助手进程路径**编译为系统绝对路径
  `/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/`，gtk 插件捆绑了 lib 但不捆
  助手。**真干净系统**（无 webkit2gtk 系统包）上 webview 会 spawn 失败。
  本轮验证以 spawn 垫片桥接证明 AppImage 自身 bundle 可用。后续选项：
  出包后处理把助手拷进 AppDir 同路径 + `patchelf` 改 lib 的编译路径，
  或 README 登记「需系统 webkit2gtk-4.1（届时与 deb 同依赖面）」；
- `libEGL.so.1`（mesa 面）未捆绑——桌面系统普遍存在（沙箱无头镜像缺），
  gtk 插件设计上不收 mesa。

---

## 3. Windows NSIS ✅ 链路就位（#228 R4 修复，2026-11-06 更新）

> Q3-3 原状（阻塞登记）：`makensis` 不在环境、模板未验——本节保留
> 历史事实，修复后链路如下。**安装行为面（注册表/快捷方式/.md 关联/
> 自检弹窗）仍需 Lead Windows 实机复核**（CI 只产包不装包）。

### 三断链处置（审计 P2-37 → #228）

| 断链 | 根因 | 处置 |
|---|---|---|
| 1. `{{VERSION}}` 无注入机制 | 模板占位符零机制；示例 `-DVERSION` 与 NSIS 方言不匹配（Windows 为 `/D`，且模板从不消费 define）→ 字面进 exe 名/注册表 | `packaging/nsis/build-installer.py` 从根 Cargo.toml `[workspace.package].version`（= `mdcore::VERSION` 单源，r24 锁背书）渲染注入；残留占位符 / APPVERSION 不一致 / 产物名不含版本 → 非零退出（链路可验证） |
| 2. WebView2 引导链（URL 定义未用 + `ExecWait` 幽灵 `$TEMP\wv2setup.exe`） | 该面属旧 tao+wry 壳（mdshell-native） | **面移除**：主线壳 mang-shell 走 GPU 自绘渲染（DirectX 后端），无 WebView 运行时依赖（windows-check.yml 对 mdshell-native 的 GTK 排除注释同源）；旧壳为 Linux GTK 面，不随 Windows 安装包分发 |
| 3. `File "..\.."` 与仓根执行示例矛盾 | **NSIS `File`/`OutFile` 按脚本目录解析而非调用 CWD**（本轮探针实证：脚本在 X 目录、cwd=Y 时，`File "a/f"` 找 X/a/f、OutFile 落 X/）——相对路径在「脚本目录 vs CWD」两套语义下天然歧义，旧模板与仓根示例任一单侧假设都必 not-found | 载荷统一 staging（`build/nsis/payload/mdcore.exe` = mang-shell.exe 重命名 + `pdfium.dll`），模板路径一律构建器注入**绝对路径**（as_posix，Windows makensis 同收正斜杠）——任何解析语义/平台零歧义；构建器与 makensis 均以仓根为工作目录（统一口径，双层保险） |

### 新增：安装后部署自检（#224 能力复用）

`Call DeploySmoke` 跑 `mdcore.exe --check-pdfium`（退出码合同：0 = 已
绑定 / 3 = 未绑定）。失败弹窗给修复指引（复跑命令），**不回滚安装**
（Markdown/Word 面不依赖 pdfium）；报告全文进安装日志（详情页）。

### 命令（可复现；Lead Windows 实机正式出包口径）

```bash
# 0) 前置：NSIS 3.x（`choco install nsis -y` 或官网安装）+ python3 + rustup
# 1) 构建主线壳（release）
cargo build --manifest-path mang-shell/Cargo.toml --release --locked
# 2) pdfium 取件（pin chromium/8086，与 windows-test.yml / docs/INSTALL.md 同源）
#    https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8086/pdfium-win-x64.tgz
#    SHA256 1fd8af952832dbb0eb16d9249f68fe09e5f5ebf7c3dd9f6066ea2720cc28487d
#    解出 bin/pdfium.dll 放到 bin/lib/pdfium.dll
# 3) 一键出包（仓根执行；版本注入断言内置）
python packaging/nsis/build-installer.py
# 产物：build/nsis/mdcore-setup-<VERSION>.exe + SHA256 输出
```

### 验证状态

- **本地（Linux，Debian nsis 3.11 + NSISDIR 指路）**：stub 载荷全链绿
  ——版本提取 0.1.0 → staging → 渲染（占位符零残留）→ makensis 编译 →
  产物 `mdcore-setup-0.1.0.exe`（109,192 B，stub）+ SHA256；
- **CI（`windows-installer.yml`）**：debug 内测口径（restore 复用
  test-shell 的 mang-shell 缓存面控分钟数）产 installer artifact；
- **待 Lead Windows 实机复核清单**：安装/卸载走查；注册表
  `DisplayName=mang` / `DisplayVersion=<版本>` / `InstallDir`；开始菜单
  快捷方式；.md 关联；安装后自检弹窗（拔掉 `pdfium.dll` 复跑安装应见
  告警而非静默）。

### 登记边界（诚实面）

- CI 内测包 = **debug** 构建（正式出包走上文 release 命令）；
- mang-shell 为独立 workspace，版本号未接入根 `[workspace.package]`
  单源机制（结构上无法 `version.workspace` 继承）——当前两值一致
  （0.1.0）；若根版本 bump 后漂移，安装包版本以 mdcore::VERSION
  （根单源）为准，mang-shell `--version` 对齐留后续任务；
- NSIS 方言验证 = Debian makensis 3.11（本地）+ choco nsis 3.x（CI）
  双实证；安装行为面无法在 CI 复现（不装包），以实机复核清单收口。

---

## 4. macOS .app ⏳ 脚本就位（#228 任务 4——待 Lead/macOS 环境复核）

> 原状：仅 Info.plist 模板（纸面平台）。#228 第二支（feat/packaging-macos）
> 落地构建路径；本沙箱无 macOS，实体构建按 issue 验收口径由 Lead/macOS
> 复核，如实登记。

**命令**（可复现；Lead/macOS 实机）：

```bash
# 前置：macOS 12+ / rustup / python3（CLT 自带）
bash packaging/macos/build-app.sh
# 产物：build/macos/mang.app + mang-<VERSION>-macos.zip（含 SHA256）
```

**链路**（build-app.sh 单脚本全链）：

1. `cargo build --release --locked`（mang-shell 主线壳，独立 workspace）；
2. pdfium univ 自动取件（pin chromium/8086 mac-univ，SHA256 校验——与
   windows-test.yml 同 release 同 pin 纪律；升级两处同步改）；
3. bundle：`mang.app/Contents/MacOS/mdcore`（= mang-shell 重命名）+
   `libpdfium.dylib` 同束（exe 同目录 = 定位序 exe-dir 候选，#223/#224
   ——与 Windows 安装包 pdfium.dll 同语义）+ Info.plist 渲染；
4. Info.plist：`{{VERSION}}` 单源注入（packaging/version.py）+
   CFBundleIconFile 键剥离（图标资产 = checklist C5 轮）+ 良构断言
   （plistlib + CFBundleExecutable=mdcore / CFBundleName=mang）；
5. ad-hoc 签名（`codesign --force --sign -`；正式签名/公证 = checklist F3）；
6. 部署自检：`--check-pdfium`（#224 复用，退出码非零即脚本失败）；
7. 分发 zip + SHA256（checklist F2）。

**本地（Linux）已验证面**：`bash -n` 语法过；`--render-only` 模板面全链
——版本 0.1.0 注入 ×2、占位符零残留、图标键剥离、plistlib 良构 +
命名域断言过。

**待 Lead/macOS 实机复核清单**：cargo release 构建通过；bundle 布局与
Finder 显示（mang / 芒）；ad-hoc 签名后本机启动；`--check-pdfium` 退出
码 0（同束 dylib 命中 exe-dir 候选）；Gatekeeper 首启引导文案
（checklist D3）；zip 解包可用 + 校验和一致。

---

## 5. 汇总

| 平台 | 构建 | 包体检验 | 启动 | 安装到干净系统 | 产物 |
|---|---|---|---|---|---|
| Linux deb | ✅ | ✅ 四面 | ✅ GUI | ⛔ 无 docker（命令已备） | 2.4 MiB |
| Linux AppImage | ✅ | ✅ | ✅ GUI | ⚠️ webkit 助手路径限制（登记） | 100.5 MiB |
| Windows NSIS | ✅（CI + 本地 makensis 3.11） | ✅ 版本注入断言（#228） | —（CI 不装包） | ⛔ 待 Lead 实机（清单见 §3） | 见 §3 |
| macOS .app | ⏳ 脚本就位（实体待 Lead/macOS，清单 §4） | ✅ plist 良构 + 命名域断言 | ⏳ 待实机 | ⏳ 待实机 | 见 §4 |

**Q3-3 结论**：deb/AppImage 两条构建链在修复 5 个实测缺陷后**可构建、
可启动**；NSIS 链路已由 #228 修复就位（见 §3——安装行为面待 Lead 实机
复核）；macOS 构建路径已落地脚本（见 §4——实体构建待 Lead/macOS 复核）；
docker 安装面阻塞原因明确可复现。定名/商标流程（checklist A1/A2）未
完成，`{{APP_NAME}}`/`mang` 面零触碰，未发布。
