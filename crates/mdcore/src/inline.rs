//! Markdown 行内 marks 子集解析（v0）——issue #15（A3/P0）立项。
//!
//! # 定位
//!
//! 模型（[`crate::tree::BlockTree`]）是 **markdown 源文本层**：块级解析
//! （[`crate::markdown::markdown_to_tree`]）不剥行内定界符，CM6 同步协议
//! （快路径 `replace_text` 直写原始行文本）以此为基石。行内格式因此是
//! **消费时派生**（读侧解析），不是存储态——本模块提供该派生的单一
//! 真源，供 docx 导出面（`mdcore-wasm/src/docx_export.rs`）等发射层
//! 消费；html/xlsx 发射层如需行内语义，共用本模块，不另立第二解析器。
//!
//! # 支持子集（v0，CommonMark 子集 + GFM 扩展）
//!
//! | 源文本 | mark kind | 备注 |
//! |---|---|---|
//! | `**粗体**` | `strong` | 双星定界 |
//! | `*斜体*` | `em` | 单星；词内允许（CommonMark `*` 语义） |
//! | `_斜体_` | `em` | 下划线；**词内禁用**（`snake_case_name` 防误伤——CommonMark `_` 语义） |
//! | `***粗斜***` | `strong` + `em` | 三星 = 双 mark 叠加（同区间） |
//! | `` `代码` `` | `code` | 单反引号对；内容逐字（内部不再解析任何定界符） |
//! | `$数学$` | `math` | #261：单 dollar 对；净文本内容逐字（code 同构，含 pass B 叠加口径——内部 `*`/`_` 仍可产 em/strong mark，同 `` `a*b*c``` 既有行为）；opener 后随非空白 / closer 前邻非空白（`$5` 价格缘拒配）；`$$` 连跑整跑字面量（显示数学是块级域——markdown.rs）；`\$` 转义 |
//! | `~~删除~~` | `del` | GFM 双波浪线（恰 2——1/3+ 按字面量） |
//! | `<u>下划线</u>` | `u` | 小写精确标签对 |
//!
//! mark kind 词汇 = [`crate::op`] `SetMark` 自由字符串域（与 html.rs
//! 行内子集、docx 导出面 `mark_style` 五位映射一致）。
//!
//! # 定界符配对算法（closer 驱动 + 等长配对，CommonMark 核心简化）
//!
//! 1. **无歧义对先行**（pass A）：转义 `\x`（x ∈ `* _ ~ \` < \`）与
//!    code span、`<u>` 对——字面量定界、无栈参与；被转义/被消费字符
//!    对后续定界符扫描掩码。
//! 2. **强调栈**（pass B）：`*` / `_` / `~` 的**极大定界符连跑**
//!    （长度 1-3 参与配对，`*`/`_` 4+ 整跑字面量；`~` 仅恰 2）。
//!    closer 优先：能闭合时向栈内回溯找**同字符等长** opener（混合
//!    长度不配对——`**a*` 整体字面量，不猜测）；配对后 opener/closer
//!    连跑全部消费，区间落 mark。等长规则下 3↔3 配对产出
//!    strong+em 双叠加（嵌套语义经 mark 交叠免费获得——runs_from
//!    五位覆盖集合并语义）。
//! 3. **flanking（v0 简化）**：opener 需后随非空白；closer 需前邻
//!    非空白；`_` 另需词边界（前/后邻非字母数字）。
//!
//! # 未闭合 / 不可配对 → 字面量（不猜测）
//!
//! 无匹配 closer 的 opener、无匹配 opener 的 closer、越界长度连跑、
//! 空 code span、空 `<u>` 对、未闭合标签——定界符全部按原字符保留在
//! 净文本中（issue #15 验收要求：未闭合标记按 markdown 语义当字面量）。
//!
//! # 坐标语义
//!
//! 输出 marks 的 `start`/`end` 是**净文本**（剥除被消费定界符后）的
//! char 偏移，与 [`crate::tree::MarkSpan`]（op 面文本坐标系）同构——
//! 消费方（如 docx 导出面 `runs_from`）可直接共用。
//!
//! # 已知边界（登记，v0 不做）
//!
//! - 多字符 code span（``` ``a`b`` ```）——单反引号对足够覆盖编辑器
//!   工具条（Mod-e 单星包裹）与常规文档；
//! - 词内 `$` 数字缘（`$5$` / `总价 $5，另一处 $6$`）——flanking 仅挡空白缘
//!   不挡数字缘（`$5^2$` 是合法数学，数字缘挡则误伤）；价格语境建议转义
//!   `\$` 或依赖无闭合落字面量（`$5 and $10` 第二 `$` 前邻空白天然拒配）；
//! - 链接/图片 `[t](u)`——模型无链接载荷（html.rs 同口径登记）；
//! - HTML 行内标签（除 `<u>`）——透明字面量；
//! - 嵌套**部分消费**（CommonMark 混合长度拆跑）——等长配对子集化；
//! - 自动链接、实体引用、HTML 转义——面外。

