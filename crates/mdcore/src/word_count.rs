//! 文档字数统计（issue #29 N-3）——标书评审口径的引擎级单一真源。
//!
//! # 为什么在引擎
//!
//! 字数统计是文档语义的一部分（Lead 裁决）：块树（BlockTree）持有全部
//! 内容与结构真相，任何视图/导出侧自行数数都会口径漂移。本模块把口径
//! 钉在核心，wasm/宿主只消费不重算。
//!
//! # 口径表（钉定；测试矩阵 + 独立 Python 参考实现双背书）
//!
//! | 项 | 字段 | 定义 |
//! |---|---|---|
//! | 汉字数 | [`WordCount::cjk_chars`] | CJK 统一表意文字字符数。码位区间**恰两段**：URO `U+4E00..=U+9FFF`（20,992 字，[`CJK_URO_START`]/[`CJK_URO_END`]）+ 扩展 A `U+3400..=U+4DBF`（6,592 字，[`CJK_EXT_A_START`]/[`CJK_EXT_A_END`]）。**其余一律不计汉字**：CJK 标点（U+3000 区段，如 〇 U+3007/U+3001 、）、全角字母数字（U+FF00 区段，如 １２３）、扩展 B..H（U+20000+，如 𠀀）、兼容表意（U+F900 区段）——它们只进 [`WordCount::total_chars`]。 |
//! | 西文词数 | [`WordCount::western_words`] | 连续拉丁字母/数字序列（`[A-Za-z0-9]+` 极大匹配）算一个词。分隔符 = 一切非 ASCII 字母数字字符：空格/标点/下划线（`my_var` = 2 词）/撇号（`don't` = 2 词）/全角字符（全角数字 １２３ 非西文词）。 |
//! | 总字数 | [`WordCount::total_words`] | `cjk_chars + western_words`（**标书评审口径**，单独成字段恒等于两分项之和）。 |
//! | 总字符数 | [`WordCount::total_chars`] | 全部非空白字符数（空白判定 = `char::is_whitespace`，含 U+3000 全角空格/Tab/换行）。 |
//! | 段落数 | [`WordCount::paragraphs`] | 块树 `BlockKind::Paragraph` 节点计数（与标题互斥分计）。 |
//! | 标题数 | [`WordCount::headings`] | 块树 `BlockKind::Heading` 节点计数。 |
//! | 表格数 | [`WordCount::tables`] | 块树 `BlockKind::Table` 节点计数。 |
//!
//! # 计数语义裁决（本模块钉定，测试锁死）
//!
//! - **统计基面 = 净文本投影**：行内标记（`**`/`*`/`_`/`` ` ``/`~~`/`<u>`）
//!   经 [`crate::inline::parse_inline_marks`] 剥除（已配对定界符剔除；未闭合
//!   /不可配对按字面量保留——issue #15 语义）。块级语法（标题 `#` 前缀、
//!   代码围栏、列表标记、引用 `>` 前缀）在 markdown 解析（[`crate::markdown`]）
//!   时已剥出块 text——不重复计。`node.marks` 非空（op 面 SetMark 路径，
//!   text 已是净文本）时按原文直计（与 wasm 导出面 `effective_inline`
//!   同规）。
//! - **代码块计入**：CodeBlock 文本按字面计入汉字/西文/字符三项（文档
//!   内容语义——标书的伪代码/配置清单同样占篇幅），不计入段落/标题/
//!   表格三个结构计数。
//! - **表格按单元格文本计入**：管道行拆分（剥前导/尾随 `|`、未转义 `|`
//!   分隔、`\|` 反转义、单元格 trim）+ GFM 分隔行（`:?-+:?` 形）剔除
//!   （仅首个数据行之后紧跟的一行）——语义与 wasm 导出面
//!   `docx_export::table_rows` 同规（核心内独立实现，三域纪律禁反向
//!   依赖 wasm crate）。管道符与分隔行本身不计入任何计数。单元格内容
//!   恒按源文本做行内投影（marks 覆盖面不映射进单元格——产品路径
//!   表格恒为源文本态）。
//! - **列表项单独类**：ListItem 文本计入三项字符/词统计，但**不并入
//!   段落数**（BlockKind 分计——块树语义即文档语义；引用内 Paragraph
//!   仍计段落）。
//! - **引用容器透明**：Blockquote 递归子块（子 Paragraph 计入段落数）；
//!   空引用（无子块，op 面防御态）按自身 text 计入三项统计但不计
//!   段落数——镜像 [`crate::markdown`] 序列化语义。
//! - **ThematicBreak / Document**：零文本（Document 虚根透明递归）。
//!
//! # 独立参考实现（验收双实现路径）
//!
//! 40 万字级验收 = 本实现 vs `test-artifacts/word-count/count_ref.py`
//! （Python 正则路径，独立实现同一口径表）——见
//! `crates/mdcore/tests/n3_word_count.rs` 与 `test-artifacts/word-count/README.md`。

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::inline::parse_inline_marks;
use crate::tree::{BlockKind, BlockTree, MarkSpan, NodeId};

