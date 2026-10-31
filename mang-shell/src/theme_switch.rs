//! 主题切换（#68 阶段 F6）——浅色 / 深色 / 跟随系统。
//!
//! 现状约束：闭包 JustBase 基础主题只有 One Dark（zed 闭包未带浅色
//! 主题，`zed_default_dark` 亦为 pub(crate) 不可外用）。本模块以运行时
//! 基础主题（启动时的全局主题 = One Dark）为蓝本派生浅色主题——
//! **只精调 mang-shell 实际消费的语义色字段**（全仓 rg 统计 17 个：
//! background/panel/title_bar/surface/element 系/border/text 系/
//! editor 行号与活动行系），编辑器语法高亮（token_style）同样映射自
//! 这些语义字段——整界面一致变浅，无残留深色面。浅色视觉本身由
//! Xvfb 烟雾 + Lead 真机背书（ThemeColors 无 Default/Serialize，
//! 纯逻辑单测无法构造 143 字段基线）。
//!
//! 切换经 `GlobalTheme::update_theme` + `refresh_windows`（全窗重绘）；
//! 模式持久化随 settings.json（`theme` 键）。

use std::sync::Arc;

use gpui::{App, AppContext, Hsla, hsla};
use theme::{Appearance, Theme};

/// 主题模式（settings.json `theme` 键的取值）。
pub const MODE_SYSTEM: &str = "system";
pub const MODE_LIGHT: &str = "light";
pub const MODE_DARK: &str = "dark";

/// 派生浅色主题（One Light 风格手调色板——覆盖 mang-shell 全部消费面）。
pub fn derive_light(base: &Theme) -> Theme {
    let mut theme = base.clone();
    theme.id = "mang-light".to_string();
    theme.name = "Mang Light".into();
    theme.appearance = Appearance::Light;
    let c = &mut theme.styles.colors;
    // 底面系：近白灰阶（panel/title_bar 比 background 微深一层）。
    c.background = light_gray(98.);
    c.panel_background = light_gray(95.);
    c.title_bar_background = light_gray(94.);
    c.surface_background = light_gray(96.);
    c.elevated_surface_background = light_gray(100.);
    c.element_background = light_gray(92.);
    c.element_hover = light_gray(89.);
    c.element_active = light_gray(85.);
    // 选中系：浅蓝（element_selected 实底 + 半透明选区底）。
    let blue = hsla(210. / 360., 60. / 100., 40. / 100., 1.);
    c.element_selected = hsla(210. / 360., 60. / 100., 88. / 100., 1.);
    c.element_selection_background = hsla(210. / 360., 60. / 100., 80. / 100., 0.35);
    c.border_selected = blue;
    c.border_focused = blue;
    // 边框：浅灰。
    c.border = light_gray(85.);
    c.border_variant = light_gray(90.);
    // 文本系：近黑 / 中灰 / 深蓝强调。
    c.text = hsla(210. / 360., 15. / 100., 15. / 100., 1.);
    c.text_muted = hsla(210. / 360., 10. / 100., 45. / 100., 1.);
    c.text_disabled = hsla(210. / 360., 8. / 100., 65. / 100., 1.);
    c.text_placeholder = hsla(210. / 360., 8. / 100., 65. / 100., 1.);
    c.text_accent = blue;
    // 编辑器行号/活动行（浅底上的灰号 + 极浅活动行底）。
    c.editor_line_number = hsla(210. / 360., 8. / 100., 65. / 100., 1.);
    c.editor_active_line_number = c.text;
    c.editor_active_line_background = light_gray(96.);
    c.editor_document_highlight_read_background = light_gray(93.);
    theme
}

/// 灰阶快捷构造（低饱和冷灰——与 One Light 底面同族）。
fn light_gray(lightness: f32) -> Hsla {
    hsla(210. / 360., 20. / 100., lightness / 100., 1.)
}

/// 解析主题模式（未知值 → system）。
pub fn parse_mode(value: Option<&str>) -> &'static str {
    match value {
        Some(MODE_LIGHT) => MODE_LIGHT,
        Some(MODE_DARK) => MODE_DARK,
        _ => MODE_SYSTEM,
    }
}

/// 模式的下一档（循环：system → light → dark → system）。
pub fn next_mode(mode: &str) -> &'static str {
    match mode {
        MODE_SYSTEM => MODE_LIGHT,
        MODE_LIGHT => MODE_DARK,
        _ => MODE_SYSTEM,
    }
}

/// 模式中文标签（通知面）。
pub fn mode_label(mode: &str) -> &'static str {
    match mode {
        MODE_LIGHT => "浅色",
        MODE_DARK => "深色",
        _ => "跟随系统",
    }
}

/// 应用主题模式（system = 读 [`theme::SystemAppearance`]——Linux X11 无
/// 系统级深浅检测时恒为深色[边界登记]；light = 派生浅色；dark = 基础
/// 主题原样）。全窗重绘由 `refresh_windows` 收口。
pub fn apply_mode(mode: &str, base_theme: &Arc<Theme>, cx: &mut App) {
    let theme: Arc<Theme> = match mode {
        MODE_LIGHT => Arc::new(derive_light(base_theme)),
        MODE_DARK => base_theme.clone(),
        _ => {
            // 跟随系统：SystemAppearance 由 theme::init 初始化（平台窗口
            // 外观快照——macOS 真检测，Linux 默认 Dark）。
            match *theme::SystemAppearance::global(cx) {
                Appearance::Light => Arc::new(derive_light(base_theme)),
                Appearance::Dark => base_theme.clone(),
            }
        }
    };
    theme::GlobalTheme::update_theme(cx, theme);
    // update_theme 只写全局（gpui Global）——refresh_windows 触发全窗
    // 重绘，否则要等下一次任意 notify。
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parsing_and_cycling() {
        assert_eq!(parse_mode(Some("light")), MODE_LIGHT);
        assert_eq!(parse_mode(Some("dark")), MODE_DARK);
        assert_eq!(parse_mode(Some("system")), MODE_SYSTEM);
        assert_eq!(parse_mode(None), MODE_SYSTEM, "缺省跟随系统");
        assert_eq!(parse_mode(Some("乱写")), MODE_SYSTEM, "未知值回退");

        assert_eq!(next_mode(MODE_SYSTEM), MODE_LIGHT);
        assert_eq!(next_mode(MODE_LIGHT), MODE_DARK);
        assert_eq!(next_mode(MODE_DARK), MODE_SYSTEM, "循环回 system");
    }

    #[test]
    fn mode_labels_cover_all() {
        for mode in [MODE_SYSTEM, MODE_LIGHT, MODE_DARK] {
            assert!(!mode_label(mode).is_empty());
        }
    }
}
