# C3 · mang-shell keymap 增量深审（#217/#218，基线 96596d7 → 6322f86）

- 审计对象：`git diff 96596d7..6322f86 -- mang-shell/`（3,455 行插入 / 320 行删除，11 文件）
- 方法：纯静态阅读（mang-shell 在本沙箱无法编译——GPUI 系统依赖闭包），一切行号指 `main@6322f86` 工作树实读，未发明行号
- 关联前轮结论：终稿 Q2 曾警告「当前叠加新能力面会在双轨与门禁失真的地基上继续加高」——keymap 是该警告之后**第一个大新增面**，本报告独立评估其质量
- 铁律遵守：仓库零改动、零 commit；本笔记只落 `/home/z/my-project/audit-out/C3-keymap.md`

---

## ① #217/#218 变更全景（diffstat 解读）

| 文件 | +/− | 解读 |
|---|---|---|
| `src/keymap.rs` | +2,839/−0 | 新模块：数据模型 + 默认表 + 匹配引擎 + 用户覆盖合并 + 标签生成 + 全局装配 + **36 个内嵌单测** |
| `src/editor/mod.rs` | +219/−175 | `ctrl_k_pending: bool` → `chord_pending: Option<KeyQuery>`（7 处构造器同步）；`dispatch_key_inner` 的 match 臂（约 30 臂）整体换为表驱动 `match outcome`；新增动作级只读兜底门 |
| `src/shell.rs` | +83/−63 | `chord_until: Option<Instant>` → `chord: Option<(KeyQuery, Instant)>`；`on_key_down` 的 11 个 if 分支换为 `resolve_shell` + `ShellAction` 分派 |
| `src/settings.rs` | +129/−0 | `keymap` 用户配置读取：`load_keymap_overrides`（薄壳）+ `parse_keymap_entries`（纯函数）+ 3 个单测 |
| `src/menu_bar.rs` | +34/−23 | `shortcut: Option<&'static str>` → `Option<String>`；新增 `shortcut_for(action_id)`（标签改从 keymap 表取）；16 处菜单标签换源 |
| `src/context_menu.rs` | +21/−8 | `MenuItem::with_shortcut_for`（右键菜单标签同源化） |
| `src/menu_bar_view.rs` | +1/−1 | String 直传（E0521 CI 修复痕迹） |
| `src/main.rs` / `src/lib.rs` | +8/+6 | `keymap::init(&settings::load_keymap_overrides())` 开窗前装载；lib 树 `#[path]` 挂载 |
| `tests/command_coverage.rs` | +86/−41 | ④ `default_keys_reach_dispatch_key` 重写为**双证据**（行为级 keymap 解析 + 源码级执行臂对账） |
| `tests/ui_wiring_faces.rs` | +29/−9 | 面 2 由「键条件字面量」改为「`ShellAction::变体` → 处理调用」对账 |

增量结构判断：**替换式重构而非叠加式扩张**——editor/shell 的键分派代码净变化仅 +44/+20 行，核心重量全在新增的纯逻辑层。这与既往「视图侧自研负 ROI」的病历不同：本次把**已有行为**（两处 match 臂）抽象为数据表，宣称「默认行为零变化」并有 36 个单测 + 2 个集成测试面钉定。

分层宣称核实（keymap.rs:1-6 模块文档）：keymap.rs 仅 `use std::time::Instant`（:152）+ `std::sync::OnceLock`（:1673），**零 gpui、零 serde、纯 std 属实**。耦合方向单向：settings/editor/shell/menu_bar/context_menu → keymap，keymap 不 import 任何上层模块——叶子层定位成立。

---

## ② 五个 review 修复声称逐条验证

### 验证表

