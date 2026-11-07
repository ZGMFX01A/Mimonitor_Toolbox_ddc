use gpui::prelude::*;
use gpui::{div, px, rgb, Context, Div, Entity, IntoElement, Render, SharedString, Window};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::device::source::VideoSource;
use crate::message::{AppCommand, MessageBus};
use crate::ui::components::{action_button, badge, card_container};
use crate::ui::theme::ThemeColors;

/// 信号源管理页面实体，独立管理真实信号源回读、切换待确认与延时验证状态
pub struct SourcePage {
    bus: Arc<MessageBus>,
    is_connected: bool,
    current_source: Option<VideoSource>,
    raw_source: String,
    is_reading: bool,
    is_switching: bool,
    target_source: Option<VideoSource>,
    pending_confirm: Option<VideoSource>,
    error_message: Option<String>,
    status_message: Option<String>,
    generation: u64,
}

impl SourcePage {
    pub fn new(bus: Arc<MessageBus>, _cx: &mut Context<Self>) -> Self {
        let gen = bus.active_generation.load(Ordering::Relaxed);
        Self {
            bus,
            is_connected: false,
            current_source: None,
            raw_source: String::new(),
            is_reading: false,
            is_switching: false,
            target_source: None,
            pending_confirm: None,
            error_message: None,
            status_message: None,
            generation: gen,
        }
    }

    pub fn set_connected(&mut self, connected: bool, generation: u64, cx: &mut Context<Self>) {
        let gen_changed = self.generation != generation;
        self.is_connected = connected;
        self.generation = generation;
        if !connected || gen_changed {
            self.current_source = None;
            self.raw_source.clear();
            self.is_reading = false;
            self.is_switching = false;
            self.target_source = None;
            self.pending_confirm = None;
            self.status_message = None;
            self.error_message = None;
        }
        cx.notify();
    }

    pub fn on_source_updated(
        &mut self,
        source: Option<VideoSource>,
        raw_value: &str,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.bus.active_generation.load(Ordering::Relaxed) {
            return;
        }
        self.current_source = source;
        self.raw_source = raw_value.to_string();
        self.is_reading = false;
        self.error_message = None;
        cx.notify();
    }

    pub fn on_switch_success(
        &mut self,
        source: VideoSource,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.bus.active_generation.load(Ordering::Relaxed) {
            return;
        }
        self.is_switching = false;
        self.target_source = None;
        self.current_source = Some(source);
        self.status_message = Some(format!("已成功切换并延时验证信号源为 {}", source.name()));
        self.error_message = None;
        cx.notify();
    }

    pub fn on_switch_failed(
        &mut self,
        source: VideoSource,
        error: &str,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.bus.active_generation.load(Ordering::Relaxed) {
            return;
        }
        self.is_switching = false;
        self.target_source = None;
        self.error_message = Some(format!("切换至 {} 失败: {}", source.name(), error));
        self.status_message = None;
        cx.notify();
    }

    pub fn on_read_failed(&mut self, error: &str, generation: u64, cx: &mut Context<Self>) {
        if generation != self.bus.active_generation.load(Ordering::Relaxed) {
            return;
        }
        self.is_reading = false;
        self.error_message = Some(error.to_string());
        cx.notify();
    }

    pub fn request_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.is_connected || self.is_switching || self.is_reading {
            return;
        }
        self.is_reading = true;
        self.error_message = None;
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        self.generation = gen;
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::RefreshSource { generation: gen });
        cx.notify();
    }

    pub fn request_switch(&mut self, target: VideoSource, cx: &mut Context<Self>) {
        if !self.is_connected || self.is_switching {
            return;
        }
        self.pending_confirm = Some(target);
        self.error_message = None;
        cx.notify();
    }

    pub fn confirm_switch(&mut self, target: VideoSource, cx: &mut Context<Self>) {
        if !self.is_connected || self.is_switching {
            return;
        }
        self.pending_confirm = None;
        self.is_switching = true;
        self.target_source = Some(target);
        self.status_message = Some(format!(
            "正在切换至 {} 并等待延时回读确认...",
            target.name()
        ));
        self.error_message = None;
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        self.generation = gen;
        let _ = self.bus.cmd_sender.send(AppCommand::SwitchSource {
            source: target,
            generation: gen,
        });
        cx.notify();
    }

    pub fn cancel_confirm(&mut self, cx: &mut Context<Self>) {
        self.pending_confirm = None;
        cx.notify();
    }
}

