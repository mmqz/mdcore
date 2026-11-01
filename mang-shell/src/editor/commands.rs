//! commands —— 命令系统（#68 阶段 E）。
//!
//! 抄 Zed `actions.rs` 的**设计**（上游在 editor/gpui crate 深耦合面，不搬；
//! r42 同款纪律自研干净实现）：
//!
//! - **注册表**：静态 `CommandSpec` 表（id / 标题 / 默认键位 / 分组）——
//!   数据与执行分离（keymap 可配置的基础：换绑=改表）。
//! - **执行**：`Cmd` 枚举 → `Editor::exec_command`（mod.rs）；命令面板
//!   （Ctrl+Shift+P）按标题模糊搜索（子序列匹配）后回车执行。
//! - 本批已接线命令集 = [`Cmd::ALL`]（面板只列可执行的——零桩：未接线的
//!   不入表）；dispatch_key 的既有分支保持不变（面板与键位双入口，同
//!   一批方法调用）。

/// 命令标识（枚举——面板回车与键位双入口的分派目标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    WordLeft,
    WordRight,
    MoveHome,
    MoveEnd,
    DocStart,
    DocEnd,
    ToggleFold,
    /// #132-3：折叠光标所在块（Ctrl+Shift+[）。
    FoldCurrent,
    /// #132-3：展开光标所在块（Ctrl+Shift+]）。
    UnfoldCurrent,
    /// #132-3：全部折叠（Ctrl+K Ctrl+0）。
    FoldAll,
    /// #132-3：全部展开（Ctrl+K Ctrl+J）。
    UnfoldAll,
    OpenFind,
    /// G1：打开替换条（Ctrl+H）。
    OpenReplace,
    /// G2：跳转到行输入框（Ctrl+G——导航类，只读态可用）。
    GotoLine,
    Save,
    SelectAll,
    Undo,
    Redo,
}

impl Cmd {
    /// 全部已接线命令（面板候选集——按文档序）。
    pub const ALL: &[Cmd] = &[
        Cmd::MoveLeft,
        Cmd::MoveRight,
        Cmd::MoveUp,
        Cmd::MoveDown,
        Cmd::WordLeft,
        Cmd::WordRight,
        Cmd::MoveHome,
        Cmd::MoveEnd,
        Cmd::DocStart,
        Cmd::DocEnd,
        Cmd::ToggleFold,
        Cmd::FoldCurrent,
        Cmd::UnfoldCurrent,
        Cmd::FoldAll,
        Cmd::UnfoldAll,
        Cmd::OpenFind,
        Cmd::OpenReplace,
        Cmd::GotoLine,
        Cmd::Save,
        Cmd::SelectAll,
        Cmd::Undo,
        Cmd::Redo,
    ];