| # | 提交 | 声称 | 实际 | 证据 | 判定 |
|---|---|---|---|---|---|
| 1 | 8966e15 | 只读键位重映射绕过封堵，动作级兜底，双层防御第二层 | 第一层=物理键白名单（editor/mod.rs:2338-2342，既有）；第二层=`Matched(action) && read_only && !readonly_safe()` 整体拒绝（editor/mod.rs:2356-2363，新增）。**确实在动作级**：门在查表之后、分派之前，`Matched` 统一经门（含 chord 续接解析结果——续接路径无需另加） | readonly_safe 分类 keymap.rs:520-557；不变量测试 :2757-2787；绕道回归 :2794-2828 | **属实** |
| 2 | 3c0fb55 | chord 解绑赢家规则镜像 dispatch 两段式 + 单键越界 panic 封堵 | `chord_unbind_winner`（keymap.rs:1431-1450）= 候选 p1.matches(c1) && p2.matches(c2)，赢家按**尾键**精确度、同度表序先者——与 `best_continuation`（:1376-1398）的 `spec > best_spec` 严格比较逐字一致；`single_unbind_winner`（:1403-1422）镜像 `best_single`（:1355-1371）。越界封堵：canon.get(1) 守卫（:1156-1163），原实现 `canon[1]` 对单键 bind（如 `bind "ctrl+k" → 任意动作`，该键赢家是 chord 首）确会 index panic | 镜像测试 :2501-2555（含 dispatch 前提对拍）+ 反向对称 :2560-2598 + panic 回归 :2605-2628（警告恰指 unfold_all 抢占——因 sim 先移除了 fold_all 旧绑定，与单测断言吻合） | **属实** |
| 3 | ee319d1 | 不支持修饰词显式报错（meta+k 幻影键，Ctrl+S 失效原例） | `UNSUPPORTED_MODIFIERS`（keymap.rs:1521-1523：meta/cmd/command/win/windows/super/option/altgr/control）+ `BARE_MODIFIERS`（:1527-1530）+ 残段含 `+` 且不以 `+` 开头报错（:1581-1586）；`strip_modifier_ci`（:1604-1609）词级匹配——"windows+d" 不会误报成 "win"，"ctrl++" 保持合法（残段以 `+` 开头）。解析失败发生在 `apply_bind` 变异之前（:1038-1044 先 parse 后 retain），旧绑定结构性不受影响 | 幻影词面测试 :2148-2206；Ctrl+S 回归锁 :2213-2247（bind/unbind 双拒 + ctrl+s 仍解析出 Save + 全表零移除） | **属实** |
| 4 | d4eed9e | 键非数组类型警告（原静默）+ 解析提取纯函数 | `parse_keymap_entries(&serde_json::Value)`（settings.rs:157-190）确为纯函数（仅 serde_json+std）；非数组 → eprintln 带中文类型名（:163-169，`json_type_name_cn` :195-206）+ 空。**可测且有单测消费**：3 个测试（settings.rs:621/633/651——缺失静默、五种非数组类型、好坏混排） | 上述行号 | **属实** |
| 5 | 7c42d78 | 移除死代码 line_of | `line_of` 已从 command_coverage.rs 删除；全仓 grep 仅剩 wiring_integrity.rs:69 的**同名不同函数**（其在 :258/:271 有活跃调用方，非死代码）；command_coverage.rs 内无任何残留调用 | grep 证据如左 | **属实** |

### 验证细节与保留意见

1. **8966e15 保留意见一（口径镜像的可持续性）**：`readonly_allows_mirror`（keymap.rs:2685-2692）是 docx_doc.rs:1101-1118 的**誊抄副本**——语义逐键对拍等价（`"a"|"c"|"f"|"tab"|"="|"+"|"-"|"0"|"t"|"g" => ctrl` 的折叠形态与原文逐臂相同），但属双源同步隐患。代码注释自认「两处需同步改」，缓解是双侧各有钉定测试（docx_doc.rs:1325-1352 钉物理面、keymap.rs:2757-2787 钉不变量）。可接受，已登记为 P3-5。
2. **8966e15 保留意见二（门位置不对称）**：动作级拒绝发生在 `self.chord_pending = None` 重置**之后**（editor/mod.rs:2349 → :2356），物理级拒绝发生在重置**之前**（:2338）——即动作级拒绝会顺带终结挂起，物理级拒绝保留陈旧挂起。这是与原 `ctrl_k_pending` 同款的挂起残留 quirk（原代码 readonly 拒绝同样不清 pending），**非回归**；两门之间微小不对称仅为注释宣称「拒绝路径逐字同款」的修辞溢出。置信：已确认（代码序静态可读）。
3. **3c0fb55 镜像论证的边界**：镜像成立的根据是 unbind 的 canonical 首键 == 用户复放该序列时的实际按键快照（快照原样存 `KeyQuery`，editor/mod.rs:2368；shell.rs:1756），且候选谓词同为 `p1.matches(快照)`——候选集与 dispatch 续接候选集恒同，赢家规则（尾键 spec、严格大于、表序）恒同。唯一理论偏差：单键解绑只从 Single 条目中选赢家，而 dispatch 若该键赢家是 chord 首则产生 Pending（键「什么都不做」）——此时 unbind 单键仍会移除一条 Single 条目而不警告「该键仍被 chord 前缀占用」。与文档「移除该键实际触发的绑定」语义自洽（chord 首不触发），列 P3-2 UX 瑕疵。
4. **ee319d1 残面**：非修饰词的自由键名仍可造出幻影键（如 `ctrl+中文`、`ctrl+++`、`ctrl+f5` 之外的任意串）——bind 落表后永不触发、unbind 时给「无此绑定」警告。提交信息明示这是**有意取舍**（键名自由串是既有面）。置信：已确认；定级 P3-7（设计偏好）。

