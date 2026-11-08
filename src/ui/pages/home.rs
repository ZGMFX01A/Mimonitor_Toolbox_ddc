use crate::adb::AdbDeviceState;
use crate::ui::components::{action_button, badge, card_container, TextInput};
use crate::ui::theme::ThemeColors;
use gpui::prelude::*;
use gpui::{div, px, rgb, Context, Div, Entity};

/// 渲染主页与连接管理页面
pub fn render_home_page<V: 'static>(
    text_input: Entity<TextInput>,
    state: &AdbDeviceState,
    model: &str,
    backlight: Option<u32>,
    is_connecting: bool,
    is_scanning: bool,
    discovered: &[crate::adb::DiscoveredDevice],
    connected_ip: &str,
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
        .gap_3()
        .w_full()
        .min_w_0()
        // 顶部精简协议指引
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            div()
                                .text_base()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("设备连接"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .px_2()
                                .py(px(2.0))
                                .rounded_md()
                                .bg(rgb(ThemeColors::ACCENT_BG_SUBTLE()))
                                .border_1()
                                .border_color(rgb(ThemeColors::BORDER()))
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("普通显示器在右上角「亮度面板」直接控制"),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                        .child("小米 / Redmi 智能显示器支持局域网自动扫描与 IP 直连；普通显示器免连即用。"),
                ),
        )
        // 核心连接与设备发现统一卡片（确保首屏 860x600 完整可见）
        .child(
            card_container()
                // 卡片标题与当前状态
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                .child("局域网设备连接"),
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
                                    ThemeColors::STATUS_SUCCESS_BG()
                                } else {
                                    ThemeColors::BG_DISABLED()
                                },
                                if is_connecting {
                                    ThemeColors::ACCENT_CYAN()
                                } else if is_connected {
                                    ThemeColors::STATUS_SUCCESS()
                                } else {
                                    ThemeColors::TEXT_MUTED()
                                },
                            ),
                        ),
                )
                // 已连接设备信息紧凑栏（合并原3大列卡片，仅一行呈现，未连接时不占版面）
                .children(if is_connected {
                    Some(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .p_2p5()
                            .rounded_md()
                            .bg(rgb(ThemeColors::STATUS_SUCCESS_BG()))
                            .border_1()
                            .border_color(rgb(ThemeColors::STATUS_SUCCESS()))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(ThemeColors::STATUS_SUCCESS()))
                                            .child("● 已连接设备:"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                            .child(if model.is_empty() {
                                                "智能显示器".to_string()
                                            } else {
                                                model.to_string()
                                            }),
                                    )
                                    .children(backlight.map(|b| {
                                        div()
                                            .text_xs()
                                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                            .child(format!("背光: {}%", b))
                                    })),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(ThemeColors::STATUS_SUCCESS()))
                                    .child("在线"),
                            ),
                    )
                } else {
                    None
                })
                // 主操作区：IP 输入与操作按钮（仅显示当前相关动作，不向用户要求端口）
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                        .child("显示器 IP"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                        .child("如 192.168.31.200"),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(200.0))
                                        .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                                            if event.keystroke.key == "enter" && !is_connecting {
                                                on_connect(this, cx);
                                                cx.stop_propagation();
                                            }
                                        }))
                                        .child(text_input),
                                )
                                .child(
                                    div()
                                        .id("btn_connect_action")
                                        .flex_shrink_0()
                                        .child(action_button(
                                            if is_connected { "重新连接" } else { "连接" },
                                            true,
                                            is_connecting,
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !is_connecting {
                                                on_connect(this, cx);
                                            }
                                        })),
                                )
                                .children(if is_connecting {
                                    Some(
                                        div()
                                            .id("btn_cancel_connect_action")
                                            .flex_shrink_0()
                                            .child(action_button("取消连接", false, !is_connecting))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if is_connecting {
                                                    on_cancel_connect(this, cx);
                                                }
                                            })),
                                    )
                                } else {
                                    None
                                })
                                .child(
                                    div()
                                        .id("btn_scan_action")
                                        .flex_shrink_0()
                                        .child(action_button(
                                            if is_scanning { "扫描中..." } else { "扫描并连接" },
                                            false,
                                            is_scanning,
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !is_scanning {
                                                on_scan(this, cx);
                                            }
                                        })),
                                )
                                .children(if is_scanning {
                                    Some(
                                        div()
                                            .id("btn_cancel_scan_action")
                                            .flex_shrink_0()
                                            .child(action_button("停止扫描", false, !is_scanning))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if is_scanning {
                                                    on_cancel_scan(this, cx);
                                                }
                                            })),
                                    )
                                } else {
                                    None
                                })
                                .children(if is_connected {
                                    Some(
                                        div()
                                            .id("btn_disconnect_action")
                                            .flex_shrink_0()
                                            .child(action_button("断开连接", false, !is_connected))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if is_connected {
                                                    on_disconnect(this, cx);
                                                }
                                            })),
                                    )
                                } else {
                                    None
                                }),
                        ),
                )
                // 局域网发现设备列表（紧邻操作区，首屏可见，支持纵向适度滚动）
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .pt_2()
                        .border_t_1()
                        .border_color(rgb(ThemeColors::BORDER()))
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
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                                .child("局域网发现设备"),
                                        )
                                        .children(if is_scanning {
                                            Some(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                                    .child("正在扫描中..."),
                                            )
                                        } else {
                                            None
                                        }),
                                )
                                .child(
                                    badge(
                                        format!("{} 台", discovered.len()),
                                        ThemeColors::BG_ROOT(),
                                        ThemeColors::TEXT_SECONDARY(),
                                    ),
                                ),
                        )
                        .child(if discovered.is_empty() {
                            div()
                                .p_3()
                                .rounded_md()
                                .bg(rgb(ThemeColors::BG_ROOT()))
                                .border_1()
                                .border_color(rgb(ThemeColors::BORDER()))
                                .text_xs()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_color(rgb(if is_scanning {
                                            ThemeColors::ACCENT_CYAN()
                                        } else {
                                            ThemeColors::TEXT_MUTED()
                                        }))
                                        .child(if is_scanning {
                                            "正在探测局域网设备，请稍候..."
                                        } else {
                                            "点击「扫描并连接」查找显示器，或输入 IP 后按回车连接"
                                        }),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .id("discovered_devices_list")
                                .max_h(px(200.0))
                                .min_h_0()
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .children(discovered.iter().enumerate().map(|(idx, dev)| {
                                    let dev_ip = dev.ip.clone();
                                    let dev_model = if dev.model.is_empty() {
                                        "智能显示设备".to_string()
                                    } else {
                                        dev.model.clone()
                                    };
                                    let is_dev_connected = is_connected && connected_ip == dev.ip;

                                    let mut item = div()
                                        .id(("disc_dev", idx))
                                        .flex()
                                        .flex_row()
                                        .justify_between()
                                        .items_center()
                                        .p_2p5()
                                        .bg(rgb(if is_dev_connected {
                                            ThemeColors::ACCENT_BG_SUBTLE()
                                        } else {
                                            ThemeColors::BG_ROOT()
                                        }))
                                        .border_1()
                                        .border_color(rgb(if is_dev_connected {
                                            ThemeColors::STATUS_SUCCESS()
                                        } else {
                                            ThemeColors::BORDER()
                                        }))
                                        .rounded_md();

                                    if !is_dev_connected {
                                        item = item
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(rgb(ThemeColors::BG_HOVER()))
                                                    .border_color(rgb(ThemeColors::ACCENT_CYAN()))
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                on_select_discovered_ip(this, &dev_ip, cx);
                                            }));
                                    }

                                    item.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_0p5()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::BOLD)
                                                            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                                            .truncate()
                                                            .child(dev.ip.clone()),
                                                    )
                                                    .child(
                                                        if is_dev_connected {
                                                            badge(
                                                                "已连接",
                                                                ThemeColors::STATUS_SUCCESS_BG(),
                                                                ThemeColors::STATUS_SUCCESS(),
                                                            )
                                                        } else {
                                                            badge(
                                                                "可连接",
                                                                ThemeColors::BG_CARD(),
                                                                ThemeColors::TEXT_SECONDARY(),
                                                            )
                                                        },
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                                    .truncate()
                                                    .child(format!("型号: {}", dev_model)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .child(if is_dev_connected {
                                                div()
                                                    .px_2()
                                                    .py_1()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(rgb(ThemeColors::STATUS_SUCCESS()))
                                                    .child("已连接")
                                                    .into_any_element()
                                            } else {
                                                action_button("点击连接", false, is_connecting).into_any_element()
                                            }),
                                    )
                                    .into_any_element()
                                }))
                                .into_any_element()
                        }),
                ),
        )
}
