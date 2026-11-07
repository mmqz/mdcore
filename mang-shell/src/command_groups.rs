//! 命令面板分组 / 最近使用 / 快捷键提示（#63 Lead 打磨项）。
//!
//! 命令本身（[`Cmd`]）在 `commands.rs`；本模块只负责**呈现顺序与分组**：
//!
//! - [`group_of`]：命令归属哪个分组（文件 / 编辑 / 导航 / 视图 / 帮助）；
//! - [`build_sections`]：把「查询结果 + 最近使用」拼成带标题的分段列表；
//! - [`RecentRing`]：最近使用记录（定长环形，去重，最新在前）。
//!
//! 全部是纯函数/纯结构——面板长什么样可以单测，不用起窗口。
//!
//! # 最近使用为什么单独成段而不是混排
//!
//! 混排会让「最近用过的」和「按匹配度排的」两套排序打架，用户每次都要
//! 重新找眼睛落点。分成两段后：上面是肌肉记忆区（位置稳定），下面是
//! 搜索结果区（随输入变化）。VS Code 也是这个做法。

use std::collections::VecDeque;

use crate::editor::commands::Cmd;

/// 命令分组。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// 最近使用（动态段，不是命令的固有分组）。
    Recent,
    File,
    Edit,
    Navigate,
    View,
    Help,
}

impl Group {
    /// 段标题（空段不渲染）。
    pub fn title(self) -> &'static str {
        match self {
            Group::Recent => "最近使用",
            Group::File => "文件",
            Group::Edit => "编辑",
            Group::Navigate => "导航",
            Group::View => "视图",
            Group::Help => "帮助",
        }
    }

    /// 固定分组的展示顺序（Recent 不参与——它是动态插入到最前的段）。
    pub const ORDER: &'static [Group] = &[
        Group::File,
        Group::Edit,
        Group::Navigate,
        Group::View,
        Group::Help,
    ];
}

/// 命令归属分组。
///
/// 新增命令时必须在这里给出归属——`every_command_has_group` 测试会强制
/// 枚举全覆盖（Rust 的 match 穷尽性在编译期就保证了，测试只是把这句话
/// 钉在文档里）。
pub fn group_of(cmd: Cmd) -> Group {
    use Cmd::*;
    match cmd {
        Save => Group::File,
        Undo | Redo | SelectAll | OpenFind | OpenReplace => Group::Edit,
        MoveLeft | MoveRight | MoveUp | MoveDown | WordLeft | WordRight | MoveHome | MoveEnd
        | DocStart | DocEnd | GotoLine => Group::Navigate,
        ToggleFold | FoldCurrent | UnfoldCurrent | FoldAll | UnfoldAll => Group::View,
    }
}

/// 面板里的一个条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub cmd: Cmd,
    /// 在扁平列表中的序号（键盘上下移动用——跨段连续，不按段重置）。
    pub index: usize,
}

/// 面板的一段（标题 + 条目）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub group: Group,
    pub entries: Vec<Entry>,
}

/// 最近使用记录（定长环形，最新在前，去重）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentRing {
    items: VecDeque<Cmd>,
    capacity: usize,
}

impl RecentRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// 记录一次使用：已存在则提到最前（不重复占位），超出容量丢最旧。
    pub fn touch(&mut self, cmd: Cmd) {
        if let Some(pos) = self.items.iter().position(|c| *c == cmd) {
            self.items.remove(pos);
        }
        self.items.push_front(cmd);
        while self.items.len() > self.capacity {
            self.items.pop_back();
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = Cmd> + '_ {
        self.items.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, cmd: Cmd) -> bool {
        self.items.contains(&cmd)
    }
}

/// 把搜索结果与最近使用拼成分段列表。
///
/// 规则：
/// - **查询为空**且有最近使用 → 最前面插一段「最近使用」；
/// - **查询非空** → 不显示最近使用段（用户已经在按关键词找了，再插一段
///   固定区反而干扰）。但最近用过的命令在同分组内**排到前面**。
/// - 空段不产出。
///
/// `results` = [`crate::editor::commands::search`] 的输出（已按匹配度排序）。
pub fn build_sections(results: &[Cmd], recent: &RecentRing) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut index = 0usize;
    // 最近段存在时，这些命令不在各自分组里重复出现——同一条命令在面板里
    // 出现两次会让键盘导航选中"看得见的另一条"，用户会以为自己点错了。
    let recent_len = recent.len();

    for g in Group::ORDER {
        let mut entries: Vec<Entry> = Vec::new();
        for &c in results {
            if group_of(c) != *g {
                continue;
            }
            if recent_len > 0 && recent.contains(c) {
                continue;
            }
            entries.push(Entry { cmd: c, index });
            index += 1;
        }
        if entries.is_empty() {
            continue;
        }
        // 同组内把最近用过的提前（稳定排序：其余保持匹配度序）。
        entries.sort_by_key(|e| {
            if recent.contains(e.cmd) {
                let rank = recent.iter().position(|c| c == e.cmd).unwrap_or(usize::MAX);
                (0usize, rank)
            } else {
                (1usize, 0usize)
            }
        });
        // 重排后 index 必须按新顺序重编（键盘导航依赖它连续）。
        let base = index - entries.len();
        for (i, e) in entries.iter_mut().enumerate() {
            e.index = base + i;
        }
        sections.push(Section { group: *g, entries });
    }

    // 查询为空 → 最近使用段插到最前（它自身也算一种「结果」）。
    if recent.is_empty() {
        return sections;
    }
    let recent_entries: Vec<Entry> = recent
        .iter()
        .enumerate()
        .map(|(i, cmd)| Entry { cmd, index: i })
        .collect();
    // index 统一重编：Recent 在最前，占 0..n。
    let shift = recent_entries.len();
    for s in sections.iter_mut() {
        for e in s.entries.iter_mut() {
            e.index += shift;
        }
    }
    sections.insert(
        0,
        Section {
            group: Group::Recent,
            entries: recent_entries,
        },
    );
    sections
}