**五条声称全部属实，且每条都带回归测试**——review 修复纪律显著优于前轮审计所见的一般水平。

---

## ③ 匹配引擎深读

### 数据模型与匹配算法

- **三态修饰模型**（keymap.rs:158-186）：`Mod::Any/Yes/No`，`satisfies`/`weight`/`specificity`（约束数）。这个设计的动机在模块文档 :29-42 讲透了：原 match 各臂守卫口径不统一（`"a" if ctrl` 不看 shift/alt、`"k" if ctrl&&!shift` 不看 alt、shell `plain_ctrl` 三查），「精确集合相等」无法镜像，「三态 + 约束数排序」才能。这是对**既有怪癖的忠实建模**而非理想化重设计——正确选择。
- **单键匹配** `best_single`（:1355-1371）：全表扫描 head 匹配者，`spec > best_spec` 严格大于取最高约束数，**并列取表序先者**。chord 条目按首键参与竞争（命中即挂起候选）。表序 = 原 match 臂序的抽象，默认表下每键至多一条胜出，与单臂执行等价（:1351-1354 注释）。
- **chord 两段式**：首键 → `best_single` 赢家为 chord 则 `Pending`（接线层存**本键实际按键快照**）；续接 → `best_continuation`（:1376-1398）：`p1.matches(快照) && p2.matches(当前键)`，按**尾键**精确度、同度表序。首键精确度不参与续接排名——「首键已发生，分流只看尾键」，这是正确的两段式语义（也是 3c0fb55 镜像的靶子）。
- **前缀消费/挂起终结语义**：续接落空 → 挂起终结 + 本键当新序列起点下沉（resolve_editor_main :919-929，先试续接、失败回落 best_single）——精确复现原「取消前缀并照常下沉（不吞键）」。

### 逐臂对照复核（默认行为零变化红线）

对 E0-E32 / S1-S11 逐条与旧代码 diff 对拍（本审计独立复核对关键臂）：

- E15 tab `!ctrl`：`tab(No,Any,Any)`（keymap.rs:707）——Ctrl+Tab 不命中编辑器、冒泡 shell 标签循环 ✓（测试 :1842-1847 钉定）。
- E21/E22 z 双臂：`z(Yes,Yes,Any)→Redo` 在 `z(Yes,Any,Any)→Undo` 前（:733-740）——约束数 2>1 复现臂序 ✓。
- E25/E28 f 双臂：`f(Any,Any,Yes)→ToggleFold` 在 `f(Yes,Any,Any)→OpenFind` 前（:753-756 vs :767-770）——同约束数 1 并列时表序复现臂序，故 ctrl+alt+f 走 ToggleFold（原怪癖照抄，测试 :1885-1887 钉定）✓。
- E30/E31 s 双臂：`s(Yes,No,Any)→Save` spec 2 胜 `s(Yes,Any,Any)→Pass` spec 1——ctrl+s 走 Save、ctrl+shift+s 走 Pass 冒泡 ✓（测试 :1900-1908）。
- E32/S1 chord 怪癖：续接键 `0`/`j` 修饰全 Any（:787-800）——Ctrl+K Ctrl+0 同样触发折叠（原守卫只看 key 不看修饰），测试 :1964-1976 按四种修饰组合钉定 ✓。
- S1-S11 shell 表（:814-856）与原 11 个 if 分支逐条等价：`plain = (Yes,No,No)` 对应 `plain_ctrl`；`=(Yes,Any,No)` 对应 `ctrl && !alt`（shift 兼容）；表序=分支序 ✓。
- 挂起快照 vs 原 bool/Instant：编辑器无超时挂起、shell 2s 窗口（`now <= *until` 闭合边界与原 `Instant::now() <= until` 一致，:943-944）✓。
- **挂起中再按首键**：续接落空 → best_single 再命中首键 → Pending 刷新（测试 :1977-1981）——与原「取消再置位」净效果等价 ✓。
- **readonly 物理门位置不变**：仍在主表查表之前（editor/mod.rs:2338，批 3 原位），palette/goto 前置查表点仍在物理门之前（:2301/:2316）——与原代码序一致 ✓。

