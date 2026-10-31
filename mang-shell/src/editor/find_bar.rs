//! 查找条（Ctrl+F）—— 纯逻辑状态机与输入处理（#68 阶段 1.2 查找；
//! #68 阶段 G1 在同一状态机上扩展**替换态**：Ctrl+H 追加替换输入行，
//! 替换当前（替换行 Enter）/ 全部替换（Ctrl+Alt+Enter，一次批量编辑
//! 一次撤销）+ 区分大小写 / 全词匹配两选项）。
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
    /// G1：替换态（Ctrl+H 打开——渲染层追加替换输入行）。
    replace_mode: bool,
    /// G1：替换文本。
    replacement: String,
    /// G1：replacement 输入光标（char 索引）。
    repl_cursor_col: usize,
    /// G1：替换态当前输入行（true = 替换行）。查找态恒 false。
    focus_replacement: bool,
    /// G1：区分大小写选项（跨开关持久——经典编辑器一致行为）。
    case_sensitive: bool,
    /// G1：全词匹配选项。
    whole_word: bool,
}

/// 查找条按键处理结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindKeyOutcome {
    /// 已处理（纯光标移动等，无需重算匹配）。
    Handled,
    /// 已处理且 query/选项变化（需 refresh 重算匹配——本模块内已内联
    /// refresh，Outcome 供渲染层重绘与调用方感知）。
    QueryChanged,
    /// Enter——跳下一匹配。
    JumpForward,
    /// Shift+Enter——跳上一匹配。
    JumpBackward,
    /// Esc——关闭查找条。
    Close,
    /// G1：替换当前激活匹配（替换行回车）——调用方执行 buffer 变更。
    ReplaceCurrent,
    /// G1：全部替换（Ctrl+Alt+Enter）——调用方执行批量替换。
    ReplaceAll,
    /// G1：选项切换（Alt+C / Alt+W——内部已 refresh）。
    OptionsChanged,
    /// G1：输入行焦点切换（Tab/上下键——纯 UI 态）。
    FocusSwitched,
}

impl FindState {
    /// 打开查找条：以当前选中文本初始化 query（无选区则空），
    /// 激活匹配取光标后方最近的一个。选项/替换文本保留先前值
    /// （`FindState::default` 起手 = 全关）——跨开关持久由 Editor 层
    /// 的 saved 状态承载。
    pub fn open(&mut self, buffer: &Buffer) {
        let selected = buffer.selection_text();
        if !selected.is_empty() && !selected.contains('\n') {
            self.query = selected;
        } else {
            self.query.clear();
        }
        self.cursor_col = self.query.chars().count();
        self.replace_mode = false;
        self.focus_replacement = false;
        self.refresh(buffer);
        self.seek_active_after_cursor(buffer);
    }

    /// G1：打开替换条（Ctrl+H）——open 同链 + `replace_mode = true`
    ///（激活行仍为 query——Tab/下键切到替换行；经典编辑器一致）。
    pub fn open_replace(&mut self, buffer: &Buffer) {
        self.open(buffer);
        self.replace_mode = true;
    }

