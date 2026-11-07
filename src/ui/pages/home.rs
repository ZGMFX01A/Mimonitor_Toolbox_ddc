use crate::adb::AdbDeviceState;
use crate::ui::components::{action_button, badge, card_container, TextInput};
use crate::ui::theme::ThemeColors;
use gpui::prelude::*;
use gpui::{div, rgb, Context, Div, Entity};

/// 渲染主页与连接管理页面
pub fn render_home_page<V: 'static>(
    text_input: Entity<TextInput>,
    state: &AdbDeviceState,
    model: &str,
    backlight: Option<u32>,
    is_connecting: bool,
    is_scanning: bool,
    discovered: &[crate::adb::DiscoveredDevice],
    toast: Option<&(String, bool)>,
    cx: &mut Context<V>,
    on_connect: impl Fn(&mut V, &mut Context<V>) + 'static + Copy,
    on_cancel_connect: impl Fn(&mut V, &mut Context<V>) + 'static + Copy,
    on_disconnect: impl Fn(&mut V, &mut Context<V>) + 'static + Copy,
    on_scan: impl Fn(&mut V, &mut Context<V>) + 'static + Copy,
    on_cancel_scan: impl Fn(&mut V, &mut Context<V>) + 'static + Copy,
    on_select_discovered_ip: impl Fn(&mut V, &str, &mut Context<V>) + 'static + Copy,
) -> Div {
    let is_connected = state.is_connected();

    div()
        .flex()
        .flex_col()
        .gap_4()
        .w_full()
        // 标题卡片
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                        .child("设备连接"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                        .child("小米 / Redmi 显示器支持局域网 IP 直连；普通显示器可通过右上角或托盘「亮度」直接调控"),
                ),
        )
        // 连接配置与动作卡片
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
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                .child("局域网 IP 连接"),
                        )
                        .child(
                            badge(
                                if is_connecting {
                                    "正在连接..."
                                } else if is_connected {
                                    "已连接"
                                } else {
                                    "未连接"
                                },
                                if is_connecting {
                                    ThemeColors::ACCENT_BG_SUBTLE()
                                } else if is_connected {
                                    ThemeColors::STATUS_SUCCESS()
                                } else {
                                    ThemeColors::BG_DISABLED()
                                },
                                if is_connecting {
                                    ThemeColors::ACCENT_CYAN()
                                } else if is_connected {
                                    ThemeColors::TEXT_ON_ACCENT()
                                } else {
                                    ThemeColors::TEXT_MUTED()
                                },
                            ),
                        ),
                )
                // IP:端口 编辑输入行
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("设备 IP 地址及端口（如 192.168.31.200:5555）:"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .child(text_input),
                                ),
                        ),
                )
                // 动作按钮组
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .items_center()
                        .pt_2()
                        .child(
                            div()
                                .id("btn_connect_action")
                                .child(action_button(
                                    if is_connected { "重新连接" } else { "开始连接" },
                                    true,
                                    is_connecting,
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !is_connecting {
                                        on_connect(this, cx);
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("btn_cancel_connect_action")
                                .child(action_button("取消连接", false, !is_connecting))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if is_connecting {
                                        on_cancel_connect(this, cx);
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("btn_disconnect_action")
                                .child(action_button("断开连接", false, !is_connected))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if is_connected {
                                        on_disconnect(this, cx);
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("btn_scan_action")
                                .child(action_button(
                                    if is_scanning { "扫描中..." } else { "扫描设备" },
                                    false,
                                    is_scanning,
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !is_scanning {
                                        on_scan(this, cx);
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("btn_cancel_scan_action")
                                .child(action_button("停止扫描", false, !is_scanning))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if is_scanning {
                                        on_cancel_scan(this, cx);
                                    }
                                })),
                        ),
                ),
        )
        // 设备信息概览卡片
        .child(
            card_container()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                        .child("已连接设备信息"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(div().text_color(rgb(ThemeColors::TEXT_SECONDARY())).child("设备型号:"))
                                .child(div().font_weight(gpui::FontWeight::BOLD).child(if model.is_empty() {
                                    "未连接".to_string()
                                } else {
                                    model.to_string()
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(div().text_color(rgb(ThemeColors::TEXT_SECONDARY())).child("当前背光亮度:"))
                                .child(div().font_weight(gpui::FontWeight::BOLD).text_color(rgb(ThemeColors::ACCENT_CYAN())).child(match backlight {
                                    Some(b) => format!("{}%", b),
                                    None => "离线".to_string(),
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(div().text_color(rgb(ThemeColors::TEXT_SECONDARY())).child("控制方式:"))
                                .child(div().child("局域网直连")),
                        ),
                ),
        )
        // 内网扫描候选设备列表卡片
        .child(
            card_container()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("局域网发现设备"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                .child(format!("共发现 {} 台候选设备", discovered.len())),
                        ),
                )
                .children(if discovered.is_empty() {
                    vec![
                        div()
                            .p_3()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .child(if is_scanning {
                                "正在扫描局域网设备，请稍候..."
                            } else {
                                "未发现候选设备，点击上方「扫描设备」或直接输入 IP"
                            })
                            .into_any_element()
                    ]
                } else {
                    discovered.iter().enumerate().map(|(idx, dev)| {
                        let dev_ip = dev.ip.clone();
                        let dev_model = if dev.model.is_empty() { "显示设备".to_string() } else { dev.model.clone() };
                        div()
                            .id(("disc_dev", idx))
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .p_2()
                            .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(ThemeColors::BG_HOVER())))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_sm().font_weight(gpui::FontWeight::BOLD).child(dev.ip.clone()))
                                    .child(div().text_xs().text_color(rgb(ThemeColors::TEXT_SECONDARY())).child(format!("型号: {}", dev_model))),
                            )
                            .child(action_button("填入 IP", false, false))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_select_discovered_ip(this, &dev_ip, cx);
                            }))
                            .into_any_element()
                    }).collect()
                }),
        )
        // Toast 反馈信息
        .children(toast.map(|(msg, is_err)| {
            div()
                .p_3()
                .rounded_md()
                .bg(rgb(if *is_err { ThemeColors::BG_DISABLED() } else { ThemeColors::ACCENT_BG_SUBTLE() }))
                .border_1()
                .border_color(rgb(if *is_err { ThemeColors::STATUS_ERROR() } else { ThemeColors::STATUS_SUCCESS() }))
                .text_xs()
                .text_color(rgb(if *is_err { ThemeColors::STATUS_ERROR() } else { ThemeColors::TEXT_PRIMARY() }))
                .child(msg.clone())
        }))
}