结论：**默认行为零变化宣称在静态层面成立**，且被 36 个内嵌单测（含修饰组合扫描、chord 全生命周期、过期作废、刷新）+ 2 个集成测试面（④双证据、面 2）钉定。置信：已确认（静态）；运行时复核依赖 Windows GPUI CI（见 ⑥）。

### 修饰词归一化与幻影键路径

- **Ctrl↔Cmd 平台映射**：`KeyQuery.ctrl = modifiers.control || modifiers.platform`（editor/mod.rs:2274；shell.rs:1672）——macOS ⌘ 与 Linux/Windows Ctrl 同槽位。副作用：mac 上 Ctrl 与 Cmd **无法分别绑定**（同槽合并），文档 :318-320 有声明。设计取舍，非缺陷。
- **Shift 大小写**：键名统一 `to_lowercase()`（:1597/:2276），大写字母输入归到「键+shift 修饰」模型——用户写 `SHIFT+Ctrl+A` 与 `shift+a` 等价（测试 :2125-2128）。
- **幻影键路径封堵**：见 ②#3——修饰词类已封；自由键名类有意放行。死键残留路径仅剩：合法解析但 dispatch 永不产生的键名（多字节字符、`ctrl+++` 等），bind 后永不触发，用户无感知线索（unbind 才给「无此绑定」警告）——P3-7。
- **「Ctrl+S 失效原例」回归锁**：`phantom_modifier_bind_rejected_keeps_default_save`（:2213-2247）锁住「bind meta+k → save 被拒且 ctrl+s 默认绑定完好 + unbind 幻影键零移除」——原例确有测试钉死。✓

### B·找反例：什么输入下会派发/解绑错绑定？

**反例一（dispatch 双执行，用户配置可触发）——跨域 chord 尾键与对域单键碰撞，冲突检测不设防：**

`conflicts()`（keymap.rs:290-299）的跨形态臂只比较 **chord 首键** 与单键（`(Chord(a1,_), Single(b)) => a1.intersects(b)`），**尾键从不与对域单键比较**。而运行时双执行架构成立（编辑器不 `stop_propagation`——除 canvas 外 editor/mod.rs 无一处调用；shell.rs 根 `on_key_down` 对冒泡键照单全收；仓库自身文档 keymap.rs:999-1001 即写明「编辑器不拦冒泡，同键两域会叠加执行」）。具体反例：

- 配置 `bind "ctrl+k w" → save`（编辑器域 chord）。检测：与 shell S1 `[ctrl+k, ctrl+o]` 比——首键相交但尾键 w≠o → 不冲突；与 shell S4 `ctrl+w` 比——跨形态只看首键 k≠w → 不冲突 → **放行**。
- 运行时按 `Ctrl+K`：编辑器 Pending（存快照，:2367-2369）**且** shell 也 Pending（S1 首，shell.rs:1755-1758）——双挂起。再按 `Ctrl+W`（2s 内）：编辑器续接 `[k,w]→save` 命中 → **执行 Save**（不吞冒泡）；shell 续接落空 → best_single(ctrl+w) → **执行 CloseTab**。一次按键序列，两个域的动作叠加执行。
- 镜像方向同样成立：`bind "ctrl+k f" → prompt_open_file`（shell 域 chord）后，`Ctrl+K Ctrl+F` → 编辑器 OpenFind + shell PromptOpenFile 双开。
- 这直接证伪 apply_user 文档「保守面：只要有**一种**按键组合双触发就拒绝」（keymap.rs:999-1001）与 `check_conflicts` 的跨域报告完备性（:1292-1299 同样依赖 conflicts）。

**反例二（解绑/绑定错位类）——3c0fb55 修复方向正确，未发现新反例**：对 `chord_unbind_winner`/`single_unbind_winner` 与 dispatch 谓词、比较语义逐行对拍（见 ②#2），未构造出能移错绑定或派发错绑定的输入。理论上唯一偏差（单键解绑在键被 chord 首占用时仍移除 Single）与文档语义自洽。