/// CJK 统一表意（URO）区间起点：U+4E00（一）。
pub const CJK_URO_START: u32 = 0x4E00;
/// CJK 统一表意（URO）区间终点：U+9FFF（鿿）。
pub const CJK_URO_END: u32 = 0x9FFF;
/// CJK 扩展 A 区间起点：U+3400（㐀）。
pub const CJK_EXT_A_START: u32 = 0x3400;
/// CJK 扩展 A 区间终点：U+4DBF（䶿）。
pub const CJK_EXT_A_END: u32 = 0x4DBF;

/// 汉字判定（口径表「汉字数」的代码锚点）：CJK 统一表意 URO
/// （`U+4E00..=U+9FFF`）+ 扩展 A（`U+3400..=U+4DBF`）恰两段。
/// 其余（CJK 标点/全角形式/扩展 B+/兼容表意）返回 false——见模块文档。
pub fn is_cjk_ideograph(c: char) -> bool {
    let cp = c as u32;
    (CJK_URO_START..=CJK_URO_END).contains(&cp) || (CJK_EXT_A_START..=CJK_EXT_A_END).contains(&cp)
}

/// 文档字数统计结果（口径表见模块文档；全部字段确定性输出）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordCount {
    /// 汉字数（URO + 扩展 A）。
    pub cjk_chars: usize,
    /// 西文词数（`[A-Za-z0-9]+` 极大匹配）。
    pub western_words: usize,
    /// 总字数 = 汉字数 + 西文词数（标书评审口径）。
    pub total_words: usize,
    /// 总字符数（全部非空白字符）。
    pub total_chars: usize,
    /// 段落数（BlockKind::Paragraph 计数；与标题互斥分计）。
    pub paragraphs: usize,
    /// 标题数（BlockKind::Heading 计数）。
    pub headings: usize,
    /// 表格数（BlockKind::Table 计数）。
    pub tables: usize,
}

/// 三项文本计数累加器（词内状态跨字符、不跨块/单元格边界）。
#[derive(Default)]
struct TextCount {
    cjk_chars: usize,
    western_words: usize,
    total_chars: usize,
}

impl TextCount {
    /// 计入一段净文本：汉字逐字、西文极大词匹配、非空白字符逐个。
    /// 词状态不跨调用（每块/每单元格独立成词域——块边界即词边界）。
    fn add_str(&mut self, s: &str) {
        let mut in_word = false;
        for c in s.chars() {
            if is_cjk_ideograph(c) {
                self.cjk_chars += 1;
                in_word = false;
            } else if c.is_ascii_alphanumeric() {
                if !in_word {
                    self.western_words += 1;
                    in_word = true;
                }
            } else {
                in_word = false;
            }
            if !c.is_whitespace() {
                self.total_chars += 1;
            }
        }
    }
}

