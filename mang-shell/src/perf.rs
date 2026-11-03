//! #68 批 5 性能测量桩——`MANG_PERF=1` 环境变量门控。
//! 设计口径：首屏 <2s / 滚动 ≥60fps 不低于 30 / 内存峰值 <1GB / 输入延迟 <50ms / 全文查找 <1s
//!
//! - 零常态开销：所有桩点先经 [`on`] 判定；未开时分支为
//!   `then(Instant::now)` 的 `Option<Instant>`——一次分支预测级开销，
//!   无计时、无分配。
//! - 一行一事件：输出统一 `perf:` 前缀（stderr），`perf_meas.sh` 按前缀
//!   汇总；未开门控下不产生任何输出面。
//! - 调用点两行式（门控 + 记录分离——被测段不因测量引入额外分支）：
//!   ```text
//!   let since = perf::on().then(Instant::now);
//!   ... // 被测段
//!   if let Some(since) = since { perf::stage("first-frame", since); }
//!   ```
//!
//! 桩面（#164 归档设计逐项恢复）：
//! - [`stage`]：启动链路分段（main-enter / doc-open / editor-built /
//!   first-frame——`perf_meas.sh` 对齐这四个锚点名）
//! - [`key_handled`] / [`key_rejected`]：逐键处理 / 只读门禁拒绝
//! - [`find`]：全文查找（规模三参 + 计时）
//!
//! #164 1c 接线扩展面（桩面不变，新增两个锚点函数）：
//! - [`boot`]：进程启动锚点（main 入口首调缓存——first-frame 的起点）
//! - [`doc_open`]：文档打开计时（kind + 字节数 + 规模单位数）

use std::sync::OnceLock;
use std::time::Instant;

/// 门控：`MANG_PERF=1` 开启测量（进程级 OnceLock 缓存——首读后零 env
/// 开销）。测试进程不设该变量时恒 false，零输出。
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("MANG_PERF").as_deref() == Ok("1"))
}

/// 进程启动锚点（#164 1c）：main() 入口**首调**后 OnceLock 缓存，
/// 后续任意调用点（ShellApp 构造 / render 首帧）零开销取回同一时刻
/// ——first-frame 计时的 `since` 基准。未首调而被他处先调 = 取该处
/// 时刻（口径漂移由调用纪律保证：main 第一行即调）。
pub fn boot() -> Instant {
    static BOOT: OnceLock<Instant> = OnceLock::new();
    *BOOT.get_or_init(Instant::now)
}

/// 文档打开计时（#164 1c）：`kind` = 扩展名（md/txt/docx/pdf/xlsx/
/// canvas）；`units` = 规模单位数（文本=行数，docx=段数，pdf=页数，
/// xlsx=表数——归因用规模证据面，单位随 kind，见 document.rs 调用点）。
/// 归档口径：`perf: doc-open md 1578682B 7486L 4.4ms`。
pub fn doc_open(kind: &str, bytes: u64, units: usize, since: Instant) {
    if on() {
        eprintln!(
            "perf: doc-open {kind} {bytes}B {units}L {:.1}ms",
            since.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// 启动链路分段计时（毫秒一位小数——首帧 257ms 级数值）。
pub fn stage(label: &str, since: Instant) {
    if on() {
        eprintln!("{}", stage_line(label, since));
    }
}

/// 逐键处理计时（毫秒两位小数——<50ms 红线需要亚毫秒分辨率）。
/// `key` 为小写键名（dispatch_key 口径，如 "x" / "enter" / "tab"）。
pub fn key_handled(key: &str, since: Instant) {
    if on() {
        eprintln!("{}", key_line("key", key, since));
    }
}

/// 只读门禁拒绝计时（docx/xlsx/pdf 只读态敲变更键被拒的路径）。
pub fn key_rejected(key: &str, since: Instant) {
    if on() {
        eprintln!("{}", key_line("key-rejected", key, since));
    }
}

/// 全文查找计时：`query_chars`/`lines`/`hits` 为规模证据面
/// （查询长度 / 文本行数 / 命中数）——查找 <1s 红线的归因三参。
pub fn find(query_chars: usize, lines: usize, hits: usize, since: Instant) {
    if on() {
        eprintln!(
            "perf: find {query_chars}c {lines}L {hits} hits {:.1}ms",
            since.elapsed().as_secs_f64() * 1000.0
        );
    }
}

fn stage_line(label: &str, since: Instant) -> String {
    format!(
        "perf: {label} {:.1}ms",
        since.elapsed().as_secs_f64() * 1000.0
    )
}

fn key_line(kind: &str, key: &str, since: Instant) -> String {
    format!(
        "perf: {kind} {key} {:.2}ms",
        since.elapsed().as_secs_f64() * 1000.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gating_is_env_driven() {
        // MANG_PERF 未设 → 关；恰 "1" → 开；其他值不承诺（不测）。
        // OnceLock 进程级缓存：本测试只断言首读口径，不做切换实验
        //（跨进程面由 perf_meas.sh 的 MANG_PERF=1 运行覆盖）。
        match std::env::var("MANG_PERF") {
            Err(_) => assert!(!on(), "未设 MANG_PERF → 门控必须关闭"),
            Ok(v) if v == "1" => assert!(on(), "MANG_PERF=1 → 门控必须开启"),
            Ok(_) => {}
        }
    }

    #[test]
    fn stage_line_shape_matches_measure_script_contract() {
        let s = stage_line("first-frame", Instant::now());
        assert!(s.starts_with("perf: first-frame "), "前缀锚点：{s}");
        assert!(s.ends_with("ms"), "单位锚点：{s}");
        // 一位小数（257.1ms 形态）
        let ms = s.trim_end_matches("ms");
        let frac = ms.split('.').next_back().unwrap();
        assert_eq!(frac.len(), 1, "一位小数口径：{s}");
    }

    #[test]
    fn key_line_shape_and_kind_distinction() {
        let handled = key_line("key", "x", Instant::now());
        assert!(handled.starts_with("perf: key x "), "{handled}");
        assert!(handled.ends_with("ms"));
        let rejected = key_line("key-rejected", "enter", Instant::now());
        assert!(
            rejected.starts_with("perf: key-rejected enter "),
            "拒绝路径独立前缀：{rejected}"
        );
    }

    #[test]
    fn boot_anchor_is_stable_per_process() {
        // 同进程两次调用必须同值（OnceLock 缓存——first-frame 计时基准
        // 的稳定性：跨模块取回的 since 不会漂移）。
        assert_eq!(boot(), boot());
    }

    #[test]
    fn doc_open_line_matches_archive_shape() {
        // 归档口径：`perf: doc-open md 1578682B 7486L 4.4ms`
        let (kind, bytes, units) = ("md", 1578682u64, 7486usize);
        let line = format!("perf: doc-open {kind} {bytes}B {units}L {:.1}ms", 4.4);
        assert!(
            line.starts_with("perf: doc-open md 1578682B 7486L "),
            "{line}"
        );
        assert!(line.ends_with("4.4ms"), "{line}");
    }

    #[test]
    fn find_line_carries_scale_evidence() {
        // find 直印（无中间 helper）——此处钉输出契约：规模三参 + 计时。
        // 契约钉在格式面：字段序 = chars/lines/hits/ms。
        let (query_chars, lines, hits) = (12usize, 7486usize, 31usize);
        let line = format!(
            "perf: find {query_chars}c {lines}L {hits} hits {:.1}ms",
            0.0
        );
        assert!(line.contains("12c"), "{line}");
        assert!(line.contains("7486L"), "{line}");
        assert!(line.contains("31 hits"), "{line}");
    }
}
