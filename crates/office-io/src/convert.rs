//! DOCX → PDF 导出桥（#307 阶段①）——LibreOffice headless 通道。
//!
//! # 定位
//!
//! 研究仓 C 路线（LO headless）的主仓落地：本模块只做
//! **DOCX → PDF 单向导出**（阶段② PDF→DOCX 显式不在本面——PDF 是
//! 「画好的纸」，反向需结构猜测，后续按 #307 阶段②单独立项）。
//!
//! ```text
//!   docx bytes ──► temp 目录落盘 ──► soffice --headless --convert-to pdf
//!                        │                        （独立 profile + 超时）
//!                        ▼                                    │
//!                 唯一 UserInstallation                        ▼
//!                （防并发锁：LO 单 profile                     pdf bytes
//!                 二实例即锁死——docx_fields.rs 先例）
//! ```
//!
//! # 错误分类（验收：无 LO 显式指引，不静默失败）
//!
//! [`ExportPdfError`] 四类：`NoLibreOffice`（带安装指引——桌面用户
//! 可操作的下一步）、`LaunchFailed`、`ConvertFailed`（LO 非零退出 +
//! 截断的 stderr）、`OutputMissing`（退出 0 但产物缺席——LO 偶发
//! 静默失败的兜底）。超时 = `ConvertFailed` 的 `timed_out` 位。
//!
//! # 敌意输入纪律
//!
//! LO 转换外部进程处理**不可信 docx**：默认 60s 超时（`DEFAULT_TIMEOUT`）
//! 杀进程——炸弹文档（解压炸弹/超深结构）不会拖住调用方；temp 目录
//! 用完即清（Drop 守卫）。
//!
//! # 版本面
//!
//! [`detect_libreoffice`] 返回解析出的版本串（诊断/遥测面）；**不做
//! 硬版本门**——writer_pdf_Export 滤镜面跨 LTS 稳定（研究仓 25.2/26.8
//! 双版本验证），加硬门只会制造「装了 LO 却被拒」的假阴性。版本随
//! [`LibreOfficeInfo`] 暴露，调用方自行决定是否展示。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// LO 转换默认超时（秒）——敌意输入的硬上界。
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// 探测到的 LibreOffice 安装。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibreOfficeInfo {
    /// 可执行文件绝对路径（`soffice` / `soffice.exe` / `soffice.bin`）。
    pub path: PathBuf,
    /// `--version` 原始首行（如 `LibreOffice 25.2.3.2 520(Build:2)`）。
    pub version_line: String,
    /// 解析出的 `主.次` 版本（如 `25.2`）；解析失败为 `None`。
    pub version: Option<String>,
}

/// 导出失败分类。全部携带可呈现信息——「不静默失败」纪律的载体。
#[derive(Debug)]
pub enum ExportPdfError {
    /// 未找到 LibreOffice——`guidance()` 给安装指引。
    NoLibreOffice,
    /// 找到可执行但启动失败（权限/平台错配等）。
    LaunchFailed(PathBuf, String),
    /// LO 进程非零退出或超时——携带截断的 stderr（诊断面）。
    ConvertFailed {
        timed_out: bool,
        stderr_tail: String,
    },
    /// LO 退出 0 但产物缺席（LO 偶发静默失败的兜底）。
    OutputMissing,
    /// 输入不是 DOCX 容器（早期拒绝——不必惊动外部进程）。
    NotDocx(String),
}

impl std::fmt::Display for ExportPdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoLibreOffice => write!(f, "未找到 LibreOffice"),
            Self::LaunchFailed(p, e) => write!(f, "LibreOffice 启动失败（{}）：{}", p.display(), e),
            Self::ConvertFailed {
                timed_out,
                stderr_tail,
            } => {
                if *timed_out {
                    write!(f, "LibreOffice 转换超时（>{DEFAULT_TIMEOUT_SECS}s）")
                } else {
                    write!(f, "LibreOffice 转换失败：{}", stderr_tail)
                }
            }
            Self::OutputMissing => write!(f, "LibreOffice 退出但未产出 PDF（静默失败）"),
            Self::NotDocx(m) => write!(f, "输入不是 DOCX：{}", m),
        }
    }
}

impl std::error::Error for ExportPdfError {}