use crate::tree::MarkSpan;

/// 行内 marks 解析：markdown 源文本 → (净文本, char 偏移 marks)。
///
/// 纯函数、零分配快路径（无定界符时仅一次 `Vec<char>` 物化——调用方
/// 为发射层，频度 = 导出/渲染次数，非每键击）。语义与边界见模块文档。
pub fn parse_inline_marks(text: &str) -> (String, Vec<MarkSpan>) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    // 消费掩码：true = 定界符机器字符（转义反斜杠、已配对定界符连跑、
    // <u> 标签字面量）——净文本剔除。被转义字符本身不消费（字面保留）
    // 但以 escaped 掩码跳过定界符扫描。
    let mut consumed = vec![false; len];
    let mut escaped = vec![false; len];
    // 原始坐标 span（消费后统一映射到净坐标）。
    let mut spans: Vec<(usize, usize, &'static str)> = Vec::new();

    // ---- pass A：转义 / code span / <u> 对（无歧义，先于强调栈）----
    let mut i = 0usize;
    while i < len {
        let c = chars[i];
        if c == '\\' && i + 1 < len && is_escapable(chars[i + 1]) {
            consumed[i] = true; // 反斜杠本身剔除
            escaped[i + 1] = true; // 后随字符字面量化 + 掩码
            i += 2;
        } else if c == '`' {
            // 单反引号 code span：找下一个未转义反引号；内容非空才配对。
            if let Some(j) = (i + 1..len).find(|&j| chars[j] == '`' && !escaped[j]) {
                if j > i + 1 {
                    spans.push((i + 1, j, "code"));
                    consumed[i] = true;
                    consumed[j] = true;
                    i = j + 1;
                    continue;
                }
            }
            i += 1; // 未闭合 / 空内容 → 字面量
        } else if starts_with_tag(&chars[i..], "<u>") {
            // <u> 对：字面量标签定界（大小写敏感、精确形）。
            if let Some(j) = find_close_tag(&chars[i + 3..], "</u>") {
                let close_at = i + 3 + j;
                if close_at > i + 3 {
                    spans.push((i + 3, close_at, "u"));
                    for c in &mut consumed[i..i + 3] {
                        *c = true;
                    }
                    for c in &mut consumed[close_at..close_at + 4] {
                        *c = true;
                    }
                    i = close_at + 4;
                    continue;
                }
            }
            i += 1; // 未闭合 / 空内容 → 字面量
        } else if c == '$' && !escaped[i] {
            // $...$ 行内数学（#261）：单 dollar 对，净文本内容逐字
            // （code span 同构——含 pass B 在内部叠加 em/strong 的既有
            // 口径，文本不受影响）。
            // 连跑规则：极大 $ 连跑长度 1 参与配对，≥2 整跑字面量
            // （`$$` 是块级显示数学定界——markdown.rs 域，行内不消费；
            // pass B `*`/`_` 4+ 同构裁决）。
            let mut run_end = i + 1;
            while run_end < len && chars[run_end] == '$' && !escaped[run_end] {
                run_end += 1;
            }
            if run_end - i == 1 {
                // 找下一个「孤 $」closer：自身未转义，且两侧都不是 $
                // （连跑成员不可作 closer——`$x$$y` 不许拿连跑尾配对）。
                if let Some(j) = (i + 1..len).find(|&j| {
                    chars[j] == '$'
                        && !escaped[j]
                        && (j + 1 >= len || chars[j + 1] != '$')
                        && chars[j - 1] != '$'
                }) {
                    // 内容非空 + flanking（opener 后随非空白 / closer 前邻
                    // 非空白——`$5 and $10` 第二 `$` 前邻空白拒配；
                    // 边界位视同满足，同 flanking() 口径）。
                    let after_open = i + 1 < len && !chars[i + 1].is_whitespace();
                    let before_close = !chars[j - 1].is_whitespace();
                    if j > i + 1 && after_open && before_close {
                        spans.push((i + 1, j, "math"));
                        consumed[i] = true;
                        consumed[j] = true;
                        i = j + 1;
                        continue;
                    }
                }
            }
            // 连跑 ≥2 / 未闭合 / 空内容 / flanking 拒绝 → 整跑字面量。
            i = run_end;
        } else {
            i += 1;
        }
    }

    // ---- pass B：强调定界符栈（closer 驱动 + 等长配对）----
    // 栈项：(run 起, run 止, 字符, 长度)——run 止为开区间端点。
    let mut openers: Vec<(usize, usize, char, usize)> = Vec::new();
    let mut i = 0usize;
    while i < len {
        let c = chars[i];
        if c != '*' && c != '_' && c != '~' {
            i += 1;
            continue;
        }
        // 极大连跑（被掩码字符截断——转义/已消费字符不参与定界符）。
        let run_start = i;
        let mut run_end = i;
        while run_end < len && chars[run_end] == c && !escaped[run_end] && !consumed[run_end] {
            run_end += 1;
        }
        let run_len = run_end - run_start;
        if run_len == 0 {
            // 首字符即掩码 → 步进（防死循环）。
            i += 1;
            continue;
        }
        // 参与长度域：* / _ → 1-3；~ → 恰 2。越域整跑字面量。
        let in_domain = if c == '~' { run_len == 2 } else { run_len <= 3 };
        if in_domain {
            let (can_open, can_close) = flanking(&chars, run_start, run_end, c);
            if can_close {
                // 回溯找同字符等长 opener（近者赢；混合长度不拆跑）。
                if let Some(pos) = openers
                    .iter()
                    .rposition(|&(_, _, oc, ol)| oc == c && ol == run_len)
                {
                    let (o_start, o_end, _, o_len) = openers.remove(pos);
                    for c in &mut consumed[o_start..o_end] {
                        *c = true;
                    }
                    for c in &mut consumed[run_start..run_end] {
                        *c = true;
                    }
                    // 1 → em；2 → strong；3 → strong + em 叠加；
                    // `~` 域内长度恒 2 → del（GFM 删除线）。
                    if c == '~' {
                        spans.push((o_end, run_start, "del"));
                    } else {
                        if o_len >= 2 {
                            spans.push((o_end, run_start, "strong"));
                        }
                        if o_len % 2 == 1 {
                            spans.push((o_end, run_start, "em"));
                        }
                    }
                    i = run_end;
                    continue;
                }
            }
            if can_open {
                openers.push((run_start, run_end, c, run_len));
            }
            // 既不可开也不可闭（或可闭但无匹配且不可开）→ 整跑字面量。
        }
        i = run_end;
    }

    // ---- 净文本构建 + 坐标映射 ----
    // clean_of[orig] = 净文本中该原始位的偏移（消费位顺延不增）。
    let mut clean_of = vec![0usize; len + 1];
    let mut clean = String::with_capacity(len);
    for k in 0..len {
        clean_of[k] = clean.chars().count();
        if !consumed[k] {
            clean.push(chars[k]);
        }
    }
    clean_of[len] = clean.chars().count();

    let mut marks: Vec<MarkSpan> = spans
        .into_iter()
        .filter_map(|(s, e, kind)| {
            // 防御：净区间为空（内容全被内层消费）→ 丢弃该 mark。
            let (cs, ce) = (clean_of[s], clean_of[e]);
            (cs < ce).then(|| MarkSpan {
                start: cs,
                end: ce,
                kind: kind.to_string(),
            })
        })
        .collect();
    // 输出定序（start, end, kind）——消费方（runs_from 覆盖集）不依赖
    // 顺序，但 API 确定性让回归断言与潜在顺序敏感消费方免于漂移。
    marks.sort_by(|a, b| (a.start, a.end, &a.kind).cmp(&(b.start, b.end, &b.kind)));
    (clean, marks)
}

