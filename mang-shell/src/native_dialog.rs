//! #185：系统原生错误对话框——GUI 框架起不来时的「用户看得见」兜底。
//!
//! ## 为什么需要它
//!
//! wgpu/GPU 初始化失败发生在 `cx.open_window`——此时 GPUI 窗口系统尚未
//! 建立，**用 GPUI 弹对话框是死循环**（弹窗本身需要窗口）。桌面双击启动
//! 的用户 stderr 无处落（无终端父进程），表现是「点了没反应」。
//!
//! 唯一可行通道是**借用 OS 的原生提示工具**（不经本进程的图形栈）：
//!
//! | 平台 | 候选（按优先序——模态对话框优先，气泡兜底） |
//! |---|---|
//! | Linux | `zenity` → `kdialog` → `xmessage` → `notify-send` |
//! | Windows | `powershell`（WinForms `MessageBox::Show`） |
//! | macOS | `osascript`（`display dialog`） |
//!
//! ## 送达判定
//!
//! 工具**存在**不等于**送达**：无 `$DISPLAY`/Wayland 会话时 zenity 能
//! spawn 但立即非零退出。所以判定 = `spawn` 成功**且** `wait` 退出码 0；
//! 否则试下一个候选；全部失败返回 `false`（调用方回退纯 stderr——
//! 至少诊断面还在）。
//!
//! ## 单测口径
//!
//! `candidate_commands` 是纯函数（候选序/参数形/转义——不 spawn），
//! 沙箱可全断言；`show_error` 用受控 `PATH` 测试（空 PATH = 必 false；
//! 假 zenity 脚本 = true；假工具失败 = 换下一候选），**不依赖沙箱真装了
//! 哪个桌面工具**——跨环境（CI 无桌面）不飘。

use std::process::{Command, Stdio};

/// 对话框内容（纯数据——命令构造与单测共用）。
pub struct DialogSpec {
    /// 窗口/气泡标题。
    pub title: String,
    /// 正文（支持 `\n` 换行——各工具的换行语义见 [`candidate_commands`]
    /// 内注释）。
    pub message: String,
}

/// PowerShell 单引号字符串转义：`'` → `''`（PS 的单引号串里唯一转义）。
///（仅 Windows 分支消费——Linux/macOS 构建下不编译；`test` 保单测面。）
#[cfg(any(target_os = "windows", test))]
fn escape_powershell_single(s: &str) -> String {
    s.replace('\'', "''")
}

/// AppleScript 双引号字符串转义：`"` → `\"`（`\n` 字面量按换行保留——
/// AppleScript 字符串里 `\n` 即换行转义，无需再处理）。
///（仅 macOS 分支消费——其他平台构建下不编译；`test` 保单测面。）
#[cfg(any(target_os = "macos", test))]
fn escape_applescript(s: &str) -> String {
    s.replace('"', "\\\"")
}

/// 按平台给出候选命令序列（纯函数——单测锚点，不执行）。
///
/// 每项 = (程序, 参数数组)。`notify-send` 是**非模态气泡**（立即返回，
/// 无确认语义）——放末位兜底；`-u critical` 让它尽量置顶/驻留。
pub fn candidate_commands(spec: &DialogSpec) -> Vec<(String, Vec<String>)> {
    #[cfg(target_os = "windows")]
    {
        // WinForms MessageBox：buttons=OK（0 枚举）、icon=Error（16 枚举）。
        // `| Out-Null` 吞返回值；整个脚本作为**单个参数**传给 -Command
        //（Command::args 数组直传，不经 shell——无注入面）。
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             [System.Windows.Forms.MessageBox]::Show('{}', '{}', 0, 16) | Out-Null",
            escape_powershell_single(&spec.message),
            escape_powershell_single(&spec.title),
        );
        vec![(
            "powershell".to_string(),
            vec![
                "-NoProfile".to_string(),
                "-STA".to_string(),
                "-Command".to_string(),
                script,
            ],
        )]
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display dialog \"{}\" with title \"{}\" with icon stop \
             buttons {{\"OK\"}} default button \"OK\"",
            escape_applescript(&spec.message),
            escape_applescript(&spec.title),
        );
        vec![("osascript".to_string(), vec!["-e".to_string(), script])]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        vec![
            // ① zenity（GNOME 系；模态，OK 退出 0）。
            (
                "zenity".to_string(),
                vec![
                    "--error".to_string(),
                    format!("--title={}", spec.title),
                    format!("--text={}", spec.message),
                ],
            ),
            // ② kdialog（KDE 系；模态）。xmessage 的换行支持好（\n 直译），
            // zenity/kdialog 的 message 里 \n 同样按换行渲染。
            (
                "kdialog".to_string(),
                vec![
                    "--error".to_string(),
                    spec.message.clone(),
                    "--title".to_string(),
                    spec.title.clone(),
                ],
            ),
            // ③ xmessage（X11 裸奔兜底——无 GTK/Qt 依赖；-default OK 让
            // Enter 即关；点 X 关闭退出码非 0 → 顺延 ④ 再发一个气泡，
            // 双保险无害）。
            (
                "xmessage".to_string(),
                vec![
                    "-center".to_string(),
                    "-buttons".to_string(),
                    "OK:0".to_string(),
                    "-default".to_string(),
                    "OK".to_string(),
                    format!("{}\n\n{}", spec.title, spec.message),
                ],
            ),
            // ④ notify-send（freedesktop 气泡——非模态，立即退 0；
            // -u critical 尽量驻留）。
            (
                "notify-send".to_string(),
                vec![
                    "-u".to_string(),
                    "critical".to_string(),
                    "-a".to_string(),
                    "mang-shell".to_string(),
                    spec.title.clone(),
                    spec.message.clone(),
                ],
            ),
        ]
    }
}

