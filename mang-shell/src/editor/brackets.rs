//! 括号匹配（issue #132 任务 1）——纯逻辑、零 gpui 依赖。
//!
//! 输入整篇文本 + 光标点（buffer 坐标：行、行内 char 索引），输出
//! [`BracketMatch`]：光标所在括号的配对位置 / 孤立括号 / 不在括号上。
//! 渲染（匹配高亮 + 不匹配标红）在 `super::Editor` 的样式合成层完成。
//!
//! ## 语义（VS Code 口径）
//!
//! 1. 语法集 = 三种成对括号 + 三种单字符裸括号（单侧缺失也判定为
//!    「括号」——它就是个括号，只是没配对）。
//! 2. 字符串/注释内的括号不算：复用 G5 的语法扫描器（[`super::highlight`]）
//!    判断 token 归属——按行取 `TokenKind` run，落在 [`TokenKind::Str`] /
//!    [`TokenKind::Comment`] 内的字符一律不作括号看待。围栏代码块的
//!    语言态决定「什么算字符串/注释」：rust 的引号与 `//`、python 的引号
//!    与 `#`、js 的引号与 `//`；未标注语言的围栏整行 [`TokenKind::BlockCode`]，
//!    无字符串/注释语义（括号全部参与匹配）。
//! 3. 匹配仅在同一 token 上下文内进行：开括号只在后续的「非字符串 /
//!    非注释」字符里找配对闭括号（跨行，全文档）；反向同理。嵌套按栈深
//!    计数——栈深 0 时遇到的同类闭括号即配对。
//! 4. 「光标在括号上」= 光标前的字符是括号，或光标处的字符是括号
//!    （前优先——光标落在括号右侧时优先解释为「刚输入完的那个括号」）。
//!
//! ## 与 md 行内标记的关系（登记）
//!
//! 围栏外的 md 行内 token（粗体/行内代码/链接）不影响括号归属——md 的
//! markdown 标记语义与代码的「字符串字面量」不是一回事（反引号在中文
//! 行文里常用于引号，把整段吞成字符串会让括号匹配大面积失效）。只有围栏
//! 内、且语言标注已识别的代码行才按 Str/Comment 排除。

use super::highlight::{self, FenceState, TokenKind};

/// 括号匹配结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BracketMatch {
    /// 找到匹配：`open` = 开括号 (行, 列)，`close` = 闭括号 (行, 列)。
    Matched {
        open: (usize, usize),
        close: (usize, usize),
    },
    /// 只有一个（不匹配——渲染层标红）。
    Unmatched { pos: (usize, usize) },
    /// 光标不在括号上。
    None,
}

/// 括号方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Open,
    Close,
}

/// 字符 -> (括号种类 0/1/2 = 圆/方/花, 方向)。
fn classify(c: char) -> Option<(u8, Dir)> {
    match c {
        '(' => Some((0, Dir::Open)),
        ')' => Some((0, Dir::Close)),
        '[' => Some((1, Dir::Open)),
        ']' => Some((1, Dir::Close)),
        '{' => Some((2, Dir::Open)),
        '}' => Some((2, Dir::Close)),
        _ => None,
    }
}

/// 一行拆成「字符 + 该字符是否可视作括号」（字符串/注释内的不算），
/// 一并返回该行行尾围栏态——同一次高亮的产物（`Doc::new` 复用，
/// 省一遍重复扫描）。
///
/// 归属判定直接用 G5 的行级 token：落在 `Str`/`Comment` run 内的字符置
/// `false`。其余类别一律 `true`——括号本身不在任何 token run 里（扫描器
/// 不产括号 token），所以「不在 Str/Comment 内」即「真括号」。
///
/// 注意返回的 `fence_at_end` 是**所传 `fence` 行首态相对**的语义
/// （见 [`FenceWalker::advance`]）——调用方拿它推进围栏时必须保证
/// 行首态就是 walker 自身视角。
fn bracket_chars(line: &str, fence: FenceState) -> (Vec<(char, bool)>, bool) {
    let chars: Vec<char> = line.chars().collect();
    let h = highlight::highlight_line_ex(line, fence);
    let excluded = |i: usize| {
        h.runs
            .iter()
            .any(|(r, k)| matches!(k, TokenKind::Str | TokenKind::Comment) && r.contains(&i))
    };
    let out = chars
        .into_iter()
        .enumerate()
        .map(|(i, c)| (c, classify(c).is_some() && !excluded(i)))
        .collect();
    (out, h.fence_at_end)
}

