use crate::hotkey::AdjustParam;
use crate::message::AppCommand;
use crate::ui::components::{action_button, badge, card_container, ContinuousSlider};
use crate::ui::theme::ThemeColors;
use crate::AppState;
use gpui::prelude::*;
use gpui::{
    div, rgb, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render, Subscription, Window,
};
use std::sync::atomic::Ordering;

/// 参数状态只读查询结果封装
struct ParamStatus {
    pub confirmed_value: Option<u32>,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
}

/// 托盘单一数值滑杆悬浮窗视图 (严格零同步 IO，由全局共享 AppState 订阅驱动)
pub struct TrayValuePopupView {
    pub app_state: Entity<AppState>,
    pub param: AdjustParam,
    _subscription: Subscription,
    pub slider: Entity<ContinuousSlider>,
    pub initial_generation: u64,
    pub is_refreshing: bool,
    pub is_closing: bool,
    pub status_msg: Option<String>,
    pub is_error: bool,
    pub pending_value: Option<u32>,
    pending_request_id: Option<u64>,
    pub on_open_toolbox: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
    pub on_close: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
    pub focus_handle: FocusHandle,
}

impl TrayValuePopupView {
    pub fn new(
        app_state: Entity<AppState>,
        param: AdjustParam,
        cx: &mut Context<Self>,
        on_open_toolbox: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
        on_close: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let initial_generation = app_state
            .read(cx)
            .bus
            .active_generation
            .load(Ordering::Acquire);

        let min_val = param.min() as u32;
        let max_val = param.max() as u32;
        let unit = match param {
            AdjustParam::Backlight => "%",
            _ => "",
        };

        let weak_view = cx.entity().downgrade();
        let slider = cx.new(|_| {
            ContinuousSlider::new_param(
                param.label(),
                None,
                min_val,
                max_val,
                1,
                unit,
                true,
                Some("正在读取参数...".to_string()),
                move |val, _window, cx| {
                    if let Some(view) = weak_view.upgrade() {
                        view.update(cx, |this, cx| {
                            this.on_slider_change(val, cx);
                        });
                    }
                },
            )
        });

        // 订阅 AppState 变更，实现与后台事件及主窗口的响应式同步
        let _subscription = cx.observe(&app_state, |this, state, cx| {
            this.sync_from_app_state(&state, cx);
        });

        let mut view = Self {
            pending_request_id: None,
            app_state: app_state.clone(),
            param,
            _subscription,
            slider,
            initial_generation,
            is_refreshing: false,
            is_closing: false,
            status_msg: None,
            is_error: false,
            pending_value: None,
            on_open_toolbox,
            on_close,
            focus_handle,
        };

        // 初始同步共享状态
        view.sync_from_app_state(&app_state, cx);

        // 构造时不执行同步 IO，发送异步读取命令
        view.send_fetch_command(cx);

        view
    }