**反例三（同域死绑定）——chord bind 可静默杀死同键既有单键绑定**：`apply_bind` 的同域让位面只做两件事（keymap.rs:1076）：移除同动作绑定 + 移除被**同形态**覆盖条目（`covered_by` 跨形态恒 false，:305-313）。若表里已有单键 `ctrl+k` 绑定（合法路径：先 `unbind "ctrl+k ctrl+0"`、`unbind "ctrl+k ctrl+j"`、再 `bind "ctrl+k" → x`），随后 `bind "ctrl+k y" → z`：canonical 检测走 chord 分支（canon 有第二键 y，续接命中新条目）→ 放行。此后 `best_single(ctrl+k)` 恒被 chord 首（spec 2）胜出 → 恒 Pending → **既有单键绑定永不触发，无任何警告**；`check_conflicts` 对同域 chord×单键相交且不等者按「取舍确定」豁免不报（:1308 注释）——「取舍确定」对 chord×单键形态是错误断言（确定的不是胜者而是死键）。P3-1。

---

## ④ 安全防线分析（只读模式双层防御）

### 层级位置

```
dispatch_key_inner（editor/mod.rs）
  ├─ canvas 键域（:2291-2295）                       ← 模态守卫，原位
  ├─ L0 前置查表：palette（:2301）/ goto（:2316）    ← 物理门之前（原位原序）
  ├─ palette/goto 模态拦截（:2308/:2330）            ← 原位
  ├─ L1 物理键白名单 readonly_allows（:2338-2342）    ← 第一层（既有，批 3）
  ├─ 主表 resolve_editor_main（:2346）+ 挂起重置（:2349）
  ├─ L2 动作级 readonly_safe 门（:2356-2363）        ← 第二层（8966e15 新增）
  └─ match outcome 分派（:2364-…）
```

### 穷举绕过面

| 绕过向量 | 状态 | 证据 |
|---|---|---|
| 放行键（ctrl+c）重映射到破坏性动作（delete） | **已封**（L2 在动作级拒绝 DeleteForward——readonly_safe=false） | keymap.rs:539-555 变更类清单；回归 :2794-2828（物理放行对拍 + 解析出 DeleteForward + `!readonly_safe` ⇒ 必拒） |
| chord 续接路径重映射（[ctrl+c, ctrl+a] → 破坏性动作） | **已封**（续接结果也是 `Matched`，统一经 L2；提交信息与 :2350-2351 注释明示） | resolve_editor_main :911-917 返回 Matched → 同门 |
| chord 首键挂起本身 | 无害（不执行动作） | :2367-2369 仅存快照 |
| 前置层动作（palette/goto）重映射 | 不经只读门，但两动作均分类 safe（UI 模态/导航） | readonly_safe :525-536；前置查表在 L1 之前是**既有**设计（palette/goto 原本就在批 3 门之前） |
| `_` 可打印插入路径 | 已封（L1 白名单在插入前拦截；白名单键全部被表命中或带 ctrl 不入 `_` 分支） | :2371-2383 + docx_doc.rs:1101-1118 |
| shell 域动作（save_as/close_tab/font/theme） | **不经编辑器只读模型**（shell 键钩子无 read_only 概念）——Ctrl+Shift+S/Ctrl+W 在只读文档照常触发 PromptSaveAs/CloseTab | shell.rs:1735-1815 无只读检查；**既有行为**（键位表化未改变， readonly_allows 拒绝不拦冒泡） |
| 用户把 shell 动作绑到编辑器不消费的键 | 同上（shell 域动作本就不属只读威胁模型——不改缓冲区；SaveAs 写新路径属既有语义） | 既有架构 |

判定：**编辑器动作面（真正能改坏文档的面）双层防御完整**，第二层确在动作执行层且覆盖 chord 续接。shell 域动作绕过只读是**前轮既有架构属性**，keymap 未引入也未修复（keymap 把 shell 动作纳入可重绑面后，用户可把 SaveAs 绑到任意键——但威胁等级不变：对话框显式确认）。**反例一（③节）是本增量引入的唯一新防线缺口**：它不是只读绕过，而是冲突检测承诺的破洞——若用户 chord 尾键与 shell 单键碰撞，只读态下编辑器侧被 L2 拒、shell 侧照发（错序单执行，比双执行轻）。

---

## ⑤ 结构质量（2,839 行单文件）