impl ExportPdfError {
    /// 用户可操作的指引文案（`NoLibreOffice` 必有；其余给出诊断方向）。
    pub fn guidance(&self) -> String {
        match self {
            Self::NoLibreOffice => "导出 PDF 需要 LibreOffice（免费开源）：\n\
                 · Windows：https://www.libreoffice.org/download/ 安装后重试\n\
                 · macOS：安装 LibreOffice.app 或 `brew install --cask libreoffice`\n\
                 · Linux（Debian/Ubuntu）：sudo apt install libreoffice-writer\n\
                 安装后无需配置——导出时会自动探测。"
                .to_string(),
            Self::LaunchFailed(p, _) => format!(
                "请检查 LibreOffice 可执行（{}）是否可运行；必要时重装。",
                p.display()
            ),
            Self::ConvertFailed { timed_out, .. } if *timed_out => {
                "文档过大或结构异常导致转换超时。可先用 Word/LibreOffice 打开检查该文件是否损坏。"
                    .to_string()
            }
            Self::ConvertFailed { .. } => {
                "转换失败（详见错误详情）。该文档可能使用了 LibreOffice 不支持的特性。".to_string()
            }
            Self::OutputMissing => {
                "LibreOffice 报告成功但没有产出文件——偶发的 profile 损坏，重试一次通常可恢复。"
                    .to_string()
            }
            Self::NotDocx(_) => "只能从 .docx 导出（.doc 旧格式请先另存为 .docx）".to_string(),
        }
    }
}

/// 探测 LibreOffice：PATH → 平台常见安装位；`--version` 验活 + 解析。
///
/// 探测不缓存（调用方自行决定缓存策略）：安装/卸载后无需重启应用
/// 即可重新探测——「导出时发现刚装好」优于启动时的快照。
pub fn detect_libreoffice() -> Option<LibreOfficeInfo> {
    let candidates: Vec<PathBuf> = candidate_paths();
    for path in candidates {
        // 版本探针兼验活：不可执行/挂起的候选直接跳过。
        let Ok(output) = Command::new(&path)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if line.is_empty() {
            continue;
        }
        let version = parse_version(&line);
        return Some(LibreOfficeInfo {
            path,
            version_line: line,
            version,
        });
    }
    None
}

/// 各平台候选路径（PATH 在前——用户自装位置优先于系统包）。
fn candidate_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            v.push(dir.join(soffice_name()));
        }
    }
    // 平台常见安装位（PATH 未覆盖的场景：GUI 启动不继承 shell PATH）。
    if cfg!(windows) {
        for base in [
            r"C:\Program Files\LibreOffice\program",
            r"C:\Program Files (x86)\LibreOffice\program",
        ] {
            v.push(PathBuf::from(base).join("soffice.exe"));
        }
    } else if cfg!(target_os = "macos") {
        v.push(PathBuf::from(
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        ));
    } else {
        for p in [
            "/usr/bin/soffice",
            "/usr/local/bin/soffice",
            "/opt/libreoffice/program/soffice",
        ] {
            v.push(PathBuf::from(p));
        }
    }
    v.retain(|p| p.is_file());
    // 去重（PATH 里可能重复出现同一目录）。
    v.sort();
    v.dedup();
    v
}

#[cfg(windows)]
fn soffice_name() -> &'static str {
    "soffice.exe"
}
#[cfg(not(windows))]
fn soffice_name() -> &'static str {
    "soffice"
}

/// `LibreOffice 25.2.3.2 520(Build:2)` → `25.2`（宽松解析：取版本串
/// 前两段数字；解析不出 = `None`，不作为探测失败）。
fn parse_version(line: &str) -> Option<String> {
    let rest = line.split_whitespace().nth(1)?;
    let mut it = rest.split('.');
    let major = it.next()?.parse::<u32>().ok()?;
    let minor = it.next().and_then(|m| m.parse::<u32>().ok()).unwrap_or(0);
    Some(format!("{major}.{minor}"))
}

/// DOCX 字节 → PDF 字节。
///
/// 全流程 temp 目录（输入落盘 + 唯一 profile + 产物回收），目录在
/// 返回前清理。超时由 [`DEFAULT_TIMEOUT_SECS`] 控制。
pub fn docx_to_pdf(docx: &[u8]) -> Result<Vec<u8>, ExportPdfError> {
    docx_to_pdf_with_timeout(docx, Duration::from_secs(DEFAULT_TIMEOUT_SECS))
}

