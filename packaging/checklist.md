# 分发合规核对单（R24-c；发布前逐项打勾，未全勾不得出包）

> 每次正式发布（含公测）逐项核验。勾选记录随发布 tag 归档
> （`packaging/release_<version>.md`）。门禁失败的项禁止跳过——
> 阻塞项走 TRADEMARK_RULES / COMPLIANCE 的对应流程。

## A. 命名与商标（TRADEMARK_RULES）

- [ ] A1 正式商品名已定名，三件套一致（商品名 / 域名 / 仓库组织名）
- [ ] A2 商标检索五步完成（§4：中国商标网 9/42 类 / WIPO / common-law /
      域名与账号），检索记录归档 `trademark_search_<date>.md`
- [ ] A3 定名后复跑 `bash compliance/scan.sh --all` 零命中（新名与四参考
      对象无近似混淆）
- [ ] A4 `{{APP_NAME}}` 占位符全量替换（模板 + 代码内显示名单一来源），
      packaging/README.md 变量表已登记决议
- [ ] A5 法律声明区归属句在位（TRADEMARK_RULES §2.5 通用归属句）

## B. 版本与产物

- [ ] B1 版本单源：`根 Cargo.toml [workspace.package].version` 为唯一
      手写点；`r24_packaging.rs` 一致性锁绿
- [ ] B2 发布产物版本 = `mdcore::VERSION`（deb control / NSIS / Info.plist /
      AppImage 元数据四处注入，无手抄）
- [ ] B3 tag 纪律：`v<VERSION>`，与 Cargo.lock 一起冻结（`--locked` 构建）
- [ ] B4 三平台安装 + 首次启动 + 卸载冒烟（R24-d 执行：AppImage/deb、
      NSIS、.app）

## C. 依赖与合规（COMPLIANCE.md §4 常驻门禁）

- [ ] C1 `bash compliance/scan.sh` 产品面 0 命中（含 packaging/）
- [ ] C2 `cargo test --workspace` 全绿（语义锁零回归）
- [ ] C3 SBOM 重跑（宿主壳启用 wry/tao 后依赖面变化），
      THIRD-PARTY-NOTICES 版本一致、全白名单
- [ ] C4 wasm32 `cargo check` 通过
- [ ] C5 图标/资产经 ASSET_CHECK 登记（来源可证，无第三方侵权素材）

## D. 平台注册面

- [ ] D1 Linux：`.desktop` 入口 + `text/markdown` mime 关联 +
      `x-scheme-handler/mdcore` scheme 注册（update-desktop-database /
      update-mime-database 刷新验证）
- [ ] D2 Windows：NSIS 安装/卸载注册表面（开始菜单快捷方式 + 默认程序
      注册 + WebView2 Runtime 检测引导）
- [ ] D3 macOS：Info.plist 文档类型（.md）+ URL scheme（mdcore://）声明，
      Gatekeeper 冒烟（未签名时的用户引导文案）

## E. 隐私与安全

- [ ] E1 自定义协议面只服务本地 bundle（mdcore://app 无网络面——
      CONTRACT §2，发布版 webview 网络策略复核）
- [ ] E2 无遥测/无自动上传（v0 产品语义；如后续引入须 ADR + 明示）
- [ ] E3 崩溃转储/日志本地化（不外发）

## F. 发布物料

- [ ] F1 Release notes（版本 + 变更 + 已知问题）
- [ ] F2 校验和（SHA-256 per artifact）随发布公布
- [ ] F3（可选）签名/公证：macOS notarization、Windows Authenticode
- [ ] F4 EULA/许可文本（闭源 EULA 起草 = ADR-C1 挂起项，发布前完成）
