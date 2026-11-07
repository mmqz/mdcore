//! #333：Wayland seat 前置探测——无 seat 合成器的优雅降级。
//!
//! # 背景（为什么要有这个模块）
//!
//! gpui_linux 的启动链是 [`run()` → `connect_initially()` → 用户闭包]，
//! 其中 `connect_initially()`（third_party/zed/crates/gpui_linux/src/
//! linux/platform.rs:381）对 `connect()` 的错误做的是
//! `.unwrap_or_else(|error| panic!("{error:#}"))`——**裸 panic，exit 101**。
//!
//! 无 `wl_seat` 的合成器（weston headless/kiosk 等 kiosk·嵌入场景——
//! #270 验收实测 weston 14 headless 即如此，globals 有 xdg_wm_base v5
//! 却无 wl_seat）会在这里 panic，而 main.rs 闭包里已有的 GPU 失败
//! 降级路径（stderr 双语 + `native_dialog` 兜底 + 退出码 70，#185）
//! **根本没机会执行**——它在 panic 点之后。
//!
//! # 修法选择（方案 B，第一方预检）
//!
//! 不动 third_party/zed 搬运闭包（PROVENANCE「物理复制，源码零修改」
//! 口径零风险）：在 `application()` 之前用**同库同原语**（wayland-client
//! ——`gpui_platform` 的 `wayland` 特性既有传递依赖，Cargo.lock 0.31.15；
//! 此处显式声明 = 零新增 crate，循 serde_json/regex/async-channel 的
//! 「闭包内既有」先例）连接 + globals 枚举查 `wl_seat`，缺失则走 #185
//! 同款降级。上游闭包方案（把 panic 改结构化错误）作为备选登记在
//! issue #333，由 Lead 决定是否走闭包豁免流程。
//!
//! # 探测边界（诚实面）
//!
//! | 情形 | 行为 | 理由 |
//! |---|---|---|
//! | `WAYLAND_DISPLAY` 未设 | 放行 | gpui 走 X11/其他路径，与本探测无关 |
//! | 连接失败（socket 死/权限） | 放行 | 不抢跑——GPUI 自身错误面处理连接失败（避免双轨分叉）；本模块只管「已证实会 panic 的 seat 缺失」一个场景 |
//! | roundtrip 失败 | 放行 | 同上 |
//! | 连接成功且 globals 齐，**无 wl_seat** | exit 70 + 双语提示 | 对齐 GPU 失败路径的降级合同 |
//! | 有 wl_seat | 放行 | 正常启动（cage/wlroots 实测路径） |
//!
//! 「探测失败就放行」不是漏洞：探测失败 ⇒ 真连接多半也会失败 ⇒
//! panic 的前提（连接成功但 seat 缺失）不成立；连接失败场景 GPUI 的
//! 报错链已经可读（weston socket 不存在时的实测输出），不在本模块职责内。
//!
//! # 退出码合同
//!
//! `70`（EX_SOFTWARE）——与 GPU/图形栈初始化失败同一语义族：
//! 「环境能力缺失，应用无法启动」。部署脚本/CI 冒烟可判。

/// 纯判定面：globals 接口名集合里有没有 `wl_seat`。
///
/// 从 globals 枚举回调中解耦出来做单测——枚举本身需要真连接
/// （见 [`ensure_seat_or_exit`]），判定逻辑不需要。
#[cfg(target_os = "linux")]
pub fn seat_in_globals(interfaces: &[String]) -> bool {
    interfaces.iter().any(|i| i == "wl_seat")
}

/// 启动前置探测：`application()` 之前调用。
///
/// 无 seat 的 Wayland 会话 → stderr 双语错误链 + 退出码 70；
/// 其余情形（非 Wayland / 探测不结论 / seat 在位）静默放行。
#[cfg(target_os = "linux")]
pub fn ensure_seat_or_exit() {
    use wayland_client::{Connection, Dispatch, QueueHandle, protocol::wl_registry};

    // 门 1：非 Wayland 会话（WAYLAND_DISPLAY 未设）——gpui 的
    // GraphicalEnvironment::detect() 会走 X11/其他路径，不归本模块管。
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY") else {
        return;
    };
    // 空串等价于未设（connect_to_env 会失败→探测不结论→放行）。
    if display.is_empty() {
        return;
    }

    // 连接（与 gpui wayland 后端同一原语：connect_to_env）。
    // 失败 → 放行（见模块文档「探测边界」：交给 GPUI 自身错误面）。
    let Ok(conn) = Connection::connect_to_env() else {
        return;
    };

    // globals 枚举：registry + 一次 roundtrip 收齐 Global 事件
    //（与 gpui_linux/wayland/client.rs 的枚举序同构——同原语不双轨）。
    #[derive(Default)]
    struct GlobalsCollector {
        interfaces: Vec<String>,
    }
    impl Dispatch<wl_registry::WlRegistry, ()> for GlobalsCollector {
        fn event(
            state: &mut Self,
            _: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global { interface, .. } = event {
                state.interfaces.push(interface);
            }
        }
    }
    let mut collector = GlobalsCollector::default();
    let mut queue = conn.new_event_queue();
    let _registry = conn.display().get_registry(&queue.handle(), ());
    // roundtrip 失败 → 放行（同「探测边界」）。
    if queue.roundtrip(&mut collector).is_err() {
        return;
    }

    // 判定：连接成功、globals 齐、seat 缺失 → #185 同款降级。
    if seat_in_globals(&collector.interfaces) {
        return;
    }

    eprintln!("mang-shell: Wayland 会话缺少输入 seat / Wayland session has no wl_seat:");
    eprintln!(
        "  当前合成器（WAYLAND_DISPLAY={:?}）在 globals 中未提供 wl_seat——\
         无键盘/指针通道，编辑器无法工作。",
        display
    );
    eprintln!(
        "提示：该合成器为无输入配置（常见于 headless/kiosk 后端）。\
         · 如需使用，请换带输入的合成器（普通桌面会话均自带）；\
         · 如本机有 X11 会话，可尝试 unset WAYLAND_DISPLAY 后重启；\
         · 纯脚本冒烟场景请改用 Xvfb + X11 路径。"
    );
    std::process::exit(70);
}

/// 非 Linux 平台的无操作占位（Windows/macOS 构建面无 wayland 依赖）。
#[cfg(not(target_os = "linux"))]
pub fn ensure_seat_or_exit() {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::seat_in_globals;

    fn ifaces(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn seat_present() {
        // cage/wlroots 无头实测的 globals 形态（节选）
        assert!(seat_in_globals(&ifaces(&[
            "wl_compositor",
            "wl_seat",
            "xdg_wm_base",
            "wl_shm"
        ])));
    }

    #[test]
    fn seat_missing() {
        // weston headless+kiosk 实测形态：有 xdg_wm_base 无 wl_seat（#333 原始现场）
        assert!(!seat_in_globals(&ifaces(&[
            "wl_compositor",
            "xdg_wm_base",
            "wl_shm",
            "wl_output"
        ])));
    }

    #[test]
    fn empty_globals_is_missing() {
        // 连接成功但零 global 的极端合成器——同样无 seat
        assert!(!seat_in_globals(&[]));
    }

    #[test]
    fn exact_name_match_only() {
        // 不允许前缀/子串误判（如假设的 "wl_seat_manager_x"）
        assert!(!seat_in_globals(&ifaces(&["wl_seat_extension"])));
    }
}