    /// 从 AppState 提取指定参数的当前确认值与可用状态
    fn read_param_state(
        app_state: &AppState,
        param: AdjustParam,
        initial_generation: u64,
        cx: &Context<Self>,
    ) -> ParamStatus {
        let connected = app_state.adb_state.is_connected();
        if !connected {
            return ParamStatus {
                confirmed_value: None,
                disabled: true,
                disabled_reason: Some("ADB 离线 / 未连接".to_string()),
            };
        }

        let cur_gen = app_state.bus.active_generation.load(Ordering::Relaxed);
        if cur_gen != initial_generation {
            return ParamStatus {
                confirmed_value: None,
                disabled: true,
                disabled_reason: Some("设备连接已改变，请重新打开面板".to_string()),
            };
        }

        match param {
            AdjustParam::Backlight => {
                let val = app_state.adb_backlight;
                let (disabled, reason) = if val.is_none() {
                    (true, Some("正在读取亮度...".to_string()))
                } else {
                    (false, None)
                };
                ParamStatus {
                    confirmed_value: val,
                    disabled,
                    disabled_reason: reason,
                }
            }
            AdjustParam::BlackLevel => {
                let pic = app_state.picture_page.read(cx);
                let val = pic.black_level.confirmed;
                let (disabled, reason) = if val.is_none() {
                    (true, Some("正在读取黑色级别...".to_string()))
                } else {
                    (false, None)
                };
                ParamStatus {
                    confirmed_value: val,
                    disabled,
                    disabled_reason: reason,
                }
            }
            AdjustParam::Contrast => {
                let pic = app_state.picture_page.read(cx);
                let val = pic.contrast.confirmed;
                let (disabled, reason) = if val.is_none() {
                    (true, Some("正在读取对比度...".to_string()))
                } else {
                    (false, None)
                };
                ParamStatus {
                    confirmed_value: val,
                    disabled,
                    disabled_reason: reason,
                }
            }
            AdjustParam::Saturation => {
                let pic = app_state.picture_page.read(cx);
                let val = pic.saturation.confirmed;
                let (disabled, reason) = if val.is_none() {
                    (true, Some("正在读取饱和度...".to_string()))
                } else {
                    (false, None)
                };
                ParamStatus {
                    confirmed_value: val,
                    disabled,
                    disabled_reason: reason,
                }
            }
            AdjustParam::Hue => {
                let pic = app_state.picture_page.read(cx);
                let val = pic.hue.confirmed;
                let (disabled, reason) = if val.is_none() {
                    (true, Some("正在读取色调...".to_string()))
                } else {
                    (false, None)
                };
                ParamStatus {
                    confirmed_value: val,
                    disabled,
                    disabled_reason: reason,
                }
            }
            AdjustParam::Sharpness => {
                let pic = app_state.picture_page.read(cx);
                let val = pic.sharpness.confirmed;
                let (disabled, reason) = if val.is_none() {
                    (true, Some("正在读取锐度...".to_string()))
                } else {
                    (false, None)
                };
                ParamStatus {
                    confirmed_value: val,
                    disabled,
                    disabled_reason: reason,
                }
            }
            AdjustParam::RedGain | AdjustParam::GreenGain | AdjustParam::BlueGain => {
                let pic = app_state.picture_page.read(cx);
                let adv = pic.advanced.read(cx);
                let p_state = adv.state();
                if p_state.color_temp != Some(3) {
                    ParamStatus {
                        confirmed_value: None,
                        disabled: true,
                        disabled_reason: Some("需选择自定义色温模式".to_string()),
                    }
                } else {
                    let val = match param {
                        AdjustParam::RedGain => p_state.red_gain,
                        AdjustParam::GreenGain => p_state.green_gain,
                        _ => p_state.blue_gain,
                    };
                    let (disabled, reason) = if p_state.red_gain.is_none()
                        || p_state.green_gain.is_none()
                        || p_state.blue_gain.is_none()
                    {
                        (true, Some("正在读取 RGB 参数...".to_string()))
                    } else {
                        (false, None)
                    };
                    ParamStatus {
                        confirmed_value: val,
                        disabled,
                        disabled_reason: reason,
                    }
                }
            }
            AdjustParam::AtmosphereIllumination => {
                let light = &app_state.light_state;
                if !light.is_fully_known() {
                    ParamStatus {
                        confirmed_value: None,
                        disabled: true,
                        disabled_reason: Some("屏幕灯参数未完整读取".to_string()),
                    }
                } else {
                    let val = light.illumination.map(|v| v + 1);
                    let (disabled, reason) = if val.is_none() {
                        (true, Some("正在读取屏幕灯参数...".to_string()))
                    } else {
                        (false, None)
                    };
                    ParamStatus {
                        confirmed_value: val,
                        disabled,
                        disabled_reason: reason,
                    }
                }
            }
        }
    }

