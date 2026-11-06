//! #240 壳面打印流——把 PDF 交给系统打印（最小闭环：方案 C）。
//!
//! 分层定位（issue #240 裁定）：
//!
//! ```text
//! 渲染原语（DPI/页区间/注释开关） → 引擎（mdc-pdf::print——已交付）
//! 打印（送系统打印流）            → 壳（本模块，平台相关）
//! ```
//!
//! ## 技术路线（方案 C——Lead 倾向且经本仓侦察证实）
//!
//! GPUI **无打印 API**（third_party/zed 全域检索：print 命中全部是
//! IME printable-keys 语义，无 PrintOperation/NSPrint/StartDoc 封装），
//! 自绘打印对话框（方案 A）成本超最小闭环。方案 C：**交系统默认
//! 处理程序/CUPS**——
//!
//! ```text
//! Windows：powershell Start-Process -Verb Print（系统打印对话框——
//!          默认 PDF 处理程序承载，页区间/份数/缩放全在系统 UI）
//! macOS：  open -p（系统打印面板——同上，系统 UI 承载设置）
//! Linux：  lp（CUPS 直投默认打印机——页区间/份数/缩放映射为
//!          lp 参数，无对话框环境的标准路径）
//! ```
//!
//! ## 可测性纪律
//!
//! 平台 exec 无打印机环境跑不了真流程（CI 只验编译——issue #240
//! 纪律第 5 条）。因此**参数构造全部拆成纯函数**（`*_args`——单测
//! 逐字锁），exec 层只留「存在性校验 + 启动 + 退出码」三步；文件
//! 不存在的错误路径跨平台可测。
//!
//! ## 设置面
//!
//! [`PrintSettings`]（页区间/份数/缩放）在 Windows/macOS 由系统
//! 对话框承载（构造时不映射）；Linux/CUPS 映射为 lp 参数
//! （[`lp_args`]——`-P`/`-n`/`-o fit-to-page`）。用户输入 1 基页号
//! 与 lp `-P` 口径一致（打印页码 = 1 基——CUPS 语义），无换基。

use std::path::Path;
use std::process::Command;

/// 打印缩放模式（Acrobat「适合页宽 / 实际大小」对齐项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PrintScaling {
    /// 实际大小（100%——纸张小于页面时裁切）。
    #[default]
    ActualSize,
    /// 适合页面（缩放到纸张内——CUPS `fit-to-page`）。
    ///
    /// 构造面：lib 单测（参数映射锁）+ 设置 UI（后续接线位）；bin 树
    /// 暂无构造点（缺省 = `ActualSize`，#240 最小闭环不带设置面板）——
    /// API 完整性保留位，非死代码。
    #[allow(dead_code)]
    FitPage,
}

/// 打印设置（页区间 1 基闭区间；None = 全部页）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PrintSettings {
    /// 页区间（1 基闭区间 `(from, to)`；`None` = 全部）。
    pub range: Option<(u32, u32)>,
    /// 份数（<1 规整为 1——[`PrintSettings::normalized`]）。
    pub copies: u32,
    /// 缩放模式。
    pub scaling: PrintScaling,
}

impl PrintSettings {
    /// 规整：份数下限 1；页区间翻转自动交换（容错用户输入「5-3」→
    /// 「3-5」——打印面不因可推断的笔误拒绝作业）。
    pub fn normalized(&self) -> PrintSettings {
        PrintSettings {
            range: self.range.map(|(a, b)| (a.min(b), a.max(b))),
            copies: self.copies.max(1),
            scaling: self.scaling,
        }
    }
}

/// 打印作业（文档名 + 源文件 + 设置）。
pub struct PrintJob<'a> {
    /// 打印任务名（lp `-t`——打印队列里显示的作业名）。
    pub doc_name: &'a str,
    /// 待打印的 PDF 源文件。
    pub file: &'a Path,
    /// 打印设置（exec 前经 [`PrintSettings::normalized`] 规整）。
    pub settings: PrintSettings,
}

/// 打印结果（后端 + 实际 argv——用户可见的「已送打印」凭据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintOutcome {
    /// 后端描述（「CUPS lp」/「Windows 打印动词」/「macOS open -p」）。
    pub backend: &'static str,
    /// 完整命令行（argv[0] = 程序名）。
    pub argv: Vec<String>,
}

impl PrintOutcome {
    /// 用户可读凭据（通知横幅承载）。
    pub fn describe(&self) -> String {
        format!("{}：{}", self.backend, self.argv.join(" "))
    }
}

/// 打印失败（typed——不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintError {
    /// 待打印文件不存在（路径携带）。
    NotFound(String),
    /// 平台无打印后端（Linux 无 `lp` 命令——修复指引见 Display）。
    NoBackend,
    /// 后端启动失败或非零退出（诊断字符串）。
    Spawn(String),
}

impl std::fmt::Display for PrintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrintError::NotFound(path) => write!(f, "待打印文件不存在：{path}"),
            PrintError::NoBackend => write!(
                f,
                "未找到打印后端（lp）。Linux 需要 CUPS 客户端：\
                 安装 cups-ipc 或 printer-driver-cups-pdf 后重试"
            ),
            PrintError::Spawn(detail) => write!(f, "打印后端执行失败：{detail}"),
        }
    }
}