/// 弹出错误对话框（阻塞至用户确认或所有候选失败）。
///
/// 送达判定见模块文档：`spawn` 成功且退出码 0 才算展示；全失败 `false`
/// （调用方回退纯 stderr）。stdio 三路全 null——对话框工具的任何输出
/// 不污染本进程 stdout/stderr（错误链已由调用方落 stderr）。
pub fn show_error(spec: &DialogSpec) -> bool {
    for (prog, args) in candidate_commands(spec) {
        let Ok(mut child) = Command::new(&prog)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue; // 工具不存在 → 下一候选
        };
        match child.wait() {
            Ok(status) if status.success() => return true, // 已送达并确认
            _ => continue,                                 // 工具在但失败（无显示会话等）→ 下一候选
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> DialogSpec {
        DialogSpec {
            title: "mang 无法启动".into(),
            message: "打开主窗口失败（GPU 初始化）".into(),
        }
    }

    /// Unix 候选序：zenity 首位（模态）→ notify-send 末位（气泡兜底）；
    /// 标题与正文必须原样出现在参数里（不经 shell——零转义损耗）。
    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn unix_candidate_order_and_payload() {
        let cands = candidate_commands(&spec());
        let progs: Vec<&str> = cands.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            progs,
            vec!["zenity", "kdialog", "xmessage", "notify-send"],
            "模态工具在前，气泡兜底在末"
        );
        let (zenity_prog, zenity_args) = &cands[0];
        assert_eq!(zenity_prog, "zenity");
        assert!(
            zenity_args.contains(&"--error".to_string()),
            "zenity 错误图标"
        );
        assert!(
            zenity_args.iter().any(|a| a == "--title=mang 无法启动"),
            "标题经参数直传（含中文/空格原样）"
        );
        assert!(
            zenity_args
                .iter()
                .any(|a| a == "--text=打开主窗口失败（GPU 初始化）"),
            "正文经参数直传"
        );
        // notify-send：critical 级别 + 应用标识。
        let (_, notify_args) = &cands[3];
        assert!(notify_args.contains(&"critical".to_string()));
        assert!(notify_args.contains(&"mang-shell".to_string()));
    }

    /// PowerShell 单引号转义：`'` 加倍（`it's` → `it''s`）。
    #[test]
    fn powershell_single_quote_escaping() {
        assert_eq!(escape_powershell_single("it's"), "it''s");
        assert_eq!(escape_powershell_single("plain"), "plain");
        assert_eq!(escape_powershell_single("''"), "''''");
    }

    /// AppleScript 双引号转义：`"` → `\"`。
    #[test]
    fn applescript_double_quote_escaping() {
        assert_eq!(escape_applescript("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(escape_applescript("plain"), "plain");
    }

    /// 受控 PATH 矩阵（unix，单测试串行——PATH 是进程级环境变量，
    /// 拆两测试会并行互踩）：
    /// ① 空 PATH → 必 false（跨环境稳定负例——不依赖沙箱/CI 装了什么）；
    /// ② 假 zenity exit 0 → 送达 true；
    /// ③ 假 zenity exit 1（无 DISPLAY 同型失败）且无后续候选 → false。
    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn show_error_with_controlled_path() {
        // 受控 PATH 基座：空目录（只含假工具时才注入）。
        let dir = std::env::temp_dir().join(format!("mang-dialog-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("zenity");

        // 全程恢复式改 PATH（单测试内串行——无并发竞态面）。
        let saved = std::env::var_os("PATH");
        let restore = || match &saved {
            Some(p) => unsafe { std::env::set_var("PATH", p) },
            None => unsafe { std::env::remove_var("PATH") },
        };

        // ① 空 PATH（目录无任何工具）：任何候选都找不到 → false。
        unsafe { std::env::set_var("PATH", &dir) };
        let r_empty = show_error(&spec());

        // ② 假 zenity exit 0 → 送达 true。
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        make_executable(&fake);
        unsafe { std::env::set_var("PATH", &dir) };
        let r_delivered = show_error(&spec());

        // ③ 假 zenity exit 1 且无后续候选（kdialog 等不在 PATH）→ false。
        std::fs::write(&fake, "#!/bin/sh\nexit 1\n").unwrap();
        unsafe { std::env::set_var("PATH", &dir) };
        let r_exhausted = show_error(&spec());

        // 恒恢复（断言失败也不残留 PATH 污染同进程其他测试）。
        restore();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(!r_empty, "空 PATH 下任何工具不可达 → 必 false");
        assert!(r_delivered, "假 zenity exit 0 → 判定送达");
        assert!(
            !r_exhausted,
            "假 zenity exit 1（无 DISPLAY 同型失败）→ 换候选全尽 → false"
        );
    }

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }
}