impl Render for SourcePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_connected = self.is_connected;
        let is_switching = self.is_switching;
        let is_reading = self.is_reading;
        let current_source = self.current_source;
        let pending_confirm = self.pending_confirm;

        div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full()
            // 顶部标题栏与手动刷新操作
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
                                    .child("信号源切换"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                    .child("HDMI 1 / HDMI 2 / DP / Type-C 端口切换与异步回读验证"),
                            ),
                    )
                    .child(
                        div()
                            .id("btn_refresh_source")
                            .child(action_button(
                                if is_reading { "读取中..." } else { "刷新数据" },
                                false,
                                !is_connected || is_switching || is_reading,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.request_refresh(cx);
                            })),
                    ),
            )
            // 未连接提示卡片
            .when(!is_connected, |d| {
                d.child(
                    card_container()
                        .border_color(rgb(ThemeColors::STATUS_WARNING()))
                        .bg(rgb(ThemeColors::STATUS_WARNING_BG()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::STATUS_WARNING()))
                                .child("⚠️ 设备未连接：请先在「主页 & 连接」页面建立无线 ADB 连接，连接后方可读取与切换输入信号源。"),
                        ),
                )
            })
            // 当前活跃信号源状态展示卡片
            .child(
                card_container()
                    .items_center()
                    .py_6()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("当前活跃输入信号源"),
                    )
                    .child(
                        if is_switching {
                            div()
                                .text_2xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::STATUS_WARNING()))
                                .child(format!(
                                    "正在切换至 {} (请稍候)...",
                                    self.target_source.map(|s| s.name()).unwrap_or("目标源")
                                ))
                        } else if let Some(src) = current_source {
                            div()
                                .text_3xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                .child(format!("{} (ID: {})", src.name(), src.id()))
                        } else if !self.raw_source.is_empty() {
                            div()
                                .text_2xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                .child(format!("未知 ({})", self.raw_source))
                        } else {
                            div()
                                .text_2xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                .child(if is_connected { "未知 (未读取)" } else { "未知 (未连接)" })
                        },
                    ),
            )
            // 应用内切源确认卡片 (防误触防黑屏)
            .when_some(pending_confirm, |d, target| {
                d.child(
                    card_container()
                        .border_color(rgb(ThemeColors::STATUS_WARNING()))
                        .bg(rgb(ThemeColors::STATUS_WARNING_BG()))
                        .gap_3()
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
                                        .text_color(rgb(ThemeColors::STATUS_WARNING()))
                                        .child("⚠️ 屏幕短暂黑屏预警与操作确认"),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child(format!(
                                    "即将切换显示器信号源至「{}」(端口 ID: {})。切换过程中屏幕将短暂断开信号并黑屏数秒，待新源建立握手后自动亮起。是否确认立即切换？",
                                    target.name(),
                                    target.id()
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_3()
                                .child(
                                    div()
                                        .id("btn_confirm_switch_action")
                                        .child(action_button("确认切换", true, false))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.confirm_switch(target, cx);
                                        })),
                                )
                                .child(
                                    div()
                                        .id("btn_cancel_switch_action")
                                        .child(action_button("取消", false, false))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.cancel_confirm(cx);
                                        })),
                                ),
                        ),
                )
            })
            // 端口选择列表卡片 (包含完整 23 HDMI 1, 24 HDMI 2, 29 DP, 30 Type-C)
            .child(
                card_container()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                            .child("可用视频输入端口"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_3()
                            .children(VideoSource::all().iter().map(|&source| {
                                let is_active = current_source == Some(source);
                                let is_disabled = !is_connected || is_switching || is_reading;
                                let btn_id = format!("source_btn_{}", source.id());

                                div()
                                    .id(SharedString::from(btn_id))
                                    .flex_1()
                                    .min_w(px(140.0))
                                    .p_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(if is_active {
                                        rgb(ThemeColors::ACCENT_CYAN())
                                    } else {
                                        rgb(ThemeColors::BORDER())
                                    })
                                    .bg(if is_active {
                                        rgb(ThemeColors::ACCENT_BG_SUBTLE())
                                    } else if is_disabled {
                                        rgb(ThemeColors::BG_DISABLED())
                                    } else {
                                        rgb(ThemeColors::BG_CARD())
                                    })
                                    .cursor(if is_disabled {
                                        gpui::CursorStyle::Arrow
                                    } else {
                                        gpui::CursorStyle::PointingHand
                                    })
                                    .when(!is_disabled && !is_active, |el| {
                                        el.hover(|s| s.bg(rgb(ThemeColors::BG_HOVER())))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !is_disabled {
                                            this.request_switch(source, cx);
                                        }
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
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
                                                            .text_color(if is_active {
                                                                rgb(ThemeColors::ACCENT_CYAN())
                                                            } else if is_disabled {
                                                                rgb(ThemeColors::TEXT_DISABLED())
                                                            } else {
                                                                rgb(ThemeColors::TEXT_PRIMARY())
                                                            })
                                                            .child(source.name()),
                                                    )
                                                    .when(is_active, |d| {
                                                        d.child(badge("当前活跃", ThemeColors::STATUS_SUCCESS_BG(), ThemeColors::STATUS_SUCCESS()))
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                                    .child(format!("端口 ID: {}", source.id())),
                                            ),
                                    )
                            })),
                    ),
            )
            // 状态提示或错误通知
            .when_some(self.status_message.as_ref(), |d, msg| {
                d.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(rgb(ThemeColors::STATUS_SUCCESS_BG()))
                        .border_1()
                        .border_color(rgb(ThemeColors::STATUS_SUCCESS()))
                        .text_xs()
                        .text_color(rgb(ThemeColors::STATUS_SUCCESS()))
                        .child(msg.clone()),
                )
            })
            .when_some(self.error_message.as_ref(), |d, err| {
                d.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(rgb(ThemeColors::STATUS_ERROR_BG()))
                        .border_1()
                        .border_color(rgb(ThemeColors::STATUS_ERROR()))
                        .text_xs()
                        .text_color(rgb(ThemeColors::STATUS_ERROR()))
                        .child(err.clone()),
                )
            })
            // 底部运行提示
            .child(
                card_container()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_MUTED()))
                            .child("说明：切换信号源过程中屏幕可能会短暂黑屏数秒，待新信号源建立连接后自动恢复显示。"),
                    ),
            )
    }
}

/// 渲染信号源管理真实页面
pub fn render_source_page(source_page: Entity<SourcePage>) -> Div {
    div().w_full().child(source_page)
}
