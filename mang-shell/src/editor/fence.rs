//! #205：fence 缓存重建（纯逻辑层，零 gpui 依赖——只依赖
//! [`super::highlight`] 的围栏识别原语）。
//!
//! 围栏态是**有状态的行序列性质**：每行的行首态由前文决定（围栏
//! 奇偶 + 语言标注）。编辑器把它缓存为两条并行数组
//! （`Editor::fence_cache` / `Editor::fence_lang_cache`），供渲染层
//! （`line_tokens`）与折叠（`fence_span_at` / `top_level_fold_spans`）
//! 零重扫消费——每次编辑后由 [`super::Editor::refresh_caches`] 刷新。
//!
//! ## 缓存不变量（全量/增量共用——增量产物必须与之逐位一致）
//!
//! - `states[i]` = 行 i **行首**的围栏态：`` ``` `` 开行/闭行的行首均
//!   `false`（开行行首在外、闭行行首在内）。
//! - `langs[i]` = 行 i 行首处于围栏内时的语言标注；**行首在围栏外恒
//!   `None`**（开围栏行捕获标注、闭围栏行复位——归纳可证）。
//! - 两条数组长度恒等于当前总行数。
//!
//! ## 单行推进（与 `highlight_line_ex` 严格一致）
//!
//! [`FenceCursor::advance`] 是唯一推进单元，全量与增量**共用同一
//! 函数**（R15 修复的「共享同一单行生成函数」先例——杜绝两处逻辑
//! 漂移）。与 [`super::highlight::highlight_line_ex`] 的 `fence_at_end`
//! 逐位等价（行首态相对性见 highlight.rs 的 #151 注释）：
//!
//! | 行首态 | 行是围栏标记行（`` ``` ``/`~~~`，前导空白 ≤ 3） | `fence_at_end` | 推进后行首态 |
//! |--------|------------------------------------------------|----------------|--------------|
//! | 围栏外 | 是 | `true`  | 入栏，语言 = `fence_lang_of(行)` |
//! | 围栏外 | 否 | `false` | 保持栏外（lang 恒 `None`） |
//! | 围栏内 | 是 | `false` | 出栏，lang 复位 `None` |
//! | 围栏内 | 否 | `true`  | 原态不变 |
//!
//! ## 增量算法（种子 / 推进 / 停机 / 回退）
//!
//! - **种子**：单区间编辑不改动脏行 `d` 之前的任何行 → 其行首态不变，
//!   种子 = 旧缓存在 `d` 处的值（无需额外存档）。
//! - **推进**：`d` 起逐行 `advance`；行数变化时后段整体平移
//!   `delta = n_new − n_old`。平移用「旧尾**预移**到终位」技巧：
//!   把旧尾 `[e_new − delta, n_old)` 一次 memmove 到 `[e_new, n_new)`
//!   ——此后位置 `j ≥ e_new` 恰好持有旧值 `old[j − delta]`，收敛时
//!   尾部即已在终位（零后拷贝）；未收敛则被扫描逐位覆写（读先于写，
//!   安全）。delta < 0（删行）先移后截断。
//! - **停机**：越过编辑影响区（新行 `j ≥ e_new`——单区间编辑几何
//!   保证 `j` 起的行与旧文档 `j − delta` 位逐行同文）后，若计算行首态
//!   与旧值完全一致（inside + lang 双等），则其后行内容相同且行首态
//!   相同 → 缓存值必然一致 → 停机。传播距离由**真实分歧**决定：
//!   - 围栏外/围栏内的普通打字不改变任何行首态 → **零传播**（在
//!     `e_new` 立即收敛）；
//!   - 只改语言标注（`` ```rust `` → `` ```js ``）→ 分歧持续到该块
//!     闭栏行后收敛；
//!   - 改变围栏奇偶（删/写开闭标记）→ 由于标记行对 inside 双向翻转、
//!     非标记行保持，**奇偶一旦分歧永不再合**（同文后缀下归纳可证）
//!     → 传播到文末——这些行的行首态确实全部翻转，全量重算本就是
//!     唯一正确结果（最坏情况，罕见）。
//! - **回退**：前置条件不满足（[`rebuild_fences_incremental`] 返回
//!   `false`——多区间编辑、缓存长度失配、脏行越界等由调用方判定）
//!   → 全量重建（[`rebuild_fences_full`]——同一推进函数，语义恒等）。
//!
//! 典型逐键成本从 O(总行数) 降为 O(编辑行数 + 传播距离 + 尾部一次
//! memmove)；围栏外/内单字符打字为 O(编辑行)（零传播 + delta=0 无平移）。

