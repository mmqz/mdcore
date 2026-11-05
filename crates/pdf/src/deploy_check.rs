//! pdfium 部署自检（#68 方向一；feature `engine-pdfium`）。
//!
//! **为什么需要这个模块**：生产定位序（[`crate::engine_pdfium`]）绑定
//! 失败时的错误消息只列出「试过的路径」，且只有打开 PDF 撞错才可见
//! ——用户无法主动诊断「每一层具体找哪个路径、为什么没找到、下一步
//! 放哪里」。本模块把同一套定位序展开成**逐层探针 + 绑定走查 +
//! 可操作修复指引**，供 CLI 子命令（`mang-shell --check-pdfium`）输出。
//!
//! ## 与生产定位序的关系（不双轨）
//!
//! - 候选清单：探针收集与绑定走查都消费 [`PdfiumEngine::library_candidate_roles`]
//!   （单一事实源——角色名/顺序/路径只在 engine_pdfium 维护）；
//! - 绑定原语：走查逐层调用 [`try_bind`] / [`try_bind_system`]（与生产
//!   `bind_by_resolution` 同一函数；成功副作用 = crate 进程单例就位，
//!   自检进程与应用进程的绑定行为面一致）；
//! - 走查序（env → 候选清单 → 系统）与生产三步序一致，由单测
//!   [`tests::probes_match_production_resolution_order`] 钉死（探针层序
//!   = env + `library_candidates()` + system，逐项同源比对）。
//!
//! ## 测试纪律（单测零真实绑定）
//!
//! pdfium 绑定是进程级单例（pdfium-render 全局 OnceCell）。本模块
//! 把「探针收集」（[`collect_probes`]——纯 fs 面，零绑定副作用）与
//! 「绑定走查」（[`bind_walk`]——真实绑定，首个成功 = 进程单例就位）
//! **拆开**：单测只覆盖探针收集与渲染（合成报告），绝不触发真实
//! 绑定；真实走查（[`run_check`] 全链）由独立集成测试二进制
//! `tests/pdfk_check.rs` 覆盖（独立进程 = 首绑定语义，循
//! `tests/pdfk_deploy.rs` 先例；环境合同同款：库源缺失 = 明确 panic
//! 打印全部已尝试位置，不静默 skip）。
//!
//! ## 退出码合同（CLI 消费方）
//!
//! `mang-shell --check-pdfium`：`0` = 已绑定（库加载 + 全符号表解析 +
//! `FPDF_InitLibrary` 初始化调用通过——pdfium-render `DynamicPdfiumBindings`
//! 构造期急切解析全部 FPDF 符号，绑定成功即三者全过），`3` = 未绑定
//! （全部层未命中）。部署脚本 / CI 冒烟可按退出码判定。

use std::path::PathBuf;

use pdfium_render::prelude::*;

use crate::engine_pdfium::{try_bind, try_bind_system, PdfiumEngine, PDFIUM_LIB_ENV};

/// pdfium 官方二进制发布页（bblanchon/pdfium-binaries——与 INSTALL.md、
/// 生产 `pdfium_missing_library_hint` 同一来源）。
pub const PDFIUM_RELEASES_URL: &str = "https://github.com/bblanchon/pdfium-binaries/releases";

/// 定位层的种类（绑定走查与渲染按种类分派——不做标题嗅探）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    /// 环境变量层（`MDC_PDFIUM_LIB`——优先级最高）。
    Env,
    /// 编译期候选层（`library_candidate_roles` 展开——exe 同目录最前）。
    Candidate,
    /// 系统装载层（`bind_to_system_library`——装载器搜索序兜底）。
    System,
}

/// 一层探针的文件状态（fs 层——不触发绑定；「为什么没找到」的第一半：
/// 文件在不在、在的话是不是文件、多大）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeFile {
    /// env 未设置（本层无期望路径，生产定位序跳过）。
    EnvUnset,
    /// env 已设置但值非 UTF-8（生产 `std::env::var` 按未设置处理——
    /// 探针如实展示该差异，不假装未设置）。
    EnvNonUtf8,
    /// 期望路径处的文件不存在（最常见的「缺什么」形态）。
    Missing,
    /// 期望路径存在但不是常规文件（目录/坏符号链接——用户放对了
    /// 名字、放错了东西的可自助诊断形态）。
    NotAFile,
    /// 常规文件在位（含字节数——排障时先核大小是否离谱：0 字节或与
    /// 发布包体量差数量级 = 下载不完整）。
    File { bytes: u64 },
}