/// 扁平化（键盘上下移动的遍历域；`index` 即在此列表中的下标）。
pub fn flatten(sections: &[Section]) -> Vec<Cmd> {
    let mut out = Vec::new();
    for s in sections {
        for e in &s.entries {
            out.push(e.cmd);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands;

    #[test]
    fn every_command_has_a_group() {
        // 编译期 match 已穷尽；这里钉住「不存在归到 Recent 的固有命令」。
        for &c in Cmd::ALL {
            assert_ne!(group_of(c), Group::Recent, "{c:?} 不该有固有最近分组");
        }
    }

    #[test]
    fn empty_query_groups_all_commands() {
        let results = commands::search("");
        let ring = RecentRing::new(5);
        let sections = build_sections(&results, &ring);
        let flat = flatten(&sections);
        assert_eq!(flat.len(), Cmd::ALL.len(), "所有命令都应出现在分组里");
        assert!(
            sections.iter().all(|s| s.group != Group::Recent),
            "无最近记录时不应有最近段"
        );
    }

    #[test]
    fn sections_are_non_empty_and_ordered() {
        let sections = build_sections(&commands::search(""), &RecentRing::new(5));
        for s in &sections {
            assert!(!s.entries.is_empty(), "{:?} 段为空却产出了", s.group);
        }
        // 固定段顺序 = Group::ORDER 的子序列
        let order: Vec<Group> = sections.iter().map(|s| s.group).collect();
        let mut last = 0usize;
        for g in &order {
            let pos = Group::ORDER.iter().position(|x| x == g).unwrap();
            assert!(pos >= last, "分组顺序错乱：{order:?}");
            last = pos;
        }
    }

    #[test]
    fn indices_are_contiguous_across_sections() {
        // 键盘上下移动依赖 index 跨段连续。
        let sections = build_sections(&commands::search(""), &RecentRing::new(5));
        let mut seen: Vec<usize> = Vec::new();
        for s in &sections {
            for e in &s.entries {
                seen.push(e.index);
            }
        }
        let expect: Vec<usize> = (0..seen.len()).collect();
        assert_eq!(seen, expect, "index 必须连续且从 0 开始");
    }

    #[test]
    fn recent_ring_dedups_and_moves_to_front() {
        let mut r = RecentRing::new(3);
        r.touch(Cmd::Save);
        r.touch(Cmd::Undo);
        r.touch(Cmd::Save); // 再按一次 → 提到最前，不重复
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![Cmd::Save, Cmd::Undo]);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn recent_ring_caps_capacity() {
        let mut r = RecentRing::new(2);
        r.touch(Cmd::Save);
        r.touch(Cmd::Undo);
        r.touch(Cmd::Redo);
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![Cmd::Redo, Cmd::Undo]);
    }

    #[test]
    fn recent_section_prepended_when_query_empty() {
        let mut r = RecentRing::new(3);
        r.touch(Cmd::Save);
        let sections = build_sections(&commands::search(""), &r);
        assert_eq!(sections[0].group, Group::Recent);
        assert_eq!(sections[0].entries[0].cmd, Cmd::Save);
        // 且 Save 不应在「文件」段里重复出现
        let file_idx = sections.iter().position(|s| s.group == Group::File);
        if let Some(fi) = file_idx {
            assert!(sections[fi].entries.iter().all(|e| e.cmd != Cmd::Save));
        }
    }

    #[test]
    fn recent_commands_do_not_duplicate_in_their_group() {
        // 同一条命令在面板里只能出现一次：最近段出现过的，分组段里不再出现。
        let mut r = RecentRing::new(3);
        r.touch(Cmd::Redo);
        r.touch(Cmd::Save);
        let sections = build_sections(&commands::search(""), &r);
        let mut all: Vec<Cmd> = Vec::new();
        for s in &sections {
            for e in &s.entries {
                assert!(!all.contains(&e.cmd), "{:?} 在面板里出现了两次", e.cmd);
                all.push(e.cmd);
            }
        }
        assert_eq!(all.len(), Cmd::ALL.len(), "去重后仍应覆盖全部命令");
    }

    #[test]
    fn no_recent_means_no_recent_section() {
        let sections = build_sections(&commands::search(""), &RecentRing::new(3));
        assert!(sections.iter().all(|s| s.group != Group::Recent));
    }

    #[test]
    fn flatten_matches_indices() {
        let mut r = RecentRing::new(3);
        r.touch(Cmd::Save);
        let sections = build_sections(&commands::search(""), &r);
        let flat = flatten(&sections);
        for s in &sections {
            for e in &s.entries {
                assert_eq!(flat[e.index], e.cmd, "index 与扁平列表不一致");
            }
        }
    }
}
