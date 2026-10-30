//! 查找条（Ctrl+F）—— 纯逻辑状态机与输入处理（#68 阶段 1.2 查找）。
//!
//! 设计为 `Editor` 的内嵌状态（非独立 Entity）：编辑区与查找条是同一
//! 视图内的两个焦点域（各自 `FocusHandle`），避免实体间回调接线。
//! 本模块只做数据与键语义；渲染在 `super::Editor`。
//!
//! - `query` 输入光标按 **char 索引**（与 `Buffer` col 语义一致）。
//! - `matches` 为缓存：query 变化或文档编辑后由 `refresh` 重算
//!   （大小写不敏感，全量重扫——批 1 规模可接受，阶段 6 大文档再增量）。
//! - 打开时以当前选中文本初始化 query（经典编辑器行为）。

use gpui::Keystroke;

use super::buffer::{Buffer, Point};

/// 查找条状态。
#[derive(Clone, Debug, Default)]
pub struct FindState {
    /// 查询文本。
    query: String,
    /// query 输入光标（char 索引）。
    cursor_col: usize,
    /// 匹配点缓存（文档序）。
    matches: Vec<Point>,
    /// 当前激活匹配下标（`matches` 空时无意义）。
    active: usize,
}

/// 查找条按键处理结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindKeyOutcome {
    /// 已处理（纯光标移动等，无需重算匹配）。
    Handled,
    /// 已处理且 query 变化（需 refresh 重算匹配）。
    QueryChanged,
    /// Enter——跳下一匹配。
    JumpForward,
    /// Shift+Enter——跳上一匹配。
    JumpBackward,
    /// Esc——关闭查找条。
    Close,
}

impl FindState {
    /// 打开查找条：以当前选中文本初始化 query（无选区则空），
    /// 激活匹配取光标后方最近的一个。
    pub fn open(&mut self, buffer: &Buffer) {
        let selected = buffer.selection_text();
        if !selected.is_empty() && !selected.contains('\n') {
            self.query = selected;
        } else {
            self.query.clear();
        }
        self.cursor_col = self.query.chars().count();
        self.refresh(buffer);
        self.seek_active_after_cursor(buffer);
    }

    /// 重算匹配（query 或文档变化后调用）。
    pub fn refresh(&mut self, buffer: &Buffer) {
        self.matches = buffer.find_matches(&self.query);
        if self.active >= self.matches.len() {
            self.active = 0;
        }
    }

    /// 激活匹配定位到光标后方最近者（打开 / 编辑后保持视觉连续）。
    fn seek_active_after_cursor(&mut self, buffer: &Buffer) {
        if self.matches.is_empty() {
            self.active = 0;
            return;
        }
        let cursor = buffer.cursor();
        let next = self.matches.iter().position(|m| *m >= cursor).unwrap_or(0);
        self.active = next;
    }

    /// 跳转匹配（`backward`：Shift+Enter 上一处）。空匹配集无操作。
    /// 光标落匹配点（选区锚在匹配尾——可视高亮由渲染层消费）。
    pub fn jump(&mut self, buffer: &mut Buffer, backward: bool) {
        if self.matches.is_empty() {
            return;
        }
        if backward {
            self.active = (self.active + self.matches.len() - 1) % self.matches.len();
        } else {
            self.active = (self.active + 1) % self.matches.len();
        }
        let m = self.matches[self.active];
        // 匹配尾点（列 + query 长，clamp 行长——编辑中文档可能变短）。
        let end_col = (m.col + self.query.chars().count()).min(buffer.line_len(m.line));
        buffer.set_cursor(Point::new(m.line, end_col));
        // 用零宽 API 无法直接造选区——set_cursor 清锚后手动恢复：
        // 以锚=匹配头、光标=匹配尾呈现选中。
        buffer.set_selection(Point::new(m.line, m.col), Point::new(m.line, end_col));
    }

    /// 查询文本。
    pub fn query(&self) -> &str {
        &self.query
    }

