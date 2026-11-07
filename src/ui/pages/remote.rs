use gpui::prelude::*;
use gpui::{div, px, rgb, Context, Div, Entity, IntoElement, Render, SharedString, Window};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::device::remote::RemoteKey;
use crate::message::{AppCommand, MessageBus};
use crate::ui::components::card_container;
use crate::ui::theme::ThemeColors;

/// 虚拟遥控器管理页面实体，独立管理按键发送状态与错误响应
pub struct RemotePage {
    bus: Arc<MessageBus>,
    is_connected: bool,
    pending_key: Option<RemoteKey>,
    last_status: Option<String>,
    last_error: Option<String>,
    generation: u64,
}

impl RemotePage {
    pub fn new(bus: Arc<MessageBus>, _cx: &mut Context<Self>) -> Self {
        let gen = bus.active_generation.load(Ordering::Relaxed);
        Self {
            bus,
            is_connected: false,
            pending_key: None,
            last_status: None,
            last_error: None,
            generation: gen,
        }
    }

    pub fn set_connected(&mut self, connected: bool, generation: u64, cx: &mut Context<Self>) {
        let gen_changed = self.generation != generation;
        self.is_connected = connected;
        self.generation = generation;
        if !connected || gen_changed {
            self.pending_key = None;
            self.last_status = None;
            self.last_error = None;
        }
        cx.notify();
    }

    pub fn on_key_success(&mut self, key: RemoteKey, generation: u64, cx: &mut Context<Self>) {
        if generation != self.bus.active_generation.load(Ordering::Relaxed) {
            return;
        }
        self.pending_key = None;
        self.last_status = Some(format!("按键「{}」发送成功", key.label()));
        self.last_error = None;
        cx.notify();
    }

    pub fn on_key_failed(
        &mut self,
        key: RemoteKey,
        error: &str,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.bus.active_generation.load(Ordering::Relaxed) {
            return;
        }
        self.pending_key = None;
        self.last_error = Some(format!("发送按键「{}」失败: {}", key.label(), error));
        self.last_status = None;
        cx.notify();
    }

    pub fn send_key(&mut self, key: RemoteKey, cx: &mut Context<Self>) {
        if !self.is_connected || self.pending_key.is_some() {
            return;
        }
        self.pending_key = Some(key);
        self.last_error = None;
        self.last_status = None;
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        self.generation = gen;
        let _ = self.bus.cmd_sender.send(AppCommand::SendRemoteKey {
            key,
            generation: gen,
        });
        cx.notify();
    }
}