/// 结构三计数（段落/标题/表格——块树语义分计）。
#[derive(Default)]
struct StructureCount {
    paragraphs: usize,
    headings: usize,
    tables: usize,
}

/// 净文本投影（effective_inline 同规）：`marks` 空 = markdown 源文本态
/// → 解析剥定界符；`marks` 非空 = op 面净文本态 → 原文直计。
fn net_text<'a>(text: &'a str, marks: &[MarkSpan]) -> Cow<'a, str> {
    if marks.is_empty() {
        Cow::Owned(parse_inline_marks(text).0)
    } else {
        Cow::Borrowed(text)
    }
}

/// 单管道行 → 单元格序列：剥前导/尾随 `|`；未转义 `|` 分隔；`\|` 反转义
/// 为 `|`；单元格 trim；孤立 `\` 原样透传。（与 wasm 导出面
/// `docx_export::split_pipe_row` 同规——核心内独立实现。）
pub fn split_pipe_row_pub(line: &str) -> Vec<String> {
    let s = line.strip_prefix('|').unwrap_or(line);
    let s = s.strip_suffix('|').unwrap_or(s);
    let mut cells: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c == '|' {
                cur.push('|');
            } else {
                cur.push('\\');
                cur.push(c);
            }
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur = String::new();
        } else {
            cur.push(c);
        }
    }
    if esc {
        cur.push('\\');
    }
    cells.push(cur.trim().to_string());
    cells
}

/// GFM 分隔行判定（每个 trim 后单元格 `:?-+:?` 形）。
pub fn is_separator_row_pub(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            let core = t.strip_prefix(':').unwrap_or(t);
            let core = core.strip_suffix(':').unwrap_or(core);
            !core.is_empty() && core.chars().all(|ch| ch == '-')
        })
}

/// 表格块文本 → 逐单元格计数：空白行跳过；首个数据行后紧跟的分隔形行
/// 剔除（其余位置的分隔形行按内容行计——镜像导出面 `table_rows` 语义）；
/// 单元格内容做行内投影后计入。
fn count_table(text: &str, t: &mut TextCount) {
    let mut rows_emitted = 0usize;
    for line in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        if line.trim().is_empty() {
            continue;
        }
        let cells = split_pipe_row_pub(line);
        if rows_emitted == 1 && is_separator_row_pub(&cells) {
            continue;
        }
        rows_emitted += 1;
        for cell in &cells {
            let net = net_text(cell, &[]);
            t.add_str(&net);
        }
    }
}

fn walk(tree: &BlockTree, id: NodeId, t: &mut TextCount, s: &mut StructureCount) {
    let node = tree.node(id);
    match node.kind {
        // 虚根透明递归（top_level 已绕过；op 面禁止 Document 子块——防御）。
        BlockKind::Document => {
            for &c in &node.children {
                walk(tree, c, t, s);
            }
        }
        BlockKind::Paragraph => {
            s.paragraphs += 1;
            let text = node.text.to_string();
            let net = net_text(&text, &node.marks);
            t.add_str(&net);
        }
        BlockKind::Heading => {
            s.headings += 1;
            let text = node.text.to_string();
            let net = net_text(&text, &node.marks);
            t.add_str(&net);
        }
        BlockKind::ListItem => {
            // 文本计入三项统计；不并入段落数（口径裁决，见模块文档）。
            let text = node.text.to_string();
            let net = net_text(&text, &node.marks);
            t.add_str(&net);
            for &c in &node.children {
                walk(tree, c, t, s);
            }
        }
        BlockKind::Blockquote => {
            // 子块优先（解析路径：子 Paragraph 计段落）；空引用按自身
            // text 计三项（op 面防御态，不计段落）。
            if node.children.is_empty() {
                let text = node.text.to_string();
                let net = net_text(&text, &node.marks);
                t.add_str(&net);
            }
            for &c in &node.children {
                walk(tree, c, t, s);
            }
        }
        BlockKind::CodeBlock => {
            // 字面计入（无行内投影——代码逐字）；不计结构三计数。
            t.add_str(&node.text.to_string());
        }
        // 数学块同构 CodeBlock：LaTeX 源逐字计入（#261——定界符是
        // 字面内容，非行内投影面）；不计结构三计数。
        BlockKind::Math => {
            t.add_str(&node.text.to_string());
        }
        BlockKind::Table => {
            s.tables += 1;
            count_table(&node.text.to_string(), t);
        }
        BlockKind::ThematicBreak => {}
    }
}