    /// 重算匹配（query / 选项 / 文档变化后调用；G1 起按两选项算）。
    pub fn refresh(&mut self, buffer: &Buffer) {
        self.matches = buffer.find_matches_opts(&self.query, self.case_sensitive, self.whole_word);
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

    // ------------------------------------------------------------------
    // G1：替换态访问器
    // ------------------------------------------------------------------

    /// G1：替换态标记（渲染层据此追加替换输入行）。
    pub fn is_replace_mode(&self) -> bool {
        self.replace_mode
    }

    /// G1：替换文本。
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    /// G1：替换行输入光标（char 索引）。
    pub fn repl_cursor_col(&self) -> usize {
        self.repl_cursor_col
    }

    /// G1：当前输入行是否为替换行（渲染光标呈现行）。
    pub fn focus_is_replacement(&self) -> bool {
        self.replace_mode && self.focus_replacement
    }

    /// G1：区分大小写选项状态。
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// G1：全词匹配选项状态。
    pub fn whole_word(&self) -> bool {
        self.whole_word
    }

    /// G1：切换大小写选项（渲染层选项钮与 Alt+C 同一状态源）。
    pub fn toggle_case_sensitive(&mut self, buffer: &Buffer) {
        self.case_sensitive = !self.case_sensitive;
        self.refresh(buffer);
    }

    /// G1：切换全词选项（渲染层选项钮与 Alt+W 同一状态源）。
    pub fn toggle_whole_word(&mut self, buffer: &Buffer) {
        self.whole_word = !self.whole_word;
        self.refresh(buffer);
    }

    /// G1：点击输入行设置当前输入行（true = 替换行；查找态忽略）。
    pub fn set_focus_replacement(&mut self, on: bool) {
        if self.replace_mode {
            self.focus_replacement = on;
        }
    }

    // ------------------------------------------------------------------
    // G1：替换执行（buffer 变更在此——Editor 只做分发）
    // ------------------------------------------------------------------

    /// G1：替换当前激活匹配（返回是否发生替换）。光标落在替换文本尾
    ///（经典编辑器同语义）；替换后 refresh + 激活项推进到光标后方
    /// 最近的下一个匹配（渲染层高亮呈现）。单点替换 = 单条 undo。
    pub fn replace_current(&mut self, buffer: &mut Buffer) -> bool {
        let Some(&m) = self.matches.get(self.active) else {
            return false;
        };
        if self.query.is_empty() {
            return false;
        }
        let q_len = self.query.chars().count();
        let end_col = (m.col + q_len).min(buffer.line_len(m.line));
        buffer.replace_range(
            Point::new(m.line, m.col),
            Point::new(m.line, end_col),
            &self.replacement,
        );
        self.refresh(buffer);
        self.seek_active_after_cursor(buffer);
        true
    }

    /// G1：全部替换——非重叠命中一次批量编辑（[`Buffer::replace_batch`]，
    /// 一条 undo 记录，Ctrl+Z 一次整批回滚）。返回替换数。
    ///
    /// 非重叠口径：文档序扫描，跳过起点落入上一替换占位内的命中
    ///（`"aaa"` 查 `"aa"` 只替换 col 0 一处——经典编辑器同语义）；
    /// 跨行必然不重叠（query 不含换行、命中不跨行）。
    pub fn replace_all(&mut self, buffer: &mut Buffer) -> usize {
        if self.query.is_empty() {
            return 0;
        }
        let q_len = self.query.chars().count();
        let repl = self.replacement.clone();
        let mut edits: Vec<(Point, Point, String)> = Vec::new();
        let mut prev_line = 0usize;
        let mut prev_end_col = 0usize;
        for &m in &self.matches {
            if m.line == prev_line && m.col < prev_end_col {
                continue; // 落入上一替换占位 → 跳过
            }
            let end_col = (m.col + q_len).min(buffer.line_len(m.line));
            edits.push((m, Point::new(m.line, end_col), repl.clone()));
            prev_line = m.line;
            prev_end_col = end_col;
        }
        let n = buffer.replace_batch(edits);
        self.refresh(buffer);
        self.active = 0;
        n
    }

    /// 处理查找条按键（查找条聚焦时全键吞掉，未识别键兜底 `Handled`；
    /// query/选项变化时内联 `refresh`——与 Editor 分发路径同一语义）。
    /// G1：替换态下文本键按当前输入行路由；Tab/上下键切换输入行。
    pub fn handle_key(&mut self, buffer: &Buffer, ks: &Keystroke) -> FindKeyOutcome {
        let ctrl = ks.modifiers.control || ks.modifiers.platform;
        // G1：全部替换（Ctrl+Alt+Enter）——先于通用修饰键吞吐分支。
        if ks.key == "enter" && ctrl && ks.modifiers.alt {
            return FindKeyOutcome::ReplaceAll;
        }
        // Enter 家族：跳转（query 行）/ 替换当前（替换行）。
        if ks.key == "enter" {
            if ks.modifiers.shift {
                return FindKeyOutcome::JumpBackward;
            }
            if self.replace_mode && self.focus_replacement {
                return FindKeyOutcome::ReplaceCurrent;
            }
            return FindKeyOutcome::JumpForward;
        }
        if ks.key == "escape" {
            return FindKeyOutcome::Close;
        }
        // G1：选项切换（Alt+C 大小写 / Alt+W 全词；Ctrl 家族不占用）。
        if ks.modifiers.alt && !ctrl {
            return match ks.key.to_lowercase().as_str() {
                "c" => {
                    self.case_sensitive = !self.case_sensitive;
                    self.refresh(buffer);
                    FindKeyOutcome::OptionsChanged
                }
                "w" => {
                    self.whole_word = !self.whole_word;
                    self.refresh(buffer);
                    FindKeyOutcome::OptionsChanged
                }
                _ => FindKeyOutcome::Handled,
            };
        }
        // G1：替换态输入行切换（Tab / Shift+Tab / 上下键）。
        if self.replace_mode && (ks.key == "tab" || ks.key == "up" || ks.key == "down") && !ctrl {
            let down = ks.key == "down" || (ks.key == "tab" && !ks.modifiers.shift);
            let up = ks.key == "up" || (ks.key == "tab" && ks.modifiers.shift);
            if (down && !self.focus_replacement) || (up && self.focus_replacement) {
                self.focus_replacement = !self.focus_replacement;
                return FindKeyOutcome::FocusSwitched;
            }
            return FindKeyOutcome::Handled; // 同向越界（已在目标行）不动作
        }
        // 带修饰键（除 shift）不作为文本输入。
        if ctrl || ks.modifiers.alt {
            return FindKeyOutcome::Handled;
        }
        // 以下文本编辑键按当前输入行路由（查找态 / 查找行 = query）。
        let (field, field_col): (&mut String, &mut usize) =
            if self.replace_mode && self.focus_replacement {
                (&mut self.replacement, &mut self.repl_cursor_col)
            } else {
                (&mut self.query, &mut self.cursor_col)
            };
        match ks.key.as_str() {
            "backspace" => {
                if *field_col > 0 {
                    let b0 = char_byte(field, *field_col - 1);
                    let b1 = char_byte(field, *field_col);
                    field.replace_range(b0..b1, "");
                    *field_col -= 1;
                    if self.replace_mode && self.focus_replacement {
                        FindKeyOutcome::Handled // 替换文本变化不影响匹配集
                    } else {
                        self.refresh(buffer);
                        FindKeyOutcome::QueryChanged
                    }
                } else {
                    FindKeyOutcome::Handled
                }
            }
            "left" => {
                *field_col = field_col.saturating_sub(1);
                FindKeyOutcome::Handled
            }
            "right" => {
                let len = field.chars().count();
                *field_col = (*field_col + 1).min(len);
                FindKeyOutcome::Handled
            }
            "home" => {
                *field_col = 0;
                FindKeyOutcome::Handled
            }
            "end" => {
                *field_col = field.chars().count();
                FindKeyOutcome::Handled
            }
            _ => {
                // 可打印字符输入（key_char 平台布局感知；全字符非控制）。
                if let Some(c) = &ks.key_char
                    && !c.is_empty()
                    && c.chars().all(|ch| !ch.is_control())
                {
                    let b = char_byte(field, *field_col);
                    field.insert_str(b, c);
                    *field_col += 1;
                    if self.replace_mode && self.focus_replacement {
                        return FindKeyOutcome::Handled; // 替换文本变化不影响匹配集
                    }
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

// =============================================================================
// G1 查找替换——专项单测（替换态键路由 / 选项 / 单点与批量替换）
// =============================================================================

#[cfg(test)]
mod g1_replace_tests {
    use super::*;

    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }

    fn ks(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke {
            modifiers: Default::default(),
            key: key.to_string(),
            key_char: key_char.map(|c| c.to_string()),
        }
    }

    fn mod_ks(key: &str, key_char: Option<&str>, ctrl: bool, alt: bool, shift: bool) -> Keystroke {
        Keystroke {
            modifiers: gpui::Modifiers {
                control: ctrl,
                alt,
                shift,
                ..Default::default()
            },
            key: key.to_string(),
            key_char: key_char.map(|c| c.to_string()),
        }
    }

    /// 键入文本（可打印字符逐个送——virtual 键名走 key_char 通道）。
    fn type_text(f: &mut FindState, b: &Buffer, text: &str) {
        for c in text.chars() {
            f.handle_key(b, &ks("virtual", Some(c.to_string().as_str())));
        }
    }

    fn open_replace_foo() -> (Buffer, FindState) {
        let b = Buffer::from_plain_text("foo bar\nfoo baz\nFOO");
        let mut f = FindState::default();
        f.open_replace(&b);
        type_text(&mut f, &b, "foo");
        (b, f)
    }

    // ---------------- 打开与键路由 ----------------

    #[test]
    fn open_replace_sets_mode_and_keeps_query_line_focused() {
        let (b, mut f) = open_replace_foo();
        assert!(f.is_replace_mode());
        assert!(!f.focus_is_replacement(), "打开后光标在 query 行");
        assert_eq!(f.matches().len(), 3, "大小写不敏感 3 处 foo");
        let _ = b;
    }

    #[test]
    fn tab_and_shift_tab_switch_input_lines() {
        let (b, mut f) = open_replace_foo();
        assert_eq!(
            f.handle_key(&b, &ks("tab", None)),
            FindKeyOutcome::FocusSwitched
        );
        assert!(f.focus_is_replacement());
        type_text(&mut f, &b, "X");
        assert_eq!(f.replacement(), "X", "文本键路由到替换行");
        assert_eq!(f.query(), "foo", "query 不受替换行输入影响");
        // Shift+Tab 回 query 行
        assert_eq!(
            f.handle_key(&b, &mod_ks("tab", None, false, false, true)),
            FindKeyOutcome::FocusSwitched
        );
        assert!(!f.focus_is_replacement());
    }

    #[test]
    fn enter_routes_by_input_line() {
        let (b, mut f) = open_replace_foo();
        // query 行 Enter → 跳转
        assert_eq!(
            f.handle_key(&b, &ks("enter", None)),
            FindKeyOutcome::JumpForward
        );
        // 切到替换行 → Enter → ReplaceCurrent
        f.handle_key(&b, &ks("tab", None));
        assert_eq!(
            f.handle_key(&b, &ks("enter", None)),
            FindKeyOutcome::ReplaceCurrent
        );
        // Ctrl+Alt+Enter（任意行）→ ReplaceAll
        assert_eq!(
            f.handle_key(&b, &mod_ks("enter", None, true, true, false)),
            FindKeyOutcome::ReplaceAll
        );
    }

    #[test]
    fn alt_c_alt_w_toggle_options() {
        let (b, mut f) = open_replace_foo();
        assert_eq!(f.matches().len(), 3);
        // Alt+C → 大小写敏感：只剩 2 处小写 foo
        assert_eq!(
            f.handle_key(&b, &mod_ks("c", Some("c"), false, true, false)),
            FindKeyOutcome::OptionsChanged
        );
        assert!(f.case_sensitive());
        assert_eq!(f.matches().len(), 2, "敏感后 FOO 不命中");
        // Alt+W → 全词：foo 前后都是空格/行首——仍是 2 处
        assert_eq!(
            f.handle_key(&b, &mod_ks("w", Some("w"), false, true, false)),
            FindKeyOutcome::OptionsChanged
        );
        assert!(f.whole_word());
        assert_eq!(f.matches().len(), 2);
        // 关回大小写不敏感 → 3 处（选项即时反映在匹配集）
        f.handle_key(&b, &mod_ks("c", Some("c"), false, true, false));
        assert_eq!(f.matches().len(), 3);
    }

    #[test]
    fn whole_word_excludes_substring_hits() {
        let b = Buffer::from_plain_text("foo foobar foo");
        let mut f = FindState::default();
        f.open_replace(&b);
        type_text(&mut f, &b, "foo");
        f.handle_key(&b, &mod_ks("w", Some("w"), false, true, false));
        assert_eq!(f.matches().len(), 2, "foobar 的 foo 前缀非全词");
    }

    // ---------------- 替换当前 ----------------

    #[test]
    fn replace_current_single_match_replaces_and_cursor_at_tail() {
        let (mut b, mut f) = open_replace_foo();
        f.handle_key(&b, &ks("tab", None));
        type_text(&mut f, &b, "quux");
        // active=0（光标文首）→ 替换第一处
        assert!(f.replace_current(&mut b));
        assert_eq!(b.to_plain_text(), "quux bar\nfoo baz\nFOO");
        assert_eq!(b.cursor(), p(0, 4), "光标落替换文本尾");
        // 激活项推进到下一处（光标后方最近）
        assert_eq!(f.matches()[f.active_index()], p(1, 0));
        // 再替换一次 → 第二处
        assert!(f.replace_current(&mut b));
        assert_eq!(b.to_plain_text(), "quux bar\nquux baz\nFOO");
        // 两次替换 = 两条 undo 记录，逐条回滚
        assert!(b.undo());
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "foo bar\nfoo baz\nFOO");
    }

    #[test]
    fn replace_current_without_matches_is_noop() {
        let (mut b, mut f) = open_replace_foo();
        f.handle_key(&b, &mod_ks("q", Some("q"), false, false, false));
        f.handle_key(&b, &mod_ks("q", Some("q"), false, false, false));
        f.handle_key(&b, &mod_ks("q", Some("q"), false, false, false));
        let before = b.to_plain_text();
        assert!(!f.replace_current(&mut b), "零匹配不替换");
        assert_eq!(b.to_plain_text(), before);
    }

    // ---------------- 全部替换 ----------------

    #[test]
    fn replace_all_batch_and_single_undo_restores() {
        let (mut b, mut f) = open_replace_foo();
        f.handle_key(&b, &ks("tab", None));
        type_text(&mut f, &b, "MANG");
        let n = f.replace_all(&mut b);
        assert_eq!(n, 3, "大小写不敏感 3 处全替换");
        assert_eq!(b.to_plain_text(), "MANG bar\nMANG baz\nMANG");
        assert_eq!(b.cursor(), p(2, 4), "光标落文档序最后替换的尾部");
        assert_eq!(b.last_undo_batch_count(), 3, "一条批量 undo 记录");
        // 一次 undo 整批回滚
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "foo bar\nfoo baz\nFOO");
    }

    #[test]
    fn replace_all_overlapping_query_skips_consumed_hits() {
        let mut b = Buffer::from_plain_text("aaa");
        let mut f = FindState::default();
        f.open_replace(&b);
        type_text(&mut f, &b, "aa");
        assert_eq!(f.matches().len(), 2, "重叠命中 2 处（col0/col1）");
        let n = f.replace_all(&mut b);
        assert_eq!(n, 1, "非重叠口径只替换 col0");
        assert_eq!(b.to_plain_text(), "a");
    }

    #[test]
    fn replace_all_respects_options() {
        // 大小写敏感：只替换小写 foo，FOO 不动
        let mut b = Buffer::from_plain_text("foo FOO foo");
        let mut f = FindState::default();
        f.open_replace(&b);
        type_text(&mut f, &b, "foo");
        f.handle_key(&b, &mod_ks("c", Some("c"), false, true, false));
        f.handle_key(&b, &ks("tab", None)); // 切到替换行
        type_text(&mut f, &b, "z"); // 替换文本 z
        let n = f.replace_all(&mut b);
        assert_eq!(n, 2);
        assert_eq!(b.to_plain_text(), "z FOO z");
    }

    #[test]
    fn replace_all_empty_query_or_replacement_edge() {
        let (mut b, mut f) = open_replace_foo();
        // 空 query → 0
        let mut f2 = FindState::default();
        f2.open_replace(&b);
        assert_eq!(f2.replace_all(&mut b), 0);
        // 空 replacement = 全部删除语义
        f.handle_key(&b, &ks("tab", None));
        let n = f.replace_all(&mut b);
        assert_eq!(n, 3);
        assert_eq!(b.to_plain_text(), " bar\n baz\n");
    }
}