    /// 输入光标（char 索引）。
    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    /// 匹配计数标签：`"3/17"`；空查询或零匹配 `""` 或 `"0/0"`。
    pub fn match_label(&self) -> String {
        if self.query.is_empty() {
            String::new()
        } else if self.matches.is_empty() {
            "0 结果".to_string()
        } else {
            format!("{}/{}", self.active + 1, self.matches.len())
        }
    }

    /// 当前激活匹配下标（渲染层比对）。
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// 全部匹配点（渲染层在正文行内高亮命中）。
    pub fn matches(&self) -> &[Point] {
        &self.matches
    }

    /// 处理查找条按键（查找条聚焦时全键吞掉，未识别键兜底 `Handled`；
    /// query 变化时内联 `refresh`——与 Editor 分发路径同一语义）。
    pub fn handle_key(&mut self, buffer: &Buffer, ks: &Keystroke) -> FindKeyOutcome {
        let ctrl = ks.modifiers.control || ks.modifiers.platform;
        // Enter 家族：跳转。
        if ks.key == "enter" {
            return if ks.modifiers.shift {
                FindKeyOutcome::JumpBackward
            } else {
                FindKeyOutcome::JumpForward
            };
        }
        if ks.key == "escape" {
            return FindKeyOutcome::Close;
        }
        // 带修饰键（除 shift）不作为文本输入。
        if ctrl || ks.modifiers.alt {
            return FindKeyOutcome::Handled;
        }
        match ks.key.as_str() {
            "backspace" => {
                if self.cursor_col > 0 {
                    let b0 = char_byte(&self.query, self.cursor_col - 1);
                    let b1 = char_byte(&self.query, self.cursor_col);
                    self.query.replace_range(b0..b1, "");
                    self.cursor_col -= 1;
                    FindKeyOutcome::QueryChanged
                } else {
                    FindKeyOutcome::Handled
                }
            }
            "left" => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
                FindKeyOutcome::Handled
            }
            "right" => {
                let len = self.query.chars().count();
                self.cursor_col = (self.cursor_col + 1).min(len);
                FindKeyOutcome::Handled
            }
            "home" => {
                self.cursor_col = 0;
                FindKeyOutcome::Handled
            }
            "end" => {
                self.cursor_col = self.query.chars().count();
                FindKeyOutcome::Handled
            }
            _ => {
                // 可打印字符输入（key_char 平台布局感知；全字符非控制）。
                if let Some(c) = &ks.key_char
                    && !c.is_empty()
                    && c.chars().all(|ch| !ch.is_control())
                {
                    let b = char_byte(&self.query, self.cursor_col);
                    self.query.insert_str(b, c);
                    self.cursor_col += 1;
                    self.refresh(buffer);
                    return FindKeyOutcome::QueryChanged;
                }
                FindKeyOutcome::Handled
            }
        }
    }
}

/// char 索引 → 字节偏移（clamp 末尾）。
fn char_byte(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(b, _)| b).unwrap_or(s.len())
}