impl Render for RemotePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_connected = self.is_connected;
        let pending_key = self.pending_key;
        let is_busy = pending_key.is_some();
        let is_disabled = !is_connected || is_busy;

        div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full()
            // 顶部标题栏
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
                            .child("遥控器"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("快捷控制显示器电源、方向舵、导航与音量"),
                    ),
            )
            // 未连接提示
            .when(!is_connected, |d| {
                d.child(
                    card_container()
                        .border_color(rgb(ThemeColors::STATUS_WARNING()))
                        .bg(rgb(ThemeColors::STATUS_WARNING_BG()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::STATUS_WARNING()))
                                .child("⚠️ 设备未连接：请先在「主页 & 连接」页面建立无线 ADB 连接，连接后方可发送遥控按键。"),
                        ),
                )
            })
            // 执行状态与错误展示区 (待执行状态与错误可见，不能先显成功)
            .child(
                card_container()
                    .p_3()
                    .child(
                        if let Some(key) = pending_key {
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(ThemeColors::STATUS_WARNING()))
                                        .child(format!("⏳ 正在向显示器发送按键「{}」...", key.label())),
                                )
                        } else if let Some(ref err) = self.last_error {
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::STATUS_ERROR()))
                                .child(err.clone())
                        } else if let Some(ref msg) = self.last_status {
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::STATUS_SUCCESS()))
                                .child(msg.clone())
                        } else {
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                .child(if is_connected {
                                    "遥控器已就绪，点击下方按键控制显示器"
                                } else {
                                    "遥控器已禁用 (等待 ADB 连接)"
                                })
                        },
                    ),
            )
            // 实体拟物遥控器面板容器 (居中布局)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_center()
                    .w_full()
                    .child(
                        div()
                            .w(px(320.0))
                            .p_6()
                            .rounded_xl()
                            .border_1()
                            .border_color(rgb(ThemeColors::BORDER()))
                            .bg(rgb(ThemeColors::BG_CARD()))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_5()
                            // 遥控器顶部品牌标
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                    .child("显示器遥控"),
                            )
                            // 顶部电源键行
                            .child(
                                div()
                                    .id("remote_btn_power")
                                    .w(px(100.0))
                                    .py_2()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if is_disabled {
                                        rgb(ThemeColors::BORDER_DISABLED())
                                    } else {
                                        rgb(ThemeColors::STATUS_ERROR())
                                    })
                                    .bg(if is_disabled {
                                        rgb(ThemeColors::BG_DISABLED())
                                    } else {
                                        rgb(ThemeColors::STATUS_ERROR())
                                    })
                                    .cursor(if is_disabled {
                                        gpui::CursorStyle::Arrow
                                    } else {
                                        gpui::CursorStyle::PointingHand
                                    })
                                    .when(!is_disabled, |el| {
                                        el.hover(|s| s.bg(rgb(ThemeColors::STATUS_ERROR())))
                                    })
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send_key(RemoteKey::Power, cx);
                                    }))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(if is_disabled {
                                                rgb(ThemeColors::TEXT_DISABLED())
                                            } else {
                                                rgb(0xFFFFFF)
                                            })
                                            .child("⏻ 电源"),
                                    ),
                            )
                            // 导航功能键行 (主页、菜单、返回)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .w_full()
                                    .gap_2()
                                    .child(render_nav_btn(
                                        "remote_btn_home",
                                        RemoteKey::Home,
                                        "主页",
                                        is_disabled,
                                        cx,
                                    ))
                                    .child(render_nav_btn(
                                        "remote_btn_menu",
                                        RemoteKey::Menu,
                                        "菜单",
                                        is_disabled,
                                        cx,
                                    ))
                                    .child(render_nav_btn(
                                        "remote_btn_back",
                                        RemoteKey::Back,
                                        "返回",
                                        is_disabled,
                                        cx,
                                    )),
                            )
                            // 方向舵十字轮盘区域 (上/下/左/右/确定 5 键)
                            .child(
                                div()
                                    .w(px(200.0))
                                    .h(px(200.0))
                                    .rounded_full()
                                    .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                                    .border_1()
                                    .border_color(rgb(ThemeColors::BORDER()))
                                    .flex()
                                    .flex_col()
                                    .justify_between()
                                    .items_center()
                                    .p_2()
                                    // 舵轮顶部：上键
                                    .child(
                                        div()
                                            .id("remote_btn_dpad_up")
                                            .w(px(56.0))
                                            .h(px(40.0))
                                            .rounded_md()
                                            .flex()
                                            .justify_center()
                                            .items_center()
                                            .bg(if is_disabled { rgb(ThemeColors::BG_DISABLED()) } else { rgb(ThemeColors::BG_BUTTON()) })
                                            .cursor(if is_disabled { gpui::CursorStyle::Arrow } else { gpui::CursorStyle::PointingHand })
                                            .when(!is_disabled, |el| el.hover(|s| s.bg(rgb(ThemeColors::BG_HOVER()))))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.send_key(RemoteKey::DpadUp, cx);
                                            }))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(if is_disabled { rgb(ThemeColors::TEXT_MUTED()) } else { rgb(ThemeColors::TEXT_PRIMARY()) })
                                                    .child("▲ 上"),
                                            ),
                                    )
                                    // 舵轮中部：左键、OK / 确定、右键
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .justify_between()
                                            .items_center()
                                            .w_full()
                                            .px_2()
                                            // 左键
                                            .child(
                                                div()
                                                    .id("remote_btn_dpad_left")
                                                    .w(px(46.0))
                                                    .h(px(46.0))
                                                    .rounded_md()
                                                    .flex()
                                                    .justify_center()
                                                    .items_center()
                                                    .bg(if is_disabled { rgb(ThemeColors::BG_DISABLED()) } else { rgb(ThemeColors::BG_BUTTON()) })
                                                    .cursor(if is_disabled { gpui::CursorStyle::Arrow } else { gpui::CursorStyle::PointingHand })
                                                    .when(!is_disabled, |el| el.hover(|s| s.bg(rgb(ThemeColors::BG_HOVER()))))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.send_key(RemoteKey::DpadLeft, cx);
                                                    }))
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::BOLD)
                                                            .text_color(if is_disabled { rgb(ThemeColors::TEXT_MUTED()) } else { rgb(ThemeColors::TEXT_PRIMARY()) })
                                                            .child("◀ 左"),
                                                    ),
                                            )
                                            // 中心确定/OK键
                                            .child(
                                                div()
                                                    .id("remote_btn_dpad_center")
                                                    .w(px(64.0))
                                                    .h(px(64.0))
                                                    .rounded_full()
                                                    .flex()
                                                    .justify_center()
                                                    .items_center()
                                                    .bg(if is_disabled {
                                                        rgb(ThemeColors::BG_DISABLED())
                                                    } else {
                                                        rgb(ThemeColors::ACCENT_CYAN())
                                                    })
                                                    .cursor(if is_disabled { gpui::CursorStyle::Arrow } else { gpui::CursorStyle::PointingHand })
                                                    .when(!is_disabled, |el| el.hover(|s| s.bg(rgb(ThemeColors::ACCENT_HOVER()))))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.send_key(RemoteKey::DpadCenter, cx);
                                                    }))
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(gpui::FontWeight::BOLD)
                                                            .text_color(if is_disabled { rgb(ThemeColors::TEXT_DISABLED()) } else { rgb(ThemeColors::TEXT_ON_ACCENT()) })
                                                            .child("OK 确定"),
                                                    ),
                                            )
                                            // 右键
                                            .child(
                                                div()
                                                    .id("remote_btn_dpad_right")
                                                    .w(px(46.0))
                                                    .h(px(46.0))
                                                    .rounded_md()
                                                    .flex()
                                                    .justify_center()
                                                    .items_center()
                                                    .bg(if is_disabled { rgb(ThemeColors::BG_DISABLED()) } else { rgb(ThemeColors::BG_BUTTON()) })
                                                    .cursor(if is_disabled { gpui::CursorStyle::Arrow } else { gpui::CursorStyle::PointingHand })
                                                    .when(!is_disabled, |el| el.hover(|s| s.bg(rgb(ThemeColors::BG_HOVER()))))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.send_key(RemoteKey::DpadRight, cx);
                                                    }))
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::BOLD)
                                                            .text_color(if is_disabled { rgb(ThemeColors::TEXT_MUTED()) } else { rgb(ThemeColors::TEXT_PRIMARY()) })
                                                            .child("▶ 右"),
                                                    ),
                                            ),
                                    )
                                    // 舵轮底部：下键
                                    .child(
                                        div()
                                            .id("remote_btn_dpad_down")
                                            .w(px(56.0))
                                            .h(px(40.0))
                                            .rounded_md()
                                            .flex()
                                            .justify_center()
                                            .items_center()
                                            .bg(if is_disabled { rgb(ThemeColors::BG_DISABLED()) } else { rgb(ThemeColors::BG_BUTTON()) })
                                            .cursor(if is_disabled { gpui::CursorStyle::Arrow } else { gpui::CursorStyle::PointingHand })
                                            .when(!is_disabled, |el| el.hover(|s| s.bg(rgb(ThemeColors::BG_HOVER()))))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.send_key(RemoteKey::DpadDown, cx);
                                            }))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(if is_disabled { rgb(ThemeColors::TEXT_MUTED()) } else { rgb(ThemeColors::TEXT_PRIMARY()) })
                                                    .child("▼ 下"),
                                            ),
                                    ),
                            )
                            // 底部音量按键行 (音量-、静音、音量+)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .w_full()
                                    .gap_2()
                                    .child(render_nav_btn(
                                        "remote_btn_voldown",
                                        RemoteKey::VolumeDown,
                                        "🔉 音量 -",
                                        is_disabled,
                                        cx,
                                    ))
                                    .child(render_nav_btn(
                                        "remote_btn_mute",
                                        RemoteKey::VolumeMute,
                                        "🔇 静音",
                                        is_disabled,
                                        cx,
                                    ))
                                    .child(render_nav_btn(
                                        "remote_btn_volup",
                                        RemoteKey::VolumeUp,
                                        "🔊 音量 +",
                                        is_disabled,
                                        cx,
                                    )),
                            ),
                    ),
            )
            // 底部操作说明
            .child(
                card_container()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_MUTED()))
                            .child("说明：点击上方按键可快速向显示器发送遥控指令，支持电源、主页、菜单、返回、方向舵及音量调节。"),
                    ),
            )
    }
}

