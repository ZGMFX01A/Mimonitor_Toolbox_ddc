pub mod slider;
pub mod text_input;

pub use slider::ContinuousSlider;
pub use text_input::{bind_text_input_keys, TextInput};

use crate::ui::theme::ThemeColors;
use gpui::prelude::*;
use gpui::{div, px, rgb, Div, SharedString};

/// 构建带 Fluent 风格圆角和边框的卡片容器
pub fn card_container() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .p_4()
        .bg(rgb(ThemeColors::BG_CARD()))
        .border_1()
        .border_color(rgb(ThemeColors::BORDER()))
        .rounded_lg()
        .gap_3()
}

/// 构建通用静态按钮
pub fn button(label: impl Into<SharedString>) -> Div {
    div()
        .px_3()
        .py_1()
        .bg(rgb(ThemeColors::BG_BUTTON()))
        .border_1()
        .border_color(rgb(ThemeColors::BORDER()))
        .rounded_md()
        .text_xs()
        .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
        .child(label.into())
}

/// 构建可交互动作按钮
pub fn action_button(label: impl Into<SharedString>, is_primary: bool, is_disabled: bool) -> Div {
    let base = div().px_3().py_1().rounded_md().text_xs().border_1();

    if is_disabled {
        base.bg(rgb(ThemeColors::BG_DISABLED()))
            .border_color(rgb(ThemeColors::BORDER_DISABLED()))
            .text_color(rgb(ThemeColors::TEXT_DISABLED()))
    } else if is_primary {
        base.bg(rgb(ThemeColors::ACCENT_CYAN()))
            .border_color(rgb(ThemeColors::ACCENT_CYAN()))
            .text_color(rgb(ThemeColors::TEXT_ON_ACCENT()))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(ThemeColors::ACCENT_HOVER())))
    } else {
        base.bg(rgb(ThemeColors::BG_BUTTON()))
            .border_color(rgb(ThemeColors::BORDER()))
            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(ThemeColors::BG_BUTTON_HOVER())))
    }
    .child(label.into())
}

/// 构建通用状态胶囊徽章
pub fn badge(label: impl Into<SharedString>, bg_color: u32, text_color: u32) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .text_xs()
        .bg(rgb(bg_color))
        .text_color(rgb(text_color))
        .child(label.into())
}

pub fn option_button(label: impl Into<SharedString>, selected: bool, disabled: bool) -> Div {
    action_button(label, selected, disabled)
}

/// 进度条与滑块视觉展示
pub fn progress_bar(val: u32, max: u32) -> Div {
    let pct = if max == 0 {
        0.0
    } else {
        (val as f32 / max as f32).clamp(0.0, 1.0)
    };
    div()
        .w_full()
        .h(px(6.0))
        .bg(rgb(ThemeColors::BG_TRACK()))
        .rounded_full()
        .child(
            div()
                .h_full()
                .w(gpui::DefiniteLength::Fraction(pct))
                .bg(rgb(ThemeColors::ACCENT_CYAN()))
                .rounded_full(),
        )
}
