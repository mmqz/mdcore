//! Markdown 行级语法高亮（#68 阶段 1.3；G5 扩围到围栏语言代码）。
//!
//! 纯函数、零 gpui 依赖：`highlight_line_ex(line, fence)` 输入一行
//! 文本与 [`FenceState`]（行首围栏态——含语言标注），输出行内 token
//! 区间（**char 索引**，与 `Buffer` 的 col 语义对齐）与「该行行尾的
//! 围栏状态」。
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
    /// G5：代码关键字（rust/python/js——词表见 [`CodeLang::keywords`]）。
    Keyword,
    /// G5：代码字符串字面量（成对引号段，含定界符）。
    Str,
    /// G5：代码行注释（`//` / `#` 至行尾）。
    Comment,
    /// G5：代码数字字面量（数字起头 token）。
    Number,
}

/// G5：围栏语言（最小集——rust/python/js；未标注 / 未识别语言 =
/// `CodeBlock(None)` 维持既有整行 BlockCode 底色语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeLang {
    Rust,
    Python,
    Js,
}

impl CodeLang {
    /// fence info string → 语言（大小写不敏感；别名集——py/python、
    /// js/javascript/ts）。未知标注 None（无语言块口径）。
    pub fn parse(tag: &str) -> Option<CodeLang> {
        match tag.trim().to_lowercase().as_str() {
            "rust" | "rs" => Some(CodeLang::Rust),
            "python" | "py" => Some(CodeLang::Python),
            "js" | "javascript" | "ts" => Some(CodeLang::Js),
            _ => None,
        }
    }

    /// 关键字表（无 tree-sitter——词表扫描 + 词边界判定即「正则版」
    /// 口径的最小实现）。
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            CodeLang::Rust => &[
                "fn", "let", "mut", "const", "if", "else", "match", "loop", "while", "for", "in",
                "return", "struct", "enum", "impl", "trait", "pub", "use", "mod", "crate", "self",
                "Self", "where", "async", "await", "move", "dyn", "ref", "true", "false", "as",
                "break", "continue", "type", "unsafe",
            ],
            CodeLang::Python => &[
                "def", "class", "if", "elif", "else", "for", "while", "in", "return", "import",
                "from", "as", "with", "try", "except", "finally", "raise", "lambda", "pass",
                "break", "continue", "global", "nonlocal", "yield", "assert", "del", "not", "and",
                "or", "is", "None", "True", "False",
            ],
            CodeLang::Js => &[
                "function",
                "const",
                "let",
                "var",
                "if",
                "else",
                "for",
                "while",
                "return",
                "class",
                "extends",
                "new",
                "this",
                "typeof",
                "instanceof",
                "import",
                "from",
                "export",
                "default",
                "async",
                "await",
                "yield",
                "try",
                "catch",
                "finally",
                "throw",
                "switch",
                "case",
                "break",
                "continue",
                "delete",
                "in",
                "of",
                "null",
                "undefined",
                "true",
                "false",
            ],
        }
    }

    /// 行注释前缀（rust/js `//`、python `#`）。
    fn line_comment_prefix(self) -> &'static str {
        match self {
            CodeLang::Python => "#",
            CodeLang::Rust | CodeLang::Js => "//",
        }
    }
}

/// G5：围栏行态（查找条外/内——比 bool 携带语言标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceState {
    /// 行首处于围栏外。
    Outside,
    /// 行首处于围栏内（`Some(lang)` = 有语言标注的块——正文行按语言
    /// 扫描；`None` = 未标注/未识别语言——维持整行 BlockCode）。
    CodeBlock(Option<CodeLang>),
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

/// G5：围栏开行的语言标注（`fence_marker` 已识别围栏形态的前提下，
/// 取标记后的 info string 首词解析；非围栏行 None）。
pub fn fence_lang_of(line: &str) -> Option<CodeLang> {
    let (_, fence_ch) = fence_marker(line)?;
    // info string = 围栏标记后的段（```rust 的 `rust`）；首词为语言
    //（逗号/空格后的附加 info 忽略）。
    let rest: String = line
        .chars()
        .skip_while(|c| c.is_whitespace())
        .skip_while(|c| *c == fence_ch)
        .collect();
    let tag = rest
        .trim()
        .split(|c: char| c == ',' || c.is_whitespace())
        .next()?;
    if tag.is_empty() {
        return None;
    }
    CodeLang::parse(tag)
}