/// 转义域（CommonMark ASCII 标点子集——本模块涉及的定界符 + 反斜杠
/// 自身；#261 起 `$` 入域）。域外 `\x` 原样保留（反斜杠字面量，非转义）。
fn is_escapable(c: char) -> bool {
    matches!(c, '*' | '_' | '~' | '`' | '<' | '\\' | '$')
}

/// chars 是否以标签字面量开头（精确、大小写敏感）。
fn starts_with_tag(chars: &[char], tag: &str) -> bool {
    let tag_chars: Vec<char> = tag.chars().collect();
    chars.len() >= tag_chars.len() && chars[..tag_chars.len()] == tag_chars[..]
}

/// 在 chars 中找闭合标签字面量起点（精确、大小写敏感）。
fn find_close_tag(chars: &[char], tag: &str) -> Option<usize> {
    let tag_chars: Vec<char> = tag.chars().collect();
    if chars.len() < tag_chars.len() {
        return None;
    }
    (0..=chars.len() - tag_chars.len()).find(|&i| chars[i..i + tag_chars.len()] == tag_chars[..])
}

/// flanking 判定（v0 简化）：opener 需后随非空白；closer 需前邻非空白；
/// `_` 另需词边界（词内禁用——`snake_case` 防误伤）。边界位（首/尾）
/// 视同满足该侧约束（None 邻 = 无约束）。
fn flanking(chars: &[char], run_start: usize, run_end: usize, c: char) -> (bool, bool) {
    let prev = if run_start > 0 {
        Some(chars[run_start - 1])
    } else {
        None
    };
    let next = if run_end < chars.len() {
        Some(chars[run_end])
    } else {
        None
    };
    let non_ws_prev = prev.is_none_or(|p| !p.is_whitespace());
    let non_ws_next = next.is_none_or(|n| !n.is_whitespace());
    let word_boundary_prev = prev.is_none_or(|p| !p.is_alphanumeric());
    let word_boundary_next = next.is_none_or(|n| !n.is_alphanumeric());
    let (mut can_open, mut can_close) = (non_ws_next, non_ws_prev);
    if c == '_' {
        can_open &= word_boundary_prev;
        can_close &= word_boundary_next;
    }
    (can_open, can_close)
}

