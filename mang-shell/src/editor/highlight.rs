//! Markdown 行级语法高亮（#68 阶段 1.3）。
//!
//! 纯函数、零 gpui 依赖：`highlight_line(line, fence_at_start)` 输入一行
//! 文本与「该行行首是否处于代码围栏内」，输出行内 token 区间（**char
//! 索引**，与 `Buffer` 的 col 语义对齐）与「该行行尾的围栏状态」。
//! 颜色映射（`TokenKind` → `HighlightStyle`）由视图层完成——本模块
//! 只做结构识别，便于单测钉定。
//!
//! ## 覆盖面（最小可用集）
//!
//! - 行级：ATX 标题（`#` × 1-6）、引用（`> `）、无序/有序列表标记、
//!   围栏代码块（``` / ~~~，含语言标注）
//! - 行内：粗体（`**`/`__` 成对）、斜体（`*`/`_` 成对）、行内代码
//!   （`` ` `` 成对）、链接（`[text](url)`）
//! - 未闭合标记不高亮（避免单星号噪声）；行内扫描优先级：
//!   code > bold > italic > link（高优先级占位后低优先级避让）。
//!
//! ## 简化语义（登记）
//!
//! 围栏内出现 ```/~~~ 行即视为闭合（严格 CommonMark 要求闭合围栏不带
//! info string 且与开围栏同字符——此处放宽，编辑器场景误差可接受）。

use std::ops::Range;

/// 识别出的 token 类别（颜色映射在视图层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// ATX 标题整行（含 `#` 标记）。
    Heading,
    /// 粗体（成对 `**` / `__`，含定界符）。
    Bold,
    /// 斜体（成对 `*` / `_`，含定界符）。
    Italic,
    /// 行内代码（成对反引号，含定界符）。
    InlineCode,
    /// 围栏代码块内容行（整行）。
    BlockCode,
    /// 引用标记（`>` 及其前导空白）。
    Quote,
    /// 列表标记（`-` / `*` / `+` / `N.` 及其后空格）。
    ListMarker,
    /// 围栏语言标注（```rust 的 `rust`）。
    FenceLang,
    /// 链接整段（`[text](url)`）。
    Link,
}

/// 单行高亮结果。
pub struct LineHighlight {
    /// token 区间（char 索引、按 start 升序、互不重叠；
    /// 空隙即普通文本）。
    pub runs: Vec<(Range<usize>, TokenKind)>,
    /// 该行行尾是否仍处于代码围栏内（供下一行推进）。
    pub fence_at_end: bool,
}

/// 围栏行形态识别：``` 或 ~~~（前导空白 ≤ 3）。
/// 返回 (标记结束的 char 位置, 围栏字符)——非围栏行返回 `None`。
fn fence_marker(line: &str) -> Option<(usize, char)> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' && i < 3 {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    let c = chars[i];
    if c != '`' && c != '~' {
        return None;
    }
    let mut count = 0;
    let start = i;
    while i < chars.len() && chars[i] == c {
        i += 1;
        count += 1;
    }
    if count >= 3 {
        // 标记结束位置 = 前导空白 + 围栏字符数。
        Some((start + count, c))
    } else {
        None
    }
}

/// 列表标记宽度（`- ` / `* ` / `+ ` / `N. `）——非列表行返回 0。
/// 返回标记（含尾随空格）的 char 长度。
fn list_marker_len(line: &str) -> usize {
    let chars: Vec<char> = line.chars().collect();
    // 前导空白（≤3）
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' && i < 3 {
        i += 1;
    }
    if i >= chars.len() {
        return 0;
    }
    // 无序：- * + 后跟空格
    if (chars[i] == '-' || chars[i] == '*' || chars[i] == '+')
        && i + 1 < chars.len()
        && chars[i + 1] == ' '
    {
        return i + 2;
    }
    // 有序：数字+ `. `（数字段 1-9 位）
    let digits_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > digits_start
        && i < chars.len()
        && chars[i] == '.'
        && i + 1 < chars.len()
        && chars[i + 1] == ' '
    {
        return i + 2;
    }
    0
}

/// 行内 token 收集器：占位互斥（高优先级先占，重叠丢弃）。
#[derive(Default)]
struct RunCollector {
    runs: Vec<(Range<usize>, TokenKind)>,
}