/// 该行（**围栏外行首态**）是否为围栏形态行——即 [`highlight_line_ex`]
/// 在 `FenceState::Outside` 行首态下 `fence_at_end` 的全部语义
/// （开围栏标记：``` / ~~~，前导空白 ≤ 3）。
///
/// 围栏推进方在正文行的行首态被文件级语言覆盖成 `CodeBlock` 时
/// （brackets 的 whole_lang 路径），应**用它**取「本行是否开围栏」，
/// 而不是拿覆盖态高亮的行尾值——`fence_at_end` 是行首态相对的：
/// CodeBlock 行首下普通代码行恒 true（「仍在栏内」），会被误读成
/// 「本行开了围栏」（#151 CI 失败的根因）。
pub fn opens_fence(line: &str) -> bool {
    fence_marker(line).is_some()
}

/// G5：一行代码的粗高亮（无 tree-sitter——**单趟左到右扫描器**，
/// 按起位裁决：注释 → 字符串 → 关键字/数字，先到先占；char 索引区间）。
/// 未标注语言 `None` → 整行 BlockCode（既有语义零漂移）。
///
/// 起位裁决（与真实词法器同构）——多类扫描不分优先级批次，
/// 只看谁在更左的位置开始：
///   - `let u = "http://a";` → 引号起位在 `//` 之前 → 整段字符串
///     （串内 `//` 不误报注释）
///   - `# "引号" 在注释里` → `#` 起位更早 → 注释至行尾
///     （注释内引号不误报字符串）
fn highlight_code_line(runs: &mut RunCollector, chars: &[char], lang: Option<CodeLang>) {
    let len = chars.len();
    let Some(lang) = lang else {
        runs.try_insert(0..len, TokenKind::BlockCode);
        return;
    };
    let prefix: Vec<char> = lang.line_comment_prefix().chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut i = 0;
    while i < len {
        // ① 行注释前缀命中 → 至行尾，本行扫描结束。
        if chars[i..].starts_with(&prefix[..]) {
            runs.try_insert(i..len, TokenKind::Comment);
            break;
        }
        // ② 字符串：成对引号段（"..." / '...'——转义 `\` 跳过；
        // 未闭合不高亮——防 Rust 生命周期 `'a` 之类的单引号噪声）。
        let c = chars[i];
        if c == '"' || c == '\'' {
            if let Some(close) = find_close_quote(chars, i + 1, c)
                && runs.try_insert(i..close + 1, TokenKind::Str)
            {
                i = close + 1;
                continue;
            }
            i += 1;
            continue;
        }
        // ③ 词首：数字起头吞 [字母数字|.|_]（`3.14`/`0x1F` 单段）；
        // 否则按词字符取最大词 → 精确匹配关键字表（大小写敏感）。
        if (i == 0 || !word(chars[i - 1])) && word(c) {
            let start = i;
            let kind = if c.is_ascii_digit() {
                while i < len && (word(chars[i]) || chars[i] == '.') {
                    i += 1;
                }
                TokenKind::Number
            } else {
                while i < len && word(chars[i]) {
                    i += 1;
                }
                let tok: String = chars[start..i].iter().collect();
                if lang.keywords().contains(&tok.as_str()) {
                    TokenKind::Keyword
                } else {
                    continue; // 非关键字的普通词：不高亮，扫描点已推进
                }
            };
            runs.try_insert(start..i, kind);
            continue;
        }
        i += 1;
    }
}

/// 从 `from` 起找配对引号（转义跳过；未闭合 None——防单引号噪声）。
fn find_close_quote(chars: &[char], from: usize, quote: char) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2; // 转义字符整体跳过
            continue;
        }
        if chars[i] == quote {
            return Some(i);
        }
        i += 1;
    }
    None
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

/// #134：ATX 标题级别判定（`GetOutline` 的数据源——与高亮判定同源
/// 同语义：`#{1,6}` 后跟空格或行尾才成立，`#7`/`#x`/`#a#` 都不是标题）。
/// 返回 `Some(1..=6)`；非标题行 `None`。
pub fn heading_level(line: &str) -> Option<u8> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    let is_heading = (1..=6).contains(&hashes) && line.chars().nth(hashes).is_none_or(|c| c == ' ');
    if is_heading { Some(hashes as u8) } else { None }
}