// ---------------------------------------------------------------------------
// 测试（解析器矩阵——发射层集成测试见 mdc-wasm docx_export 与
// tests/tr15_inline_marks.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> (String, Vec<(usize, usize, String)>) {
        let (clean, marks) = parse_inline_marks(text);
        let flat = marks
            .iter()
            .map(|m| (m.start, m.end, m.kind.clone()))
            .collect();
        (clean, flat)
    }

    #[test]
    fn plain_text_unchanged() {
        let (clean, marks) = one("plain text 普通文本, 123.");
        assert_eq!(clean, "plain text 普通文本, 123.");
        assert!(marks.is_empty());
    }

    #[test]
    fn strong_basic() {
        let (clean, marks) = one("总计人民币 **壹佰贰拾万元整**。");
        assert_eq!(clean, "总计人民币 壹佰贰拾万元整。");
        assert_eq!(marks, vec![(6, 13, "strong".to_string())]);
    }

    #[test]
    fn em_star_and_underscore() {
        let (c1, m1) = one("*需求解析*");
        assert_eq!(
            (c1.as_str(), m1),
            ("需求解析", vec![(0, 4, "em".to_string())])
        );
        let (c2, m2) = one("_需求解析_");
        assert_eq!(
            (c2.as_str(), m2),
            ("需求解析", vec![(0, 4, "em".to_string())])
        );
    }

    #[test]
    fn underscore_intraword_forbidden_snake_case_safe() {
        // 词内 `_` 禁用：snake_case 标识符零误伤（CommonMark `_` 语义）。
        let (clean, marks) = one("my_var_name = 1");
        assert_eq!(clean, "my_var_name = 1");
        assert!(marks.is_empty());
    }

    #[test]
    fn triple_is_strong_plus_em_overlay() {
        let (clean, marks) = one("***重要***");
        assert_eq!(clean, "重要");
        assert_eq!(
            marks,
            vec![(0, 2, "em".to_string()), (0, 2, "strong".to_string())]
        );
    }

    #[test]
    fn nesting_via_overlay() {
        // **a *b* c** → strong 全程 + em 内层（交叠叠加语义）。
        let (clean, marks) = one("**a *b* c**");
        assert_eq!(clean, "a b c");
        assert_eq!(
            marks,
            vec![(0, 5, "strong".to_string()), (2, 3, "em".to_string())]
        );
    }

    #[test]
    fn code_span_verbatim_no_inner_parsing() {
        let (clean, marks) = one("调用 `f(**x)` 函数");
        assert_eq!(clean, "调用 f(**x) 函数");
        assert_eq!(marks, vec![(3, 9, "code".to_string())]);
    }

    #[test]
    fn gfm_del() {
        let (clean, marks) = one("~~废弃条款~~");
        assert_eq!(clean, "废弃条款");
        assert_eq!(marks, vec![(0, 4, "del".to_string())]);
    }

    #[test]
    fn underline_tag() {
        let (clean, marks) = one("<u>下划线文本</u>");
        assert_eq!(clean, "下划线文本");
        assert_eq!(marks, vec![(0, 5, "u".to_string())]);
    }

    #[test]
    fn unclosed_all_literal() {
        // 未闭合 → 字面量（不猜测）——issue #15 验收要求。
        let (clean, marks) = one("**未闭合 加粗");
        assert_eq!(clean, "**未闭合 加粗");
        assert!(marks.is_empty());
        let (c2, m2) = one("单星 * 未闭合");
        assert_eq!(c2, "单星 * 未闭合");
        assert!(m2.is_empty());
        let (c3, m3) = one("<u>未闭合下划线");
        assert_eq!(c3, "<u>未闭合下划线");
        assert!(m3.is_empty());
        let (c4, m4) = one("尾随 `code");
        assert_eq!(c4, "尾随 `code");
        assert!(m4.is_empty());
    }

    #[test]
    fn mixed_length_no_pairing() {
        // 等长配对子集：**a* 不拆跑——整体字面量（保守降级，登记边界）。
        let (clean, marks) = one("**a*");
        assert_eq!(clean, "**a*");
        assert!(marks.is_empty());
    }

    #[test]
    fn run_over_three_literal() {
        let (clean, marks) = one("****四星****");
        assert_eq!(clean, "****四星****");
        assert!(marks.is_empty());
    }

    #[test]
    fn tilde_domain_exactly_two() {
        let (c1, m1) = one("~单波浪~");
        assert_eq!(c1, "~单波浪~");
        assert!(m1.is_empty());
        let (c2, m2) = one("~~~三星~~~");
        assert_eq!(c2, "~~~三星~~~");
        assert!(m2.is_empty());
    }

    #[test]
    fn backslash_escape() {
        let (clean, marks) = one("\\*字面量星号\\* 不加粗");
        assert_eq!(clean, "*字面量星号* 不加粗");
        assert!(marks.is_empty());
        // 转义反斜杠本身：`\\*` → 字面 `\` + 定界符 `*` 可参与配对。
        let (c2, m2) = one("\\\\*em 域*");
        assert_eq!(c2, "\\em 域");
        assert_eq!(m2, vec![(1, 5, "em".to_string())]);
    }

    #[test]
    fn empty_content_literal() {
        // 空内容（定界符紧邻）→ 字面量。
        let (c1, m1) = one("****");
        assert_eq!(c1, "****");
        assert!(m1.is_empty());
        let (c2, m2) = one("a `` b");
        assert_eq!(c2, "a `` b");
        assert!(m2.is_empty());
        let (c3, m3) = one("<u></u>");
        assert_eq!(c3, "<u></u>");
        assert!(m3.is_empty());
    }

    #[test]
    fn adjacent_code_and_strong_overlay() {
        // **`x`** → strong + code 双叠加（原始坐标 span 交叠，净坐标重合）。
        let (clean, marks) = one("**`x`**");
        assert_eq!(clean, "x");
        assert_eq!(
            marks,
            vec![(0, 1, "code".to_string()), (0, 1, "strong".to_string())]
        );
    }

    #[test]
    fn multiple_spans_same_line() {
        let (clean, marks) = one("**加粗** 与 *斜体* 与 `code` 同行");
        assert_eq!(clean, "加粗 与 斜体 与 code 同行");
        assert_eq!(
            marks,
            vec![
                (0, 2, "strong".to_string()),
                (5, 7, "em".to_string()),
                (10, 14, "code".to_string())
            ]
        );
    }

    #[test]
    fn cjk_flanking_full_width_punctuation() {
        // CJK 全角标点邻接：非空白 → 可配对（中文文档主流形态）。
        let (clean, marks) = one("（**加粗**）");
        assert_eq!(clean, "（加粗）");
        assert_eq!(marks, vec![(1, 3, "strong".to_string())]);
    }

    #[test]
    fn star_intraword_allowed() {
        // * 词内允许（CommonMark `*` 语义）——a*b*c → em("b")。
        let (clean, marks) = one("a*b*c");
        assert_eq!(clean, "abc");
        assert_eq!(marks, vec![(1, 2, "em".to_string())]);
    }

    #[test]
    fn opener_before_whitespace_not_opener() {
        // opener 后随空白 → 不可开（CommonMark 左侧 flanking）。
        let (clean, marks) = one("** 加粗 **");
        assert_eq!(clean, "** 加粗 **");
        assert!(marks.is_empty());
    }

    #[test]
    fn closer_after_opener_unmatched_then_opener_role() {
        // closer 无匹配且可开 → 转 opener 角色（CommonMark 复用语义）。
        // `*a *b*` → 首个 * 无 closer... 逐步：*@0 可开入栈，*@3 可闭
        // 匹配 *@0？等长 1↔1 → em(1,4)="a *b"?? —— 不对：*@3 的 prev
        // 是空格吗？`*a *b*` chars: *,a,␠,*,b,* → *@3 prev=␠ → 不可闭；
        // next='b' → 可开 → 入栈。*@5 prev='b' 可闭 → 回溯等长 opener：
        // 最近的是 *@3 → em(4,5)="b"。*@0 留栈未配对 → 字面量。
        let (clean, marks) = one("*a *b*");
        assert_eq!(clean, "*a b");
        assert_eq!(marks, vec![(3, 4, "em".to_string())]);
    }

    #[test]
    fn two_strong_pairs_sequential() {
        let (clean, marks) = one("**一** 中间 **二**");
        assert_eq!(clean, "一 中间 二");
        assert_eq!(
            marks,
            vec![(0, 1, "strong".to_string()), (5, 6, "strong".to_string())]
        );
    }

    #[test]
    fn strong_closer_cjk_colon() {
        // closer 后随全角冒号（非空白）→ 可闭（标书报价页主流邻接形）。
        let (clean, marks) = one("**壹佰贰拾万元整**：含税总价");
        assert_eq!(clean, "壹佰贰拾万元整：含税总价");
        assert_eq!(marks, vec![(0, 7, "strong".to_string())]);
    }

    // ---- $...$ 行内数学（#261）----

    #[test]
    fn math_basic() {
        let (clean, marks) = one("$E=mc^2$ 是质能方程");
        assert_eq!(clean, "E=mc^2 是质能方程");
        assert_eq!(marks, vec![(0, 6, "math".to_string())]);
    }

    #[test]
    fn math_two_pairs_sequential() {
        let (clean, marks) = one("$a$ 与 $b$ 并列");
        assert_eq!(clean, "a 与 b 并列");
        assert_eq!(
            marks,
            vec![(0, 1, "math".to_string()), (4, 5, "math".to_string())]
        );
    }

    #[test]
    fn math_price_single_dollar_literal() {
        // 无闭合 → 字面量（$5 价格不误伤）。
        let (clean, marks) = one("总价 $5 元");
        assert_eq!(clean, "总价 $5 元");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_price_pair_flanking_reject() {
        // closer 前邻空白 → 拒配 → 两个 $ 全字面量（价格语境主防线）。
        let (clean, marks) = one("price $5 and $10 total");
        assert_eq!(clean, "price $5 and $10 total");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_opener_flanking_reject() {
        // opener 后随空白 → 拒配（`$ a $` 不猜）。
        let (clean, marks) = one("公式 $ a $ 不是数学");
        assert_eq!(clean, "公式 $ a $ 不是数学");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_unclosed_literal() {
        let (clean, marks) = one("没闭合 $x 的公式");
        assert_eq!(clean, "没闭合 $x 的公式");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_escaped_dollar() {
        // `\$` 转义：反斜杠剔除、$ 字面保留（`$` 入转义域——#261）。
        let (clean, marks) = one(r"\$5\$ 不配对");
        assert_eq!(clean, "$5$ 不配对");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_double_dollar_run_literal_inline() {
        // `$$` 连跑 = 块级显示数学域，行内整跑字面量。
        let (clean, marks) = one("行内 $$x$$ 不消费");
        assert_eq!(clean, "行内 $$x$$ 不消费");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_run_member_not_closer() {
        // 连跑成员不可作 closer：`$x$$y` 孤 $ 无孤 closer → 全字面量。
        let (clean, marks) = one("$x$$y");
        assert_eq!(clean, "$x$$y");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_empty_pair_literal() {
        // 空内容（`$$` 相邻）→ 连跑字面量（块级域；空 math 不可表达）。
        let (clean, marks) = one("空 $$ 对");
        assert_eq!(clean, "空 $$ 对");
        assert!(marks.is_empty());
    }

    #[test]
    fn math_code_span_priority_by_position() {
        // 左先赢：反引号在前的 code span 先消费，其后 $ 无闭合落字面量。
        let (clean, marks) = one("`$x` 后面 $y$");
        assert_eq!(clean, "$x 后面 y");
        assert_eq!(
            marks,
            vec![(0, 2, "code".to_string()), (6, 7, "math".to_string())]
        );
    }

    #[test]
    fn math_inside_code_span_not_paired() {
        // code span 内的 $ 是内容（i 跳过区域不扫描）。
        let (clean, marks) = one("`$a$` 代码内不配对");
        assert_eq!(clean, "$a$ 代码内不配对");
        assert_eq!(marks, vec![(0, 3, "code".to_string())]);
    }

    #[test]
    fn math_pass_b_overlay_code_isomorphic() {
        // pass B 叠加口径（code 同构）：内部 `*` 产 em，净文本逐字。
        // 输出定序 (start,end,kind)：math(0,3) 先于 em(1,2)。
        let (clean, marks) = one("$a*b*c$");
        assert_eq!(clean, "abc");
        assert_eq!(
            marks,
            vec![(0, 3, "math".to_string()), (1, 2, "em".to_string())]
        );
    }

    #[test]
    fn math_cjk_content() {
        let (clean, marks) = one("设 $速度v$ 为常量");
        assert_eq!(clean, "设 速度v 为常量");
        assert_eq!(marks, vec![(2, 5, "math".to_string())]);
    }
}
