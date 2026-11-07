//! 状态栏信息面（#63 Lead 打磨项）——**纯数据层**。
//!
//! 本模块只做「算什么、怎么显示」，不碰 GPUI：所有判定都是可单测的纯
//! 函数（[`detect_eol`] / [`detect_encoding`] / [`git_branch_at`] /
//! [`format_selection`]）。渲染在 `status_bar.rs`，两者分开的理由：
//! 这几项全部依赖「文件字节/工作区目录」这类外部事实，把它们埋进渲染
//! 闭包里就没法对着边界写断言了。
//!
//! # 四项新信息
//!
//! | 项 | 来源 | 缺省 |
//! |---|---|---|
//! | 选中字数 | 编辑器选区 | 无选区不显示 |
//! | git 分支 | 工作区向上找 `.git` | 非 git 仓库不显示 |
//! | 编码 | 文件字节 BOM + 严格 UTF-8 校验 | 未知不显示 |
//! | 换行符 | 文件字节扫描 | 无换行不显示 |
//!
//! # 编码判定的诚实边界
//!
//! GBK 判定采用「**严格 UTF-8 解不出来 + 字节落在 GBK 双字节合法区**」
//! 的保守规则：
//!
//! - UTF-8 能解 → 报 UTF-8（GBK 文本几乎不可能恰好构成合法 UTF-8 序列）；
//! - UTF-8 解不出、且全部字节都在 GBK 合法范围 → 报 GBK；
//! - 否则报「未知」。
//!
//! 这是**启发式**，不是真正的 GBK 解码器：报「未知」而不是猜一个更接近
//! 诚实。真正的编码检测需要统计模型（chardet 一类），不在本批范围。

/// 换行符风格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    /// 混合（两种都有）——如实报告，不猜「主要是哪种」。
    Mixed,
}

impl Eol {
    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
            Eol::Mixed => "混合换行",
        }
    }
}

/// 从字节扫描换行符：优先看第一个出现的行结束符，同时统计是否混合。
///
/// 空文档 / 无换行 → `None`（状态栏不显示该项，而不是显示一个猜的值）。
pub fn detect_eol(bytes: &[u8]) -> Option<Eol> {
    let mut seen_lf = false;
    let mut seen_crlf = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                seen_crlf = true;
                i += 2;
                continue;
            }
            b'\n' => seen_lf = true,
            _ => {}
        }
        i += 1;
    }
    match (seen_lf, seen_crlf) {
        (true, true) => Some(Eol::Mixed),
        (false, true) => Some(Eol::Crlf),
        (true, false) => Some(Eol::Lf),
        (false, false) => None,
    }
}

/// 源文件编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8 无 BOM。
    Utf8,
    /// UTF-8 带 BOM（EF BB BF）。
    Utf8Bom,
    /// UTF-16 小端 BOM（FF FE）。
    Utf16Le,
    /// UTF-16 大端 BOM（FE FF）。
    Utf16Be,
    /// 启发式判定为 GBK（见模块文档的诚实边界）。
    Gbk,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Gbk => "GBK",
        }
    }
}

/// BOM 长度（字节）。
pub fn bom_len(bytes: &[u8]) -> usize {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        2
    } else {
        0
    }
}

/// 检测编码。返回 `None` = 无法判定（不猜）。
pub fn detect_encoding(bytes: &[u8]) -> Option<Encoding> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some(Encoding::Utf8Bom);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some(Encoding::Utf16Le);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some(Encoding::Utf16Be);
    }
    if bytes.is_empty() {
        // 空文件：合法 UTF-8，但没有内容能说明它「是」哪种编码的另一半
        // （GBK 空文件同样是空字节）。报 UTF-8 是唯一合理缺省。
        return Some(Encoding::Utf8);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return Some(Encoding::Utf8);
    }
    if looks_like_gbk(bytes) {
        return Some(Encoding::Gbk);
    }
    None
}

