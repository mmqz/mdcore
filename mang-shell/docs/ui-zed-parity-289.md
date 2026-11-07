# #289 UI 走样对照表——zed 实现 | mang-shell 实现 | 差异 | 处置

> 交付要求 ①：「每个元素 | zed 实现 | 我们的实现 | 差异 | 修法」。
> 本文按六缺陷逐项对照，引用行号以本 PR 基线为准。
> zed 侧引用 = 本仓 vendored 快照 `third_party/zed/`（commit
> `1663af41`，见 PROVENANCE.md）。

## 缺陷② 标题栏「mang」占位 vs 菜单栏位置

| 面 | zed（title_bar.rs 上游 workspace 布局） | mang-shell（旧） | mang-shell（本 PR） |
|---|---|---|---|
| 布局序 | `[菜单栏(左缘)] [拖拽区] [窗口控制]`——Windows 版菜单贴左缘，应用名不出现在左上角 | `[「mang — 文件」标题列] [菜单栏] [拖拽区] [控制]`——应用名占了菜单位 | 同 zed：菜单贴左缘；文档标题居中在拖拽区（vscode 同款） |
| 窗口标题 | SetWindowText 承载（任务栏/Alt-Tab） | 同 | 不变（main.rs window title） |

**修法**：`title_bar.rs` 重排（菜单先行 + 居中标题 Label）；`menu_bar.rs`
的键盘锚点估算 `estimate_anchor_x` 去掉标题宽度项（`TITLE_LEFT_PAD`/
`TITLE_TO_MENUS_GAP` 废弃）。**回归门**：`anchor_estimate_monotonic`
单测新增「标题不影响锚点 + 首菜单锚点 = 自身左内边距」两断言。

## 缺陷③ 侧栏「透明按钮」

| 面 | zed | mang-shell（旧） | mang-shell（本 PR） |
|---|---|---|---|
| 图标资产源 | `assets/icons/*.svg` 全量在仓（500+ 枚） | 内嵌 15 枚（`assets.rs::ICONS`）——**`git_branch`/`git_commit`/`plus`/`list_tree` 四枚被引用但未内嵌** → git 钮/文档结构图标/设置步进钮渲染为空白（alpha mask 为空 = 透明） | 从上游同 commit 补齐四枚（`assets/icons/` + `ICONS` 表），口径与其余 15 枚一致 |
| 资产登记 | icons crate 枚举 ↔ 文件名 snake_case 映射 | 手工表——**枚举与表可漂移（本事故根因）** | 手工表 + **回归门**：`every_shell_icon_has_embedded_asset` 单测把「src/ 全部 IconName 用法集合」钉进 `SHELL_ICON_SET`，缺一枚即红 |

**根因链**：`Icon::new(IconName::GitBranch)` → `icons.rs path()` = 
`icons/git_branch.svg` → `Assets.load()` 返回 `None` → `Svg` 元素
paint 空 → 按钮背景照画、图标区域空白 = 用户看到的「透明按钮」。

## 缺陷⑤ 左侧边栏成了「右侧按钮的详情菜单」

| 面 | zed/vscode | mang-shell（旧） | mang-shell（本 PR） |
|---|---|---|---|
| 左图标条语义 | **视图切换器**——只管左栏视图（资源管理器/搜索/源代码管理）；右栏开关不在左条 | 文件树/git/搜索 + **设置钮（驱动右栏切到设置视图）** + **右栏开关（底部）**——左条按钮开右栏面板，语义错位 | 文件树/git/搜索（+底部设置钮开**弹窗**）——左条只管左栏 |
| 设置入口 | zed：标签页；vscode：齿轮弹菜单 → 设置窗格 | 右栏视图之一（与属性/来源互斥） | **模态弹窗**（`settings_modal`——zed「设置是独立界面不是 dock 视图」同款语义；Esc/点遮罩/关闭钮收起） |
| 右栏开关 | zed：右缘折叠 dock 的按钮列（dock.rs `DockButton`）；vscode：面板自身标题栏 | 左图标条底部 | **右缘 dock 条**（`right_dock.rs`，52px 恒在——zed 折叠 dock 同构） |

**修法**：`activity_bar.rs` 收口为左栏视图切换器（基线 4 钮）；新增
`right_dock.rs`（`dock-toggle-right`）；`settings_panel.rs` 改模态弹窗
（内容四项控件不动——theme_mode/font_size/indent_unit/panel_widths
全部沿用既有 ShellApp 生效链）；`shell.rs` 状态 `settings_view: bool` →
`settings_open: bool`（模态）。**回归门**：`face3_panel_toggle_changes_state`
钉新接线（含「act-toggle-right 不得在左条」负断言）；
`activity_bar_no_dead_buttons` 扩展扫描 right_dock.rs。

## 缺陷④ PDF 打开只有文字没有格式（最严重）