/// 单层探针（fs 状态 + 绑定尝试结果）。
#[derive(Debug, Clone)]
pub struct LayerProbe {
    /// 层种类（走查/渲染分派依据）。
    pub kind: LayerKind,
    /// 层名（来自 [`PdfiumEngine::library_candidate_roles`] 的角色名；
    /// env 与系统层为固定标题）。
    pub title: String,
    /// 期望路径（env 已设置与候选层有；其余 None）。
    pub expected: Option<PathBuf>,
    /// 文件状态。
    pub file: ProbeFile,
    /// 绑定尝试结果：`None` = 未尝试（env 未设置/非 UTF-8，或前层已
    /// 胜出——生产定位序同款短路）；`Some(Err(why))` = 该层尝试且失败
    /// （why = 装载器原文，如架构不匹配的 dlopen/LoadLibrary 报错）。
    pub bind: Option<Result<(), String>>,
}

/// 绑定走查终态。
#[derive(Debug, Clone)]
pub enum BindOutcome {
    /// 已绑定：胜出层标题 + 路径（系统层无显式路径）。
    Bound {
        source: String,
        path: Option<PathBuf>,
    },
    /// 全部层未命中：逐层尝试记录（格式与生产错误面的「定位详情」
    /// 同构——`<路径> → <装载器原文>`）。
    Failed { tried: Vec<String> },
}

impl BindOutcome {
    /// 是否已绑定（CLI 退出码合同：0 / 3）。
    pub fn is_bound(&self) -> bool {
        matches!(self, BindOutcome::Bound { .. })
    }
}

/// 自检报告（探针 + 走查终态 + 环境头信息）。
#[derive(Debug, Clone)]
pub struct CheckReport {
    /// 本平台库文件名（`pdfium.dll` / `libpdfium.so` / `libpdfium.dylib`）。
    pub library_name: String,
    /// 操作系统（`std::env::consts::OS`——下载包名段之一）。
    pub os: &'static str,
    /// CPU 架构（`std::env::consts::ARCH`——下载包名段之二）。
    pub arch: &'static str,
    /// 当前进程 exe 路径（「放 exe 同目录」的具体落点；`current_exe`
    /// 不可得的极端环境为 None——诚实展示，不猜）。
    pub exe: Option<PathBuf>,
    /// 逐层探针（序 = 生产定位序：env → 候选清单 → 系统）。
    pub probes: Vec<LayerProbe>,
    /// 绑定走查终态。
    pub outcome: BindOutcome,
}

/// 收集逐层探针（纯 fs 面——**零绑定副作用**，单测安全）。
///
/// 层序 = env（1 层）+ `library_candidate_roles()`（n 层）+ 系统（1 层）；
/// 各探针 `bind = None`（走查由 [`bind_walk`] 填充）。
pub fn collect_probes() -> Vec<LayerProbe> {
    let mut probes: Vec<LayerProbe> = Vec::new();
    // 层 1：env MDC_PDFIUM_LIB（var_os 探测——非 UTF-8 也能如实区分
    // 「未设置」与「设置了但生产按未设置处理」两种形态）。
    let env_probe = match std::env::var_os(PDFIUM_LIB_ENV) {
        None => LayerProbe {
            kind: LayerKind::Env,
            title: format!("环境变量 {PDFIUM_LIB_ENV}"),
            expected: None,
            file: ProbeFile::EnvUnset,
            bind: None,
        },
        Some(value) => match value.to_str() {
            None => LayerProbe {
                kind: LayerKind::Env,
                title: format!("环境变量 {PDFIUM_LIB_ENV}"),
                expected: None,
                file: ProbeFile::EnvNonUtf8,
                bind: None,
            },
            Some(path) => {
                let expected = PathBuf::from(path);
                let file = probe_file(&expected);
                LayerProbe {
                    kind: LayerKind::Env,
                    title: format!("环境变量 {PDFIUM_LIB_ENV}"),
                    expected: Some(expected),
                    file,
                    bind: None,
                }
            }
        },
    };
    probes.push(env_probe);
    // 层 2..n：编译期候选清单（角色 + 路径同源清单）。
    for (role, candidate) in PdfiumEngine::library_candidate_roles() {
        probes.push(LayerProbe {
            kind: LayerKind::Candidate,
            title: role.to_string(),
            expected: Some(candidate),
            file: ProbeFile::Missing, // 占位——下面统一探测。
            bind: None,
        });
    }
    // 统一 fs 探测（候选层）：expected 必在，直接填状态。
    for probe in probes.iter_mut().skip(1) {
        if probe.kind == LayerKind::Candidate {
            probe.file = probe_file(probe.expected.as_deref().expect("候选层必有期望路径"));
        }
    }
    // 层 n+1：系统装载（装载器搜索序——无固定期望路径）。
    probes.push(LayerProbe {
        kind: LayerKind::System,
        title: "系统装载（dlopen/LoadLibrary 搜索序）".to_string(),
        expected: None,
        file: ProbeFile::EnvUnset,
        bind: None,
    });
    probes
}