/// GBK 启发式：全部字节要么是 ASCII，要么构成合法的 GBK 双字节序列。
///
/// GBK 双字节：首字节 0x81..=0xFE，次字节 0x40..=0xFE（0x7F 除外）。
/// 只看结构性合法，不做语义校验——全角标点、生僻字都在这个范围里，
/// 但乱码二进制也可能恰好符合。所以本函数只用来**排除**「明显不是 GBK」，
/// 判定结论在 [`detect_encoding`] 里还会与「UTF-8 解不出」合取。
fn looks_like_gbk(bytes: &[u8]) -> bool {
    let mut i = 0;
    let mut saw_multibyte = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            i += 1;
            continue;
        }
        // 双字节首字节
        if !(0x81..=0xFE).contains(&b) {
            return false;
        }
        let Some(&next) = bytes.get(i + 1) else {
            // 末尾孤立首字节 → 不是合法 GBK
            return false;
        };
        if !(0x40..=0xFE).contains(&next) || next == 0x7F {
            return false;
        }
        saw_multibyte = true;
        i += 2;
    }
    saw_multibyte
}

/// 选区信息（状态栏「选中 N 字 / M 行」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionInfo {
    /// 选中字符数（含换行符——与编辑器 char_count 同口径）。
    pub chars: usize,
    /// 跨越的行数（同一行内选 = 1 行）。
    pub lines: usize,
}

/// 由选区文本与首尾行号算出显示用选区信息。`None` = 无选区。
pub fn selection_info(text: &str, start_line: usize, end_line: usize) -> Option<SelectionInfo> {
    if text.is_empty() {
        return None;
    }
    let chars = text.chars().count();
    let lines = end_line.saturating_sub(start_line) + 1;
    Some(SelectionInfo { chars, lines })
}

/// 选区标签（状态栏字符串）。`None` = 无选区 / 空选区。
pub fn format_selection(info: Option<SelectionInfo>) -> Option<String> {
    let info = info?;
    if info.chars == 0 {
        return None;
    }
    Some(if info.lines > 1 {
        format!("选中 {} 字 / {} 行", info.chars, info.lines)
    } else {
        format!("选中 {} 字", info.chars)
    })
}

/// 从起始目录向上找 `.git`，返回仓库根。
///
/// **不做** `git` 子进程调用：只要判断「这里是不是 git 仓库」，读目录就够
/// 了（启动子进程会拖慢状态栏刷新，且 Windows 上 git.exe 未必在 PATH）。
/// 代价是 worktree/submodule 这类 `.git` 是文件的形态也要认（`.git` 文件
/// 存在即视为仓库根）。
pub fn find_git_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// 读 `.git/HEAD` 判定当前分支。
///
/// - `ref: refs/heads/main` → `Some("main")`
/// - 分离头指针（HEAD 里是裸 commit sha）→ `Some("(detached)")`
/// - 读不到 → `None`
///
/// worktree/submodule 的 `.git` 是文件（内容 `gitdir: <路径>`），
/// 顺着它去找真正的 HEAD。
pub fn git_branch_at(git_root: &std::path::Path) -> Option<String> {
    let head_path = resolve_head_path(git_root)?;
    let head = std::fs::read_to_string(head_path).ok()?;
    let head = head.trim();
    if let Some(r) = head.strip_prefix("ref:") {
        let r = r.trim();
        // 分支名可能含 '/'（feat/x）：只剥 refs/heads/ 前缀，不能取最后一段
        // ——最后一段会把 feat/x 截成 x。非分支 ref（refs/tags/v1）同样剥到
        // 第一段之后，保留剩余整体。
        let short = r
            .strip_prefix("refs/heads/")
            .or_else(|| r.strip_prefix("refs/"))
            .unwrap_or(r);
        Some(short.to_string())
    } else if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) {
        Some("(detached)".to_string())
    } else {
        None
    }
}