// =============================================================================
// 单元测试
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::buffer::Point;

    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }

    fn buffer_with_foo() -> Buffer {
        Buffer::from_plain_text("foo bar\nfoo baz\nFOO")
    }

    #[test]
    fn open_with_selection_initializes_query() {
        let mut b = buffer_with_foo();
        // 选中 "bar"（b4 a5 r6——从 r 后 (0,7) 左移 3）
        b.set_cursor(p(0, 7));
        b.move_left(true);
        b.move_left(true);
        b.move_left(true);
        let mut f = FindState::default();
        f.open(&b);
        assert_eq!(f.query(), "bar");
        assert_eq!(f.match_label(), "1/1");
    }

    #[test]
    fn open_without_selection_starts_empty() {
        let b = buffer_with_foo();
        let mut f = FindState::default();
        f.open(&b);
        assert_eq!(f.query(), "");
        assert_eq!(f.match_label(), "");
    }

    #[test]
    fn refresh_tracks_document_edits() {
        let b = buffer_with_foo();
        let mut f = FindState::default();
        f.open(&b);
        // 手灌 query（无选区打开后输入 "foo"）
        f.handle_key(&b, &keystroke("f", Some("f".into())));
        f.handle_key(&b, &keystroke("o", Some("o".into())));
        f.handle_key(&b, &keystroke("o", Some("o".into())));
        assert_eq!(f.query(), "foo");
        assert_eq!(f.matches().len(), 3, "大小写不敏感 3 处");
    }

    #[test]
    fn jump_cycles_and_selects_match() {
        let mut b = buffer_with_foo();
        let mut f = FindState::default();
        f.open(&b);
        f.handle_key(&b, &keystroke("f", Some("f".into())));
        f.handle_key(&b, &keystroke("o", Some("o".into())));
        f.handle_key(&b, &keystroke("o", Some("o".into())));
        // 光标在文首 → active=0（第一个匹配）
        f.jump(&mut b, false); // → 1
        assert_eq!(b.cursor(), p(1, 3), "跳第二处 foo（bar 后），光标在匹配尾");
        assert_eq!(b.selection_text(), "foo", "跳转后匹配呈选中态");
        f.jump(&mut b, false); // → 2
        assert_eq!(b.selection_text(), "FOO", "大小写不敏感命中第三处");
        f.jump(&mut b, false); // → 0（循环）
        assert_eq!(b.cursor(), p(0, 3));
        f.jump(&mut b, true); // → 回 2（反向循环）
        assert_eq!(b.selection_text(), "FOO");
    }

    #[test]
    fn enter_escape_outcomes() {
        let b = buffer_with_foo();
        let mut f = FindState::default();
        assert_eq!(
            f.handle_key(&b, &keystroke("enter", None)),
            FindKeyOutcome::JumpForward
        );
        assert_eq!(
            f.handle_key(&b, &keystroke("enter", Some("X".into()))),
            FindKeyOutcome::JumpForward
        );
        assert_eq!(
            f.handle_key(&b, &keystroke("escape", None)),
            FindKeyOutcome::Close
        );
    }

    #[test]
    fn typed_chars_and_backspace() {
        let b = buffer_with_foo();
        let mut f = FindState::default();
        assert_eq!(
            f.handle_key(&b, &keystroke("a", Some("a".into()))),
            FindKeyOutcome::QueryChanged
        );
        assert_eq!(f.query(), "a");
        assert_eq!(
            f.handle_key(&b, &keystroke("backspace", None)),
            FindKeyOutcome::QueryChanged
        );
        assert_eq!(f.query(), "");
        assert_eq!(
            f.handle_key(&b, &keystroke("backspace", None)),
            FindKeyOutcome::Handled,
            "空 query 退格：已处理无变化"
        );
    }

    #[test]
    fn ctrl_combos_swallowed_not_typed() {
        let b = buffer_with_foo();
        let mut f = FindState::default();
        assert_eq!(
            f.handle_key(&b, &ctrl_keystroke("a", Some("a".into()))),
            FindKeyOutcome::Handled,
            "Ctrl+A 不进 query"
        );
        assert_eq!(f.query(), "");
    }

    #[test]
    fn cjk_query_input() {
        let b = Buffer::from_plain_text("中文查找\n查找中文");
        let mut f = FindState::default();
        f.open(&b);
        for c in "查找".chars() {
            f.handle_key(&b, &keystroke("virtual", Some(c.to_string())));
        }
        assert_eq!(f.query(), "查找");
        f.refresh(&b);
        assert_eq!(f.matches().len(), 2);
        assert_eq!(f.matches()[0], p(0, 2));
        assert_eq!(f.matches()[1], p(1, 0));
    }

    #[test]
    fn zero_result_label() {
        let b = buffer_with_foo();
        let mut f = FindState::default();
        f.open(&b);
        f.handle_key(&b, &keystroke("q", Some("q".into())));
        assert_eq!(f.match_label(), "0 结果", "全文无 q（z 会命中 baz）");
    }

    // ---- 测试用按键构造 ----

    fn keystroke(key: &str, key_char: Option<String>) -> Keystroke {
        Keystroke {
            modifiers: Default::default(),
            key: key.to_string(),
            key_char,
        }
    }

    fn ctrl_keystroke(key: &str, key_char: Option<String>) -> Keystroke {
        Keystroke {
            modifiers: gpui::Modifiers {
                control: true,
                ..Default::default()
            },
            key: key.to_string(),
            key_char,
        }
    }
}