/// 绑定走查（**真实绑定**——按探针层序逐层尝试，首个成功 = 进程
/// 单例就位，余层 `bind = None`；与生产 `bind_by_resolution` 同序
/// 同原语）。逐层把尝试结果回填探针并产出终态。
///
/// 供 [`run_check`] 组合；独立 pub 是为了让集成测试可以在**已知探针**
/// 上走查（tests/pdfk_check.rs）。
pub fn bind_walk(probes: &mut [LayerProbe], library_name: &str) -> BindOutcome {
    let mut tried: Vec<String> = Vec::new();
    let mut bound: Option<(String, Option<PathBuf>)> = None;
    for probe in probes.iter_mut() {
        if bound.is_some() {
            probe.bind = None; // 前层已胜出——生产同款短路。
            continue;
        }
        match probe.kind {
            LayerKind::Env => match probe.expected.as_deref() {
                // 未设置 / 非 UTF-8：生产按跳过处理，本层不尝试。
                None => probe.bind = None,
                Some(path) => probe.bind = attempt_bind(path, &mut tried, &mut bound, probe),
            },
            LayerKind::Candidate => {
                let path = probe.expected.as_deref().expect("候选层必有期望路径");
                probe.bind = attempt_bind(path, &mut tried, &mut bound, probe);
            }
            LayerKind::System => {
                probe.bind = match try_bind_system() {
                    Ok(_) => {
                        bound = Some((probe.title.clone(), None));
                        Some(Ok(()))
                    }
                    Err(why) => {
                        tried.push(format!("system({library_name}) → {why}"));
                        Some(Err(why))
                    }
                };
            }
        }
    }
    match bound {
        Some((source, path)) => BindOutcome::Bound { source, path },
        None => BindOutcome::Failed { tried },
    }
}

/// 单层尝试（成功即记录胜出层；失败记入 tried——格式与生产
/// `bind_by_resolution` 的「定位详情」同构）。
fn attempt_bind(
    path: &std::path::Path,
    tried: &mut Vec<String>,
    bound: &mut Option<(String, Option<PathBuf>)>,
    probe: &LayerProbe,
) -> Option<Result<(), String>> {
    match try_bind(path) {
        Ok(_) => {
            *bound = Some((probe.title.clone(), Some(path.to_path_buf())));
            Some(Ok(()))
        }
        Err(why) => {
            let entry = match probe.kind {
                LayerKind::Env => format!("{PDFIUM_LIB_ENV}={} → {why}", path.display()),
                _ => format!("{} → {why}", path.display()),
            };
            tried.push(entry);
            Some(Err(why))
        }
    }
}

/// 运行部署自检（探针收集 + 真实绑定走查）。
///
/// **副作用**：走查按生产定位序逐层尝试绑定，**首个成功 = 进程单例
/// 就位**（与 `PdfiumEngine::open` 同款副作用）。CLI 单命令进程即退，
/// 该副作用 = 「自检结果与应用启动行为一致」的保证，不是污染。
pub fn run_check() -> CheckReport {
    let library_name = Pdfium::pdfium_platform_library_name()
        .to_string_lossy()
        .into_owned();
    let mut probes = collect_probes();
    let outcome = bind_walk(&mut probes, &library_name);
    CheckReport {
        library_name,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        exe: std::env::current_exe().ok(),
        probes,
        outcome,
    }
}

/// fs 探针（单点——`fs::metadata` 跟随符号链接取目标事实：坏链接 =
/// `Missing`，与装载器「路径在但打不开」的体感一致）。
fn probe_file(path: &std::path::Path) -> ProbeFile {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => ProbeFile::File { bytes: meta.len() },
        Ok(_) => ProbeFile::NotAFile,
        Err(_) => ProbeFile::Missing,
    }
}