/// 围栏态推进（与 [`highlight::highlight_line_ex`] 的 `fence_at_end`
/// 以及 `Editor::refresh_caches` 的推进口径同源）。
///
/// `advance` 收到的 `fence_at_end` 必须是**以 walker 自身行首态**评估
/// 的值：围栏外（Outside）行首下 true = 本行开了围栏；栏内（CodeBlock）
/// 行首下 false = 本行闭合围栏。传入别的行首态算出的值会误判——典型
/// 陷阱：whole_lang 把正文行首态覆盖成 CodeBlock 后，普通代码行的
/// 行尾值恒 true（「仍在栏内」），直接喂进来会让 walker 凭空入栏
/// （#151 CI 失败的根因，见 [`Doc::new`] 的推进注释）。
struct FenceWalker {
    fence: bool,
    lang: Option<highlight::CodeLang>,
}

impl FenceWalker {
    fn new() -> Self {
        Self {
            fence: false,
            lang: None,
        }
    }

    /// 本行行首的高亮态。
    fn state(&self) -> FenceState {
        if self.fence {
            FenceState::CodeBlock(self.lang)
        } else {
            FenceState::Outside
        }
    }

    /// 按本行行尾态推进。
    fn advance(&mut self, line: &str, fence_at_end: bool) {
        if !self.fence && fence_at_end {
            self.fence = true;
            self.lang = highlight::fence_lang_of(line);
        } else if self.fence && !fence_at_end {
            self.fence = false;
            self.lang = None;
        }
    }
}

/// 全文档的括号字符表（逐行、围栏语言态随行推进）。
struct Doc {
    lines: Vec<Vec<(char, bool)>>,
}

impl Doc {
    /// `whole_lang`：整篇文本的代码语言（`.rs`/`.py`/`.ts` 直接打开时
    /// 无围栏可依，正文按该语言做字符串/注释识别）。围栏态仍然优先。
    ///
    /// 围栏推进必须以 **walker 自身视角**评估行尾态：`fence_at_end` 是
    /// 行首态相对的——Outside 行首下 true = 本行开围栏；CodeBlock 行首
    /// 下 true = 行尾仍在栏内（普通代码行恒 true）。修复前这里直接把
    /// whole_lang 覆盖态（CodeBlock）的行尾值喂给 walker，源码文件第 0
    /// 行就把 walker 误置成「已入栏（无语言）」，此后全部正文行退化为
    /// `CodeBlock(None)`=整行 BlockCode——字符串/注释排除整体失效
    /// （#151 CI 失败：`(1,14)` 期望 `None` 实得 `Unmatched{(1,13)}`）。
    fn new(text: &str, whole_lang: Option<highlight::CodeLang>) -> Self {
        let mut lines = Vec::new();
        let mut walker = FenceWalker::new();
        for raw in text.split('\n') {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            // whole_lang（代码文件：.rs/.py/.ts 直接打开）时整篇都是同一语言。
            //
            // 关键：`fence_at_end` 是【行首态相对】的——
            //   Outside 行首下 true = 本行开了围栏（``` / ~~~）
            //   CodeBlock 行首下 true = 行尾仍在栏内（普通代码行恒 true）
            //
            // 曾经的 bug（#157 / #151 CI 失败）：把 whole_lang 覆盖态
            // （CodeBlock）的行尾值直接喂给 walker，于是第 0 行
            // `fn main() {` 就把 walker 误置成「已入栏（无语言）」，
            // 此后所有行退化为 CodeBlock(None) = 整行 BlockCode，
            // 字符串/注释排除整体失效 → 字符串里的括号被当成真括号。
            //
            // 修法：覆盖态下评估行尾值必须换回 walker 视角——
            // 是否开围栏只看行形态（opens_fence），不看覆盖态的行尾值。
            let walker_state = walker.state();
            let state = match whole_lang {
                Some(lang) if !walker.fence => FenceState::CodeBlock(Some(lang)),
                _ => walker_state,
            };
            let (chars, fence_at_end) = bracket_chars(line, state);
            lines.push(chars);
            let walker_fence_at_end = if state == walker_state {
                // 无覆盖（md 文件 / 真围栏内的行）：本次高亮的行首态
                // 就是 walker 视角，行尾值直接可用。
                fence_at_end
            } else {
                // whole_lang 覆盖（栏外源码正文行）：是否开围栏只看
                // 行形态（Outside 行首态下行尾值的全部语义）。
                highlight::opens_fence(line)
            };
            walker.advance(line, walker_fence_at_end);
        }
        Doc { lines }
    }