impl RunCollector {
    /// 若 `range` 与已收集区间无重叠则收录（返回是否收录）。
    fn try_insert(&mut self, range: Range<usize>, kind: TokenKind) -> bool {
        if range.start >= range.end {
            return false;
        }
        let overlap = self
            .runs
            .iter()
            .any(|(r, _)| range.start < r.end && r.start < range.end);
        if overlap {
            return false;
        }
        self.runs.push((range, kind));
        true
    }

    /// 排序输出。
    fn sorted(mut self) -> Vec<(Range<usize>, TokenKind)> {
        self.runs.sort_by_key(|(r, _)| r.start);
        self.runs
    }
}

/// 在 `chars` 中从 `from` 起找下一个 `needle`（跳过转义 `\`），
/// 返回命中位置（None = 未找到）。
fn find_unescaped(chars: &[char], from: usize, needle: &[char]) -> Option<usize> {
    let mut i = from;
    while i + needle.len() <= chars.len() {
        if chars[i] == '\\' {
            i += 2; // 跳过转义字符
            continue;
        }
        if chars[i..i + needle.len()] == *needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 行内扫描（code → bold → italic → link 优先级占位）。
/// `from`：扫描起点（跳过行级标记前缀）。
fn scan_inline(runs: &mut RunCollector, chars: &[char], from: usize) {
    // 行内代码：`...`
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '`'
            && let Some(close) = find_unescaped(chars, i + 1, &['`'])
        {
            runs.try_insert(i..close + 1, TokenKind::InlineCode);
            i = close + 1;
            continue;
        }
        i += 1;
    }
    // 粗体：**...** / __...__
    i = from;
    while i < chars.len() {
        let delim: [char; 2] = if chars[i] == '*' {
            ['*', '*']
        } else if chars[i] == '_' {
            ['_', '_']
        } else {
            i += 1;
            continue;
        };
        if chars[i..].starts_with(&delim)
            && let Some(close) = find_unescaped(chars, i + 2, &delim)
            && runs.try_insert(i..close + 2, TokenKind::Bold)
        {
            i = close + 2;
            continue;
        }
        i += 1;
    }
    // 斜体：*...* / _..._（成对定界符如 ** 已被粗体扫描消费或需跳过——
    // 未闭合粗体不退化为斜体：**abc 无 bold 时 italic 跳过 ** 序列）。
    i = from;
    while i < chars.len() {
        let d = chars[i];
        if (d == '*' || d == '_') && i + 1 < chars.len() && chars[i + 1] == d {
            i += 2; // 成对定界符（未配对成功的粗体残留）——整体跳过
            continue;
        }
        if (d == '*' || d == '_')
            && i + 1 < chars.len()
            && let Some(close) = find_unescaped(chars, i + 1, &[d])
            && runs.try_insert(i..close + 1, TokenKind::Italic)
        {
            i = close + 1;
            continue;
        }
        i += 1;
    }
    // 链接：[text](url)
    i = from;
    while i < chars.len() {
        if chars[i] == '['
            && let Some(bracket) = find_unescaped(chars, i + 1, &[']'])
            && bracket + 1 < chars.len()
            && chars[bracket + 1] == '('
            && let Some(paren) = find_unescaped(chars, bracket + 2, &[')'])
            && runs.try_insert(i..paren + 1, TokenKind::Link)
        {
            i = paren + 1;
            continue;
        }
        i += 1;
    }
}

/// 高亮一行。
///
/// - `line`：行文本（无换行符）
/// - `fence_at_start`：该行行首是否处于代码围栏内
///
/// 返回 token 区间（char 索引）与行尾围栏状态。
pub fn highlight_line(line: &str, fence_at_start: bool) -> LineHighlight {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut runs = RunCollector::default();

    // ---- 围栏态优先 ----
    if fence_at_start {
        // 围栏内：整行 BlockCode；若本行是围栏形态则视为闭合。
        let closes = fence_marker(line).is_some();
        runs.try_insert(0..len, TokenKind::BlockCode);
        return LineHighlight {
            runs: runs.sorted(),
            fence_at_end: !closes,
        };
    }
    if let Some((mark_end, _)) = fence_marker(line) {
        // 开围栏：标记段 BlockCode + 语言段 FenceLang（互斥分段——
        // 整行占位会挤掉语言标注）。
        let lang_end = chars
            .iter()
            .rposition(|c| !c.is_whitespace())
            .map(|p| p + 1)
            .unwrap_or(mark_end);
        runs.try_insert(0..mark_end, TokenKind::BlockCode);
        if lang_end > mark_end {
            runs.try_insert(mark_end..lang_end, TokenKind::FenceLang);
        }
        return LineHighlight {
            runs: runs.sorted(),
            fence_at_end: true,
        };
    }

    // ---- ATX 标题：#{1,6} 后跟空格或行尾 ----
    let hashes = chars.iter().take_while(|c| **c == '#').count();
    if (1..=6).contains(&hashes) && (hashes == len || chars[hashes] == ' ') {
        runs.try_insert(0..len, TokenKind::Heading);
        return LineHighlight {
            runs: runs.sorted(),
            fence_at_end: false,
        };
    }

    // ---- 引用标记 ----
    let mut scan_from = 0;
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' && i < 3 {
        i += 1;
    }
    if i < chars.len() && chars[i] == '>' {
        // 标记含前导空白 + '>' + 紧随空格。
        let mark_end = if i + 1 < chars.len() && chars[i + 1] == ' ' {
            i + 2
        } else {
            i + 1
        };
        runs.try_insert(0..mark_end, TokenKind::Quote);
        scan_from = mark_end;
    } else {
        // ---- 列表标记（引用行内不再判列表）----
        let m = list_marker_len(line);
        if m > 0 {
            runs.try_insert(0..m, TokenKind::ListMarker);
            scan_from = m;
        }
    }

    // ---- 行内扫描（标记后的正文区）----
    scan_inline(&mut runs, &chars, scan_from);

    LineHighlight {
        runs: runs.sorted(),
        fence_at_end: false,
    }
}