use super::highlight::{CodeLang, fence_lang_of, opens_fence};
// 锚定测试数据源（原 refresh_caches 全量循环对照——只测面引用；经
// #[path] 被 harness-205 引入时 `super` 恒指向 highlight 所在层，双环境
// 同一写法）。
#[cfg(test)]
use super::highlight::{FenceState, highlight_line_ex};

/// fence 扫描游标（行首态）——增量/全量共用的推进单元。
///
/// 不变量：`!inside ⇒ lang == None`（[`Self::advance`] 与
/// [`Self::OUTSIDE`] 共同保持；全量扫描从 `OUTSIDE` 起步，归纳成立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FenceCursor {
    /// 行首是否处于围栏内部。
    pub inside: bool,
    /// 行首语言标注（围栏外恒 `None`）。
    pub lang: Option<CodeLang>,
}

impl FenceCursor {
    /// 文档起始态（行首围栏外）。
    pub const OUTSIDE: Self = Self {
        inside: false,
        lang: None,
    };

    /// 单行推进：本行行首态 × 行文本 → 下一行行首态。
    ///
    /// 与 `refresh_caches` 原全量循环里 `highlight_line_ex` +
    /// `fence_lang_of` 的状态转移**逐位一致**（等价性锚定测试
    /// `full_matches_legacy_highlight_loop` 钉定）。
    pub fn advance(self, line: &str) -> Self {
        if self.inside {
            if opens_fence(line) {
                // 闭围栏：出栏 + 语言复位。
                Self::OUTSIDE
            } else {
                // 栏内普通行：态不变。
                self
            }
        } else if opens_fence(line) {
            // 开围栏：入栏 + 捕获语言标注（```rust → Rust）。
            Self {
                inside: true,
                lang: fence_lang_of(line),
            }
        } else {
            // 栏外普通行：保持栏外。
            Self::OUTSIDE
        }
    }
}

/// 全量重建 fence 缓存（就地覆写 `states`/`langs` 到 `n` 行）。
///
/// 行文本经 `line_at` **惰性取用**（Editor 侧 rope 取行
/// O(log n + 行长)；harness 侧数组直取）——只扫一遍，每行一次
/// [`FenceCursor::advance`]（不做整行 token 化）。
pub fn rebuild_fences_full(
    states: &mut Vec<bool>,
    langs: &mut Vec<Option<CodeLang>>,
    n: usize,
    line_at: impl Fn(usize) -> String,
) {
    states.clear();
    states.resize(n, false);
    langs.clear();
    langs.resize(n, None);
    let mut cursor = FenceCursor::OUTSIDE;
    for i in 0..n {
        states[i] = cursor.inside;
        langs[i] = cursor.lang;
        cursor = cursor.advance(&line_at(i));
    }
}