/// 渲染自检报告（人读 CLI 输出——`mang-shell --check-pdfium` 的 stdout）。
///
/// 失败面必须满足验收（#68 方向一）：**没有 pdfium 的机器上**，输出能
/// 明确告出「缺什么、放哪、怎么验证」——由单测
/// [`tests::render_failure_is_actionable`] 逐串钉死。
pub fn render_report(report: &CheckReport) -> String {
    let mut out = String::new();
    out.push_str("mang-shell pdfium 部署自检\n");
    out.push_str(&format!(
        "平台: {} / {}；库文件名: {}\n",
        report.os, report.arch, report.library_name
    ));
    match &report.exe {
        Some(exe) => out.push_str(&format!("进程: {}\n", exe.display())),
        None => out.push_str("进程: current_exe() 不可得（极端环境）\n"),
    }
    out.push('\n');
    out.push_str("定位序逐层探针（与实际绑定序一致）:\n");
    let mut winner_seen = false;
    for (idx, probe) in report.probes.iter().enumerate() {
        out.push_str(&format!("[{}] {}\n", idx + 1, probe.title));
        match probe.kind {
            LayerKind::Env => match (&probe.file, probe.expected.as_deref()) {
                (ProbeFile::EnvUnset, _) => out.push_str("    值: 未设置（本层跳过）\n"),
                (ProbeFile::EnvNonUtf8, _) => {
                    out.push_str("    值: 已设置但非 UTF-8（生产定位序按未设置处理，本层跳过）\n")
                }
                (file, Some(expected)) => {
                    out.push_str(&format!("    期望: {}\n", expected.display()));
                    push_file_state(&mut out, file);
                }
                // collect_probes 不会产出该组合（防御：如实展示不猜）。
                (_, None) => out.push_str("    值: 状态异常（请回报）\n"),
            },
            LayerKind::Candidate => {
                out.push_str(&format!(
                    "    期望: {}\n",
                    probe
                        .expected
                        .as_deref()
                        .expect("候选层必有期望路径")
                        .display()
                ));
                push_file_state(&mut out, &probe.file);
            }
            LayerKind::System => {
                out.push_str(&format!(
                    "    期望: {}（装载器搜索序，无固定路径）\n",
                    report.library_name
                ));
                out.push_str("    提示: Linux 为 LD_LIBRARY_PATH / ldconfig 缓存 / 默认目录；Windows 为 exe 目录 / 系统目录 / PATH\n");
            }
        }
        match &probe.bind {
            Some(Ok(())) => {
                out.push_str("    绑定: 成功 ← 本层胜出\n");
                winner_seen = true;
            }
            Some(Err(why)) => out.push_str(&format!("    绑定: 失败（{why}）\n")),
            None if winner_seen => out.push_str("    绑定: 未尝试（前层已胜出）\n"),
            None => out.push_str("    绑定: 未尝试（本层无路径）\n"),
        }
    }
    out.push('\n');
    match &report.outcome {
        BindOutcome::Bound { source, path } => {
            out.push_str("绑定结果: 已绑定\n");
            match path {
                Some(path) => out.push_str(&format!("  来源: {source} → {}\n", path.display())),
                None => out.push_str(&format!("  来源: {source}（无显式路径——装载器解析）\n")),
            }
            out.push_str("  验证: 库加载 + 全符号表解析 + FPDF_InitLibrary 初始化调用通过\n");
            out.push_str("退出码: 0（已绑定）\n");
        }
        BindOutcome::Failed { tried } => {
            out.push_str("绑定结果: 未绑定（全部层未命中）\n");
            if !tried.is_empty() {
                out.push_str("尝试记录:\n");
                for entry in tried {
                    out.push_str(&format!("  - {entry}\n"));
                }
            }
            out.push('\n');
            out.push_str("修复指引:\n");
            out.push_str(&format!(
                "  1. 缺什么: pdfium 原生库（本机 {} / {}，库文件名 {}）\n",
                report.os, report.arch, report.library_name
            ));
            out.push_str(&format!(
                "  2. 去哪下: {PDFIUM_RELEASES_URL}（选匹配 {}-{} 的包；不要 v8 变体）\n",
                map_os_label(report.os),
                map_arch_label(report.arch)
            ));
            match &report.exe {
                Some(exe) => match exe.parent() {
                    Some(dir) => out.push_str(&format!(
                        "  3. 放哪里（三选一）:\n     a. exe 同目录（推荐）: {} ← 把 {} 放进这个目录\n     b. 环境变量: 设置 {PDFIUM_LIB_ENV}=<库完整路径>（优先级最高）\n     c. 系统搜索位（Linux: LD_LIBRARY_PATH/ldconfig；Windows: PATH/系统目录）\n",
                        dir.display(),
                        report.library_name
                    )),
                    None => out.push_str(&format!(
                        "  3. 放哪里（三选一）:\n     a. exe 同目录（exe 路径无父目录——异常环境）\n     b. 环境变量: 设置 {PDFIUM_LIB_ENV}=<库完整路径>（优先级最高）\n     c. 系统搜索位（Linux: LD_LIBRARY_PATH/ldconfig；Windows: PATH/系统目录）\n"
                    )),
                },
                None => out.push_str(&format!(
                    "  3. 放哪里（三选一）:\n     a. exe 同目录（current_exe() 不可得——本进程无法给出具体路径）\n     b. 环境变量: 设置 {PDFIUM_LIB_ENV}=<库完整路径>（优先级最高）\n     c. 系统搜索位（Linux: LD_LIBRARY_PATH/ldconfig；Windows: PATH/系统目录）\n"
                )),
            }
            out.push_str("  4. 怎么验证: 重新运行 mang-shell --check-pdfium（退出码 0 = 就位）\n");
            out.push_str("退出码: 3（未绑定）\n");
        }
    }
    out
}