/// [`docx_to_pdf`] 的显式超时变体（测试/调用方定制面）。
pub fn docx_to_pdf_with_timeout(docx: &[u8], timeout: Duration) -> Result<Vec<u8>, ExportPdfError> {
    // 早期容器校验：PK 头 + zip 内 presence 由 read_docx 面承担，这里
    // 只挡明显不是 DOCX 的输入（省一次进程启动）。
    if docx.len() < 4 || &docx[..2] != b"PK" {
        return Err(ExportPdfError::NotDocx(format!(
            "缺少 ZIP 容器魔数（{} 字节）",
            docx.len()
        )));
    }
    let lo = detect_libreoffice().ok_or(ExportPdfError::NoLibreOffice)?;

    let dir = TempDir::new().map_err(|e| {
        ExportPdfError::LaunchFailed(lo.path.clone(), format!("temp 目录创建失败：{e}"))
    })?;
    let input = dir.path.join("input.docx");
    std::fs::write(&input, docx)
        .map_err(|e| ExportPdfError::LaunchFailed(lo.path.clone(), format!("输入落盘失败：{e}")))?;

    // 独立 UserInstallation：LO 单 profile 下并发二实例即锁死
    //（docx_fields.rs 测试先例同纪律）；每次调用唯一目录。
    let profile = dir.path.join("profile");
    let profile_url = path_to_file_url(&profile);
    // stdout/stderr 落临时文件而非管道：轮询 try_wait 无法收管道
    //（LO 输出虽少，管道 deadlock 风险 + 失败诊断都要 stderr —— 文件面
    // 两者全解）。javadocx 警告无害：失败判定只看退出码 + 产物。
    let stdout_path = dir.path.join("lo-stdout.txt");
    let stderr_path = dir.path.join("lo-stderr.txt");
    let stdout_file = std::fs::File::create(&stdout_path).map_err(|e| {
        ExportPdfError::LaunchFailed(lo.path.clone(), format!("stdout 重定向失败：{e}"))
    })?;
    let stderr_file = std::fs::File::create(&stderr_path).map_err(|e| {
        ExportPdfError::LaunchFailed(lo.path.clone(), format!("stderr 重定向失败：{e}"))
    })?;
    let mut child = Command::new(&lo.path)
        .arg("--headless")
        .arg("--norestore")
        .arg(format!("-env:UserInstallation={}", profile_url))
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(dir.path.as_os_str())
        .arg(input.as_os_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout_file))
        .stderr(std::process::Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| ExportPdfError::LaunchFailed(lo.path.clone(), e.to_string()))?;

    // 超时守卫：轮询 + 到点杀（wait_with_output 无原生超时）。
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ExportPdfError::ConvertFailed {
                        timed_out: true,
                        stderr_tail: stderr_tail(&stderr_path),
                    });
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(ExportPdfError::ConvertFailed {
                    timed_out: false,
                    stderr_tail: format!("wait 失败：{e}"),
                });
            }
        }
    };

    if !status.success() {
        return Err(ExportPdfError::ConvertFailed {
            timed_out: false,
            stderr_tail: stderr_tail(&stderr_path),
        });
    }

    let pdf_path = dir.path.join("input.pdf");
    match std::fs::read(&pdf_path) {
        Ok(bytes) if bytes.len() > 4 && &bytes[..4] == b"%PDF" => Ok(bytes),
        _ => Err(ExportPdfError::OutputMissing),
    }
}

/// stderr 截尾（末 400 字符——诊断面够用，不膨胀错误对象）。
fn stderr_tail(path: &Path) -> String {
    std::fs::read_to_string(path)
        .map(|s| {
            let t = s.trim();
            if t.is_empty() {
                "(无 stderr)".to_string()
            } else if t.len() > 400 {
                format!("…{}", &t[t.len() - 400..])
            } else {
                t.to_string()
            }
        })
        .unwrap_or_else(|_| "(stderr 不可读)".to_string())
}

/// `Path` → `file://` URL（`-env:UserInstallation` 参数方言——LO 要求
/// URL 形态；Windows 盘符与空格均需编码）。
fn path_to_file_url(p: &Path) -> String {
    let mut url = String::from("file://");
    let s = p.to_string_lossy();
    for ch in s.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' | '/' | ':' => url.push(ch),
            _ => {
                let mut buf = [0u8; 4];
                for b in ch.encode_utf8(&mut buf).as_bytes() {
                    url.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    url
}

/// 用完即清的 temp 目录（Drop 守卫——敌意输入场景也不留残骸）。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let base = std::env::temp_dir();
        let unique = format!(
            "mdc-convert-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = base.join(unique);
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse() {
        assert_eq!(
            parse_version("LibreOffice 25.2.3.2 520(Build:2)"),
            Some("25.2".to_string())
        );
        assert_eq!(parse_version("LibreOffice 7.6"), Some("7.6".to_string()));
        assert_eq!(parse_version("LibreOffice"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn file_url_encoding() {
        assert_eq!(path_to_file_url(Path::new("/tmp/a b")), "file:///tmp/a%20b");
        // Windows 盘符冒号保留。
        assert_eq!(
            path_to_file_url(Path::new(r"C:\Users\x")),
            "file://C:%5CUsers%5Cx"
        );
    }

    #[test]
    fn not_docx_early_reject() {
        let err = docx_to_pdf(b"not a zip").unwrap_err();
        assert!(matches!(err, ExportPdfError::NotDocx(_)));
        assert!(err.guidance().contains(".docx"));
    }

    #[test]
    fn no_libreoffice_guidance_actionable() {
        let g = ExportPdfError::NoLibreOffice.guidance();
        // 三平台指引齐备（可操作 = 有动词有去处）。
        assert!(g.contains("Windows"));
        assert!(g.contains("macOS") || g.contains("brew"));
        assert!(g.contains("Linux") || g.contains("apt"));
    }

    // 真 LO 转换面在 tests/convert_docx_pdf.rs（docx-rs 现场构造夹具
    // ——本 crate 测试惯例；手卷最小 zip 会被 LO 静默拒，不在此重复）。
}
