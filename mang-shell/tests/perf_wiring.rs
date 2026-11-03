//! #164 1c：perf 接线完整性——测量通道的存在性锚点。
//!
//! ## 钉什么
//!
//! `MANG_PERF=1` 测量通道的**接线面**（桩本体契约在 `src/perf.rs` 单测）：
//! 五个采集点任一被重构 silently 摘除，性能验证就变成「开了门控也没数据」
//! ——本文件把每个采集点的调用证据钉成红灯。
//!
//! 与 `module_wiring.rs` 的分工：那边钉 UI 模块挂载/派发安全，这边钉
//! 测量桩的消费面（perf.rs 是 lib 树 pub API——零调用时编译器不报
//! dead_code，锚点测试是唯一的失线警报）。
//!
//! ## 红绿账
//!
//! | 锚点 | 状态 | 依据 |
//! |---|---|---|
//! | `boot_anchor_wired_at_main_entry` | 🟢 | main.rs 首行 `perf::boot()`（first-frame 的 since 基准——非首行调用 = 口径漂移） |
//! | `startup_stages_wired` | 🟢 | document.rs doc-open/editor-built + shell.rs first-frame |
//! | `per_key_timing_wired` | 🟢 | editor/mod.rs dispatch_key 包装层 key_handled/key_rejected |
//! | `find_timing_wired` | 🟢 | buffer.rs find_matches_opts 包装层 perf::find |
//! | `measurement_hooks_wired` | 🟢 | main.rs --smoke-perf-keys / --smoke-perf-scroll + VmHWM |
//!
//! ## 铁律
//!
//! - 只读源码断言，不改产线代码（本文件随 #164 1c 接线一并入仓）；
//! - 失败信息必须指明「哪个采集点缺什么」。

use std::fs;
use std::path::{Path, PathBuf};

/// 读 `mang-shell/src/<rel>` 源码。缺失即 FAIL（源码进仓，不是环境件）。
fn src(rel: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("源码文件缺失/不可读（{}）：{e}", path.display()))
}

/// #164 1c：boot 锚点必须在 main() 入口（first-frame 口径的基准——
/// 晚于 main 第一行初始化 = 计时起点漂移，first-frame 数值失真）。
#[test]
fn boot_anchor_wired_at_main_entry() {
    let main_rs = src("main.rs");
    assert!(
        main_rs.contains("mod perf;"),
        "perf 模块未挂载 bin 树：main.rs 无 mod perf 声明——crate::perf 在\
         bin 编译面不可达（#164 1c）"
    );
    // boot 首调必须紧跟 main 入口（在 args 解析之前——任何前置工作都
    // 会漏出计时窗口）。
    let main_pos = main_rs.find("fn main() {").expect("main() 入口缺失");
    let boot_pos = main_rs
        .find("perf::boot()")
        .expect("perf::boot() 首调缺失——first-frame 计时无基准（#164 1c）");
    assert!(
        boot_pos > main_pos && boot_pos - main_pos < 400,
        "perf::boot() 首调距 main() 入口 {} 字节——应在前几行（args 解析\
         之前），否则 first-frame 口径漂移（#164 1c）",
        boot_pos - main_pos
    );
}

/// #164 1c：启动链三段（doc-open / editor-built / first-frame）接线。
#[test]
fn startup_stages_wired() {
    let doc = src("document.rs");
    assert!(
        doc.contains("perf::doc_open("),
        "doc-open 采集点缺失：document.rs 不调 perf::doc_open——打开链路\
         无规模证据行（#164 1c）"
    );
    assert!(
        doc.contains("perf::stage(\"editor-built\""),
        "editor-built 采集点缺失：document.rs 六形态构造不走 build_ed 计时\
         （#164 1c）"
    );
    let shell = src("shell.rs");
    assert!(
        shell.contains("perf_first_frame") && shell.contains("stage(\"first-frame\""),
        "first-frame 采集点缺失：shell.rs 无 perf_first_frame 消费——首帧\
         红线 <2s 无证据行（#164 1c）"
    );
}

/// #164 1c：逐键计时接线（dispatch_key 包装层——handled/rejected 双路径）。
#[test]
fn per_key_timing_wired() {
    let editor = src("editor/mod.rs");
    assert!(
        editor.contains("perf::key_handled("),
        "逐键采集点缺失：editor 不调 perf::key_handled——输入延迟 <50ms \
         红线无证据行（#164 1c）"
    );
    assert!(
        editor.contains("perf::key_rejected("),
        "拒键采集点缺失：只读门禁拒绝路径无 key-rejected 证据行（#164 1c）"
    );
}

/// #164 1c：全文查找计时接线（find_matches_opts 包装层）。
#[test]
fn find_timing_wired() {
    let buffer = src("editor/buffer.rs");
    assert!(
        buffer.contains("perf::find("),
        "查找采集点缺失：buffer.rs find_matches_opts 不调 perf::find——\
         查找 <1s 红线无证据行（#164 1c）"
    );
}

/// #164 1c：测量钩子与内存峰值（main.rs 的 --smoke-perf-* + VmHWM）。
#[test]
fn measurement_hooks_wired() {
    let main_rs = src("main.rs");
    assert!(
        main_rs.contains("--smoke-perf-keys")
            && main_rs.contains("dispatch_key(&handle, cx, delay, \"x\""),
        "逐键测量钩子缺失：--smoke-perf-keys 不合成真实键事件（#164 1c）"
    );
    assert!(
        main_rs.contains("--smoke-perf-scroll")
            && main_rs.contains("dispatch_scroll(&handle, cx, delay, 0.0, dy)"),
        "滚动测量钩子缺失：--smoke-perf-scroll 不合成滚轮事件（#164 1c）"
    );
    assert!(
        main_rs.contains("VmHWM"),
        "内存峰值采集点缺失：退出期不读 /proc/self/status VmHWM（#164 1c）"
    );
}
