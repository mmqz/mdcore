<?xml version="1.0" encoding="UTF-8"?>
<!-- macOS .app bundle 元数据模板（R24-c；实体验证留 R24-d）
     注入：mang {{VERSION}} {{BUNDLE_ICON}}
     说明：可执行名 = mdcore（自研命名域，TRADEMARK_RULES §1）；
           mang（显示名/Bundle display name）待定名后注入
           （checklist A4）；Bundle ID 三件套随定名复核（packaging/README）。 -->
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>mang</string>
    <key>CFBundleDisplayName</key>
    <string>mang</string>
    <key>CFBundleExecutable</key>
    <string>mdcore</string>
    <key>CFBundleIdentifier</key>
    <string>dev.mdcore.editor</string>
    <key>CFBundleVersion</key>
    <string>{{VERSION}}</string>
    <key>CFBundleShortVersionString</key>
    <string>{{VERSION}}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIconFile</key>
    <string>{{BUNDLE_ICON}}</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <!-- 文档类型：.md 关联（open 事件 → 宿主壳单实例语义 R24-b） -->
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Markdown Document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>net.daringfireball.markdown</string>
            </array>
        </dict>
    </array>
    <!-- URL scheme：mdcore://app（CONTRACT §2 自定义协议面） -->
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key>
            <string>dev.mdcore.editor.scheme</string>
            <key>CFBundleURLSchemes</key>
            <array>
                <string>mdcore</string>
            </array>
        </dict>
    </array>
    <!-- v0 无网络面（checklist E1）；WKWebView 系统 runtime，无豁免声明 -->
    <key>NSAppTransportSecurity</key>
    <dict>
        <key>NSAllowsArbitraryLoads</key>
        <false/>
    </dict>
    <!-- 沙箱：v0 不入 MAS（Mac App Store），声明留 R24-d 实测定稿 -->
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.productivity</string>
</dict>
</plist>