    /// 从全局共享 AppState 刷新参数状态与滑块展示
    pub fn sync_from_app_state(&mut self, state: &Entity<AppState>, cx: &mut Context<Self>) {
        let app_st = state.read(cx);
        let status = Self::read_param_state(&app_st, self.param, self.initial_generation, cx);
        if !app_st.adb_state.is_connected()
            || app_st.bus.active_generation.load(Ordering::Acquire) != self.initial_generation
        {
            self.pending_value = None;
            self.pending_request_id = None;
            self.is_refreshing = false;
            self.is_error = false;
            self.status_msg = status.disabled_reason.clone();
        } else if let Some((_, error)) = self.pending_request_id
            .and_then(|id| app_st.tray_value_results.get(&id))
            .filter(|(generation, _)| *generation == self.initial_generation) {
            self.pending_value = None;
            self.pending_request_id = None;
            self.is_refreshing = false;
            self.is_error = error.is_some();
            self.status_msg = Some(error.clone().unwrap_or_else(|| "设置已提交".into()));
        } else if self.pending_request_id.is_some() {
            self.status_msg = Some("正在提交设置…".into());
        } else if self.is_refreshing {
            let error = if self.param == AdjustParam::AtmosphereIllumination {
                app_st.light_state.last_error.clone()
            } else {
                let picture = app_st.picture_page.read(cx);
                picture.is_error.then(|| picture.status_message.clone()).flatten()
            };
            if let Some(error) = error {
                self.is_refreshing = false;
                self.is_error = true;
                self.status_msg = Some(error);
            } else if status.confirmed_value.is_some() {
                self.is_refreshing = false;
                self.is_error = false;
                self.status_msg = None;
            }
        }
        if status.disabled && self.pending_request_id.is_none() && !self.is_error {
            self.is_refreshing = false;
        }

        self.slider.update(cx, |s, cx| {
            s.set_disabled(status.disabled, status.disabled_reason, cx);
            s.set_value_opt(self.pending_value.or(status.confirmed_value), cx);
        });

        cx.notify();
    }

    /// 核验会话后提交数值，以独立请求标识接收此浮窗的操作结果。
    pub fn on_slider_change(&mut self, val: u32, cx: &mut Context<Self>) {
        let bus = self.app_state.read(cx).bus.clone();
        let cur_gen = bus.active_generation.load(Ordering::Relaxed);
        if cur_gen != self.initial_generation {
            self.status_msg = Some("设备连接已改变，禁止提交".to_string());
            self.is_error = true;
            cx.notify();
            return;
        }

        self.pending_value = Some(val);
        let request_id = crate::ddc::next_ddc_revision();
        self.pending_request_id = Some(request_id);
        self.status_msg = Some("提交中，等待设备确认...".to_string());
        self.is_error = false;

        if bus.cmd_sender.send(AppCommand::TrayValueCommit {
            param: self.param,
            value: val as i32,
            generation: self.initial_generation,
            request_id,
        }).is_err() {
            self.pending_value = None;
            self.pending_request_id = None;
            self.status_msg = Some("提交失败：后台已停止".into());
            self.is_error = true;
        }
        cx.notify();
    }

    /// 触发参数异步刷新
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.is_refreshing = true;
        self.status_msg = Some("正在读取参数...".to_string());
        self.is_error = false;
        self.send_fetch_command(cx);
        cx.notify();
    }

    /// 向后台消息总线分发读取命令
    pub fn send_fetch_command(&mut self, cx: &mut Context<Self>) {
        let (bus, cur_gen, rev, connected) = {
            let st = self.app_state.read(cx);
            (
                st.bus.clone(),
                st.bus.active_generation.load(Ordering::Relaxed),
                st.light_state.last_revision,
                st.adb_state.is_connected(),
            )
        };
        if cur_gen != self.initial_generation || !connected {
            return;
        }
        match self.param {
            AdjustParam::AtmosphereIllumination => {
                let _ = bus.cmd_sender.send(AppCommand::FetchScreenLight {
                    generation: self.initial_generation,
                    revision: rev,
                });
            }
            _ => {
                let _ = bus.cmd_sender.send(AppCommand::FetchPictureParameters {
                    generation: self.initial_generation,
                });
            }
        }
    }
}