    /// 该位置的括号（若在字符串/注释内则 `None`）。
    fn bracket_at(&self, line: usize, col: usize) -> Option<(u8, Dir)> {
        let (c, ok) = *self.lines.get(line)?.get(col)?;
        if !ok {
            return None;
        }
        classify(c)
    }

    /// 从开括号之后向后扫，栈深计数找同类配对闭括号。
    fn scan_forward(&self, line: usize, col: usize, kind: u8) -> Option<(usize, usize)> {
        let mut depth = 0usize;
        for (li, row) in self.lines.iter().enumerate().skip(line) {
            let start = if li == line { col + 1 } else { 0 };
            for (ci, (c, ok)) in row.iter().enumerate().skip(start) {
                if !ok {
                    continue;
                }
                let Some((k2, dir)) = classify(*c) else {
                    continue;
                };
                if k2 != kind {
                    continue;
                }
                match dir {
                    Dir::Open => depth += 1,
                    Dir::Close => {
                        if depth == 0 {
                            return Some((li, ci));
                        }
                        depth -= 1;
                    }
                }
            }
        }
        None
    }

    /// 从闭括号之前反向扫，栈深计数找同类配对开括号。
    fn scan_backward(&self, line: usize, col: usize, kind: u8) -> Option<(usize, usize)> {
        let mut depth = 0usize;
        for li in (0..=line).rev() {
            let Some(row) = self.lines.get(li) else {
                continue;
            };
            let end = if li == line {
                col.min(row.len())
            } else {
                row.len()
            };
            for ci in (0..end).rev() {
                let (c, ok) = row[ci];
                if !ok {
                    continue;
                }
                let Some((k2, dir)) = classify(c) else {
                    continue;
                };
                if k2 != kind {
                    continue;
                }
                match dir {
                    Dir::Close => depth += 1,
                    Dir::Open => {
                        if depth == 0 {
                            return Some((li, ci));
                        }
                        depth -= 1;
                    }
                }
            }
        }
        None
    }
}

/// 从整篇文本推断代码语言（文件级——`.rs` / `.py` / `.ts` 直接打开的
/// 情形没有围栏可依，靠内容特征判定；判不出 `None` 走 md 行内语义）。
pub fn infer_code_lang(text: &str) -> Option<highlight::CodeLang> {
    let head: String = text.lines().take(200).collect::<Vec<_>>().join("\n");
    // Rust 标志：fn main / use ...:: / pub fn / let mut / impl / #[...]
    let rust_hints = [
        "fn main",
        "pub fn ",
        "let mut ",
        "impl ",
        "use std::",
        "#[derive",
        "-> Result<",
    ];
    if rust_hints.iter().any(|h| head.contains(h)) {
        return Some(highlight::CodeLang::Rust);
    }
    // Python 标志：def x( / import x / from x import / if __name__
    let py_hints = ["def ", "import ", "from ", "if __name__", "elif "];
    if py_hints.iter().any(|h| head.contains(h)) && !head.contains("{\n") {
        return Some(highlight::CodeLang::Python);
    }
    // JS 标志：function / const x = => / console.log
    let js_hints = ["function ", "console.log", "=> {", "const "];
    if js_hints.iter().any(|h| head.contains(h)) {
        return Some(highlight::CodeLang::Js);
    }
    None
}