| 面 | zed（pdf 预览无内建——以图片渲染路径为参照） | mang-shell（旧） | mang-shell（本 PR） |
|---|---|---|---|
| 打开 PDF | n/a（zed 用外部插件） | `pdf_doc.rs`：pdfium `extract_text` → **文本行模型**（像 md）——表格/图片/字体全丢 | **页面位图视图**（`pdf_pages.rs`）：pdfium `Rasterizer` 光栅化每页 → `img` 元素竖排——版面/表格/图片/字体原样呈现 |
| 渲染线程 | — | n/a | `RenderWorker`（mdc-pdf 既有：专用线程 + panic 隔离 + 看门狗换代 20s）——UI 不卡帧 |
| 页缓存 | — | n/a | `PageCache` LRU 192MB，键 `(页, 64px 量子化宽)` |
| 虚拟化 | — | n/a | `uniform_list`（可见页 ±1 过扫描懒光栅——百页档不爆内存） |
| 文本能力（查找/书签/属性） | — | 有（文本模型） | **保留**——两视图可切（状态栏「页面/文本」钮；Ctrl+F 页面视图下自动切文本 + 提示）；书签点击在页面视图下随页滚动 |
| 扫描件 | — | 「OCR 不支持」提示行挡内容 | 页面视图下**正确呈现**（位图即内容） |

**修法**：`PdfPagesView` 实体（`ShellApp.pdf_pages` 持有，随 pdf 标签
切换重建/关闭丢弃）；`editor_area` 按模式分派；`Editor::pdf_page_for_line`
行号→页号锚；`panels.rs` 书签点击双锚（行 + 页）。**回归门**：
`quantize_width`/`slot_height`/`fit_width`/`rendered_page_to_image`
纯函数单测（CI 无 libpdfium 同样真跑）。

## 缺陷① 按钮点击不好使 / 点「打开文件」没反应

逐类按钮与 zed `ui::Button`/`IconButton` 对照（`third_party/zed/crates/ui
/src/components/button/`）：

| 元素 | zed 实现 | mang-shell 实现 | 对照结论 |
|---|---|---|---|
| 菜单标题（文件/编辑…） | `on_mouse_down` 捕获锚点 | 同（`menu_titles` on_mouse_down + `cx.listener`）| **接线等价** |
| 菜单条目行 | `on_click` + hitbox | 同（`action_row`：`.id()` 生成 hitbox + enabled 才挂 on_click）| **接线等价** |
| 图标条按钮 | `IconButton::on_click` | 自绘 `bar_button`（`.id()` + on_click + tooltip）| **接线等价**——但 ③ 的缺图标使部分钮**看起来**像死的（已修） |
| 窗口控制钮 | `IconButton` | 同 | **接线等价** |
| 打开文件链 | 动作 → `prompt_for_paths`（OS 对话框） | `MenuAction::OpenFile` → `prompt_open_file` → `cx.prompt_for_paths`（Windows IFileOpenDialog）| **接线等价**；失败路径走提示横幅（不静默） |

**代码面未发现接线缺陷**（on_click/hitbox/事件冒泡/灰显门与 zed 同构；
`wiring_integrity`/`ui_wiring_faces` 静态接线门全绿背书）。两点诚实登记：

1. **本环境无法验证「点击手感」**（沙箱无 GPU/无 Vulkan ICD——GPUI
   窗口起不来）。真机复核面留给 Lead（步骤见下）。
2. 用户侧「点了没反应」的一个**可复现候选**是 ③（图标空白 → 视觉死钮）
   与**旧二进制**（`感觉ui没有丝毫改变`——若运行的是安装包旧版，新
   修复全不可见）。建议 Lead 复核时先确认运行的是本 PR 构建。

### Lead 真机复核步骤（Windows 11）

```
1. cargo build --release --manifest-path mang-shell/Cargo.toml
2. ./mang-shell/target/release/mang-shell.exe
3. ② 确认左上角直接是 文件/编辑/查看/帮助（无「mang」字样）
4. ③ 确认图标条 4 钮全部有图标（git 钮为分支图标）
5. ⑤ 点左条设置钮 → 弹窗（非右栏）；右缘条有属性/来源开关
6. ④ Ctrl+O 打开 PDF → 应见页面位图（含表格/图片），状态栏可切文本视图
7. ① 逐钮点击 + 菜单「文件 → 打开」→ 对话框应弹出
```

## 缺陷⑥ 「没有所谓的手工打开，打开不了就是打开不了」

本 PR 的对应处置：所有失败路径**明示而非静默**——页视图引擎失败 →
error 面板；读盘失败/超限 → 提示横幅；Ctrl+F 页面视图 → 自动切文本 +
横幅说明；`every_shell_icon_has_embedded_asset` 把「资产缺位 = 静默透明」
这类静默缺陷钉成测试红。