impl Render for TrayValuePopupView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (is_connected, model) = {
            let st = self.app_state.read(cx);
            (st.adb_state.is_connected(), st.adb_model.clone())
        };

        let xiaomi_model_display = if model.trim().is_empty() {
            if is_connected {
                "小米显示器 (已连接)".to_string()
            } else {
                "小米显示器 (未连接)".to_string()
            }
        } else {
            model.clone()
        };

        let status =
            Self::read_param_state(&self.app_state.read(cx), self.param, self.initial_generation, cx);
        let confirmed_val = status.confirmed_value;

        let unit = match self.param {
            AdjustParam::Backlight => "%",
            _ => "",
        };

        let val_display_str = if let Some(pending) = self.pending_value {
            format!("目标: {pending}{unit} (待确认)")
        } else if let Some(confirmed) = confirmed_val {
            format!("当前: {confirmed}{unit}")
        } else {
            "当前: 未知".to_string()
        };

        div()
            .id("tray_value_popup_root")
            .key_context("TrayValuePopup")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if this.is_closing {
                        return;
                    }
                    this.is_closing = true;
                    if let Some(ref cb) = this.on_close {
                        cb(window, cx);
                    } else {
                        window.remove_window();
                    }
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .p_3()
            .bg(rgb(ThemeColors::BG_ROOT()))
            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
            .border_1()
            .border_color(rgb(ThemeColors::BORDER()))
            .rounded_xl()
            .shadow_lg()
            .gap_2()
            // 顶部标题栏
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .pb_2()
                    .border_b_1()
                    .border_color(rgb(ThemeColors::BORDER()))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                            .child(self.param.label()),
                    )
                    .child(badge(
                        if is_connected {
                            "已连接"
                        } else {
                            "未连接"
                        },
                        if is_connected {
                            ThemeColors::STATUS_SUCCESS()
                        } else {
                            ThemeColors::BG_DISABLED()
                        },
                        if is_connected {
                            ThemeColors::TEXT_ON_ACCENT()
                        } else {
                            ThemeColors::TEXT_MUTED()
                        },
                    )),
            )
            // 可滚动/自适应内容区域
            .child(
                div()
                    .id("tray_value_popup_scroll_area")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        card_container()
                            .p_3()
                            .gap_2()
                            // 设备型号行
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                            .child(xiaomi_model_display),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                            .child("小米显示器"),
                                    ),
                            )
                            // 范围与真实当前值
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .items_center()
                                    .text_xs()
                                    .child(
                                        div()
                                            .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                            .child(format!(
                                                "范围: {} .. {}{}",
                                                self.param.min(),
                                                self.param.max(),
                                                unit
                                            )),
                                    )
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(if self.pending_value.is_some() {
                                                ThemeColors::STATUS_WARNING()
                                            } else {
                                                ThemeColors::TEXT_PRIMARY()
                                            }))
                                            .child(val_display_str),
                                    ),
                            )
                            // 独立 ContinuousSlider
                            .child(self.slider.clone())
                            // 执行状态行
                            .children(if let Some(msg) = &self.status_msg {
                                Some(
                                    div()
                                        .text_xs()
                                        .p_1()
                                        .rounded_sm()
                                        .text_color(rgb(if self.is_error {
                                            ThemeColors::STATUS_ERROR()
                                        } else if self.pending_value.is_some() {
                                            ThemeColors::STATUS_WARNING()
                                        } else {
                                            ThemeColors::TEXT_SECONDARY()
                                        }))
                                        .child(msg.clone()),
                                )
                            } else {
                                None
                            }),
                    ),
            )
            // 底部操作栏: 刷新 / 关闭 / 打开工具箱
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(ThemeColors::BORDER()))
                    .child(
                        div()
                            .id("btn_tray_value_refresh")
                            .child(action_button(
                                if self.is_refreshing {
                                    "刷新中..."
                                } else {
                                    "刷新"
                                },
                                false,
                                self.is_refreshing || !is_connected,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh(cx);
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                div()
                                    .id("btn_tray_value_close")
                                    .child(action_button("关闭", false, false))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if this.is_closing {
                                            return;
                                        }
                                        this.is_closing = true;
                                        if let Some(ref cb) = this.on_close {
                                            cb(window, cx);
                                        } else {
                                            window.remove_window();
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .id("btn_tray_value_open_toolbox")
                                    .child(action_button("打开工具箱", true, false))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if this.is_closing {
                                            return;
                                        }
                                        this.is_closing = true;
                                        if let Some(ref cb) = this.on_open_toolbox {
                                            cb(window, cx);
                                        }
                                    })),
                            ),
                    ),
            )
    }
}
