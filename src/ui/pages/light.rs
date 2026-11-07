use crate::adb::AdbDeviceState;
use crate::device::light::{ScreenLightMode, ScreenLightState};
use crate::ui::components::{action_button, badge, card_container};
use crate::ui::theme::ThemeColors;
use gpui::prelude::*;
use gpui::{div, px, rgb, Context, Div};

/// 渲染屏幕灯调优控制页面 (替换静态占位)
pub fn render_light_page<V: 'static>(
    light_state: &ScreenLightState,
    adb_state: &AdbDeviceState,
    cx: &mut Context<V>,
    on_refresh: impl Fn(&mut V, &mut Context<V>) + 'static + Copy,
    on_set_mode: impl Fn(&mut V, ScreenLightMode, &mut Context<V>) + 'static + Copy,
    on_set_illumination: impl Fn(&mut V, u32, &mut Context<V>) + 'static + Copy,
    on_set_color_temp: impl Fn(&mut V, u32, &mut Context<V>) + 'static + Copy,
    on_set_color_value: impl Fn(&mut V, u32, &mut Context<V>) + 'static + Copy,
) -> Div {
    let is_connected = adb_state.is_connected();
    let is_fully_known = light_state.is_fully_known();
    let is_busy = light_state.is_reading || light_state.is_writing;
    let is_reading = light_state.is_reading;
    // 必须后台读取四字段真实值，未知/错误显示出来并禁用不安全写入
    let can_write = is_connected && is_fully_known && !is_busy;

    div()
        .flex()
        .flex_col()
        .gap_4()
        .w_full()
        // 顶部标题与刷新操作
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("屏幕灯"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("五种工作模式、亮度挡位(UI 1..15)、照明色温与纯色色彩底层调节"),
                        ),
                )
                .child(
                    div()
                        .id("btn_refresh_light")
                        .child(action_button(
                            if is_reading { "读取中..." } else { "刷新数据" },
                            false,
                            !is_connected || is_reading,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if is_connected && !is_reading {
                                on_refresh(this, cx);
                            }
                        })),
                ),
        )
        // 状态及回执卡片
        .child(
            card_container()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .items_center()
                                .child(if !is_connected {
                                    badge("ADB 未连接", ThemeColors::STATUS_WARNING_BG(), ThemeColors::STATUS_WARNING())
                                } else if is_busy {
                                    badge(if light_state.is_writing { "写入事务中" } else { "读取参数中" }, ThemeColors::ACCENT_BG_SUBTLE(), ThemeColors::ACCENT_CYAN())
                                } else if is_fully_known {
                                    badge("设置已读取", ThemeColors::STATUS_SUCCESS_BG(), ThemeColors::STATUS_SUCCESS())
                                } else {
                                    badge("存在未知字段", ThemeColors::STATUS_WARNING_BG(), ThemeColors::STATUS_WARNING())
                                })
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                        .child(if !is_connected {
                                            "设备离线，屏幕灯所有硬件写入与参数读取已禁用。".to_string()
                                        } else if let Some(ref st) = light_state.last_status {
                                            st.clone()
                                        } else {
                                            "屏幕灯设置已读取".to_string()
                                        }),
                                ),
                        ),
                )
                .children(light_state.last_error.as_ref().map(|err| {
                    div()
                        .text_xs()
                        .text_color(rgb(ThemeColors::STATUS_ERROR()))
                        .child(format!("异常提醒: {}", err))
                }))
                .children(if is_connected && !is_fully_known {
                    Some(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::STATUS_WARNING()))
                            .child("安全保护: 四项参数尚未全部从底层读取确认，为防覆盖已有硬件设置，已临时禁用参数写入。"),
                    )
                } else {
                    None
                }),
        )
        // 真实参数读数概览卡片
        .child(
            card_container()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                        .child("当前屏幕灯设置"),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_4()
                        .children([
                            ("当前模式", light_state.mode.map(|m| m.name()).unwrap_or("未知")),
                            (
                                "亮度挡位",
                                &light_state.illumination.map(|v| format!("{} 挡 (底层: {})", v + 1, v)).unwrap_or_else(|| "未知".into()),
                            ),
                            (
                                "照明色温",
                                match light_state.color_temp {
                                    Some(0) => "2700K",
                                    Some(1) => "4000K",
                                    Some(2) => "6500K",
                                    _ => "未知",
                                },
                            ),
                            (
                                "纯色颜色",
                                match light_state.color_value {
                                    Some(0) => "冰蓝",
                                    Some(1) => "流金",
                                    Some(2) => "天青",
                                    Some(3) => "草地",
                                    Some(4) => "日落",
                                    _ => "未知",
                                },
                            ),
                        ].into_iter().enumerate().map(|(idx, (label, val))| {
                            div()
                                .id(("param_view", idx))
                                .flex()
                                .flex_col()
                                .flex_1()
                                .p_2()
                                .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                                .border_1()
                                .border_color(rgb(ThemeColors::BORDER()))
                                .rounded_md()
                                .gap_1()
                                .child(div().text_xs().text_color(rgb(ThemeColors::TEXT_MUTED())).child(label))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(if val == "未知" { ThemeColors::STATUS_WARNING() } else { ThemeColors::ACCENT_CYAN() }))
                                        .child(val.to_string()),
                                )
                        })),
                ),
        )
        // 炫彩灯模式 (5模式：4关、0照明、2纯色、1屏幕同色、3七彩循环)
        .child(
            card_container()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                        .child("炫彩灯模式"),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .children([
                            (ScreenLightMode::Off, "关闭 (4)"),
                            (ScreenLightMode::Lighting, "照明 (0)"),
                            (ScreenLightMode::Solid, "纯色 (2)"),
                            (ScreenLightMode::Ambient, "屏幕同色 (1)"),
                            (ScreenLightMode::Cycle, "七彩循环 (3)"),
                        ].into_iter().enumerate().map(|(idx, (mode, label))| {
                            let is_active = light_state.mode == Some(mode);
                            let disabled = !can_write;

                            div()
                                .id(("btn_mode", idx))
                                .child(action_button(label, is_active, disabled))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if can_write {
                                        on_set_mode(this, mode, cx);
                                    }
                                }))
                        })),
                ),
        )
        // 亮度挡位 (UI 1..15 = 硬件 0..14)
        .child(
            card_container()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("亮度挡位 (UI 1..15 = 底层 0..14)"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                .child(light_state.illumination.map(|v| format!("当前: 第 {} 挡", v + 1)).unwrap_or_else(|| "当前: 未知".into())),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        // 递减按键 [-]
                        .child({
                            let cur_ill = light_state.illumination.unwrap_or(0);
                            let can_dec = can_write && cur_ill > 0;
                            div()
                                .id("btn_ill_dec")
                                .child(action_button("－ 减小", false, !can_dec))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if can_dec {
                                        on_set_illumination(this, cur_ill - 1, cx);
                                    }
                                }))
                        })
                        // 当前数值大字呈现
                        .child(
                            div()
                                .w(px(70.0))
                                .py_1()
                                .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                                .border_1()
                                .border_color(rgb(ThemeColors::BORDER()))
                                .rounded_md()
                                .flex()
                                .justify_center()
                                .items_center()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                        .child(light_state.illumination.map(|v| (v + 1).to_string()).unwrap_or_else(|| "--".into())),
                                ),
                        )
                        // 递增按键 [+]
                        .child({
                            let cur_ill = light_state.illumination.unwrap_or(14);
                            let can_inc = can_write && cur_ill < 14;
                            div()
                                .id("btn_ill_inc")
                                .child(action_button("＋ 增大", false, !can_inc))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if can_inc {
                                        on_set_illumination(this, cur_ill + 1, cx);
                                    }
                                }))
                        })
                        // 快捷挡位
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .children([
                                    (0, "1 挡"),
                                    (4, "5 挡"),
                                    (7, "8 挡"),
                                    (9, "10 挡"),
                                    (14, "15 挡"),
                                ].into_iter().enumerate().map(|(idx, (hw_val, label))| {
                                    let is_active = light_state.illumination == Some(hw_val);
                                    let disabled = !can_write;

                                    div()
                                        .id(("btn_ill_preset", idx))
                                        .child(action_button(label, is_active, disabled))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if can_write {
                                                on_set_illumination(this, hw_val, cx);
                                            }
                                        }))
                                })),
                        ),
                ),
        )
        // 照明色温 (0=2700K, 1=4000K, 2=6500K)
        .child(
            card_container()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("照明色温 (选择将切至照明模式)"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("适用照明模式"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .children([
                            (0, "2700K (暖光)"),
                            (1, "4000K (自然)"),
                            (2, "6500K (冷白)"),
                        ].into_iter().enumerate().map(|(idx, (code, label))| {
                            let is_active = light_state.color_temp == Some(code) && light_state.mode == Some(ScreenLightMode::Lighting);
                            let disabled = !can_write;

                            div()
                                .id(("btn_temp", idx))
                                .child(action_button(label, is_active, disabled))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if can_write {
                                        on_set_color_temp(this, code, cx);
                                    }
                                }))
                        })),
                ),
        )
        // 纯色颜色 (0冰蓝 1流金 2天青 3草地 4日落)
        .child(
            card_container()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("纯色颜色 (选择将切至纯色模式)"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("适用纯色静态模式"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .children([
                            (0, "冰蓝 (0)"),
                            (1, "流金 (1)"),
                            (2, "天青 (2)"),
                            (3, "草地 (3)"),
                            (4, "日落 (4)"),
                        ].into_iter().enumerate().map(|(idx, (code, label))| {
                            let is_active = light_state.color_value == Some(code) && light_state.mode == Some(ScreenLightMode::Solid);
                            let disabled = !can_write;

                            div()
                                .id(("btn_color", idx))
                                .child(action_button(label, is_active, disabled))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if can_write {
                                        on_set_color_value(this, code, cx);
                                    }
                                }))
                        })),
                ),
        )
}