/// 从光标位置找匹配括号（围栏/markdown 语义——issue #132 要求的入口）。
///
/// `cursor` = (行, 行内 char 列)，0 基、越界安全。光标前的字符优先
/// （VS Code 语义：光标停在括号右侧时匹配该括号）。
///
/// 语言态只有围栏一条来源；直接打开的 `.rs` / `.py` / `.ts` 用
/// [`find_match_lang`]（渲染层走它——否则源码文件里的字符串/注释
/// 排除不会生效）。
pub fn find_match(text: &str, cursor: (usize, usize)) -> BracketMatch {
    find_match_lang(text, cursor, None)
}

/// 显式指定整篇代码语言版本的 [`find_match`]。
///
/// `whole_lang` = 文件级语言（`.rs` 直接打开时是 `Some(Rust)`）——正文
/// 按该语言识别字符串/注释，围栏内的行仍按围栏语言标注优先。
pub fn find_match_lang(
    text: &str,
    cursor: (usize, usize),
    whole_lang: Option<highlight::CodeLang>,
) -> BracketMatch {
    // 快路径：光标周围的字符字面上就不是候选括号（或不在文本里）→ 直接
    // `None`，**不构建文档表**。这条把「光标停在普通字符上」的调用从
    // O(全文) 降到 O(光标所在行的解析成本)，是渲染层每帧调用的关键。
    let (line, col) = cursor;
    let line_text = match text.split('\n').nth(line) {
        Some(l) => l.strip_suffix('\r').unwrap_or(l),
        None => return BracketMatch::None,
    };
    let chars: Vec<char> = line_text.chars().collect();
    // 字符串/注释态必须完整解析才知道，这里只挡「两侧字面都不是括号字符」
    // 的绝大多数情形。
    let near_prev = col > 0 && chars.get(col - 1).is_some_and(|c| classify(*c).is_some());
    let near_here = chars.get(col).is_some_and(|c| classify(*c).is_some());
    if !near_prev && !near_here {
        return BracketMatch::None;
    }

    let doc = Doc::new(text, whole_lang);
    let target = if col > 0 && doc.bracket_at(line, col - 1).is_some() {
        (line, col - 1)
    } else if doc.bracket_at(line, col).is_some() {
        (line, col)
    } else {
        return BracketMatch::None;
    };

    let Some((kind, dir)) = doc.bracket_at(target.0, target.1) else {
        return BracketMatch::None;
    };
    let found = match dir {
        Dir::Open => doc.scan_forward(target.0, target.1, kind),
        Dir::Close => doc.scan_backward(target.0, target.1, kind),
    };
    match (found, dir) {
        (None, _) => BracketMatch::Unmatched { pos: target },
        (Some(p), Dir::Open) => BracketMatch::Matched {
            open: target,
            close: p,
        },
        (Some(p), Dir::Close) => BracketMatch::Matched {
            open: p,
            close: target,
        },
    }
}

/// 该字符是否属于被匹配的括号语法集（探针/渲染层的轻量判定）。
pub fn is_bracket_char(c: char) -> bool {
    classify(c).is_some()
}

/// 单行渲染辅助：给定全文 + 光标 + 目标行，返回本行需要上色的 char 列
/// （匹配对 = 两列，不匹配 = 一列）。渲染层按光标缓存一次 [`find_match`]
/// 结果即可逐行取列——本函数是它的便捷封装。
pub fn highlight_cols(text: &str, cursor: (usize, usize), line: usize) -> Vec<usize> {
    highlight_cols_lang(text, cursor, line, None)
}