/// 辅助渲染标准导航/功能按键
fn render_nav_btn(
    id_str: &'static str,
    key: RemoteKey,
    label: &'static str,
    is_disabled: bool,
    cx: &mut Context<RemotePage>,
) -> gpui::Stateful<Div> {
    div()
        .id(SharedString::from(id_str))
        .flex_1()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(if is_disabled {
            rgb(ThemeColors::BORDER())
        } else {
            rgb(ThemeColors::BORDER())
        })
        .bg(if is_disabled {
            rgb(crate::ui::theme::ThemeColors::BG_CARD())
        } else {
            rgb(ThemeColors::BG_BUTTON())
        })
        .cursor(if is_disabled {
            gpui::CursorStyle::Arrow
        } else {
            gpui::CursorStyle::PointingHand
        })
        .when(!is_disabled, |el| {
            el.hover(|s| s.bg(rgb(ThemeColors::BG_HOVER())))
        })
        .flex()
        .justify_center()
        .items_center()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.send_key(key, cx);
        }))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if is_disabled {
                    rgb(ThemeColors::TEXT_MUTED())
                } else {
                    rgb(ThemeColors::TEXT_PRIMARY())
                })
                .child(label),
        )
}

/// 渲染虚拟遥控器真实页面
pub fn render_remote_page(remote_page: Entity<RemotePage>) -> Div {
    div().w_full().child(remote_page)
}
