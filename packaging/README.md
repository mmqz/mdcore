# packaging/ — 打包分发基建（R24-c）

> 产物实体验证留 R24-d（开发环境轮，需宿主壳实体）；本目录交付
> **配置模板 + 版本单源 + 合规核对单**，三平台安装包的构建路径在此
> 落地、命名与品牌面全部参数化。

## 定名阻塞纪律（强制，TRADEMARK_RULES §4）

**正式商品名未定**（用户决策项 + 商标检索强制门）。本目录所有模板以
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
| `{{APP_NAME}}` | 对外显示名（窗口标题/安装器/桌面入口） | **待定名**（见上） |
| `{{APP_EXEC}}` | 可执行文件名（自研命名域，无需等待定名） | `mdcore` |
| `{{APP_ID}}` | 反向域名标识（macOS bundle / GNOME app id） | `dev.mdcore.editor`（待定名后复核） |
| `{{VERSION}}` | 版本号（构建脚本从 `mdcore::VERSION` 注入，**禁止手抄**） | `mdcore::VERSION` |
| `{{BUNDLE_ICON}}` | 图标集根名（ASSET_CHECK 登记后填充） | 待资产轮 |

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
│   └── installer.nsi.tpl# Windows NSIS 安装器（含卸载器 + 快捷方式）
└── macos/
    └── Info.plist.tpl   # .app bundle 元数据（文档类型 + URL scheme）
```

## 运行时依赖（tao+wry+GTK3 架构，ADR-H1）

- **Linux**：`libgtk-3-0`（>= 3.24）、`libwebkit2gtk-4.1-0`、`libc6`；
  AppImage 由 linuxdeploy 携带运行时（AppRun 设置 `WEBKIT_DISABLE_COMPOSITING_MODE=1`
  兼容部分发行版——R24-d 实测后按需裁剪）。
- **Windows**：WebView2 Runtime（Evergreen，NSIS 模板含检测 + 引导下载）。
- **macOS**：系统 WKWebView（无额外运行时；Info.plist 声明沙箱豁免面
  由 R24-d 实测定稿）。

## 与门禁的挂接

- `compliance/scan.sh` 的 PRODUCT_PATHS 已纳入 `packaging`（模板词面
  恒受品牌词门禁约束——发布面 = 产品面）。
- 依赖清单变更（宿主壳启用 wry/tao 后）→ 重跑 SBOM + 更新
  THIRD-PARTY-NOTICES（COMPLIANCE.md §4 常驻门禁）。