    /// #134：机器名（snake_case——RPC `run_command` 按名调用的口径；
    /// 与变体名的对应关系稳定可预测，新命令逆推零成本）。
    pub fn id(self) -> &'static str {
        match self {
            Cmd::MoveLeft => "move_left",
            Cmd::MoveRight => "move_right",
            Cmd::MoveUp => "move_up",
            Cmd::MoveDown => "move_down",
            Cmd::WordLeft => "word_left",
            Cmd::WordRight => "word_right",
            Cmd::MoveHome => "move_home",
            Cmd::MoveEnd => "move_end",
            Cmd::DocStart => "doc_start",
            Cmd::DocEnd => "doc_end",
            Cmd::ToggleFold => "toggle_fold",
            // #132 任务3：折叠命令族（rebase 时补——main 的 id() 早于本分支变体）。
            Cmd::FoldCurrent => "fold_current",
            Cmd::UnfoldCurrent => "unfold_current",
            Cmd::FoldAll => "fold_all",
            Cmd::UnfoldAll => "unfold_all",
            Cmd::OpenFind => "open_find",
            Cmd::OpenReplace => "open_replace",
            Cmd::GotoLine => "goto_line",
            Cmd::Save => "save",
            Cmd::SelectAll => "select_all",
            Cmd::Undo => "undo",
            Cmd::Redo => "redo",
        }
    }

    /// #134：按机器名反查（RPC `run_command` 的入口；未知名 None——
    /// 调用方引导 `list_commands`）。
    pub fn from_id(id: &str) -> Option<Cmd> {
        Cmd::ALL.iter().copied().find(|c| c.id() == id)
    }

    /// 面板标题。
    pub fn title(self) -> &'static str {
        match self {
            Cmd::MoveLeft => "移动：左",
            Cmd::MoveRight => "移动：右",
            Cmd::MoveUp => "移动：上",
            Cmd::MoveDown => "移动：下",
            Cmd::WordLeft => "词跳：左",
            Cmd::WordRight => "词跳：右",
            Cmd::MoveHome => "移动：行首",
            Cmd::MoveEnd => "移动：行尾",
            Cmd::DocStart => "移动：文首",
            Cmd::DocEnd => "移动：文尾",
            Cmd::ToggleFold => "折叠：切换围栏块",
            Cmd::FoldCurrent => "折叠：折叠当前块",
            Cmd::UnfoldCurrent => "折叠：展开当前块",
            Cmd::FoldAll => "折叠：全部折叠",
            Cmd::UnfoldAll => "折叠：全部展开",
            Cmd::OpenFind => "查找（Ctrl+F）",
            // G1：替换（只读态 exec 侧拒——面板候选集含之，执行有门）。
            Cmd::OpenReplace => "替换（Ctrl+H）",
            Cmd::GotoLine => "跳转到行（Ctrl+G）",
            Cmd::Save => "保存（Ctrl+S）",
            Cmd::SelectAll => "全选（Ctrl+A）",
            Cmd::Undo => "撤销（Ctrl+Z）",
            Cmd::Redo => "重做（Ctrl+Y）",
        }
    }

    /// 默认键位（展示用；执行键位仍由 dispatch_key 既有分支承担）。
    pub fn default_key(self) -> Option<&'static str> {
        match self {
            Cmd::MoveLeft => Some("←"),
            Cmd::MoveRight => Some("→"),
            Cmd::MoveUp => Some("↑"),
            Cmd::MoveDown => Some("↓"),
            Cmd::WordLeft => Some("Ctrl+←"),
            Cmd::WordRight => Some("Ctrl+→"),
            Cmd::MoveHome => Some("Home"),
            Cmd::MoveEnd => Some("End"),
            Cmd::DocStart => Some("Ctrl+Home"),
            Cmd::DocEnd => Some("Ctrl+End"),
            Cmd::ToggleFold => Some("Alt+F"),
            Cmd::FoldCurrent => Some("Ctrl+Shift+["),
            Cmd::UnfoldCurrent => Some("Ctrl+Shift+]"),
            Cmd::FoldAll => Some("Ctrl+K Ctrl+0"),
            Cmd::UnfoldAll => Some("Ctrl+K Ctrl+J"),
            Cmd::OpenFind => Some("Ctrl+F"),
            Cmd::OpenReplace => Some("Ctrl+H"),
            Cmd::GotoLine => Some("Ctrl+G"),
            Cmd::Save => Some("Ctrl+S"),
            Cmd::SelectAll => Some("Ctrl+A"),
            Cmd::Undo => Some("Ctrl+Z"),
            Cmd::Redo => Some("Ctrl+Y"),
        }
    }

    /// 标题模糊匹配（子序列——大小写不敏感；空查询恒命中）。
    /// 返回 (命令, 命中度)——度=匹配字符数/标题长度（排序用）。
    pub fn matches_query(self, query: &str) -> Option<f32> {
        let q: Vec<char> = query.to_lowercase().chars().collect();
        if q.is_empty() {
            return Some(0.);
        }
        let title: Vec<char> = self.title().to_lowercase().chars().collect();
        let mut qi = 0;
        for tc in title.iter() {
            if qi < q.len() && *tc == q[qi] {
                qi += 1;
            }
        }
        if qi == q.len() {
            Some(qi as f32 / title.len() as f32)
        } else {
            None
        }
    }
}

/// 按查询排序命令（匹配度降序；同度保文档序）。
pub fn search(query: &str) -> Vec<Cmd> {
    let mut out: Vec<(f32, Cmd)> = Cmd::ALL
        .iter()
        .filter_map(|&c| c.matches_query(query).map(|d| (d, c)))
        .collect();
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_lists_all() {
        assert_eq!(search("").len(), Cmd::ALL.len());
    }

    #[test]
    fn fuzzy_subsequence_matches() {
        // "移左" 子序列命中 "移动：左"（也命中"移动：行首"?"移左"——行首无左→不命中）
        let r = search("移左");
        assert!(r.contains(&Cmd::MoveLeft), "命中 移动：左");
        assert!(!r.contains(&Cmd::MoveHome), "行首不含「左」字序列");
    }

    #[test]
    fn no_match_excluded() {
        assert!(search("zzz不存在的词").is_empty());
    }

    #[test]
    fn degree_orders_tighter_match_first() {
        // "移动：左" 对查询 "移动左"：度=3/4=0.75；"移动：行首" 不命中。
        // "撤销（Ctrl+Z）" 对 "cz"：无（标题无 c z 子序列——撤销中文）
        let r = search("移动左");
        assert_eq!(r.first(), Some(&Cmd::MoveLeft), "更完整匹配优先");
    }

    #[test]
    fn every_wired_cmd_has_title_and_key() {
        for &c in Cmd::ALL {
            assert!(!c.title().is_empty(), "{:?} 标题空", c);
            assert!(c.default_key().is_some(), "{:?} 默认键缺失", c);
            // 标题与默认键唯一（面板去重依赖）
        }
    }

    // ---------------- #134：机器名面（RPC run_command 口径） ----------------

    #[test]
    fn id_from_id_roundtrip_all_commands() {
        for &c in Cmd::ALL {
            let id = c.id();
            assert!(
                id.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'),
                "{id} 应为 snake_case"
            );
            assert_eq!(Cmd::from_id(id), Some(c), "{id} 反查应命中自身");
        }
        // 机器名全局唯一（未知名不会歧义命中）
        let ids: Vec<&str> = Cmd::ALL.iter().map(|c| c.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "机器名不得重复");
    }

    #[test]
    fn from_id_unknown_is_none() {
        assert_eq!(Cmd::from_id("open_fiel"), None, "拼错名不命中");
        assert_eq!(Cmd::from_id(""), None);
        assert_eq!(Cmd::from_id("MoveLeft"), None, "变体名不是机器名");
    }
}
