//! 最近打开（#68 阶段 F5）——文件 20 / 工作区 10 的记忆列表。
//!
//! 持久化：`$XDG_CONFIG_HOME/mang/recent.json`（与 settings.json 同目录
//! ——[`crate::settings::config_dir`]；F5 起两个落盘件共用配置目录口径）。
//!
//! 语义（VS Code 惯例）：
//! - **去重前置**：重复打开把既有项移到队首（不重复入列）；
//! - **容量上限**：文件 20 / 工作区 10，超限逐出队尾；
//! - **写穿透**：每次 record 即落盘（不做退出钩子——异常退出不丢记忆）；
//! - **容错**：读失败 → 空列表；写失败静默（记忆是增强不是依赖）。
//!
//! 消费面：空态引导页最近列表（点击打开；已删除文件置灰提示）。

use std::path::{Path, PathBuf};

/// 文件记忆上限。
pub const MAX_RECENT_FILES: usize = 20;
/// 工作区记忆上限。
pub const MAX_RECENT_WORKSPACES: usize = 10;

/// 最近打开存储（内存态 + 落盘通道）。
#[derive(Default, Clone, Debug, PartialEq)]
pub struct RecentStore {
    files: Vec<PathBuf>,
    workspaces: Vec<PathBuf>,
}

impl RecentStore {
    /// 从配置目录加载（`dir` = None 或读失败 → 空列表）。
    pub fn load() -> Self {
        Self::load_from(settings_dir().as_deref())
    }

    /// 落盘到配置目录（写失败静默）。
    pub fn save(&self) {
        Self::save_to(self, settings_dir().as_deref());
    }

    /// 记录一次文件打开（去重前置 + 容量上限）。
    pub fn record_file(&mut self, path: &Path) {
        push_dedup(&mut self.files, path.to_path_buf(), MAX_RECENT_FILES);
        self.save();
    }

    /// 记录一次工作区打开（去重前置 + 容量上限）。
    pub fn record_workspace(&mut self, root: &Path) {
        push_dedup(
            &mut self.workspaces,
            root.to_path_buf(),
            MAX_RECENT_WORKSPACES,
        );
        self.save();
    }

    pub fn recent_files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn recent_workspaces(&self) -> &[PathBuf] {
        &self.workspaces
    }

    fn load_from(dir: Option<&Path>) -> Self {
        let Some(dir) = dir else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(dir.join("recent.json")) else {
            return Self::default();
        };
        let value: serde_json::Value = text.parse().unwrap_or_default();
        let strings = |key: &str| -> Vec<PathBuf> {
            value[key]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(PathBuf::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            files: strings("files"),
            workspaces: strings("workspaces"),
        }
    }

    fn save_to(store: &Self, dir: Option<&Path>) {
        let Some(dir) = dir else {
            return;
        };
        let lossy = |paths: &[PathBuf]| -> Vec<String> {
            paths.iter().map(|p| p.display().to_string()).collect()
        };
        let value = serde_json::json!({
            "files": lossy(&store.files),
            "workspaces": lossy(&store.workspaces),
        });
        let _ = std::fs::create_dir_all(dir);
        if let Ok(text) = serde_json::to_string_pretty(&value) {
            let _ = std::fs::write(dir.join("recent.json"), text);
        }
    }
}

/// 去重前置 + 容量上限（既有项移队首；超限逐出队尾）。纯函数。
pub fn push_dedup(list: &mut Vec<PathBuf>, path: PathBuf, cap: usize) {
    list.retain(|p| p != &path);
    list.insert(0, path);
    while list.len() > cap {
        list.pop();
    }
}

fn settings_dir() -> Option<PathBuf> {
    crate::settings::config_dir()
}

// ---------------------------------------------------------------------------
// 单测（纯逻辑 + temp 目录持久化往返）。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_file_dedupes_and_moves_to_front() {
        let mut store = RecentStore::default();
        store.files = vec![
            PathBuf::from("/a.md"),
            PathBuf::from("/b.md"),
            PathBuf::from("/c.md"),
        ];
        store.record_file_nosave(Path::new("/b.md"));
        assert_eq!(
            store.recent_files(),
            ["/b.md", "/a.md", "/c.md"].map(PathBuf::from),
            "重复打开移到队首（不重复入列）"
        );
    }

    #[test]
    fn record_file_caps_at_20() {
        let mut store = RecentStore::default();
        for i in 0..25 {
            store.record_file_nosave(Path::new(&format!("/f{i}.md")));
        }
        assert_eq!(store.recent_files().len(), MAX_RECENT_FILES, "容量上限");
        assert_eq!(
            store.recent_files()[0],
            PathBuf::from("/f24.md"),
            "最新在队首"
        );
        assert!(
            !store.recent_files().contains(&PathBuf::from("/f0.md")),
            "最旧被逐出"
        );
    }

    #[test]
    fn record_workspace_caps_at_10() {
        let mut store = RecentStore::default();
        for i in 0..15 {
            store.record_workspace_nosave(Path::new(&format!("/ws{i}")));
        }
        assert_eq!(store.recent_workspaces().len(), MAX_RECENT_WORKSPACES);
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mang-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = RecentStore::default();
        store.record_file_in(Path::new("/x/a.md"), Some(&dir));
        store.record_file_in(Path::new("/y/b.md"), Some(&dir));
        store.record_workspace_in(Path::new("/ws"), Some(&dir));

        let loaded = RecentStore::load_from(Some(&dir));
        assert_eq!(
            loaded.recent_files(),
            ["/y/b.md", "/x/a.md"].map(PathBuf::from),
            "往返保序"
        );
        assert_eq!(loaded.recent_workspaces(), [PathBuf::from("/ws")]);

        // 读不存在目录 → 空列表。
        assert_eq!(RecentStore::load_from(None), RecentStore::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    impl RecentStore {
        /// 测试面：跳过落盘的 record（真实 record 走 save——测试目录注入）。
        fn record_file_nosave(&mut self, path: &Path) {
            push_dedup(&mut self.files, path.to_path_buf(), MAX_RECENT_FILES);
        }

        fn record_workspace_nosave(&mut self, root: &Path) {
            push_dedup(
                &mut self.workspaces,
                root.to_path_buf(),
                MAX_RECENT_WORKSPACES,
            );
        }

        fn record_file_in(&mut self, path: &Path, dir: Option<&Path>) {
            push_dedup(&mut self.files, path.to_path_buf(), MAX_RECENT_FILES);
            RecentStore::save_to(self, dir);
        }

        fn record_workspace_in(&mut self, root: &Path, dir: Option<&Path>) {
            push_dedup(
                &mut self.workspaces,
                root.to_path_buf(),
                MAX_RECENT_WORKSPACES,
            );
            RecentStore::save_to(self, dir);
        }
    }
}
