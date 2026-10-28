# Q3-3 构建可行性验证报告（BUILD-VERIFY）

> 范围：**只做技术验证，不发布**（checklist A1/A2 定名与商标流程未完成，
> `{{APP_NAME}}`/`mang` 命名面零触碰——CI 内测包按既有约定以 `mdcore`
> 工作名出）。验证环境：Debian 13 沙箱（x86_64，无 docker / 无 sudo /
> 无 makensis；GTK3+WebKit2GTK 4.1 开发栈来自仓外 optroot 提取树）。
> 日期：2026-10-28 · 版本单源 `mdcore::VERSION` = **0.1.0**（#29 任务 3）

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

## 3. Windows NSIS ❌ 阻塞（明确原因）

`makensis` 不在环境（Linux 交叉构建 NSIS 需 `nsis` 包；本沙箱无 sudo
不可装；亦无 Windows 环境）。`packaging/nsis/installer.nsi.tpl` 模板在档
未验。解除路径：任一有 `makensis` 的环境跑
`makensis -DVERSION=0.1.0 packaging/nsis/installer.nsi.tpl`（模板的
`{{}}` 注入面届时需先核对一遍——与 AppRun 同类占位符风险）。

---

## 4. 汇总

| 平台 | 构建 | 包体检验 | 启动 | 安装到干净系统 | 产物 |
|---|---|---|---|---|---|
| Linux deb | ✅ | ✅ 四面 | ✅ GUI | ⛔ 无 docker（命令已备） | 2.4 MiB |
| Linux AppImage | ✅ | ✅ | ✅ GUI | ⚠️ webkit 助手路径限制（登记） | 100.5 MiB |
| Windows NSIS | ⛔ | — | — | — | — |

**Q3-3 结论**：deb/AppImage 两条构建链在修复 5 个实测缺陷后**可构建、
可启动**；NSIS 与 docker 安装面阻塞原因明确可复现。定名/商标流程
（checklist A1/A2）未完成，`{{APP_NAME}}`/`mang` 面零触碰，未发布。