/// 文件状态行（env 候选两层共用）。
fn push_file_state(out: &mut String, file: &ProbeFile) {
    match file {
        ProbeFile::EnvUnset | ProbeFile::EnvNonUtf8 => {
            out.push_str("    文件: 无路径语义（内部状态异常——请回报）\n")
        }
        ProbeFile::Missing => out.push_str("    文件: 不存在\n"),
        ProbeFile::NotAFile => out.push_str("    文件: 存在但不是常规文件（目录或坏链接）\n"),
        ProbeFile::File { bytes } => out.push_str(&format!("    文件: 在位（{bytes} 字节）\n")),
    }
}

/// OS → bblanchon 发布包名段（诚实映射：不确定的平台原样展示，提示
/// 用户在发布页核对——不猜一个可能不存在的包名）。
fn map_os_label(os: &str) -> String {
    match os {
        "windows" => "win".to_string(),
        "macos" => "mac".to_string(),
        other => other.to_string(),
    }
}

/// ARCH → bblanchon 发布包名段（x86_64 → x64；其余原样）。
fn map_arch_label(arch: &str) -> String {
    match arch {
        "x86_64" => "x64".to_string(),
        "aarch64" => "arm64".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层序形状锁：探针层 = [env] + `library_candidates()`（同源函数，
    /// 逐项比对）+ [system]——探针展示序与生产绑定序不可能漂移。
    /// （collect_probes 零绑定副作用——单测安全。）
    #[test]
    fn probes_match_production_resolution_order() {
        let probes = collect_probes();
        let candidates = PdfiumEngine::library_candidates();
        // 探针数 = env 1 + 候选 n + 系统 1。
        assert_eq!(
            probes.len(),
            candidates.len() + 2,
            "探针层数 = env + 候选清单 + 系统"
        );
        // 首层 = env。
        assert_eq!(probes[0].kind, LayerKind::Env, "首层必须是环境变量层");
        // 中段候选层：标题 + 期望路径与同源清单逐项相等。
        let roles = PdfiumEngine::library_candidate_roles();
        for (i, (role, path)) in roles.iter().enumerate() {
            let probe = &probes[i + 1];
            assert_eq!(
                probe.kind,
                LayerKind::Candidate,
                "候选层种类（第 {} 层）",
                i + 2
            );
            assert_eq!(probe.title, *role, "候选层标题必须同源（第 {} 层）", i + 2);
            assert_eq!(
                probe.expected.as_deref(),
                Some(path.as_path()),
                "候选层期望路径必须同源（第 {} 层）",
                i + 2
            );
        }
        // 末层 = 系统。
        assert_eq!(
            probes.last().unwrap().kind,
            LayerKind::System,
            "末层必须是系统装载层"
        );
        // 收集面零绑定尝试（走查归 bind_walk）。
        assert!(
            probes.iter().all(|p| p.bind.is_none()),
            "collect_probes 不得尝试绑定"
        );
    }

    /// 探针文件状态三分支（Missing / NotAFile / File）——临时目录构造，
    /// 不触发任何绑定。
    #[test]
    fn probe_file_states() {
        let dir = std::env::temp_dir().join(format!(
            "mdc_pdf_deploy_check_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Missing：不存在的路径。
        assert_eq!(probe_file(&dir.join("nope.so")), ProbeFile::Missing);
        // NotAFile：目录本体。
        assert_eq!(probe_file(&dir), ProbeFile::NotAFile);
        // File：常规文件 + 字节数。
        let file = dir.join("dummy.so");
        std::fs::write(&file, b"not a real library").unwrap();
        assert_eq!(
            probe_file(&file),
            ProbeFile::File {
                bytes: b"not a real library".len() as u64
            }
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 验收主断言（#68 方向一）：全层未命中的报告渲染后，必须明确告出
    /// 「缺什么、放哪、怎么验证」——逐串钉死，删任何一环即红。
    #[test]
    fn render_failure_is_actionable() {
        let report = CheckReport {
            library_name: "libpdfium.so".to_string(),
            os: "linux",
            arch: "x86_64",
            exe: Some(PathBuf::from("/opt/app/mang-shell")),
            probes: vec![
                LayerProbe {
                    kind: LayerKind::Env,
                    title: format!("环境变量 {PDFIUM_LIB_ENV}"),
                    expected: None,
                    file: ProbeFile::EnvUnset,
                    bind: None,
                },
                LayerProbe {
                    kind: LayerKind::Candidate,
                    title: "exe 同目录（产品部署位）".to_string(),
                    expected: Some(PathBuf::from("/opt/app/libpdfium.so")),
                    file: ProbeFile::Missing,
                    bind: Some(Err("库加载失败：nope".to_string())),
                },
            ],
            outcome: BindOutcome::Failed {
                tried: vec!["/opt/app/libpdfium.so → 库加载失败：nope".to_string()],
            },
        };
        let text = render_report(&report);
        // 缺什么：库文件名 + 平台。
        assert!(text.contains("libpdfium.so"), "必须点名库文件名");
        assert!(text.contains("linux / x86_64"), "必须点名平台");
        // 去哪下：发布页 URL（不猜 v8 变体）。
        assert!(text.contains(PDFIUM_RELEASES_URL), "必须给出下载地址");
        assert!(text.contains("不要 v8 变体"));
        // 放哪里：exe 同目录的具体落点 + 环境变量名。
        assert!(text.contains("/opt/app"), "必须给出 exe 同目录具体路径");
        assert!(text.contains("← 把 libpdfium.so 放进这个目录"));
        assert!(text.contains(PDFIUM_LIB_ENV));
        assert!(text.contains("<库完整路径>"));
        // 怎么验证：重跑命令 + 退出码。
        assert!(text.contains("mang-shell --check-pdfium"));
        assert!(text.contains("退出码 0 = 就位"));
        assert!(text.contains("退出码: 3（未绑定）"));
        // 尝试记录必须在场（「为什么没找到」的装载器原文面）。
        assert!(text.contains("尝试记录"));
        assert!(text.contains("库加载失败：nope"));
    }

    /// 成功面渲染：来源层 + 具体路径 + 验证声明 + 退出码 0。
    #[test]
    fn render_success_names_source() {
        let report = CheckReport {
            library_name: "pdfium.dll".to_string(),
            os: "windows",
            arch: "x86_64",
            exe: Some(PathBuf::from(r"C:\app\mang-shell.exe")),
            probes: vec![LayerProbe {
                kind: LayerKind::Candidate,
                title: "exe 同目录（产品部署位）".to_string(),
                expected: Some(PathBuf::from(r"C:\app\pdfium.dll")),
                file: ProbeFile::File { bytes: 2_842_112 },
                bind: Some(Ok(())),
            }],
            outcome: BindOutcome::Bound {
                source: "exe 同目录（产品部署位）".to_string(),
                path: Some(PathBuf::from(r"C:\app\pdfium.dll")),
            },
        };
        let text = render_report(&report);
        assert!(text.contains("绑定结果: 已绑定"));
        assert!(text.contains("exe 同目录（产品部署位）"));
        assert!(text.contains("C:\\app\\pdfium.dll"));
        assert!(text.contains("FPDF_InitLibrary"));
        assert!(text.contains("退出码: 0（已绑定）"));
        // 字节数展示（下载完整性自检面）。
        assert!(text.contains("2842112 字节"));
        // 胜出层标记。
        assert!(text.contains("← 本层胜出"));
    }

    /// 逐层探针的绑定尝试列四态渲染（成功/失败/无路径未尝试/前层胜出
    /// 未尝试——按真实走查序合成）+ 系统层文案。
    #[test]
    fn render_bind_attempt_and_special_layers() {
        let report = CheckReport {
            library_name: "libpdfium.so".to_string(),
            os: "linux",
            arch: "x86_64",
            exe: None,
            probes: vec![
                // 1) env 未设置 → 无路径未尝试。
                LayerProbe {
                    kind: LayerKind::Env,
                    title: format!("环境变量 {PDFIUM_LIB_ENV}"),
                    expected: None,
                    file: ProbeFile::EnvUnset,
                    bind: None,
                },
                // 2) 候选 A：文件在位但架构不匹配 → 尝试失败。
                LayerProbe {
                    kind: LayerKind::Candidate,
                    title: "exe 同目录（产品部署位）".to_string(),
                    expected: Some(PathBuf::from("/app/libpdfium.so")),
                    file: ProbeFile::File { bytes: 42 },
                    bind: Some(Err("dlopen: wrong ELF class".to_string())),
                },
                // 3) 候选 B：胜出。
                LayerProbe {
                    kind: LayerKind::Candidate,
                    title: "产品仓 bin/lib（开发取件位）".to_string(),
                    expected: Some(PathBuf::from("/repo/bin/lib/libpdfium.so")),
                    file: ProbeFile::File { bytes: 7_942_208 },
                    bind: Some(Ok(())),
                },
                // 4) 系统层：前层已胜出 → 未尝试。
                LayerProbe {
                    kind: LayerKind::System,
                    title: "系统装载（dlopen/LoadLibrary 搜索序）".to_string(),
                    expected: None,
                    file: ProbeFile::EnvUnset,
                    bind: None,
                },
            ],
            outcome: BindOutcome::Bound {
                source: "产品仓 bin/lib（开发取件位）".to_string(),
                path: Some(PathBuf::from("/repo/bin/lib/libpdfium.so")),
            },
        };
        let text = render_report(&report);
        assert!(text.contains("绑定: 成功 ← 本层胜出"));
        assert!(text.contains("绑定: 失败（dlopen: wrong ELF class）"));
        // 未尝试两分：无路径（env 未设置）/ 前层已胜出（系统层）。
        assert!(text.contains("绑定: 未尝试（本层无路径）"));
        assert!(text.contains("绑定: 未尝试（前层已胜出）"));
        // 系统层：搜索序说明。
        assert!(text.contains("装载器搜索序，无固定路径"));
        assert!(text.contains("LD_LIBRARY_PATH"));
        // exe 不可得的诚实展示（不猜路径）。
        assert!(text.contains("current_exe() 不可得"));
        // env 未设置展示。
        assert!(text.contains("值: 未设置（本层跳过）"));
    }

    /// env 已设置但非 UTF-8 的探针语义（生产按未设置处理——探针必须
    /// 如实展示该差异，而非假装未设置）。
    #[test]
    fn render_env_non_utf8_is_explicit() {
        let probe = LayerProbe {
            kind: LayerKind::Env,
            title: format!("环境变量 {PDFIUM_LIB_ENV}"),
            expected: None,
            file: ProbeFile::EnvNonUtf8,
            bind: None,
        };
        let report = CheckReport {
            library_name: "libpdfium.so".to_string(),
            os: "linux",
            arch: "x86_64",
            exe: None,
            probes: vec![probe],
            outcome: BindOutcome::Failed { tried: vec![] },
        };
        let text = render_report(&report);
        assert!(text.contains("非 UTF-8"));
        assert!(text.contains("按未设置处理"));
    }

    /// `is_bound` 退出码合同面。
    #[test]
    fn bind_outcome_is_bound() {
        let bound = BindOutcome::Bound {
            source: "x".to_string(),
            path: None,
        };
        let failed = BindOutcome::Failed { tried: vec![] };
        assert!(bound.is_bound());
        assert!(!failed.is_bound());
    }
}