- **内部切分**（banner 分节实测）：模块文档 :1-151；数据模型 :154-327；动作模型 :331-627；键位表 :652-874；查表/合并/自检 :877-1313；outcome :1329-1349；匹配内核（best_single/best_continuation/unbind winners/canonical）:1351-1465；键串解析 :1467-1609；UserOverride :1611-1619；标签生成 :1621-1665；全局装配 :1667-1693；**测试 :1700-2839（1,140 行，40.2%）**。纯逻辑约 1,560 行 + 文档 150 行——非膨胀型巨石。
- **模块切分合理性**：数据/引擎/解析/标签/装配五段清晰，每段有行为契约文档。相对 repo 惯例（editor/mod.rs 5,457 行）不算异类。若继续增长（未来多层站点/更多域），应拆 `keymap/{data,engine,parse,label}.rs`——当前规模未到必要阈值。设计偏好级。
- **pub 面收敛度**：19 个 pub 项实测外部消费者——被消费 11 项（KeyQuery/Action/Site/EditorAction/EditorOutcome/MoveDir→editor；ShellOutcome→shell；UserOverride/KeymapStore→settings+集成测试；store()→4 文件；init→main）。**无外部消费者的 7 项**：Binding、KeyPattern、Mod、Entry、parse_binding、label_binding、ParseError（keymap 内部+自测使用；`pub` 并非集成测试所需——command_coverage 只用 KeymapStore/KeyQuery/Site/EditorOutcome/Action）。pub(crate) 即可收敛。P3-4。
- **耦合方向**：keymap 是叶子（零上层 import）✓；settings→keymap 单向 ✓；menu_bar/context_menu 只读消费 `store().label()` ✓。**默认键表数据（37 条目：editor_main 24 + palette/goto 2 + shell 11，:677-801/:814-856）与引擎逻辑分离** ✓——「换绑=改表」承诺成立。
- **残余双源**：`Cmd::default_key`（commands.rs:140-165）与 keymap 默认表并存——命令面板仍显示静态字符串（**palette_view.rs:83 `cmd.default_key()`**），菜单栏/右键菜单已换源。用户换绑后面板显示旧键位、菜单显示新键位——「单一事实源」宣称不完整。④ 测试把两源对拍（default_key → keymap 解析），钉住**默认值**漂移但钉不住**用户换绑后的跟随性**。P3-3。

---

## ⑥ 测试覆盖

- **keymap 引擎本身有 36 个直接单测**（非仅经 UI wiring 间接测）：默认表无歧义自检（:1764）、id 往返唯一（:1772）、E4-E32 逐臂对照含修饰组合扫描（:1801-2009）、S1-S11 逐臂（:2015-2037）、shell chord 生命周期含过期作废（:2042-2096）、解析全形态+幻影词（:2102-2247）、16 对菜单标签字面量对拍（:2254-2292）、bind/unbind 合并语义 12 个（:2298-2678）、只读防线 3 个（:2696-2828）、全局装配 1（:2835）。
- **command_coverage.rs +86/−41**：④ 重写为双证据——每个 Cmd::default_key 先在默认表**行为级解析**（chord 两步：首键须 Pending + 续接命中；GotoLine 走前置层特例），再检查 `dispatch_key_inner` 源码含 `EditorAction::变体` 执行臂（command_coverage.rs:331-396）；序列键机制证据由 `ctrl_k_pending` 字面量改为 `chord_pending`（:398-407）。这比原「键名字符串出现在函数体」的证据更强（原版只证键名在场，新版证「键→动作→执行臂」全链）。
- **ui_wiring_faces.rs 面 2**：`"tab"`/`"w"` 键条件字面量断言升级为 `ShellAction::NextTab` 等变体臂 + 处理调用对账（:129-166）——与表化后 shell.rs 实际形态对齐，防「表在、执行臂断链」。
- **settings.rs +3 单测**：畸形 JSON 全覆盖（非数组五类型、非对象/缺键/值类型错混排、缺失静默）。
- **负向用例**：用户配置畸形的三个层面都有——JSON 类型层（settings 三测）、键串/动作名层（malformed_overrides :2632-2657、幻影词 :2148-2206）、冲突层（同域/跨域/精确度/被 chord 抢占 6 个拒绝用例）。
- **可执行性**：mang-shell 测试在 Windows CI 实跑——windows-test.yml `test-shell` job：`cargo check --all-targets` + `cargo test --lib -- --test-threads=1`（含 keymap/settings 内嵌 36+3 测）+ `cargo test --tests`（含 command_coverage/ui_wiring）。**提交史上三个 CI 修复提交（0b010da E0716 / 88ea6fd E0521+E0308）证明该门禁在 PR 过程中真实编译并打回**——门禁对本次增量非空转。运行时（GPUI 事件冒泡模型、真实按键路径）结论仍需该 CI 复核：置信「已确认（静态）+ 需外部环境验证（运行时）」。
- **测试盲区**：①③ 节反例一/反例三均无测试覆盖（双执行、死绑定）；readonly 物理门+动作门在**用户换绑后**的接线层组合行为只有 keymap 层单测推演（`readonly_bypass_blocked_at_action_level` 验证「条件成立」而非接线层实际拒绝——后者无 GPUI 环境不可测，属可接受残面）。