/// 显式语言版本的 [`highlight_cols`]。
pub fn highlight_cols_lang(
    text: &str,
    cursor: (usize, usize),
    line: usize,
    whole_lang: Option<highlight::CodeLang>,
) -> Vec<usize> {
    match find_match_lang(text, cursor, whole_lang) {
        BracketMatch::None => Vec::new(),
        BracketMatch::Unmatched { pos } => {
            if pos.0 == line {
                vec![pos.1]
            } else {
                Vec::new()
            }
        }
        BracketMatch::Matched { open, close } => {
            let mut v = Vec::new();
            if open.0 == line {
                v.push(open.1);
            }
            if close.0 == line {
                v.push(close.1);
            }
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 便捷：命中形态的紧凑摘要（断言可读）。
    fn hit(text: &str, line: usize, col: usize) -> String {
        match find_match(text, (line, col)) {
            BracketMatch::Matched { open, close } => format!("M{:?}-{:?}", open, close),
            BracketMatch::Unmatched { pos } => format!("U{:?}", pos),
            BracketMatch::None => "N".to_string(),
        }
    }

    // ---------------- 三种括号 ----------------

    #[test]
    fn three_paren_kinds_match() {
        assert_eq!(hit("f(a)", 0, 2), "M(0, 1)-(0, 3)");
        assert_eq!(hit("v[1]", 0, 2), "M(0, 1)-(0, 3)");
        assert_eq!(hit("s{x}", 0, 2), "M(0, 1)-(0, 3)");
    }

    #[test]
    fn cursor_both_sides_of_bracket() {
        // 光标在开括号左侧（col 指向括号本身）。
        assert_eq!(hit("f(a)", 0, 1), "M(0, 1)-(0, 3)");
        // 光标在闭括号右侧（col = 闭括号后一位）。
        assert_eq!(hit("f(a)", 0, 4), "M(0, 1)-(0, 3)");
        // 光标在闭括号左侧（col 指向闭括号）。
        assert_eq!(hit("f(a)", 0, 3), "M(0, 1)-(0, 3)");
    }

    #[test]
    fn cursor_not_on_bracket_is_none() {
        assert_eq!(hit("f(a)", 0, 0), "N");
        assert_eq!(hit("hello", 0, 2), "N");
        assert_eq!(hit("", 0, 0), "N");
        // 行/列越界不 panic。
        assert_eq!(hit("f(a)", 99, 99), "N");
        assert_eq!(hit("f(a)", 0, 99), "N");
    }

    // ---------------- 嵌套 ----------------

    #[test]
    fn nested_pairs_pick_depth_zero_partner() {
        //           01234567890
        //  f((a), [b])  -> 0f 1( 2( 3a 4) 5, 6' ' 7[ 8b 9] 10)
        let t = "f((a), [b])";
        // 光标 2 = 外层 '(' 右侧
        assert_eq!(hit(t, 0, 2), "M(0, 1)-(0, 10)");
        // 光标 3 = 内层 '(' 右侧
        assert_eq!(hit(t, 0, 3), "M(0, 2)-(0, 4)");
        // 光标 8 = '[' 右侧
        assert_eq!(hit(t, 0, 8), "M(0, 7)-(0, 9)");
    }

    #[test]
    fn cross_kind_nesting() {
        // "{ [ ( ) ] }" -> 0'{' 2'[' 4'(' 6')' 8']' 10'}'
        let t = "{ [ ( ) ] }";
        assert_eq!(hit(t, 0, 1), "M(0, 0)-(0, 10)");
        assert_eq!(hit(t, 0, 3), "M(0, 2)-(0, 8)");
        assert_eq!(hit(t, 0, 5), "M(0, 4)-(0, 6)");
    }

    #[test]
    fn cross_line_nesting() {
        // 0: "fn f() {"       -> 4'(' 5')' 7'{'
        // 1: "    g(h[i]);"   -> 6'(' 7')' — 见下
        // 2: "}"              -> 0'}'
        let t = "fn f() {\n    g(h[i]);\n}";
        // 光标 8 = '{' 右侧 — 跨两行配到 2:0 的 '}'
        assert_eq!(hit(t, 0, 8), "M(0, 7)-(2, 0)");
        // 光标 5 = ')' 右侧 — 反向配到 0:4 的 '('（空参）
        assert_eq!(hit(t, 0, 5), "M(0, 4)-(0, 5)");
        // 行 1 = "    g(h[i]);" -> 5'(' 7'[' 9']' 10')'
        // 光标 6 = '(' 右侧 — 正向跳过 '[' ']'（异类），配到 10 的 ')'
        assert_eq!(hit(t, 1, 6), "M(1, 5)-(1, 10)");
        // 光标 8 = '[' 右侧 — 配到 9 的 ']'
        assert_eq!(hit(t, 1, 8), "M(1, 7)-(1, 9)");
    }

    // ---------------- 不匹配 ----------------

    #[test]
    fn unmatched_single_bracket() {
        assert_eq!(hit("f(a", 0, 2), "U(0, 1)"); // 只有开括号
        assert_eq!(hit("fa)", 0, 3), "U(0, 2)"); // 只有闭括号
        assert_eq!(hit("{", 0, 1), "U(0, 0)");
        assert_eq!(hit("]", 0, 0), "U(0, 0)");
    }

    #[test]
    fn unmatched_wrong_kind_counterpart() {
        // "(]" —— ')' 的配对是 '('，这里只有 ']'（不同 kind）。
        assert_eq!(hit("(]", 0, 1), "U(0, 0)");
        assert_eq!(hit("(]", 0, 2), "U(0, 1)");
    }

    #[test]
    fn interleaved_brackets_stay_same_kind() {
        // "([)]" 交叉形态 -> 0'(' 1'[' 2')' 3']'。
        // 口径：只在**同 kind** 的括号上累计栈深，异类直接跳过。
        // 故 '(' @0 向后第一枚同类 ')' 是 @2（'[' 被跳过）——配 (0,0)-(0,2)，
        // 而非按位置栈见的 (0,3)。这是「同类配对」的确定性语义，不 panic。
        assert_eq!(hit("([)]", 0, 1), "M(0, 0)-(0, 2)");
        assert_eq!(hit("([)]", 0, 2), "M(0, 1)-(0, 3)");
    }

    // ---------------- 字符串内忽略 ----------------

    #[test]
    fn brackets_in_rust_string_ignored() {
        // 1: l0 e1 t2 ' '3 s4 ' '5 =6 ' '7 \"8 (9 \"10 ;11
        let t = "```rust\nlet s = \"(\";\nlet t = 1;\n```";
        assert_eq!(hit(t, 1, 10), "N", "字符串内的 '(' 不参与匹配");
        assert_eq!(hit(t, 1, 9), "N");
        assert_eq!(hit(t, 2, 3), "N");
    }

    #[test]
    fn rust_string_pair_does_not_shadow_real_brackets() {
        // 1: l0 e1 t2 ' '3 v4 ' '5 =6 ' '7 (8 \"9 )10 \"11 )12 ;13
        // 真实 '(' @8 配 ')' @12——字符串里的 ')' @10 不算。
        let t = "```rust\nlet v = (\")\");\n```";
        assert_eq!(hit(t, 1, 9), "M(1, 8)-(1, 12)");
    }

    #[test]
    fn brackets_in_unlabeled_fence_are_real() {
        // 未标注语言的围栏 = 整行 BlockCode，**无字符串语义** -> 引号里的
        // 括号照常参与匹配（这是与 rust 围栏的关键差异）。
        // 行 1 = `"(")` -> 0'"' 1'(' 2'"' 3')'
        // 引号**不**构成字符串（无语言态 -> 整行 BlockCode），括号照常配对。
        let t = "```\n\"(\")\n```";
        assert_eq!(hit(t, 1, 2), "M(1, 1)-(1, 3)");
    }

    #[test]
    fn markdown_inline_code_keeps_brackets() {
        // 围栏外的 md 行内代码**不**做字符串排除（登记语义：md 行内标记
        // ≠ 代码字符串字面量——反引号在中文行文里常作引号用）。
        //   t0 e1 x2 t3 ' '4 `5 (6 )7 `8
        let t = "text `()` here";
        assert_eq!(hit(t, 0, 7), "M(0, 6)-(0, 7)");
        // 对照：同样在反引号里的括号，放进 **rust 围栏的字符串**就不算。
        //   行 1 = "let s = \"()\";" -> 8'\"' 9'(' 10')' 11'\"'
        let t2 = "```rust\nlet s = \"()\";\n```";
        assert_eq!(hit(t2, 1, 10), "N");
    }

    // ---------------- 注释内忽略 ----------------

    #[test]
    fn brackets_in_rust_comment_ignored() {
        // 1: l0 e1 t2 ' '3 a4 ' '5 =6 ' '7 1 8 ;9 ' '10 /11 /12 ' '13 (14 ]15
        let t = "```rust\nlet a = 1; // (]\n```";
        assert_eq!(hit(t, 1, 15), "N");
        assert_eq!(hit(t, 1, 16), "N");
    }

    #[test]
    fn python_hash_comment_ignored() {
        // 1: x0 ' '1 =2 ' '3 1 4 ' '5 ' '6 #7 ' '8 (9 ]10
        let t = "```python\nx = 1  # (]\n```";
        assert_eq!(hit(t, 1, 10), "N");
        assert_eq!(hit(t, 1, 11), "N");
    }

    #[test]
    fn js_line_comment_ignored() {
        // 1: l0 e1 t2 ' '3 a4 ' '5 =6 ' '7 [8 1 9 ]10 ;11 ' '12 /13 /14 ' '15 ]16
        let t = "```js\nlet a = [1]; // ]\n```";
        assert_eq!(hit(t, 1, 9), "M(1, 8)-(1, 10)");
        assert_eq!(hit(t, 1, 17), "N");
    }

    // ---------------- 辅助形态 ----------------

    #[test]
    fn match_variants_are_distinguishable() {
        assert_eq!(
            find_match("f(a)", (0, 2)),
            BracketMatch::Matched {
                open: (0, 1),
                close: (0, 3)
            }
        );
        assert_eq!(
            find_match("f(a", (0, 2)),
            BracketMatch::Unmatched { pos: (0, 1) }
        );
        assert_eq!(find_match("abc", (0, 1)), BracketMatch::None);
    }

    #[test]
    fn highlight_cols_per_line() {
        let t = "{\n  a\n}";
        assert_eq!(highlight_cols(t, (0, 1), 0), vec![0]);
        assert_eq!(highlight_cols(t, (0, 1), 2), vec![0]);
        assert_eq!(highlight_cols(t, (0, 1), 1), Vec::<usize>::new());
    }

    #[test]
    fn is_bracket_char_grammar() {
        for c in ['(', ')', '[', ']', '{', '}'] {
            assert!(is_bracket_char(c), "{c} 应属括号语法集");
        }
        for c in ['<', 'a', '（', '）', '「', '」'] {
            assert!(!is_bracket_char(c), "{c} 不应属括号语法集");
        }
    }

    // ---------------- whole_lang（文件级语言，无围栏） ----------------

    #[test]
    fn whole_lang_string_and_comment_excluded_from_line_one() {
        // 回归（#151 CI 失败）：修复前 Doc::new 用 whole_lang 覆盖态的
        // 行尾值推进围栏 walker——源码文件第 0 行（普通代码行在
        // CodeBlock 行首态下恒 true）即把 walker 误置为「已入栏（无
        // 语言）」，此后所有行退化为整行 BlockCode，字符串/注释排除
        // 整体失效。
        let t = "fn main() {\n    let s = \"(\";\n    let a = 1; // (]\n}";
        let rust = Some(highlight::CodeLang::Rust);
        // 字符串内的 '('（行 1——回归点：曾误报 Unmatched{(1,13)}）。
        assert_eq!(find_match_lang(t, (1, 14), rust), BracketMatch::None);
        assert_eq!(find_match_lang(t, (1, 13), rust), BracketMatch::None);
        // 注释内的 '(' / ']'（行 2——首行之后仍然生效）。
        assert_eq!(find_match_lang(t, (2, 19), rust), BracketMatch::None);
        assert_eq!(find_match_lang(t, (2, 18), rust), BracketMatch::None);
        // 对照：首行之后的真实括号照常匹配。
        let t2 = "fn main() {\n    f(a);\n}";
        assert_eq!(
            find_match_lang(t2, (1, 6), rust),
            BracketMatch::Matched {
                open: (1, 5),
                close: (1, 7)
            }
        );
    }

    #[test]
    fn whole_lang_crlf_source_file() {
        // CRLF 行尾（Windows 检出/保存形态）——'\r' 剥除后与 LF 同语义
        // （本地 Windows 验证形态的回归钉；raw-string 字面量随源文件行尾
        // 漂移的问题与此无关——那要用显式 \n 写测试文本）。
        let t = "fn main() {\r\n    let s = \"(\";\r\n}";
        let rust = Some(highlight::CodeLang::Rust);
        assert_eq!(find_match_lang(t, (1, 14), rust), BracketMatch::None);
        assert_eq!(
            find_match_lang(t, (0, 11), rust),
            BracketMatch::Matched {
                open: (0, 10),
                close: (2, 0)
            }
        );
    }

    #[test]
    fn fence_inside_whole_lang_file_keeps_fence_priority() {
        // .rs 正文里嵌的 md 围栏：围栏语言优先于 whole_lang（登记语义），
        // 且闭栏后回到 whole_lang——修复不得改变这条优先级。
        // 0: "fn main() {"     whole_lang=Rust（普通代码行——不动 walker）
        // 1: "```rust"         开围栏（walker 视角：栏外行形态 → 入栏）
        // 2: "let s = \"(\";"  栏内 Rust——字符串排除
        // 3: "```"             闭围栏（walker 视角：栏内 → 出栏）
        // 4: "let u = (1);"    回到 whole_lang=Rust——真实括号匹配
        let t = "fn main() {\n```rust\nlet s = \"(\";\n```\nlet u = (1);";
        let rust = Some(highlight::CodeLang::Rust);
        assert_eq!(find_match_lang(t, (2, 10), rust), BracketMatch::None);
        assert_eq!(find_match_lang(t, (2, 9), rust), BracketMatch::None);
        assert_eq!(
            find_match_lang(t, (4, 9), rust),
            BracketMatch::Matched {
                open: (4, 8),
                close: (4, 10)
            }
        );
    }

    #[test]
    fn crlf_and_multiline_document_roundtrip() {
        // CRLF：行内文本不含 '\r'（split('\n') 后剥尾）——列索引与编辑器
        // 的 char 列语义一致，不被 '\r' 顶偏。
        // 0: "fn main() {"  -> 7'(' 8')' 10'{'
        // 1: "    x();"      -> 5'(' 6')'
        // 2: "}"            -> 0'}'
        let t = "fn main() {\r\n    x();\r\n}";
        assert_eq!(hit(t, 0, 11), "M(0, 10)-(2, 0)"); // '{' 跨行配 '}'
        assert_eq!(hit(t, 1, 6), "M(1, 5)-(1, 6)");
        assert_eq!(hit(t, 0, 8), "M(0, 7)-(0, 8)"); // "()" 空参
    }
}