/// CUPS `lp` 参数构造（Linux；不含 argv[0]——纯函数，单测逐字锁）。
pub fn lp_args(file: &Path, settings: &PrintSettings, doc_name: &str) -> Vec<String> {
    let s = settings.normalized();
    let mut args: Vec<String> = Vec::new();
    if !doc_name.is_empty() {
        args.push("-t".into());
        args.push(doc_name.into());
    }
    if s.copies > 1 {
        args.push("-n".into());
        args.push(s.copies.to_string());
    }
    if let Some((from, to)) = s.range {
        args.push("-P".into());
        // 单页区间打印为「N」（与「N-N」等价——CUPS 文档口径）。
        if from == to {
            args.push(from.to_string());
        } else {
            args.push(format!("{from}-{to}"));
        }
    }
    if matches!(s.scaling, PrintScaling::FitPage) {
        args.push("-o".into());
        args.push("fit-to-page=true".into());
    }
    args.push(file.to_string_lossy().into_owned());
    args
}

/// Windows 打印动词参数（PowerShell `Start-Process -Verb Print`——
/// 设置面由系统对话框承载，不映射）。
pub fn windows_args(file: &Path) -> Vec<String> {
    vec![
        "-NoProfile".into(),
        "-Command".into(),
        format!("Start-Process -FilePath '{}' -Verb Print", file.display()),
    ]
}

/// macOS 系统打印参数（`open -p`——打印面板承载设置面）。
pub fn macos_args(file: &Path) -> Vec<String> {
    vec!["-p".into(), file.to_string_lossy().into_owned()]
}

/// 送打印（平台分发——见模块文档的路线说明）。阻塞到后端返回
/// （`lp` spool 即返、`powershell`/`open` 交接即返——无需异步）。
pub fn print_pdf(job: &PrintJob) -> Result<PrintOutcome, PrintError> {
    if !job.file.exists() {
        return Err(PrintError::NotFound(job.file.display().to_string()));
    }
    let (program, args, backend): (&str, Vec<String>, &'static str) = if cfg!(target_os = "windows")
    {
        (
            "powershell",
            windows_args(job.file),
            "Windows 打印动词（系统默认处理程序）",
        )
    } else if cfg!(target_os = "macos") {
        ("open", macos_args(job.file), "macOS open -p（系统打印）")
    } else {
        (
            "lp",
            lp_args(job.file, &job.settings, job.doc_name),
            "CUPS lp",
        )
    };
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().cloned());
    match Command::new(program).args(&args).status() {
        Ok(status) if status.success() => Ok(PrintOutcome { backend, argv }),
        Ok(status) => Err(PrintError::Spawn(format!("{backend} 非零退出（{status}）"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(PrintError::NoBackend),
        Err(e) => Err(PrintError::Spawn(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// 单测（纯函数面——跨平台裸环境全绿；exec 面仅 NotFound 路径确定性可测）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp_args_minimal_all_defaults() {
        let file = Path::new("/tmp/report.pdf");
        let s = PrintSettings::default();
        assert_eq!(
            lp_args(file, &s, "报告"),
            vec!["-t", "报告", "/tmp/report.pdf"],
            "缺省：全部页/1 份/实际大小 → 只有任务名 + 文件"
        );
    }

    #[test]
    fn lp_args_full_settings() {
        let file = Path::new("/tmp/bid.pdf");
        let s = PrintSettings {
            range: Some((2, 5)),
            copies: 3,
            scaling: PrintScaling::FitPage,
        };
        assert_eq!(
            lp_args(file, &s, "标书"),
            vec![
                "-t",
                "标书",
                "-n",
                "3",
                "-P",
                "2-5",
                "-o",
                "fit-to-page=true",
                "/tmp/bid.pdf"
            ]
        );
    }

    #[test]
    fn lp_args_single_page_range_and_no_name() {
        let file = Path::new("x.pdf");
        let s = PrintSettings {
            range: Some((4, 4)),
            copies: 1,
            scaling: PrintScaling::ActualSize,
        };
        assert_eq!(lp_args(file, &s, ""), vec!["-P", "4", "x.pdf"]);
    }

    #[test]
    fn settings_normalized_swaps_inverted_range_and_clamps_copies() {
        let s = PrintSettings {
            range: Some((5, 3)),
            copies: 0,
            scaling: PrintScaling::FitPage,
        }
        .normalized();
        assert_eq!(s.range, Some((3, 5)), "翻转区间自动交换（5-3 → 3-5）");
        assert_eq!(s.copies, 1, "份数下限 1");
        assert_eq!(s.scaling, PrintScaling::FitPage);
    }

    #[test]
    fn windows_and_macos_args_shapes() {
        assert_eq!(
            windows_args(Path::new("C:/docs/a.pdf")),
            vec![
                "-NoProfile",
                "-Command",
                "Start-Process -FilePath 'C:/docs/a.pdf' -Verb Print"
            ]
        );
        assert_eq!(
            macos_args(Path::new("/Users/z/a.pdf")),
            vec!["-p", "/Users/z/a.pdf"]
        );
    }

    #[test]
    fn print_pdf_missing_file_is_typed_not_found() {
        let job = PrintJob {
            doc_name: "不存在",
            file: Path::new("/tmp/__mdc_print_240_不存在的文件__.pdf"),
            settings: PrintSettings::default(),
        };
        assert_eq!(
            print_pdf(&job),
            Err(PrintError::NotFound(
                "/tmp/__mdc_print_240_不存在的文件__.pdf".into()
            )),
            "存在性校验先于后端探测（跨平台确定性路径）"
        );
    }

    #[test]
    fn outcome_describe_includes_backend_and_argv() {
        let outcome = PrintOutcome {
            backend: "CUPS lp",
            argv: vec!["lp".into(), "-t".into(), "a".into()],
        };
        assert_eq!(outcome.describe(), "CUPS lp：lp -t a");
    }
}
