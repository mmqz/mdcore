//! Buffer 的正则查找面（#132 任务 2）——**分离 impl**（跨文件 `impl Buffer`）。
//!
//! 为什么单开一个文件：[`Buffer`] 主体已 1100+ 行，正则查找是**完全独立**
//! 的一块能力（只读全量扫描，不碰编辑原语/undo/坐标换算）。拆开的好处：
//!
//! - 主文件零结构改动（只在 `impl Buffer` 前加一行 `mod buffer_find;`），
//!   大段插入的拼接风险归零；
//! - 「查找相关的方法在这里」一眼可见；
//! - 私有方法（`line`/`line_count`）在本 crate 内可见，分离 impl 照常访问。
//!
//! 依赖：`regex`（mang-shell `Cargo.lock` 既有闭包——gpui 传递依赖，
//! 零新增 crate）。

use super::buffer::{Buffer, Point, byte_to_char};

impl Buffer {
    /// **正则查找**（#132 任务 2）：返回 `(起点, 终点)` 区间（文档序；
    /// 每行独立匹配——与 G1 查找一致的行内语义，不跨行）。
    ///
    /// - 编译失败 → `Err(原因)`：调用方据此呈现**红边框提示**（不静默
    ///   失败——issue #132 明确要求）；
    /// - 选项面与 [`Buffer::find_matches_opts`] 对齐：`case_sensitive` 经
    ///   `(?i)` 前缀施加，`whole_word` 经 `\b` 包裹；
    /// - 零宽匹配（如 `^$`、`a*`）保留——渲染层按零长处理；
    /// - 空模式 → 空结果（与 `find_matches` 的空 query 语义一致）。
    pub fn find_matches_regex(
        &self,
        pattern: &str,
        case_sensitive: bool,
        whole_word: bool,
    ) -> Result<Vec<(Point, Point)>, String> {
        if pattern.is_empty() {
            return Ok(Vec::new());
        }
        let source = build_regex_pattern(pattern, case_sensitive, whole_word);
        let re = regex::Regex::new(&source).map_err(|e| regex_error_message(pattern, &e))?;
        let mut hits = Vec::new();
        for li in 0..self.line_count() {
            let line = self.line(li);
            for m in re.find_iter(&line) {
                let start = byte_to_char(&line, m.start());
                let end = byte_to_char(&line, m.end());
                hits.push((Point::new(li, start), Point::new(li, end)));
            }
        }
        Ok(hits)
    }
}

/// 正则**是否可编译**（只校验不扫描——输入过程中的实时校验用，避免每键
/// 全文扫描；完整匹配由 [`Buffer::find_matches_regex`] 在需要时算）。
pub fn regex_check(pattern: &str) -> Result<(), String> {
    if pattern.is_empty() {
        return Ok(());
    }
    regex::Regex::new(pattern)
        .map(|_| ())
        .map_err(|e| regex_error_message(pattern, &e))
}

/// 正则源串装配（选项面与 [`Buffer::find_matches_opts`] 对齐）。
///
/// - 大小写不敏感：`(?i)` 前缀（regex crate 原生 Unicode 感知——比逐字符
///   lowercase 更接近 VS Code 语义）；
/// - 全词：`\b(?:…)\b`——用非捕获组，不改变用户分组的编号；
/// - 用户模式**原样嵌入**（不转义——正则模式就是要写正则）。
fn build_regex_pattern(pattern: &str, case_sensitive: bool, whole_word: bool) -> String {
    let mut s = String::new();
    if !case_sensitive {
        s.push_str("(?i)");
    }
    if whole_word {
        s.push_str(r"\b(?:");
        s.push_str(pattern);
        s.push_str(r")\b");
    } else {
        s.push_str(pattern);
    }
    s
}

/// regex crate 编译错误 → 用户能照着改的提示。
///
/// 面板只显示一条简明原因（`regex::Error` 原文是多行 + 带位置箭头，塞不下），
/// 完整原文由调用方写 stderr 证据行。
fn regex_error_message(pattern: &str, err: &regex::Error) -> String {
    let raw = err.to_string();
    let detail = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("regex parse error"))
        .unwrap_or("语法错误")
        .to_string();
    format!("正则无效：{detail}（模式 {}）", snippet(pattern))
}

/// 模式片段（面板简短显示——超长截断）。
fn snippet(pattern: &str) -> String {
    const CAP: usize = 24;
    if pattern.chars().count() <= CAP {
        return format!("{pattern:?}");
    }
    let head: String = pattern.chars().take(CAP).collect();
    format!("{head}…")
}

// =============================================================================
// 单元测试（#132 任务 2：正则查找面——区间、选项、错误、Unicode、零宽）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }

    fn buf(text: &str) -> Buffer {
        Buffer::from_plain_text(text)
    }

    #[test]
    fn regex_ranges_are_real_spans_not_just_starts() {
        let b = buf("let x = 1;\nlet y = 22;");
        let hits = b.find_matches_regex(r"\d+", true, false).expect("可编译");
        // 真实区间：一位数 vs 两位数（只看起点会让替换截错长度）
        assert_eq!(hits, vec![(p(0, 8), p(0, 9)), (p(1, 8), p(1, 10))]);
    }

    #[test]
    fn regex_case_insensitive_by_default() {
        let b = buf("Foo foo FOO");
        let ci = b.find_matches_regex("foo", false, false).expect("可编译");
        assert_eq!(ci.len(), 3, "大小写不敏感三处");
        let cs = b.find_matches_regex("foo", true, false).expect("可编译");
        assert_eq!(cs.len(), 1, "大小写敏感一处");
    }

    #[test]
    fn regex_whole_word_wraps_word_boundary() {
        let b = buf("foo foobar foo");
        let ww = b.find_matches_regex("foo", true, true).expect("可编译");
        assert_eq!(ww.len(), 2, "foobar 的 foo 前缀不算全词");
        assert_eq!(ww[0].0, p(0, 0));
        assert_eq!(ww[1].0, p(0, 11));
    }

    #[test]
    fn invalid_regex_returns_actionable_error() {
        let b = buf("anything");
        let err = b
            .find_matches_regex("(unclosed", true, false)
            .expect_err("非法正则必须报错而不是静默");
        assert!(err.contains("正则无效"), "错误消息可读：{err}");
        assert!(err.contains("unclosed"), "err 里带上用户写的模式：{err}");
        assert_eq!(
            regex_check("(unclosed"),
            Err(err),
            "实时校验与全量查找同一错误口径"
        );
        assert!(regex_check("[a-z]+").is_ok(), "合法正则通过");
        assert!(regex_check("").is_ok(), "空模式不报错");
    }

    #[test]
    fn empty_pattern_yields_no_hits() {
        let b = buf("abc");
        assert!(b.find_matches_regex("", true, false).unwrap().is_empty());
    }

    #[test]
    fn regex_matches_unicode_and_cjk() {
        let b = buf("中文查找\n查找中文");
        let hits = b.find_matches_regex("查找", true, false).expect("可编译");
        assert_eq!(
            hits,
            vec![(p(0, 2), p(0, 4)), (p(1, 0), p(1, 2))],
            "CJK 按 char 计列（与 Buffer 列语义一致）"
        );
    }

    #[test]
    fn zero_width_matches_are_kept() {
        let b = buf("ab\ncd");
        let hits = b.find_matches_regex("^", true, false).expect("可编译");
        assert_eq!(
            hits,
            vec![(p(0, 0), p(0, 0)), (p(1, 0), p(1, 0))],
            "零宽起点保留（渲染层按零长处理）"
        );
    }
}
