# packaging/ — 打包分发基建（R24-c）

> 产物实体验证留 R24-d（开发环境轮，需宿主壳实体）；本目录交付
> **配置模板 + 版本单源 + 合规核对单**，三平台安装包的构建路径在此
> 落地、命名与品牌面全部参数化。

## 定名阻塞纪律（强制，TRADEMARK_RULES §4）

> ⚠️ **流程缺口**：本文与 `checklist.md` 引用的 `TRADEMARK_RULES`（§1/§2.5/§4）
> **在仓库中不存在**。A2 的近似判定标准、类别范围、驳回处置因此无据可依，需先补齐。

**正式商品名已定：`mang`（中文「芒」），2026-10-27 决议**；但 **A2 商标检索尚未闭合**（见 §定名记录），
故 `{{APP_NAME}}` 占位符**暂不替换**。本目录所有模板以
`{{APP_NAME}}`（对外显示名）占位；可执行名/包名/scheme 用自研命名域
`mdcore`（TRADEMARK_RULES §1 允许域）。**替换 `{{APP_NAME}}` 前**必须：

1. 完成 TRADEMARK_RULES §4 五步检索（中国商标网 9/42 类、WIPO、
   common-law、域名/账号可用性），检索记录归档
   `compliance_release/trademark_search_<date>.md`；
2. 复跑 `bash compliance/scan.sh --all` 确认零命中（新名本身不得与
   四参考对象近似）；
3. 在本 README 变量表登记定名决议（日期 + 三件套一致性：商品名 /
   域名 / 仓库组织名）。

## 变量表（模板注入点）

| 占位符 | 语义 | 当前值 |
|---|---|---|
| `{{APP_NAME}}` | 对外显示名（窗口标题/安装器/桌面入口） | `mang`（**已替换，2026-10-27**） |
| `{{APP_EXEC}}` | 可执行文件名（自研命名域，无需等待定名） | `mdcore` |
| `{{APP_ID}}` | 反向域名标识（macOS bundle / GNOME app id） | `dev.mdcore.editor`（待定名后复核） |
| `{{VERSION}}` | 版本号（构建脚本从 `mdcore::VERSION` 注入，**禁止手抄**） | `mdcore::VERSION`；注入机制 = `nsis/build-installer.py` / `macos/build-app.sh`（#228 起，经 `packaging/version.py` 单源读取，残留占位符即构建失败） |
| `{{BUNDLE_ICON}}` | 图标集根名（ASSET_CHECK 登记后填充） | 待资产轮 |

## 定名记录

| 项 | 值 |
|---|---|
| 对外显示名 | **mang** |
| 中文名 | **芒** |
| 决议日期 | 2026-10-27 |
| 检索记录 | [`compliance_release/trademark_search_2026-10-27.md`](../compliance_release/trademark_search_2026-10-27.md) |
| A2 五步检索 | ⚠️ **降级为风险登记**（所有者决议：不注册域名、不承诺商用 → 不阻塞 A4；**商用/公开发行前必须重跑**） |
| A3 denylist | ✅ 通过（`mang` 与 denylist 全部参考对象词均不近似；判定依据 = compliance/denylist/sources.txt 四源字集） |
| `{{APP_NAME}}` 替换 | ✅ **已执行**（2026-10-27，见下 §A4 执行记录） |

### 三件套一致性（A1）

| 项 | 当前 | 待决 |
|---|---|---|
| 商品名 | `mang` / 芒 | — |
| 域名 | 未取得（`.dev`/`.app`/`getmang.com` 已占，`.com` 待 whois） | 需选定并注册 |
| 仓库组织名 | `mmqz`（现有） | 是否新建 Org `mang` 待定 |

**A1 按「软件显示名」口径闭合**：所有者明确 **不注册域名、不承诺商用**，
故三件套中仅商品名适用；域名与仓库组织名**豁免**（豁免理由记录在案，非遗漏）。

### A4 执行记录（2026-10-27）

`{{APP_NAME}}` 已全量替换为 `mang`，涉及文件：

```
packaging/shared/app.desktop.tpl      Linux 桌面入口 Name=
packaging/nsis/installer.nsi.tpl      Windows 安装器显示名
packaging/macos/Info.plist.tpl        macOS bundle 显示名
packaging/appimage/build-appimage.sh  AppImage 元数据
packaging/deb/build-deb.sh            deb 包元数据
```

同步改写门禁断言：`crates/shell/tests/r24_packaging.rs::naming_gate_documented_before_placeholder_replacement`
（原断言「占位符必须在位」→ 现断言「占位符已替换且显示名 = mang」）。