---

## ⑦ 新问题清单

| 级 | 问题 | 位置 | 置信 |
|---|---|---|---|
| **P2-1** | 跨域冲突检测漏 chord **尾键**×对域**单键**碰撞：`conflicts()` 跨形态臂只比首键；用户 chord（编辑器或 shell 域）尾键与对域单键同键时双域叠加执行（例：`bind "ctrl+k w" → save` 后 `Ctrl+K Ctrl+W` = Save + CloseTab 双发；镜像方向 `bind "ctrl+k f" → prompt_open_file` 后 `Ctrl+K Ctrl+F` = OpenFind + PromptOpenFile 双发）。证伪「只要有任何一种按键组合双触发就拒绝」的保守承诺；check_conflicts 同盲 | keymap.rs:290-299（conflicts 跨形态臂）+ :1187-1210（跨域检测）+ :1292-1299（自检报告）；运行时双挂起：editor/mod.rs:2367-2369 + shell.rs:1755-1758 | 已确认（静态推演 + 仓库自身文档佐证冒泡架构；运行时复现需 GUI 环境） |
| P3-1 | 同域 chord bind 静默杀死同键既有单键绑定（死绑定无警告；check_conflicts 以「取舍确定」豁免不报，该豁免对 chord×单键形态不成立）。触发需特定解绑+双 bind 序列 | keymap.rs:1076（retain 面）+ :1308（豁免注释）+ :1355-1371（chord 首 spec 竞争恒胜） | 已确认（静态推演） |
| P3-2 | 单键 unbind 在该键实际赢家是 chord 首（恒 Pending）时仍移除 Single 条目、无「该键仍被前缀占用」提示 | keymap.rs:1403-1422 | 设计偏好（与「实际触发」文档语义自洽，但 UX 有缝） |
| P3-3 | 命令面板快捷键标签仍用 `Cmd::default_key` 静态串，不随用户换绑跟随（菜单栏/右键菜单已换源）——「单一事实源」宣称不完整 | palette_view.rs:83 vs menu_bar.rs:193 / context_menu.rs:98 | 已确认 |
| P3-4 | pub 面略宽：Binding/KeyPattern/Mod/Entry/parse_binding/label_binding/ParseError 七项无外部消费者 | keymap.rs:266/191/160/647/1490/1634/1473 | 已确认（设计偏好） |
| P3-5 | `readonly_allows_mirror` 誊抄夹具与 docx_doc 真函数双源同步（注释自认「两处需同步改」，双侧钉定测试缓解） | keymap.rs:2685-2692 vs docx_doc.rs:1101-1118 | 已确认 |
| P3-6 | `store()` 惰性默认初始化与 `init()` 先后竞态：若任何查表发生在 main 的 init 之前（测试/未来改动），OnceLock 已落默认表 → init 幂等跳过 → **用户覆盖静默丢失**。当前 main.rs:250-251 装载序安全 | keymap.rs:1676-1693 | 高度怀疑（当前不可达的潜在陷阱） |
| P3-7 | 自由键名幻影残面：`ctrl+中文`/`ctrl+++` 等可 bind 成永不触发死绑定，bind 时无警告（有意取舍，文档已声明） | keymap.rs:1536-1540 注释 | 已确认（设计偏好） |
| P3-8 | macOS 下 Ctrl 与 ⌘ 同槽（control‖platform 合并），无法分别绑定——平台修饰表达力损失 | editor/mod.rs:2274、keymap.rs:318-320 | 已确认（设计取舍，文档声明） |