/// 块树字数统计（口径表与计数语义裁决见模块文档）。
/// 纯函数、确定性、零 panic 面（任何 op 面构造的树都可统计）。
pub fn count(tree: &BlockTree) -> WordCount {
    let mut t = TextCount::default();
    let mut s = StructureCount::default();
    for &child in tree.top_level() {
        walk(tree, child, &mut t, &mut s);
    }
    WordCount {
        cjk_chars: t.cjk_chars,
        western_words: t.western_words,
        total_words: t.cjk_chars + t.western_words,
        total_chars: t.total_chars,
        paragraphs: s.paragraphs,
        headings: s.headings,
        tables: s.tables,
    }
}

// ---------------------------------------------------------------------------
// 测试（口径矩阵——40 万字级验收见 tests/n3_word_count.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Attrs, MarkSpan};
    use crate::Core;

    fn wc(md: &str) -> WordCount {
        count(&crate::markdown::markdown_to_tree(md))
    }

    #[test]
    fn pure_cjk_paragraph() {
        let w = wc("中文汉字计数");
        assert_eq!(w.cjk_chars, 6);
        assert_eq!(w.western_words, 0);
        assert_eq!(w.total_words, 6);
        assert_eq!(w.total_chars, 6);
        assert_eq!((w.paragraphs, w.headings, w.tables), (1, 0, 0));
    }

    #[test]
    fn pure_western_words() {
        let w = wc("hello world 123");
        assert_eq!(w.western_words, 3);
        assert_eq!(w.cjk_chars, 0);
        assert_eq!(w.total_words, 3);
        // h,e,l,l,o(5) + w,o,r,l,d(5) + 1,2,3(3) = 13
        assert_eq!(w.total_chars, 13);
    }

    #[test]
    fn mixed_cjk_and_western() {
        let w = wc("共 3 个 API 接口");
        assert_eq!(w.cjk_chars, 4); // 共/个/接/口
        assert_eq!(w.western_words, 2); // "3" + "API"
        assert_eq!(w.total_words, 6);
        assert_eq!(w.total_chars, 8); // 共3个API接口
    }

    #[test]
    fn whitespace_excluded_everywhere() {
        let w = wc("a  b\t\tc\n\n \u{3000}");
        assert_eq!(w.western_words, 3);
        assert_eq!(w.total_chars, 3); // 全角空格 U+3000 亦为空白
    }

    #[test]
    fn inline_marks_stripped_net_text() {
        let w = wc("**加粗** 与 *斜体* 与 `code` 同行");
        // 净文本 = "加粗 与 斜体 与 code 同行"
        assert_eq!(w.cjk_chars, 8);
        assert_eq!(w.western_words, 1);
        assert_eq!(w.total_words, 9);
        assert_eq!(w.total_chars, 12); // 8 汉字 + "code" 4
    }

    #[test]
    fn unclosed_marks_literal_kept() {
        let w = wc("**未闭合 加粗");
        // 未闭合定界符按字面量保留（#15 语义）→ 星号计入字符数。
        assert_eq!(w.cjk_chars, 5); // 未闭合加粗
        assert_eq!(w.total_chars, 7); // 5 汉字 + 2 星号（空格为空白）
        assert_eq!(w.western_words, 0);
    }

    #[test]
    fn code_span_content_verbatim() {
        let w = wc("调用 `f(**x)` 函数");
        // 净文本 = "调用 f(**x) 函数"——code span 内部不解析。
        assert_eq!(w.cjk_chars, 4);
        assert_eq!(w.western_words, 2); // f / x
        assert_eq!(w.total_chars, 10);
    }

    #[test]
    fn code_block_counted_verbatim() {
        let w = wc("```rust\nlet x = 1; // 注释\n```");
        assert_eq!(w.western_words, 3); // let / x / 1
        assert_eq!(w.cjk_chars, 2); // 注释
        assert_eq!(w.total_words, 5);
        assert_eq!(w.total_chars, 11); // let(3)+x+=+1+;+//(2)+注释(2)
        assert_eq!((w.paragraphs, w.headings, w.tables), (0, 0, 0));
    }

    #[test]
    fn table_cells_counted_structure_symbols_not() {
        let w = wc("| 名称 | 数量 |\n| --- | --- |\n| **主机** | 3 台 |");
        assert_eq!(w.tables, 1);
        assert_eq!(w.cjk_chars, 7); // 名称/数量/主机/台
        assert_eq!(w.western_words, 1); // 3
        assert_eq!(w.total_chars, 8); // 管道符与分隔行不计
        assert_eq!(w.paragraphs, 0);
    }

    #[test]
    fn table_escaped_pipe_unescaped_to_literal() {
        let w = wc("| a \\| b | c |\n| :--- | ---: |\n| 1 | 2 |");
        // 单元格 "a | b"（\| 反转义）；对齐冒号分隔行剔除。
        assert_eq!(w.western_words, 5); // a/b/c/1/2
        assert_eq!(w.total_chars, 6); // a,|,b,c,1,2
        assert_eq!(w.cjk_chars, 0);
    }

    #[test]
    fn table_separator_only_after_first_row_dropped() {
        // 非首行后位置的分隔形行 = 内容行（镜像导出面 table_rows 语义）。
        let w = wc("| a | b |\n| 1 | 2 |\n| --- | --- |");
        assert_eq!(w.western_words, 4); // a/b/1/2（"---" 非词）
        assert_eq!(w.total_chars, 10); // a,b,1,2 + 6 个 '-'
        assert_eq!(w.tables, 1);
    }

    #[test]
    fn list_items_counted_not_paragraphs() {
        let w = wc("- 甲级资质\n- 乙级资质\n1. 丙级资质");
        assert_eq!(w.cjk_chars, 12);
        assert_eq!(w.paragraphs, 0);
        assert_eq!(w.western_words, 0);
    }

    #[test]
    fn blockquote_child_paragraph_counted() {
        let w = wc("> 引用文本说明");
        assert_eq!(w.paragraphs, 1); // 引用内 Paragraph 计段落
        assert_eq!(w.cjk_chars, 6);
    }

    #[test]
    fn heading_counted_exclusively() {
        let w = wc("# 第一章 总体方案");
        assert_eq!(w.headings, 1);
        assert_eq!(w.paragraphs, 0);
        assert_eq!(w.cjk_chars, 7); // "#" 前缀不在块 text 内
    }

    #[test]
    fn empty_document_all_zero() {
        let w = count(&BlockTree::new());
        assert_eq!(w, WordCount::default());
        let core = Core::new();
        assert_eq!(core.word_count(), WordCount::default());
    }

    #[test]
    fn thematic_break_zero_contribution() {
        let w = wc("---\n\n***\n\n正文");
        assert_eq!((w.paragraphs, w.headings, w.tables), (1, 0, 0));
        assert_eq!(w.cjk_chars, 2);
        assert_eq!(w.total_chars, 2);
    }

    #[test]
    fn boundary_fullwidth_digits_and_cjk_punctuation() {
        // 全角数字：非 ASCII 字母数字（非西文词）、非汉字——只进字符数。
        let w = wc("１２３４５");
        assert_eq!(w.western_words, 0);
        assert_eq!(w.cjk_chars, 0);
        assert_eq!(w.total_chars, 5);
        // CJK 标点（、。「」）与 CJK 零（〇 U+3007）：非汉字——只进字符数。
        let w = wc("、。「」〇");
        assert_eq!(w.cjk_chars, 0);
        assert_eq!(w.total_chars, 5);
        // 连续标点（3 逗 + 3 句 = 6 字符，零词零汉字）。
        let w = wc("，，，。。。");
        assert_eq!(w.cjk_chars, 0);
        assert_eq!(w.western_words, 0);
        assert_eq!(w.total_chars, 6);
    }

    #[test]
    fn boundary_cjk_range_ends_exact() {
        // 扩展 A 两端 + URO 两端恰计入。
        let w = wc("㐀䶿一鿿");
        assert_eq!(w.cjk_chars, 4);
        // 区间外一格：U+33FF（扩展 A 前哨）与 U+A000（URO 后哨）不计。
        let w = wc("\u{33FF}\u{A000}");
        assert_eq!(w.cjk_chars, 0);
        assert_eq!(w.total_chars, 2);
    }

    #[test]
    fn boundary_extension_b_not_counted() {
        // 扩展 B（U+20000，𠀀）不在口径内——登记性钉定（如需纳入扩展
        // 区必须同步改口径表 + 参考实现 + 本测试）。
        let w = wc("𠀀𠀁");
        assert_eq!(w.cjk_chars, 0);
        assert_eq!(w.total_chars, 2);
    }

    #[test]
    fn word_separators_underscore_apostrophe_hyphen() {
        let w = wc("my_var_name = 1 don't stop ISO-9001");
        // 下划线/撇号/连字符 = 词分隔符（口径表西文词定义）。
        // my/var/name/1/don/t/stop/ISO/9001 = 9 词。
        assert_eq!(w.western_words, 9);
    }

    #[test]
    fn total_words_is_sum_invariant() {
        let md = "# 标题\n\n段落 one two\n\n> 引用 three\n\n- 项目 four\n\n```text\ncode five\n```\n\n| a | b |\n| --- | --- |\n| 甲 | 乙 |";
        let w = wc(md);
        assert_eq!(w.total_words, w.cjk_chars + w.western_words);
        // 结构分计：段落 2（段落 + 引用内）+ 标题 1 + 表格 1；列表/代码不计。
        assert_eq!((w.paragraphs, w.headings, w.tables), (2, 1, 1));
    }

    #[test]
    fn marks_present_text_counted_verbatim() {
        // op 面 SetMark 路径：marks 非空 = text 已净（effective_inline 同规），
        // 不再做行内投影——定界符若残留按字面计。
        let mut tree = BlockTree::new();
        let marks = vec![MarkSpan {
            start: 0,
            end: 4,
            kind: "strong".into(),
        }];
        let n = tree.create_node(
            crate::tree::BlockKind::Paragraph,
            "**加粗文本**",
            Attrs::new(),
            marks,
        );
        let root = tree.root();
        let len = tree.node(root).children.len();
        tree.insert_child(root, len, n).expect("attach");
        let w = count(&tree);
        assert_eq!(w.cjk_chars, 4);
        assert_eq!(w.total_chars, 8); // 4 汉字 + 4 星号（净文本态不剥）
        assert_eq!(w.paragraphs, 1);
    }

    #[test]
    fn core_word_count_forwards_tree_count() {
        let core = Core::from_markdown("## 第二节\n\n正文内容 mixed 42\n");
        let w = core.word_count();
        assert_eq!(w.headings, 1);
        assert_eq!(w.paragraphs, 1);
        assert_eq!(w.cjk_chars, 7); // 第二节(3) + 正文内容(4)
        assert_eq!(w.western_words, 2); // mixed / 42
        assert_eq!(w, count(&core.tree));
    }

    #[test]
    fn word_boundary_not_crossing_blocks_or_cells() {
        // 块/单元格边界 = 词边界：两个块各含 "ab" → 2 词（非 1）。
        let w = wc("ab\ncd");
        assert_eq!(w.western_words, 2);
        let w = wc("| ab | cd |\n| --- | --- |");
        assert_eq!(w.western_words, 2);
    }
}