/// 解析 git 仓库的真实 HEAD 路径（普通仓库 = `.git/HEAD`；worktree/
/// submodule 的 `.git` 是文件——内容 `gitdir: <路径>`，顺过去找）。
fn resolve_head_path(git_root: &std::path::Path) -> Option<std::path::PathBuf> {
    let dot_git = git_root.join(".git");
    if dot_git.is_dir() {
        Some(dot_git.join("HEAD"))
    } else {
        let content = std::fs::read_to_string(&dot_git).ok()?;
        let dir = content.trim().strip_prefix("gitdir:")?.trim();
        let p = std::path::PathBuf::from(dir);
        let p = if p.is_absolute() { p } else { git_root.join(p) };
        Some(p.join("HEAD"))
    }
}

// ---------------------------------------------------------------------------
// #281（审计 P1-17 / R-e）：状态栏每帧 I/O 缓存
//
// 状态栏随 ShellApp 每次重渲被调用（gpui 祖先标脏机制：每键/每拖拽
// move 都整树重渲），此前每帧 = md 态源文件**全文件读盘**（编码/EOL
// 探测）+ `.git/HEAD` 读。缓存策略 = **mtime + size 失效**（stat 命中
// 免读盘——stat 只取元数据无数据传输，比读盘约省三个数量级；文件
// 保存/外部改写后 mtime/size 变更恰失效一次）。同 mtime 同 size 但
// 内容已变的窗口（同秒内重写同长文件）是本策略的已知边界——状态
// 标签滞后一帧级探测语义，可接受（在档）。
// ---------------------------------------------------------------------------

/// 真实读盘计数（测试观测面：断言缓存命中零 `fs::read`）。
#[cfg(test)]
static READ_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 编码/EOL 探测缓存项。
struct EncodingEolEntry {
    mtime: std::time::SystemTime,
    size: u64,
    enc: Option<Encoding>,
    eol: Option<Eol>,
}

/// 编码/EOL 探测缓存（path → 结果；mtime+size 失效）。
static ENCODING_EOL_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, EncodingEolEntry>>,
> = std::sync::OnceLock::new();

/// 带缓存的编码/EOL 探测（#281 R-e——状态栏每帧调用的读盘面收敛为
/// stat：命中直接复用；未命中/失效恰读一次重新探测）。
pub fn detect_encoding_eol_cached(
    path: &std::path::Path,
) -> Option<(Option<Encoding>, Option<Eol>)> {
    let meta = std::fs::metadata(path).ok()?;
    let (mtime, size) = (meta.modified().ok()?, meta.len());
    let mut cache = ENCODING_EOL_CACHE
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hit) = cache.get(path) {
        if hit.mtime == mtime && hit.size == size {
            return Some((hit.enc, hit.eol));
        }
    }
    #[cfg(test)]
    READ_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let bytes = std::fs::read(path).ok()?;
    let enc = detect_encoding(&bytes);
    let eol = detect_eol(&bytes);
    cache.insert(
        path.to_path_buf(),
        EncodingEolEntry {
            mtime,
            size,
            enc,
            eol,
        },
    );
    Some((enc, eol))
}

/// git 分支缓存项（HEAD mtime 失效——checkout 会更新 HEAD 的 mtime；
/// None mtime = HEAD 不可 stat，恒重试不缓存）。
struct GitBranchEntry {
    head_mtime: Option<std::time::SystemTime>,
    branch: Option<String>,
}

/// git 分支缓存（git_root → 结果）。
static GIT_BRANCH_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, GitBranchEntry>>,
> = std::sync::OnceLock::new();