/// 增量重建 fence 缓存（就地：`states`/`langs` 传入时是**旧缓存**
/// （长度 `n_old`，描述编辑前文档），返回时已改为新缓存（长度 `n_new`）。
///
/// # 参数几何（单区间替换 `[start, end)` → `text`）
///
/// - `d` = 变更起点行（`start.line`）：新文档 `[0, d)` 与旧文档逐行
///   同文 → 行首态不变（种子位）。
/// - `e_new` = 编辑影响区后的首个对齐行 = `d + text 换行数 + 1`
///   （新坐标）：`[e_new, n_new)` 与旧文档 `[e_new − delta, n_old)`
///   逐行同文（单区间编辑的结构性质）。
///
/// # 前置条件（任一不满足返回 `false`——调用方回退全量）
///
/// - 双缓存长度一致；`n_new ≥ 1`；
/// - `d < n_old`（种子可从旧缓存恢复）；
/// - `e_new ≤ n_new` 且 `e_new > d`；
/// - 平移源 `e_new − delta` 落在 `[0, n_old]`（几何自洽）。
///
/// # 返回
///
/// `true` = 增量完成（结果与全量重建逐位一致——等价性对照测试钉定）；
/// `false` = 前置条件不满足（**未改动缓存**，调用方必须回退全量）。
pub fn rebuild_fences_incremental(
    states: &mut Vec<bool>,
    langs: &mut Vec<Option<CodeLang>>,
    n_new: usize,
    d: usize,
    e_new: usize,
    line_at: impl Fn(usize) -> String,
) -> bool {
    let n_old = states.len();
    if langs.len() != n_old || n_new == 0 || d >= n_old || e_new > n_new || e_new <= d {
        return false;
    }
    let delta = n_new as isize - n_old as isize;
    // 旧尾平移源（旧坐标）：e_new − delta = 影响区后首行的旧对应位。
    // 单区间编辑几何保证 0 ≤ e_new − delta ≤ n_old；失配即数据不一致，
    // 回退全量（防御——正常路径不可能触发）。
    let src = e_new as isize - delta;
    if src < 0 || src as usize > n_old {
        return false;
    }
    let src = src as usize;

    // 种子：脏行 d 的旧行首态（d < e_new ≤ n_new，未被平移触碰）。
    let mut cursor = FenceCursor {
        inside: states[d],
        lang: if states[d] { langs[d] } else { None },
    };

    // 旧尾预移到终位（一次 memmove）：此后位置 j ≥ e_new 恰好持有
    // 旧值 old[j − delta]——收敛即已就位（零后拷贝）；未收敛则被扫描
    // 逐位覆写（扫描在位置 j 先读后写，读到的一定是未被覆写的预移值）。
    // delta == 0 时源=目的地，跳过（平移为恒等）。
    if delta != 0 {
        if delta > 0 {
            states.resize(n_new, false);
            langs.resize(n_new, None);
            states.copy_within(src..n_old, e_new);
            langs.copy_within(src..n_old, e_new);
        } else {
            // 收缩：先下移旧尾再截断（截断会毁掉 [n_new, n_old) 段——
            // 它正是平移源的一部分）。
            states.copy_within(src..n_old, e_new);
            langs.copy_within(src..n_old, e_new);
            states.truncate(n_new);
            langs.truncate(n_new);
        }
    }
    debug_assert_eq!(states.len(), n_new, "平移后长度 = 新行数");
    debug_assert_eq!(langs.len(), n_new, "平移后长度 = 新行数");

    // 推进 + 停机：[d, e_new) 是编辑影响区（必算，不做停机判定——
    // 其内容不保证与旧文档同文）；j ≥ e_new 与预移后的位置 j（=
    // old[j − delta]）完全一致即停——其后行同文 + 行首态相同 →
    // 缓存值必然一致。
    let mut j = d;
    while j < n_new {
        if j >= e_new && states[j] == cursor.inside && langs[j] == cursor.lang {
            // 收敛：[j, n_new) 已是正确旧尾（预移就位），停机。
            return true;
        }
        states[j] = cursor.inside;
        langs[j] = cursor.lang;
        cursor = cursor.advance(&line_at(j));
        j += 1;
    }
    // 推进到文末未收敛：编辑真正改变了后续行首态（奇偶翻转永不回合
    // ——见模块注释）——全量重算本就是唯一正确结果。
    true
}