P0/P1：**未发现**。无 panic 面（remove_triggered 的 unreachable! 经 canonical_queries 构造不变量确证不可达；canon.get(1) 已守卫；settings 侧 serde_json 全 .ok() 容错 + 递归限制保护）；无数据破坏路径（只读防线完整；P2-1 是双执行而非数据损毁）。

---

## ⑧ 增量健康度独立判断

**结论：质量达标（本仓上游水平之上的达标），但同时是「双轨上继续加高」的又一实例——两个判断并行不悖。**

**质量面（为什么达标）**：

1. **工程纪律罕见地好**：纯逻辑分层（零 GPUI/serde）、单向耦合、数据/引擎分离、「默认行为零变化」红线不是口号而是 36 个逐臂单测 + 2 个集成测试面 + 双证据门禁的结构性产物。五个 review 修复**全部属实且各带回归测试**——这是本仓第一次见到「review 发现 → 修复 → 回归锁」闭环完整运作的 PR（对照前轮审计的 P0-4「review 修复无验证」病历）。
2. **抽象选择正确**：三态修饰模型是对既有怪癖的忠实镜像而非理想化重写；chord 两段式 + 首键快照统一了两个不一致的状态机（bool vs Instant）且保序保义；「只映射既有执行体不写新逻辑」纪律让 2,839 行中执行层搬运零新行为。
3. **门禁非空转**：Windows CI test-shell job 实跑 `cargo test --lib/--tests`，PR 期间三次编译错误打回（0b010da/88ea6fd）证明门禁咬合。
4. 3,455 行中 1,140+129×0.4≈1,190 行（约 35%）是测试；逻辑净增约 1,600 行 + 接线替换——**替换式**而非叠加式。

**架构面（为什么仍是加高）**：

1. keymap 全部动作面（37 默认绑定、39 动作变体）都在 mang-shell（GPUI/Zed text 轨）内闭环，**与 mdcore 语义核零接触**——它让双轨的产品侧更可配置、更粘住用户，客观上加深了「二号编辑器」的独立演化（终稿 P1-3 的方向性风险未被此 PR 缓解，反被延伸）。
2. 但就终稿 Q2 的具体警告（「双轨与**门禁失真**的地基上继续加高」）而言：本次增量**没有**加门禁失真——它的测试真的在合并门禁里跑；它加的是双轨高度。批评应精确：这不是 #213/#214 那种在失真地基上的加高，而是在**门禁已验证工作**的轨道上的合规加高。

**遗留**：P2-1（跨域 chord 尾键双执行）应在下一个 shell PR 修掉——修法明确：`conflicts()` 跨形态比较需覆盖「chord 尾键 × 单键」（或至少 apply_bind 跨域检测额外遍历 chord 尾键 vs 对域单键相交）；P3-3（面板标签换源）与 P3-1（死绑定警告）属一小时内的小活。修完这三件，该模块可评为 A 级。

---

### 证据索引（关键行号速查）

| 主题 | 行号 |
|---|---|
| 三态 Mod / specificity | keymap.rs:158-186, 225-228 |
| conflicts() 跨形态臂 | keymap.rs:290-299 |
| readonly_safe 分类 | keymap.rs:520-557 |
| 默认三表 | keymap.rs:670-801, 805-856, 861-874 |
| resolve_pre / resolve_editor_main / resolve_shell | keymap.rs:888-895, 908-930, 937-964 |
| apply_bind（含 canon.get(1) 守卫、跨域检测） | keymap.rs:1037-1213（守卫 1156-1163，跨域 1187-1210） |
| remove_triggered + 单/chord 赢家 | keymap.rs:1230-1259, 1403-1422, 1431-1450 |
| check_conflicts | keymap.rs:1273-1312 |
| best_single / best_continuation | keymap.rs:1355-1371, 1376-1398 |
| 解析器（幻影词防线） | keymap.rs:1490-1602（词表 1521-1530，strip_modifier_ci 1604-1609） |
| store/init 全局装配 | keymap.rs:1673-1693 |
| L1 物理只读门 / L2 动作门 | editor/mod.rs:2338-2342 / 2356-2363 |
| 挂起快照接线 | editor/mod.rs:2384, 2367-2369；shell.rs:151, 1748-1758 |
| settings 纯函数解析 | settings.rs:144-146, 157-190, 195-206（测试 621-676） |
| 面板标签旧源 | palette_view.rs:83 |
| CI 门禁 | .github/workflows/windows-test.yml:99-146 |

*（全部行号经当前工作树实读核对；本审计零改动仓库。）*