/// G5：高亮一行（携带围栏语言态——唯一入口）。
///
/// - 围栏内 + 无语言标注 → 整行 BlockCode（既有语义零漂移）
/// - 围栏内 + 有语言标注 → [`highlight_code_line`] 代码扫描
///   （关键字/字符串/注释/数字；行是围栏形态仍视为闭合）
/// - 围栏外 → 既有 md 行级/行内语义不变
pub fn highlight_line_ex(line: &str, fence: FenceState) -> LineHighlight {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut runs = RunCollector::default();

    // ---- 围栏态优先 ----
    if let FenceState::CodeBlock(lang) = fence {
        // 围栏内；若本行是围栏形态则视为闭合。
        let closes = fence_marker(line).is_some();
        // 闭围栏形态行：整行 BlockCode 底色（与无语言块同视觉——闭栏
        // 标记不是代码内容，代码扫描对它恒产出空 run）。
        if closes {
            runs.try_insert(0..len, TokenKind::BlockCode);
        }
        highlight_code_line(&mut runs, &chars, lang);
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
    if heading_level(line).is_some() {
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

    // ---------------- #134：heading_level（GetOutline 数据源） ----------------

    #[test]
    fn heading_level_atx_forms() {
        assert_eq!(heading_level("# 标题"), Some(1));
        assert_eq!(heading_level("## 二级"), Some(2));
        assert_eq!(heading_level("###### 六级"), Some(6));
        // 纯井号行 = 标题（后跟行尾——ATX 允许）
        assert_eq!(heading_level("###"), Some(3));
        // 超级（7 个 #）不是标题
        assert_eq!(heading_level("####### 七个"), None);
        // # 后跟非空格不是标题（#7 是话题标签、#x 是普通文本）
        assert_eq!(heading_level("#7 话题"), None);
        assert_eq!(heading_level("#x"), None);
        // 普通行 / 空行 / 缩进标题（前导空格破坏 ATX——与高亮同语义）
        assert_eq!(heading_level("普通段落"), None);
        assert_eq!(heading_level(""), None);
        assert_eq!(heading_level(" # 缩进"), None);
    }

    /// 快捷：某行的 token 区间列表（char 索引）。bool 围栏口径 →
    /// 无语言标注态（legacy 测试语义零漂移）。
    fn tokens(line: &str, fence: bool) -> Vec<(Range<usize>, TokenKind)> {
        highlight_line_ex(
            line,
            if fence {
                FenceState::CodeBlock(None)
            } else {
                FenceState::Outside
            },
        )
        .runs
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
        let h = highlight_line_ex("```rust", FenceState::Outside);
        assert!(h.fence_at_end, "开围栏后进入 fence 态");
        assert!(h.runs.contains(&(0..3, TokenKind::BlockCode)), "``` 标记段");
        assert!(
            h.runs.contains(&(3..7, TokenKind::FenceLang)),
            "rust 语言标注段"
        );
    }

    #[test]
    fn fence_body_and_close() {
        let h = highlight_line_ex("let x = 1;", FenceState::CodeBlock(None));
        assert!(h.fence_at_end, "普通内容行不闭合");
        assert!(
            h.runs.contains(&(0..10, TokenKind::BlockCode)),
            "let x = 1; 共 10 字符"
        );

        let h = highlight_line_ex("```", FenceState::CodeBlock(None));
        assert!(!h.fence_at_end, "围栏形态行闭合");
        assert!(h.runs.contains(&(0..3, TokenKind::BlockCode)));

        // ~~~ 围栏同理。
        let h = highlight_line_ex("~~~", FenceState::CodeBlock(None));
        assert!(!h.fence_at_end);
    }

    #[test]
    fn fence_state_flows_through_lines() {
        // 模拟三行序列：开 → 内容 → 闭。
        let l0 = highlight_line_ex("```md", FenceState::Outside);
        assert!(l0.fence_at_end);
        let l1 = highlight_line_ex("**不在围栏外语义下高亮**", FenceState::CodeBlock(None));
        assert!(l1.fence_at_end);
        assert_eq!(
            l1.runs,
            vec![(0..14, TokenKind::BlockCode)],
            "围栏内整行代码底色，不进行内扫描"
        );
        let l2 = highlight_line_ex("```", FenceState::CodeBlock(None));
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

// =============================================================================
// G5 语法高亮扩展——专项单测（语言解析 / 代码扫描 / 围栏态推进）
// =============================================================================

#[cfg(test)]
mod g5_code_hl_tests {
    use super::*;

    fn code(line: &str, lang: CodeLang) -> Vec<(Range<usize>, TokenKind)> {
        highlight_line_ex(line, FenceState::CodeBlock(Some(lang))).runs
    }

    // ---------------- 语言标注解析 ----------------

    #[test]
    fn fence_lang_parse_aliases_case_insensitive() {
        assert_eq!(fence_lang_of("```rust"), Some(CodeLang::Rust));
        assert_eq!(fence_lang_of("```RS"), Some(CodeLang::Rust), "大写别名");
        assert_eq!(fence_lang_of("~~~python"), Some(CodeLang::Python));
        assert_eq!(fence_lang_of("```py"), Some(CodeLang::Python));
        assert_eq!(fence_lang_of("```js"), Some(CodeLang::Js));
        assert_eq!(fence_lang_of("```JavaScript"), Some(CodeLang::Js));
        assert_eq!(
            fence_lang_of("```ts"),
            Some(CodeLang::Js),
            "ts 归 JS 扫描器"
        );
        // 无标注 / 未知语言 / 非 fence 行 → None
        assert_eq!(fence_lang_of("```"), None);
        assert_eq!(fence_lang_of("```text"), None, "未识别语言走无语言口径");
        assert_eq!(fence_lang_of("普通行"), None);
        // 附加 info（逗号/空格后）忽略
        assert_eq!(fence_lang_of("```rust,ignore"), Some(CodeLang::Rust));
        assert_eq!(fence_lang_of("```rust ignore"), Some(CodeLang::Rust));
    }

    // ---------------- Rust 行 ----------------

    #[test]
    fn rust_keywords_numbers_comments() {
        let t = code("let count = 42; // 三个", CodeLang::Rust);
        assert!(t.contains(&(0..3, TokenKind::Keyword)), "let 关键字：{t:?}");
        assert!(
            !t.iter()
                .any(|(r, k)| *k == TokenKind::Keyword && r.start == 4),
            "count 非关键字"
        );
        assert!(t.contains(&(12..14, TokenKind::Number)), "42 数字：{t:?}");
        assert!(
            t.contains(&(16..21, TokenKind::Comment)),
            "// 至行尾：{t:?}"
        );
    }

    #[test]
    fn rust_string_with_escape() {
        let t = code(r#"let s = "he\"llo";"#, CodeLang::Rust);
        assert!(
            t.contains(&(8..17, TokenKind::Str)),
            "转义引号不提前闭合（开 8 / 闭 16）：{t:?}"
        );
    }

    #[test]
    fn rust_word_boundary_not_substring() {
        // "fn" 不命中 "fnord" 内部（词边界）
        let t = code("fnord fn", CodeLang::Rust);
        let kw_hits: Vec<_> = t.iter().filter(|(_, k)| *k == TokenKind::Keyword).collect();
        assert_eq!(kw_hits.len(), 1, "仅独立 fn 命中：{t:?}");
        assert!(t.contains(&(6..8, TokenKind::Keyword)));
    }

    // ---------------- Python / JS 行 ----------------

    #[test]
    fn python_comment_hash_and_keywords() {
        let t = code("def main(): # 注释", CodeLang::Python);
        assert!(t.contains(&(0..3, TokenKind::Keyword)), "def：{t:?}");
        assert!(t.contains(&(12..16, TokenKind::Comment)), "# 至行尾：{t:?}");
        // python 不认 // 注释
        let t2 = code("x = 1 // 2", CodeLang::Python);
        assert!(
            !t2.iter().any(|(_, k)| *k == TokenKind::Comment),
            "python 无 // 注释：{t2:?}"
        );
    }

    #[test]
    fn js_keywords_and_number_with_dot() {
        let t = code("const x = 3.14;", CodeLang::Js);
        assert!(t.contains(&(0..5, TokenKind::Keyword)), "const：{t:?}");
        assert!(
            t.contains(&(10..14, TokenKind::Number)),
            "3.14 含小数点：{t:?}"
        );
    }

    // ---------------- 优先级与无语言口径 ----------------

    #[test]
    fn comment_wins_over_string_inside_it() {
        // 注释里的引号不串字符串高亮
        let t = code(r#"// "not a string""#, CodeLang::Rust);
        assert!(
            !t.iter().any(|(_, k)| *k == TokenKind::Str),
            "注释段整体单 token：{t:?}"
        );
        assert!(t.contains(&(0..17, TokenKind::Comment)));
    }

    #[test]
    fn keyword_inside_string_not_highlighted() {
        let t = code(r#""let if while""#, CodeLang::Rust);
        assert!(
            !t.iter().any(|(_, k)| *k == TokenKind::Keyword),
            "字符串内关键字不误报：{t:?}"
        );
        assert!(t.contains(&(0..14, TokenKind::Str)));
    }

    #[test]
    fn untagged_fence_keeps_blockcode_semantics() {
        // 无语言标注：整行 BlockCode（既有语义零漂移）
        let h = highlight_line_ex("plain text here", FenceState::CodeBlock(None));
        assert_eq!(h.runs, vec![(0..15, TokenKind::BlockCode)]);
        assert!(h.fence_at_end, "体行不闭合");
        let h2 = highlight_line_ex("```", FenceState::CodeBlock(None));
        assert!(!h2.fence_at_end, "围栏形态行闭合");
    }

    #[test]
    fn fence_state_flow_with_lang_across_lines() {
        // 开（带语言）→ 体行（语言扫描）→ 闭：fence 态 + 语言态推进
        let open = highlight_line_ex("```rust", FenceState::Outside);
        assert!(open.fence_at_end);
        let body = highlight_line_ex(
            "let x = 1;",
            FenceState::CodeBlock(fence_lang_of("```rust")),
        );
        assert!(body.fence_at_end);
        assert!(
            body.runs.contains(&(0..3, TokenKind::Keyword)),
            "let：{:?}",
            body.runs
        );
        let close = highlight_line_ex("```", FenceState::CodeBlock(Some(CodeLang::Rust)));
        assert!(!close.fence_at_end);
    }

    #[test]
    fn cjk_chars_counted_as_char_indices() {
        // 中文注释与字符串按 char 计列（与 Buffer col 语义对齐）
        let t = code("// 中文注释", CodeLang::Rust);
        assert_eq!(t, vec![(0..7, TokenKind::Comment)], "2 标记+空格+4 汉字");
        let t2 = code(r#""你好""#, CodeLang::Rust);
        assert_eq!(t2, vec![(0..4, TokenKind::Str)], "2 引号 + 2 汉字");
    }

    // ---------------- 单趟扫描器：起位裁决 ----------------

    #[test]
    fn double_slash_inside_string_not_comment() {
        // 引号起位在 `//` 之前 → 整段字符串（URL/正则里的 // 不误报注释）
        let t = code(r#"let u = "http://a";"#, CodeLang::Rust);
        assert!(
            !t.iter().any(|(_, k)| *k == TokenKind::Comment),
            "串内 // 不当注释：{t:?}"
        );
        assert!(t.contains(&(8..18, TokenKind::Str)), "整段字符串：{t:?}");
    }

    #[test]
    fn quote_inside_comment_not_string() {
        // `#` 起位更早 → 注释至行尾（注释内引号不误报字符串）
        let t = code(r#"s = 'x' # "引号" 在注释里"#, CodeLang::Python);
        assert!(t.contains(&(4..7, TokenKind::Str)), "真字符串仍命中：{t:?}");
        let cmt = t
            .iter()
            .find(|(_, k)| *k == TokenKind::Comment)
            .expect("存在注释段");
        assert!(cmt.0.start < 11, "注释从 # 起至行尾：{t:?}");
        assert!(
            !t.iter().any(|(r, k)| *k == TokenKind::Str && r.start > 7),
            "注释内引号不串高亮：{t:?}"
        );
    }

    #[test]
    fn unclosed_single_quote_no_noise() {
        // Rust 生命周期 `'a` 未闭合 → 不高亮（防单引号噪声）
        let t = code(r#"&'a str"#, CodeLang::Rust);
        assert!(
            !t.iter().any(|(_, k)| *k == TokenKind::Str),
            "未闭合引号不高亮：{t:?}"
        );
    }

    #[test]
    fn closing_fence_line_keeps_blockcode_background() {
        // 带语言块的闭围栏行：整行 BlockCode（与无语言块同视觉）
        let h = highlight_line_ex("```", FenceState::CodeBlock(Some(CodeLang::Rust)));
        assert_eq!(h.runs, vec![(0..3, TokenKind::BlockCode)]);
        assert!(!h.fence_at_end);
        // 体行（非围栏形态）不受影响：仍走代码扫描
        let body = highlight_line_ex("fn f() {}", FenceState::CodeBlock(Some(CodeLang::Rust)));
        assert!(
            !body.runs.iter().any(|(_, k)| *k == TokenKind::BlockCode),
            "体行无整行底色：{:?}",
            body.runs
        );
        assert!(body.runs.contains(&(0..2, TokenKind::Keyword)));
    }
}