/// 带缓存的 git 分支（#281 R-e 同族 I/O——HEAD mtime 失效；`.git`
/// 目录上溯 walk 仍每帧执行（纯 stat 无数据传输，量级可忽略）。
pub fn git_branch_at_cached(git_root: &std::path::Path) -> Option<String> {
    let head_mtime = resolve_head_path(git_root)
        .and_then(|head| std::fs::metadata(head).ok())
        .and_then(|meta| meta.modified().ok());
    let mut cache = GIT_BRANCH_CACHE
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hit) = cache.get(git_root) {
        if hit.head_mtime.is_some() && hit.head_mtime == head_mtime {
            return hit.branch.clone();
        }
    }
    let branch = git_branch_at(git_root);
    cache.insert(
        git_root.to_path_buf(),
        GitBranchEntry {
            head_mtime,
            branch: branch.clone(),
        },
    );
    branch
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 换行符 ───────────────────────────────────────────────

    #[test]
    fn eol_lf_only() {
        assert_eq!(detect_eol(b"a\nb\nc\n"), Some(Eol::Lf));
    }

    #[test]
    fn eol_crlf_only() {
        assert_eq!(detect_eol(b"a\r\nb\r\n"), Some(Eol::Crlf));
    }

    #[test]
    fn eol_mixed_is_reported_as_mixed() {
        // 不猜「主要是什么」——混了就如实说混了。
        assert_eq!(detect_eol(b"a\r\nb\nc\n"), Some(Eol::Mixed));
    }

    #[test]
    fn eol_none_when_no_newline() {
        assert_eq!(detect_eol(b"single line"), None);
        assert_eq!(detect_eol(b""), None);
    }

    // ── 编码 ─────────────────────────────────────────────────

    #[test]
    fn encoding_plain_utf8() {
        assert_eq!(detect_encoding("中文内容".as_bytes()), Some(Encoding::Utf8));
    }

    #[test]
    fn encoding_utf8_bom_detected() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("hi".as_bytes());
        assert_eq!(detect_encoding(&b), Some(Encoding::Utf8Bom));
        assert_eq!(bom_len(&b), 3);
    }

    #[test]
    fn encoding_utf16_boms() {
        assert_eq!(
            detect_encoding(&[0xFF, 0xFE, 0x41, 0x00]),
            Some(Encoding::Utf16Le)
        );
        assert_eq!(
            detect_encoding(&[0xFE, 0xFF, 0x00, 0x41]),
            Some(Encoding::Utf16Be)
        );
    }

    #[test]
    fn encoding_gbk_detected_when_not_utf8() {
        // "中文" 的 GBK 编码：D6 D0 CE C4 —— 不是合法 UTF-8。
        let gbk = [0xD6u8, 0xD0, 0xCE, 0xC4];
        assert!(
            std::str::from_utf8(&gbk).is_err(),
            "前提：这串不是合法 UTF-8"
        );
        assert_eq!(detect_encoding(&gbk), Some(Encoding::Gbk));
    }

    #[test]
    fn encoding_empty_is_utf8() {
        assert_eq!(detect_encoding(b""), Some(Encoding::Utf8));
    }

    #[test]
    fn encoding_unknown_when_neither() {
        // 孤立 0x81（GBK 首字节但后面没有合法次字节）→ 判不出
        assert_eq!(detect_encoding(&[0x81]), None);
        // 0x80 不是 GBK 首字节也不是合法 UTF-8 起始
        assert_eq!(detect_encoding(&[0x80, 0x80]), None);
    }

    #[test]
    fn gbk_rejects_ascii_only() {
        // 纯 ASCII 不该被报成 GBK（虽然是合法 GBK 子集）
        assert_eq!(detect_encoding(b"hello"), Some(Encoding::Utf8));
    }

    // ── 选区 ─────────────────────────────────────────────────

    #[test]
    fn selection_single_line() {
        let info = selection_info("abcd", 3, 3);
        assert_eq!(info, Some(SelectionInfo { chars: 4, lines: 1 }));
        assert_eq!(format_selection(info).as_deref(), Some("选中 4 字"));
    }

    #[test]
    fn selection_multi_line() {
        let info = selection_info("ab\ncd\nef", 2, 4);
        assert_eq!(info, Some(SelectionInfo { chars: 8, lines: 3 }));
        assert_eq!(format_selection(info).as_deref(), Some("选中 8 字 / 3 行"));
    }

    #[test]
    fn selection_empty_is_none() {
        assert_eq!(selection_info("", 0, 0), None);
        assert_eq!(format_selection(None), None);
    }

    // ── git ──────────────────────────────────────────────────

    #[test]
    fn git_branch_parsed_from_head() {
        let dir = std::env::temp_dir().join(format!("mdc-git-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        assert_eq!(git_branch_at(&dir).as_deref(), Some("feat/x"));
        assert_eq!(find_git_root(&dir).as_deref(), Some(dir.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_detached_head_reported() {
        let dir = std::env::temp_dir().join(format!("mdc-git-det-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(
            dir.join(".git/HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();
        assert_eq!(git_branch_at(&dir).as_deref(), Some("(detached)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_absent_returns_none() {
        let dir = std::env::temp_dir().join(format!("mdc-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(find_git_root(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── #281（审计 R-e）：mtime+size 失效缓存 ─────────────────

    /// 缓存命中零读盘（帧级语义：状态栏随 ShellApp 每次重渲被调，
    /// 命中路径必须零 `fs::read`——READ_COUNT 观测）+ 文件变更
    /// （size 变）恰失效重读一次。
    #[test]
    fn encoding_eol_cache_hits_and_invalidates() {
        use std::sync::atomic::Ordering;
        let dir = std::env::temp_dir().join(format!("mdc-cache-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.txt");
        std::fs::write(&path, "a\nb\n").unwrap();
        let reads0 = READ_COUNT.load(Ordering::SeqCst);
        // 未命中 → 恰读一次 + 探测。
        let (enc, eol) = detect_encoding_eol_cached(&path).unwrap();
        assert_eq!(enc, Some(Encoding::Utf8));
        assert_eq!(eol, Some(Eol::Lf));
        assert_eq!(
            READ_COUNT.load(Ordering::SeqCst),
            reads0 + 1,
            "未命中恰读一次"
        );
        // 命中 → 五次重渲零读盘、结果一致。
        for _ in 0..5 {
            let (enc2, eol2) = detect_encoding_eol_cached(&path).unwrap();
            assert_eq!((enc2, eol2), (enc, eol), "命中结果与首探测一致");
        }
        assert_eq!(
            READ_COUNT.load(Ordering::SeqCst),
            reads0 + 1,
            "五次重渲零读盘"
        );
        // 文件变更（size 变）→ 失效恰重读一次 + 新内容探测正确。
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let (_, eol3) = detect_encoding_eol_cached(&path).unwrap();
        assert_eq!(eol3, Some(Eol::Crlf), "失效后按新内容重新探测");
        assert_eq!(
            READ_COUNT.load(Ordering::SeqCst),
            reads0 + 2,
            "失效恰重读一次"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// git 分支缓存：HEAD 未变命中；checkout（HEAD 重写）后失效
    /// 读新分支（mtime 间隔用 sleep 保证跨粒度）。
    #[test]
    fn git_branch_cache_invalidates_on_checkout() {
        let dir = std::env::temp_dir().join(format!("mdc-cache-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join("repo");
        let dot_git = git.join(".git");
        std::fs::create_dir_all(&dot_git).unwrap();
        std::fs::write(dot_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(git_branch_at_cached(&git).as_deref(), Some("main"));
        // 命中（同 HEAD）。
        assert_eq!(git_branch_at_cached(&git).as_deref(), Some("main"));
        // checkout → HEAD 重写（跨 mtime 粒度）→ 失效读新分支。
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dot_git.join("HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        assert_eq!(
            git_branch_at_cached(&git).as_deref(),
            Some("feat/x"),
            "checkout 后分支缓存失效（新分支名）"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