// =============================================================================
// 单元测试 —— 结构识别全绿是本模块的交付门
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 快捷：某行的 token 区间列表（char 索引）。
    fn tokens(line: &str, fence: bool) -> Vec<(Range<usize>, TokenKind)> {
        highlight_line(line, fence).runs
    }

    // ---------------- 标题 ----------------

    #[test]
    fn heading_levels_1_to_6() {
        for level in 1..=6 {
            let line = format!("{} 标题{}", "#".repeat(level), level);
            let t = tokens(&line, false);
            assert_eq!(t.len(), 1, "H{level} 整行单一 token");
            assert_eq!(t[0].1, TokenKind::Heading);
            assert_eq!(t[0].0, 0..line.chars().count());
        }
    }

    #[test]
    fn seven_hashes_is_not_heading() {
        // >6 个 # 不是标题——但 7 个 # 开头按普通文本处理（无 token）。
        let t = tokens("####### x", false);
        assert!(t.iter().all(|(_, k)| *k != TokenKind::Heading));
    }

    #[test]
    fn hash_without_space_is_not_heading() {
        let t = tokens("#hashtag", false);
        assert!(t.iter().all(|(_, k)| *k != TokenKind::Heading));
    }

    // ---------------- 粗体 / 斜体 ----------------

    #[test]
    fn bold_star_pair() {
        // a **b** c → 粗体含定界符 col 2..7
        let t = tokens("a **b** c", false);
        assert!(t.contains(&(2..7, TokenKind::Bold)));
    }

    #[test]
    fn bold_underscore_and_cjk() {
        let t = tokens("__中文__", false);
        assert!(
            t.contains(&(0..6, TokenKind::Bold)),
            "下划线粗体 + CJK char 计数"
        );
    }

    #[test]
    fn italic_single_star() {
        // a *b* c → 斜体 col 2..5
        let t = tokens("a *b* c", false);
        assert!(t.contains(&(2..5, TokenKind::Italic)));
    }

    #[test]
    fn unclosed_markers_not_highlighted() {
        let t = tokens("a * b", false);
        assert!(t.is_empty(), "单个星号不成对不产生 token");
        let t = tokens("a **b c", false);
        assert!(t.is_empty(), "未闭合粗体不产生 token");
    }

    #[test]
    fn bold_takes_priority_over_italic() {
        // **a** 整段粗体，内部不再产生斜体 token。
        let t = tokens("**a**", false);
        assert_eq!(t, vec![(0..5, TokenKind::Bold)]);
    }

    // ---------------- 行内代码 ----------------

    #[test]
    fn inline_code_pair() {
        let t = tokens("用 `run()` 执行", false);
        // `run()` = col 2..8（含定界符 6 字符）
        assert!(
            t.contains(&(2..9, TokenKind::InlineCode)),
            "`run()` 含双反引号共 7 字符：col 2..9"
        );
    }

    #[test]
    fn code_inside_bold_still_code() {
        // code 优先占位：**a `x` b** 中 `x` 仍是 code。
        let t = tokens("**a `x` b**", false);
        assert!(
            t.iter()
                .any(|(r, k)| *k == TokenKind::InlineCode && *r == (4..7))
        );
    }

    // ---------------- 引用 / 列表 ----------------

    #[test]
    fn quote_marker() {
        let t = tokens("> 引用内容", false);
        assert!(t.contains(&(0..2, TokenKind::Quote)), "标记 > + 空格");
        // 正文区继续行内扫描
        let t = tokens("> **粗** 引用", false);
        assert!(t.contains(&(0..2, TokenKind::Quote)));
        assert!(
            t.contains(&(2..7, TokenKind::Bold)),
            "**粗** 共 5 字符：col 2..7"
        );
    }

    #[test]
    fn list_markers_all_forms() {
        assert!(tokens("- 项", false).contains(&(0..2, TokenKind::ListMarker)));
        assert!(tokens("* 项", false).contains(&(0..2, TokenKind::ListMarker)));
        assert!(tokens("+ 项", false).contains(&(0..2, TokenKind::ListMarker)));
        assert!(tokens("12. 项", false).contains(&(0..4, TokenKind::ListMarker)));
        // 缩进列表（≤3 空格）
        assert!(tokens("  - 项", false).contains(&(0..4, TokenKind::ListMarker)));
    }

    #[test]
    fn minus_without_space_not_list() {
        let t = tokens("-负号", false);
        assert!(t.iter().all(|(_, k)| *k != TokenKind::ListMarker));
    }

    // ---------------- 围栏代码块 ----------------

    #[test]
    fn fence_open_with_lang() {
        let h = highlight_line("```rust", false);
        assert!(h.fence_at_end, "开围栏后进入 fence 态");
        assert!(h.runs.contains(&(0..3, TokenKind::BlockCode)), "``` 标记段");
        assert!(
            h.runs.contains(&(3..7, TokenKind::FenceLang)),
            "rust 语言标注段"
        );
    }

    #[test]
    fn fence_body_and_close() {
        let h = highlight_line("let x = 1;", true);
        assert!(h.fence_at_end, "普通内容行不闭合");
        assert!(
            h.runs.contains(&(0..10, TokenKind::BlockCode)),
            "let x = 1; 共 10 字符"
        );

        let h = highlight_line("```", true);
        assert!(!h.fence_at_end, "围栏形态行闭合");
        assert!(h.runs.contains(&(0..3, TokenKind::BlockCode)));

        // ~~~ 围栏同理。
        let h = highlight_line("~~~", true);
        assert!(!h.fence_at_end);
    }

    #[test]
    fn fence_state_flows_through_lines() {
        // 模拟三行序列：开 → 内容 → 闭。
        let l0 = highlight_line("```md", false);
        assert!(l0.fence_at_end);
        let l1 = highlight_line("**不在围栏外语义下高亮**", l0.fence_at_end);
        assert!(l1.fence_at_end);
        assert_eq!(
            l1.runs,
            vec![(0..14, TokenKind::BlockCode)],
            "围栏内整行代码底色，不进行内扫描"
        );
        let l2 = highlight_line("```", l1.fence_at_end);
        assert!(!l2.fence_at_end);
    }

    // ---------------- 链接 ----------------

    #[test]
    fn link_full_form() {
        // [文本](地址) 整段 Link。
        let t = tokens("见 [文档](https://a.b) 说明", false);
        assert!(
            t.contains(&(2..19, TokenKind::Link)),
            "[文档](https://a.b) 共 17 字符：col 2..19"
        );
    }

    #[test]
    fn link_missing_paren_not_highlighted() {
        let t = tokens("[文档](https://a.b", false);
        assert!(t.iter().all(|(_, k)| *k != TokenKind::Link));
    }

    // ---------------- 纯文本 / 空行 ----------------

    #[test]
    fn plain_and_empty_lines_have_no_tokens() {
        assert!(tokens("", false).is_empty());
        assert!(tokens("普通文本 123", false).is_empty());
    }

    #[test]
    fn escaped_marker_not_highlighted() {
        // \*a\*：转义星号不构成斜体。
        let t = tokens("\\*a\\*", false);
        assert!(t.iter().all(|(_, k)| *k != TokenKind::Italic));
    }
}