// =============================================================================
// 单元测试 —— 增量 ≡ 全量（逐位）是本模块的交付门；
// 本模块经 #[path] 被 harness-205 引入时，这批测试在 harness 里同样跑
//（编辑器仓 GPUI 面无法本地编译——纯层测试双环境共用）。
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 全量重建结果。
    fn full(lines: &[&str]) -> (Vec<bool>, Vec<Option<CodeLang>>) {
        let mut s = Vec::new();
        let mut l = Vec::new();
        rebuild_fences_full(&mut s, &mut l, lines.len(), |i| lines[i].to_string());
        (s, l)
    }

    /// **锚定测试**的数据源：refresh_caches 的原全量循环
    /// （highlight_line_ex 逐行推进——循环体照抄自 #205 修复前实现）。
    fn legacy_full(lines: &[&str]) -> (Vec<bool>, Vec<Option<CodeLang>>) {
        let mut states = Vec::with_capacity(lines.len());
        let mut langs = Vec::with_capacity(lines.len());
        let mut fence = false;
        let mut cur_lang: Option<CodeLang> = None;
        for line in lines {
            states.push(fence);
            langs.push(if fence { cur_lang } else { None });
            let state = if fence {
                FenceState::CodeBlock(cur_lang)
            } else {
                FenceState::Outside
            };
            let h = highlight_line_ex(line, state);
            if !fence && h.fence_at_end {
                cur_lang = fence_lang_of(line);
            } else if fence && !h.fence_at_end {
                cur_lang = None;
            }
            fence = h.fence_at_end;
        }
        (states, langs)
    }

    /// 应用一次单区间编辑到行模型（坐标语义同 `Buffer::replace_range`：
    /// `(line, col)` char 索引、`[start, end)` 删除 + `text` 插入），
    /// 返回 (新行集, 脏行 d, 影响区后首行 e_new)。
    fn apply_edit(
        lines: &[String],
        start: (usize, usize),
        end: (usize, usize),
        text: &str,
    ) -> (Vec<String>, usize, usize) {
        let (sl, sc) = start;
        let (el, ec) = end;
        let first: String = lines[sl].chars().take(sc).collect();
        let tail: String = lines[el].chars().skip(ec).collect();
        let mut out: Vec<String> = lines[..sl].to_vec();
        out.extend(format!("{first}{text}{tail}").split('\n').map(String::from));
        out.extend(lines[el + 1..].to_vec());
        let d = sl;
        let e_new = d + text.matches('\n').count() + 1;
        (out, d, e_new)
    }

    /// 对旧缓存做增量（就地层 + 断言成功），与全量重建逐位比对。
    fn incremental_eq_full(
        old_lines: &[String],
        new_lines: &[String],
        d: usize,
        e_new: usize,
        ctx: &str,
    ) {
        let (mut s, mut l) = full(&old_lines.iter().map(|x| x.as_str()).collect::<Vec<_>>());
        let ok = rebuild_fences_incremental(&mut s, &mut l, new_lines.len(), d, e_new, |i| {
            new_lines[i].clone()
        });
        assert!(ok, "前置条件应满足（{ctx}）");
        let (fs, fl) = full(&new_lines.iter().map(|x| x.as_str()).collect::<Vec<_>>());
        assert_eq!(s, fs, "states 增量 ≡ 全量（{ctx}）");
        assert_eq!(l, fl, "langs 增量 ≡ 全量（{ctx}）");
    }

    // ---------------- 锚定：全量 ≡ 旧循环 ----------------

    #[test]
    fn full_matches_legacy_highlight_loop() {
        let cases: Vec<Vec<&str>> = vec![
            vec![],
            vec![""],
            vec!["```rust", "fn main() {}", "```", "正文"],
            vec!["~~~python", "def f():", "  pass", "~~~", "x"],
            vec!["   ```", "缩进围栏", "   ```"],
            vec!["``", "两个反引号不开栏", "``"],
            vec!["```js", "未闭合到文末"],
            vec!["正文", "```", "a", "```", "```", "b", "```"],
            vec!["#", "> 引用", "- 列表", "```rust"],
            // 栏内行含围栏形态即闭栏（简化语义——highlight.rs 头注释登记）。
            vec!["```", "代码里的 ``` 会闭栏", "后文"],
        ];
        for lines in cases {
            let (want_s, want_l) = legacy_full(&lines);
            let (got_s, got_l) = full(&lines);
            assert_eq!(got_s, want_s, "states 全量 ≡ 旧循环：{lines:?}");
            assert_eq!(got_l, want_l, "langs 全量 ≡ 旧循环：{lines:?}");
        }
    }

    #[test]
    fn cursor_advance_table() {
        use CodeLang::{Js, Python, Rust};
        // (行, 行首态, 期望行尾态)。
        let cases: Vec<(&str, FenceCursor, FenceCursor)> = vec![
            (
                "```rust",
                FenceCursor::OUTSIDE,
                FenceCursor {
                    inside: true,
                    lang: Some(Rust),
                },
            ),
            (
                "~~~py",
                FenceCursor::OUTSIDE,
                FenceCursor {
                    inside: true,
                    lang: Some(Python),
                },
            ),
            (
                "```",
                FenceCursor::OUTSIDE,
                FenceCursor {
                    inside: true,
                    lang: None,
                },
            ),
            ("``", FenceCursor::OUTSIDE, FenceCursor::OUTSIDE),
            ("普通行", FenceCursor::OUTSIDE, FenceCursor::OUTSIDE),
            (
                "```js",
                FenceCursor {
                    inside: true,
                    lang: Some(Rust),
                },
                FenceCursor::OUTSIDE,
            ),
            (
                "代码行",
                FenceCursor {
                    inside: true,
                    lang: Some(Js),
                },
                FenceCursor {
                    inside: true,
                    lang: Some(Js),
                },
            ),
            (
                "```",
                FenceCursor {
                    inside: true,
                    lang: None,
                },
                FenceCursor::OUTSIDE,
            ),
        ];
        for (line, from, want) in cases {
            assert_eq!(from.advance(line), want, "advance({line:?}, {from:?})");
        }
    }

    // ---------------- 停机正确性专项 ----------------

    #[test]
    fn typing_outside_fence_zero_propagation() {
        // 围栏外单字符编辑：行首态未变 → 立即收敛（delta=0 零平移）。
        let old: Vec<String> = vec![
            "# 标题".into(),
            "正文".into(),
            "```rust".into(),
            "fn a() {}".into(),
            "```".into(),
            "尾文".into(),
        ];
        let (new, d, e) = apply_edit(&old, (1, 2), (1, 2), "x");
        assert_eq!((d, e), (1, 2), "影响区 = 脏行 1 一行");
        incremental_eq_full(&old, &new, d, e, "围栏外打字");
    }

    #[test]
    fn typing_inside_fence_zero_propagation() {
        // 围栏内普通打字：行首态（inside + lang）未变 → 同样立即收敛
        // （状态比较停机——不必传播到闭围栏）。
        let old: Vec<String> = vec![
            "```rust".into(),
            "let x = 1;".into(),
            "let y = 2;".into(),
            "```".into(),
            "正文".into(),
        ];
        let (new, d, e) = apply_edit(&old, (1, 8), (1, 8), "x");
        assert_eq!((d, e), (1, 2));
        incremental_eq_full(&old, &new, d, e, "围栏内打字");
    }

    #[test]
    fn lang_tag_edit_converges_at_block_end() {
        // 只改开栏语言标注（```rust → ```js）：块内 lang 分歧、奇偶
        // 不变 → 闭栏行后收敛。
        let old: Vec<String> = vec![
            "```rust".into(),
            "let x = 1;".into(),
            "```".into(),
            "正文".into(),
            "```".into(),
            "y".into(),
            "```".into(),
        ];
        let (new, d, e) = apply_edit(&old, (0, 3), (0, 7), "js");
        incremental_eq_full(&old, &new, d, e, "改语言标注");
        let (s, l) = full(&new.iter().map(|x| x.as_str()).collect::<Vec<_>>());
        assert_eq!(l[1], Some(CodeLang::Js), "块内语言切换为 js");
        assert_eq!(l[4], None, "下一块无标注");
        let _ = s;
    }

    #[test]
    fn delete_closing_fence_flips_parity_to_eof() {
        // 删闭围栏标记：奇偶从脏行后翻转且**永不回合**（标记行双向
        // 翻转 + 非标记行保持——同文后缀下新旧态恒相反）→ 传播到文末。
        let old: Vec<String> = vec![
            "```rust".into(),
            "a".into(),
            "```".into(),
            "正文".into(),
            "```".into(),
            "b".into(),
            "```".into(),
            "尾".into(),
        ];
        // 行 2 "```" 删一个反引号 → "``"（不再是标记行）。
        let (new, d, e) = apply_edit(&old, (2, 2), (2, 3), "");
        incremental_eq_full(&old, &new, d, e, "删闭围栏（奇偶翻转到文末）");
        // 语义核对：翻转后原栏外正文被吸入，原第二块闭栏行成为开栏。
        let (s, _) = full(&new.iter().map(|x| x.as_str()).collect::<Vec<_>>());
        assert_eq!(s[3], true, "原正文行被吸进围栏");
        assert_eq!(s[4], true, "原开栏行行首仍在栏内（前块的延续）");
        assert_eq!(s[5], false, "原开栏行成为闭栏 → 其后出栏");
        assert_eq!(s[7], true, "文末处于新开的围栏内");
    }

    #[test]
    fn delete_opening_fence_flips_parity_to_eof() {
        // 删开围栏标记 + 其后无更多闭栏 → 翻转传播到文末（最坏情况：
        // 原闭栏行变成开栏行，文末处于新围栏内）。
        let old: Vec<String> = vec![
            "```rust".into(),
            "fn a() {}".into(),
            "```".into(),
            "正文一".into(),
            "正文二".into(),
        ];
        // 行 0 "```rust" 删第三个反引号 → "``rust"（不再开栏）。
        let (new, d, e) = apply_edit(&old, (0, 2), (0, 3), "");
        incremental_eq_full(&old, &new, d, e, "删开围栏（翻转传播到文末）");
        let (s, _) = full(&new.iter().map(|x| x.as_str()).collect::<Vec<_>>());
        // 奇偶整体翻转：原 [F,T,T,F,F] → [F,F,F,T,T]（原闭栏行成为
        // 开栏行，其后全部入栏到文末）。
        assert_eq!(s, vec![false, false, false, true, true]);
    }

    // ---------------- 行数增减（delta ≠ 0 平移） ----------------

    #[test]
    fn insert_lines_delta_positive() {
        let old: Vec<String> = vec![
            "a".into(),
            "```rust".into(),
            "code".into(),
            "```".into(),
            "z".into(),
        ];
        // 行 1 行首粘贴多行文本（含围栏标记——影响区后整段平移）。
        let (new, d, e) = apply_edit(&old, (1, 0), (1, 0), "x\ny\n```\nw");
        assert_eq!((d, e), (1, 5), "插入 3 个换行 → 影响区 [1,5)");
        assert_eq!(new.len(), old.len() + 3);
        incremental_eq_full(&old, &new, d, e, "多行粘贴（行数 +3）");
    }

    #[test]
    fn delete_lines_delta_negative() {
        let old: Vec<String> = vec![
            "```rust".into(),
            "a".into(),
            "b".into(),
            "c".into(),
            "```".into(),
            "尾".into(),
        ];
        // 跨行删除（行数 -2）。
        let (new, d, e) = apply_edit(&old, (1, 1), (3, 1), "");
        incremental_eq_full(&old, &new, d, e, "跨行删除（行数 -2）");
        assert_eq!(new.len(), old.len() - 2);
    }

    #[test]
    fn backspace_join_lines() {
        // 行首退格（与前行合并）：d = 前行，影响区单行，行数不变。
        let old: Vec<String> = vec!["```rust".into(), "a".into(), "```".into(), "b".into()];
        let (new, d, e) = apply_edit(&old, (1, 1), (2, 0), "");
        assert_eq!(
            new,
            vec!["```rust".to_string(), "a```".to_string(), "b".to_string()]
        );
        assert_eq!((d, e), (1, 2));
        incremental_eq_full(&old, &new, d, e, "行首退格合并");
    }

    #[test]
    fn edit_at_last_line_and_append() {
        let old: Vec<String> = vec!["```rust".into(), "a".into(), "```".into()];
        // 末行编辑（影响区到文末，无尾部可拼）。
        let (new, d, e) = apply_edit(&old, (2, 3), (2, 3), "x");
        assert_eq!(e, new.len(), "末行编辑影响区到文末");
        incremental_eq_full(&old, &new, d, e, "末行编辑");
        // 文末追加多行（delta > 0，平移源为空）。
        let (new2, d2, e2) = apply_edit(&new, (2, 4), (2, 4), "\n```\ncode");
        incremental_eq_full(&new, &new2, d2, e2, "文末追加围栏");
    }

    #[test]
    fn replace_all_text_full_influence_zone() {
        // 整文档替换（reload 路径）：影响区覆盖全文，无尾部——全推进。
        let old: Vec<String> = vec![
            "a".into(),
            "```".into(),
            "b".into(),
            "```".into(),
            "c".into(),
        ];
        let (new, d, e) = apply_edit(&old, (0, 0), (4, 1), "X\nY");
        incremental_eq_full(&old, &new, d, e, "整文档替换");
    }

    // ---------------- 种子恢复：任意脏行起增量 ≡ 全量 ----------------

    #[test]
    fn seed_recovery_from_every_line() {
        let lines: Vec<String> = vec![
            "h".into(),
            "```rust".into(),
            "let s = 1;".into(),
            "~~~".into(),
            "尾部".into(),
            "```js".into(),
            "let y = 2;".into(),
            "```".into(),
            "z".into(),
        ];
        // 从行 d 起把后文整体换成新内容（影响区到文末——整段从种子
        // 重算，种子错则全错）：每行 d 的种子恢复都必须给出与全量
        // 逐位一致的结果。
        let tail_texts = ["新文", "```py", "code", "```", "", "~~~ts", "x"];
        for d in 0..lines.len() {
            for tail in tail_texts {
                let mut new: Vec<String> = lines[..d].to_vec();
                new.extend(tail.split('\n').map(String::from));
                let (mut s, mut l) = full(&lines.iter().map(|x| x.as_str()).collect::<Vec<_>>());
                let ok = rebuild_fences_incremental(&mut s, &mut l, new.len(), d, new.len(), |i| {
                    new[i].clone()
                });
                assert!(ok, "d={d} 前置条件应满足");
                let want = full(&new.iter().map(|x| x.as_str()).collect::<Vec<_>>());
                assert_eq!(s, want.0, "d={d} tail={tail:?} states 种子恢复恒等");
                assert_eq!(l, want.1, "d={d} tail={tail:?} langs 种子恢复恒等");
            }
        }
    }

    // ---------------- 回退触发（前置条件不满足） ----------------

    #[test]
    fn incremental_fallback_preconditions() {
        let lines: Vec<String> = vec!["```rust".into(), "a".into(), "```".into()];
        let (s0, l0) = full(&lines.iter().map(|x| x.as_str()).collect::<Vec<_>>());
        let fetch = |i: usize| lines[i].clone();

        // 脏行越界（d >= n_old——越界编辑点防御，Buffer 侧 clamp 场景）。
        let (mut s, mut l) = (s0.clone(), l0.clone());
        assert!(!rebuild_fences_incremental(&mut s, &mut l, 3, 3, 4, fetch));
        assert_eq!((s, l), (s0.clone(), l0.clone()), "回退路径不改缓存");

        // 影响区越界（e_new > n_new）。
        let (mut s, mut l) = (s0.clone(), l0.clone());
        assert!(!rebuild_fences_incremental(&mut s, &mut l, 3, 1, 4, fetch));

        // 影响区为空（e_new <= d）。
        let (mut s, mut l) = (s0.clone(), l0.clone());
        assert!(!rebuild_fences_incremental(&mut s, &mut l, 3, 1, 1, fetch));

        // n_new == 0（Buffer 恒 ≥ 1 行——防御）。
        let (mut s, mut l) = (s0.clone(), l0.clone());
        assert!(!rebuild_fences_incremental(&mut s, &mut l, 0, 0, 1, fetch));

        // 双缓存长度不一致。
        let (mut s, mut l) = (s0.clone(), l0[..1].to_vec());
        assert!(!rebuild_fences_incremental(&mut s, &mut l, 3, 0, 2, fetch));
    }

    /// 随机源（xorshift——纯 std，harness/CI 双环境可复现）。
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// 小型随机等价性（大强度版本在 harness-205——这里钉基本形态）：
    /// 随机文档 × 随机单区间编辑 × 每步增量 ≡ 全量。
    #[test]
    fn random_edits_incremental_eq_full() {
        const WORDS: &[&str] = &[
            "```rust", "```", "~~~py", "~~~", "``", "`x`", "code", "# h", "> q", "- l", "", "正文",
            "    ```", "a``b",
        ];
        for seed in 1..=40u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let n0 = 3 + rng.below(20);
            let mut lines: Vec<String> = (0..n0)
                .map(|_| WORDS[rng.below(WORDS.len())].to_string())
                .collect();
            let mut caches = full(&lines.iter().map(|x| x.as_str()).collect::<Vec<_>>());
            for _step in 0..30 {
                // 随机编辑：起点/终点（有序）+ 随机文本（偶发多行）。
                let mut sl = rng.below(lines.len());
                let mut sc = rng.below(lines[sl].chars().count() + 1);
                let mut el = rng.below(lines.len());
                let mut ec = rng.below(lines[el].chars().count() + 1);
                if (sl, sc) > (el, ec) {
                    std::mem::swap(&mut sl, &mut el);
                    std::mem::swap(&mut sc, &mut ec);
                }
                let text = if rng.below(4) == 0 {
                    format!(
                        "{}\n{}",
                        WORDS[rng.below(WORDS.len())],
                        WORDS[rng.below(WORDS.len())]
                    )
                } else {
                    WORDS[rng.below(WORDS.len())].to_string()
                };
                let (new, d, e) = apply_edit(&lines, (sl, sc), (el, ec), &text);
                let ok = rebuild_fences_incremental(
                    &mut caches.0,
                    &mut caches.1,
                    new.len(),
                    d,
                    e,
                    |i| new[i].clone(),
                );
                assert!(ok, "seed={seed} 前置条件应满足");
                let want = full(&new.iter().map(|x| x.as_str()).collect::<Vec<_>>());
                assert_eq!(caches.0, want.0, "seed={seed} states 增量 ≡ 全量");
                assert_eq!(caches.1, want.1, "seed={seed} langs 增量 ≡ 全量");
                lines = new;
            }
        }
    }
}