**未改动项（有意为之）**：

| 项 | 保持 | 理由 |
|---|---|---|
| `{{APP_EXEC}}` | `mdcore` | 自研命名域，与显示名解耦；改可执行名会破坏 CONTRACT/scheme |
| `{{APP_ID}}` | `dev.mdcore.editor` | 反向域名标识；**待复核是否随显示名改为 `dev.mang.editor`**（涉及 macOS bundle 迁移成本） |
| URL scheme | `mdcore://` | CONTRACT §2 冻结面 |

### 已知风险（决议时接受）

- ma- 词族联想（mango / manga / mangle），且 Mango 系产品在协作/文档类目已有占位；
- 中文「忙」歧义；
- npm 裸名 `mang` 已占用（web 包需 scope）；
- 4 字母域名基本售罄。

详见检索记录 §3。

---
## 版本单源（R24-c 机制合同）

```
根 Cargo.toml [workspace.package].version     ← 唯一手写点
  └─ 四 crate version.workspace = true        ← 继承（禁止独立声明）
      └─ mdcore::VERSION（env!("CARGO_PKG_VERSION")）← 运行时唯一出口
          └─ 打包脚本 / 宿主 --version / 关于页   ← 全部取此值
```

跨 crate 一致性由 `crates/shell/tests/r24_packaging.rs` 锁（编译期
`env!` 对比 + 发布语义锚）。

## 目录

```
packaging/
├── README.md            # 本文件（手册 + 定名纪律 + 变量表）
├── checklist.md         # 分发合规核对单（发布前逐项打勾，含商标门）
├── BUILD-VERIFY.md      # 构建可行性验证报告（Q3-3 + #228 更新）
├── version.py           # 版本单源读取器（根 [workspace.package].version →
│                        #   NSIS/Info.plist 注入链共用，#228）
├── shared/
│   └── app.desktop.tpl  # Linux 桌面入口（deb/AppImage 共用）
├── appimage/
│   ├── AppRun           # AppImage 运行时入口脚本
│   └── build-appimage.sh# AppImage 构建（linuxdeploy/appimagetool）
├── deb/
│   ├── control.tpl      # debian/control 模板（依赖含 webkit2gtk-4.1）
│   ├── postinst.tpl     # 安装后钩子（mime/scheme 注册缓存刷新）
│   └── build-deb.sh     # deb 打包（cargo deb 或 dpkg-deb 手工布局）
├── nsis/
│   ├── installer.nsi.tpl# Windows NSIS 安装器模板（#228 重构：mang-shell
│   │                    #   主线化 + pdfium 载荷 + 安装后自检）
│   └── build-installer.py # NSIS 构建器（staging→渲染→断言→makensis，#228）
└── macos/
    └── Info.plist.tpl   # .app bundle 元数据（文档类型 + URL scheme）
```

## 运行时依赖（#228 后：Windows/macOS = mang-shell 主线壳，GPU 自绘渲染；Linux = mdshell-native tao+wry+GTK3，ADR-H1）

- **Linux**（mdshell-native 壳）：`libgtk-3-0`（>= 3.24）、
  `libwebkit2gtk-4.1-0`、`libc6`；AppImage 由 linuxdeploy 携带运行时
  （AppRun 设置 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 兼容部分发行版
  ——R24-d 实测后按需裁剪）。
- **Windows**（mang-shell 壳，#228 起）：GPU 自绘渲染（DirectX 后端）——**无 WebView2
  运行时依赖**（旧 tao+wry 壳的 WebView2 面已随主线壳切换移除，处置
  记录见 BUILD-VERIFY §3）；PDF 面 = `pdfium.dll`，NSIS 安装包捆载
  （exe 同目录 = 定位序候选，安装后 `--check-pdfium` 自检，#223/#224）。
- **macOS**（mang-shell 壳，#228 起构建路径落地）：GPU 自绘渲染（Metal 后端）+
  `libpdfium.dylib` 同束（bundle 内 exe 同目录 = 定位序候选）。
- 详径（定位序/env 覆盖/排障）见 `docs/INSTALL.md`。

## 与门禁的挂接

- `compliance/scan.sh` 的 PRODUCT_PATHS 已纳入 `packaging`（模板词面
  恒受品牌词门禁约束——发布面 = 产品面）。
- 依赖清单变更（宿主壳启用 wry/tao 后）→ 重跑 SBOM + 更新
  THIRD-PARTY-NOTICES（COMPLIANCE.md §4 常驻门禁）。
