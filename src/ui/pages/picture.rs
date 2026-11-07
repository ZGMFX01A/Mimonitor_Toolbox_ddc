use gpui::prelude::*;
use gpui::{div, rgb, Context, Div, Entity, IntoElement, Render, WeakEntity, Window};
use std::sync::Arc;

use crate::adb::AdbDeviceState;
use crate::device::picture::{get_picture_scene_name, PictureParametersState, PictureSliderParam};
use crate::message::{AppCommand, MessageBus};
use crate::ui::components::{
    action_button, badge, card_container, option_button, ContinuousSlider,
};
use crate::ui::theme::ThemeColors;

/// 单个画质连续滑块状态封装
pub struct SliderParamState {
    pub param: PictureSliderParam,
    pub label: &'static str,
    pub confirmed: Option<u32>,
    pub pending: Option<u32>,
    pub revision: u64,
    pub awaiting_confirm: bool,
    pub entity: Entity<ContinuousSlider>,
}

/// 画面设置页面持久实体
pub struct PicturePage {
    pub advanced: Entity<super::advanced_picture::AdvancedPicture>,
    pub bus: Arc<MessageBus>,
    pub generation: u64,
    pub is_connected: bool,
    pub is_fetching: bool,
    pub status_message: Option<String>,
    pub is_error: bool,

    // 画面模式预设
    pub confirmed_mode: Option<i32>,
    pub pending_mode: Option<i32>,
    pub mode_revision: u64,
    pub mode_awaiting_confirm: bool,

    // 自动光感开关
    pub confirmed_light_sensor: Option<bool>,
    pub pending_light_sensor: Option<bool>,
    pub light_sensor_revision: u64,
    pub light_sensor_awaiting_confirm: bool,

    // 精密控光 (0/1/2/3)
    pub confirmed_local_dimming: Option<u32>,
    pub pending_local_dimming: Option<u32>,
    pub local_dimming_revision: u64,
    pub local_dimming_awaiting_confirm: bool,

    // 6 个核心画质滑块（持久持有实体，杜绝每次 render 重建）
    pub backlight: SliderParamState,
    pub black_level: SliderParamState,
    pub contrast: SliderParamState,
    pub saturation: SliderParamState,
    pub hue: SliderParamState,
    pub sharpness: SliderParamState,
}

impl PicturePage {
    pub fn new(bus: Arc<MessageBus>, cx: &mut Context<Self>) -> Self {
        let advanced = cx.new(|cx| super::advanced_picture::AdvancedPicture::new(bus.clone(), cx));
        let page_weak = cx.entity().downgrade();

        let make_slider = |param: PictureSliderParam,
                           label: &'static str,
                           min: u32,
                           max: u32,
                           step: u32,
                           unit: &'static str,
                           page_weak: WeakEntity<Self>,
                           cx: &mut Context<Self>|
         -> SliderParamState {
            let entity = cx.new(|_| {
                ContinuousSlider::new_param(
                    label,
                    None,
                    min,
                    max,
                    step,
                    unit,
                    true,
                    Some("ADB 离线 / 未连接".to_string()),
                    move |val, _window, cx| {
                        if let Some(page_h) = page_weak.upgrade() {
                            page_h.update(cx, |this, cx| {
                                this.on_slider_user_change(param, val, cx);
                            });
                        }
                    },
                )
            });

            SliderParamState {
                param,
                label,
                confirmed: None,
                pending: None,
                revision: 0,
                awaiting_confirm: false,
                entity,
            }
        };

        let backlight = make_slider(
            PictureSliderParam::Backlight,
            "背光",
            1,
            100,
            1,
            "%",
            page_weak.clone(),
            cx,
        );
        let black_level = make_slider(
            PictureSliderParam::BlackLevel,
            "黑色级别",
            0,
            100,
            1,
            "",
            page_weak.clone(),
            cx,
        );
        let contrast = make_slider(
            PictureSliderParam::Contrast,
            "对比度",
            0,
            100,
            1,
            "",
            page_weak.clone(),
            cx,
        );
        let saturation = make_slider(
            PictureSliderParam::Saturation,
            "饱和度",
            0,
            100,
            1,
            "",
            page_weak.clone(),
            cx,
        );
        let hue = make_slider(
            PictureSliderParam::Hue,
            "色调",
            0,
            100,
            1,
            "",
            page_weak.clone(),
            cx,
        );
        let sharpness = make_slider(
            PictureSliderParam::Sharpness,
            "锐度",
            0,
            100,
            1,
            "",
            page_weak,
            cx,
        );

        Self {
            advanced,
            bus,
            generation: 0,
            is_connected: false,
            is_fetching: false,
            status_message: None,
            is_error: false,

            confirmed_mode: None,
            pending_mode: None,
            mode_revision: 0,
            mode_awaiting_confirm: false,

            confirmed_light_sensor: None,
            pending_light_sensor: None,
            light_sensor_revision: 0,
            light_sensor_awaiting_confirm: false,

            confirmed_local_dimming: None,
            pending_local_dimming: None,
            local_dimming_revision: 0,
            local_dimming_awaiting_confirm: false,

            backlight,
            black_level,
            contrast,
            saturation,
            hue,
            sharpness,
        }
    }

    /// 主动触发后台异步读取硬件画面参数
    pub fn fetch_parameters(&mut self, cx: &mut Context<Self>) {
        if !self.is_connected {
            return;
        }
        self.is_fetching = true;
        self.status_message = Some("正在读取硬件画面参数...".to_string());
        self.is_error = false;

        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::FetchPictureParameters {
                generation: self.generation,
            });
        cx.notify();
    }

    /// 用户触发画面模式切换
    pub fn set_mode(&mut self, mode: i32, cx: &mut Context<Self>) {
        if !self.is_connected {
            return;
        }
        self.mode_revision += 1;
        let rev = self.mode_revision;
        self.pending_mode = Some(mode);
        self.mode_awaiting_confirm = false;
        self.status_message = None;
        self.is_error = false;

        let _ = self.bus.cmd_sender.send(AppCommand::SetPictureMode {
            mode,
            generation: self.generation,
            revision: rev,
        });
        cx.notify();
    }

    /// 用户触发自动光感切换
    pub fn set_light_sensor(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !self.is_connected {
            return;
        }
        self.light_sensor_revision += 1;
        let rev = self.light_sensor_revision;
        self.pending_light_sensor = Some(enabled);
        self.light_sensor_awaiting_confirm = false;
        self.status_message = None;
        self.is_error = false;

        let _ = self.bus.cmd_sender.send(AppCommand::SetLightSensor {
            enabled,
            generation: self.generation,
            revision: rev,
        });
        cx.notify();
    }

    /// 用户触发精密控光档位切换
    pub fn set_local_dimming(&mut self, value: u32, cx: &mut Context<Self>) {
        if !self.is_connected {
            return;
        }
        self.local_dimming_revision += 1;
        let rev = self.local_dimming_revision;
        self.pending_local_dimming = Some(value);
        self.local_dimming_awaiting_confirm = false;
        self.status_message = None;
        self.is_error = false;

        let _ = self.bus.cmd_sender.send(AppCommand::SetLocalDimming {
            value,
            generation: self.generation,
            revision: rev,
        });
        cx.notify();
    }

    /// 用户拖拽或点击滑块事件
    pub fn on_slider_user_change(
        &mut self,
        param: PictureSliderParam,
        val: u32,
        cx: &mut Context<Self>,
    ) {
        if !self.is_connected {
            return;
        }
        let slider = self.get_slider_mut(param);
        slider.revision += 1;
        let rev = slider.revision;
        slider.pending = Some(val);
        slider.awaiting_confirm = false;

        self.status_message = None;
        self.is_error = false;

        let _ = self.bus.cmd_sender.send(AppCommand::SetPictureSlider {
            param,
            value: val,
            generation: self.generation,
            revision: rev,
        });
        cx.notify();
    }

    /// ADB 状态同步事件
    pub fn on_adb_state_changed(
        &mut self,
        state: &AdbDeviceState,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let was_connected = self.is_connected;
        let gen_changed = self.generation != generation;
        self.is_connected = state.is_connected();
        self.generation = generation;

        let (disabled, reason) = if !self.is_connected {
            (true, Some("ADB 离线 / 未连接".to_string()))
        } else {
            (false, None)
        };

        self.backlight
            .entity
            .update(cx, |s, cx| s.set_disabled(disabled, reason.clone(), cx));
        self.black_level
            .entity
            .update(cx, |s, cx| s.set_disabled(disabled, reason.clone(), cx));
        self.contrast
            .entity
            .update(cx, |s, cx| s.set_disabled(disabled, reason.clone(), cx));
        self.saturation
            .entity
            .update(cx, |s, cx| s.set_disabled(disabled, reason.clone(), cx));
        self.hue
            .entity
            .update(cx, |s, cx| s.set_disabled(disabled, reason.clone(), cx));
        self.sharpness
            .entity
            .update(cx, |s, cx| s.set_disabled(disabled, reason, cx));

        if !self.is_connected {
            self.clear_all_pending(cx);
            self.confirmed_mode = None;
            self.confirmed_light_sensor = None;
            self.confirmed_local_dimming = None;

            self.backlight.confirmed = None;
            self.backlight
                .entity
                .update(cx, |s, cx| s.set_value_opt(None, cx));
            self.black_level.confirmed = None;
            self.black_level
                .entity
                .update(cx, |s, cx| s.set_value_opt(None, cx));
            self.contrast.confirmed = None;
            self.contrast
                .entity
                .update(cx, |s, cx| s.set_value_opt(None, cx));
            self.saturation.confirmed = None;
            self.saturation
                .entity
                .update(cx, |s, cx| s.set_value_opt(None, cx));
            self.hue.confirmed = None;
            self.hue
                .entity
                .update(cx, |s, cx| s.set_value_opt(None, cx));
            self.sharpness.confirmed = None;
            self.sharpness
                .entity
                .update(cx, |s, cx| s.set_value_opt(None, cx));

            self.is_fetching = false;
            self.status_message = None;
            self.is_error = false;
        } else if !was_connected || gen_changed {
            // 连接建立后主动读取硬件参数
            self.fetch_parameters(cx);
        }
        cx.notify();
    }

    /// 后台整页读取成功事件 (严格遵循：旧整页读取绝不覆盖用户 pending 字段；匹配实际值才确认)
    pub fn on_parameters_fetched(
        &mut self,
        state: PictureParametersState,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        self.advanced
            .update(cx, |view, cx| view.update_state(state.clone(), cx));
        self.is_fetching = false;

        // 1. 画面模式确认逻辑
        if let Some(target) = self.pending_mode {
            if state.mode == Some(target) {
                self.confirmed_mode = state.mode;
                self.pending_mode = None;
                self.mode_awaiting_confirm = false;
            }
        } else {
            self.confirmed_mode = state.mode;
        }

        // 2. 自动光感确认逻辑
        if let Some(target) = self.pending_light_sensor {
            if state.light_sensor == Some(target) {
                self.confirmed_light_sensor = state.light_sensor;
                self.pending_light_sensor = None;
                self.light_sensor_awaiting_confirm = false;
            }
        } else {
            self.confirmed_light_sensor = state.light_sensor;
        }

        // 3. 精密控光确认逻辑
        if let Some(target) = self.pending_local_dimming {
            if state.local_dimming == Some(target) {
                self.confirmed_local_dimming = state.local_dimming;
                self.pending_local_dimming = None;
                self.local_dimming_awaiting_confirm = false;
            }
        } else {
            self.confirmed_local_dimming = state.local_dimming;
        }

        // 4. 6 个滑块确认逻辑
        Self::sync_slider_param(&mut self.backlight, state.backlight, cx);
        Self::sync_slider_param(&mut self.black_level, state.black_level, cx);
        Self::sync_slider_param(&mut self.contrast, state.contrast, cx);
        Self::sync_slider_param(&mut self.saturation, state.saturation, cx);
        Self::sync_slider_param(&mut self.hue, state.hue, cx);
        Self::sync_slider_param(&mut self.sharpness, state.sharpness, cx);

        // 5. 状态提示
        let has_pending = self.pending_mode.is_some()
            || self.pending_light_sensor.is_some()
            || self.pending_local_dimming.is_some()
            || self.backlight.pending.is_some()
            || self.black_level.pending.is_some()
            || self.contrast.pending.is_some()
            || self.saturation.pending.is_some()
            || self.hue.pending.is_some()
            || self.sharpness.pending.is_some();

        if has_pending {
            self.status_message = Some("部分参数仍在待确认中 (回读值尚未匹配)".to_string());
        } else {
            self.status_message = Some("参数读取完成".to_string());
        }
        self.is_error = false;
        cx.notify();
    }

    /// 后台读取失败
    pub fn on_fetch_failed(&mut self, message: String, generation: u64, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        self.is_fetching = false;
        self.status_message = Some(message);
        self.is_error = true;
        cx.notify();
    }

    /// 收到 PictureModeUpdated (命令执行完毕，仅作为确认触发，非真实回读)
    pub fn on_mode_updated(
        &mut self,
        _mode: i32,
        generation: u64,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation || revision < self.mode_revision {
            return;
        }
        self.mode_awaiting_confirm = true;
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::FetchPictureParameters {
                generation: self.generation,
            });
        cx.notify();
    }

    /// 收到 LightSensorUpdated
    pub fn on_light_sensor_updated(
        &mut self,
        _enabled: bool,
        generation: u64,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation || revision < self.light_sensor_revision {
            return;
        }
        self.light_sensor_awaiting_confirm = true;
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::FetchPictureParameters {
                generation: self.generation,
            });
        cx.notify();
    }

    /// 收到 LocalDimmingUpdated
    pub fn on_local_dimming_updated(
        &mut self,
        _value: u32,
        generation: u64,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation || revision < self.local_dimming_revision {
            return;
        }
        self.local_dimming_awaiting_confirm = true;
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::FetchPictureParameters {
                generation: self.generation,
            });
        cx.notify();
    }

    /// 收到 PictureSliderUpdated
    pub fn on_slider_updated(
        &mut self,
        param: PictureSliderParam,
        _value: u32,
        generation: u64,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        let slider = self.get_slider_mut(param);
        if revision < slider.revision {
            return;
        }
        slider.awaiting_confirm = true;
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::FetchPictureParameters {
                generation: self.generation,
            });
        cx.notify();
    }

    /// 外部背光更新通知同步
    pub fn on_backlight_updated(
        &mut self,
        value: Option<u32>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        if let Some(target) = self.backlight.pending {
            if value == Some(target) {
                self.backlight.confirmed = value;
                self.backlight.pending = None;
                self.backlight.awaiting_confirm = false;
                self.backlight
                    .entity
                    .update(cx, |s, cx| s.set_value_opt(value, cx));
            }
        } else {
            self.backlight.confirmed = value;
            self.backlight
                .entity
                .update(cx, |s, cx| s.set_value_opt(value, cx));
        }
        cx.notify();
    }

    /// ADB 操作失败 (清除 pending 并显示原因，拒绝假绿)
    pub fn on_adb_operation_failed(
        &mut self,
        message: &str,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        self.clear_all_pending(cx);
        self.status_message = Some(format!("操作失败: {}", message));
        self.is_error = true;
        self.is_fetching = false;
        cx.notify();
    }

    /// 清空所有未确认写状态并将滑块实体复位为真实已确认值
    fn clear_all_pending(&mut self, cx: &mut Context<Self>) {
        self.pending_mode = None;
        self.mode_awaiting_confirm = false;

        self.pending_light_sensor = None;
        self.light_sensor_awaiting_confirm = false;

        self.pending_local_dimming = None;
        self.local_dimming_awaiting_confirm = false;

        Self::reset_slider_pending(&mut self.backlight, cx);
        Self::reset_slider_pending(&mut self.black_level, cx);
        Self::reset_slider_pending(&mut self.contrast, cx);
        Self::reset_slider_pending(&mut self.saturation, cx);
        Self::reset_slider_pending(&mut self.hue, cx);
        Self::reset_slider_pending(&mut self.sharpness, cx);
    }

    fn reset_slider_pending(slider: &mut SliderParamState, cx: &mut Context<Self>) {
        slider.pending = None;
        slider.awaiting_confirm = false;
        let confirmed = slider.confirmed;
        slider
            .entity
            .update(cx, |s, cx| s.set_value_opt(confirmed, cx));
    }

    fn sync_slider_param(
        slider: &mut SliderParamState,
        read_val: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = slider.pending {
            if read_val == Some(target) {
                slider.confirmed = read_val;
                slider.pending = None;
                slider.awaiting_confirm = false;
                slider
                    .entity
                    .update(cx, |s, cx| s.set_value_opt(read_val, cx));
            }
        } else {
            slider.confirmed = read_val;
            slider
                .entity
                .update(cx, |s, cx| s.set_value_opt(read_val, cx));
        }
    }

    fn get_slider_mut(&mut self, param: PictureSliderParam) -> &mut SliderParamState {
        match param {
            PictureSliderParam::Backlight => &mut self.backlight,
            PictureSliderParam::BlackLevel => &mut self.black_level,
            PictureSliderParam::Contrast => &mut self.contrast,
            PictureSliderParam::Saturation => &mut self.saturation,
            PictureSliderParam::Hue => &mut self.hue,
            PictureSliderParam::Sharpness => &mut self.sharpness,
            _ => &mut self.backlight,
        }
    }
}

impl Render for PicturePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_connected = self.is_connected;
        let is_fetching = self.is_fetching;

        // 真实场景名展示逻辑
        let scene_text = match self.confirmed_mode {
            Some(m) => get_picture_scene_name(m),
            None => "未知场景",
        };
        let is_scene_known = self.confirmed_mode.is_some();

        // 渲染顶部标题与刷新工具栏
        let header = div()
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
                            .child("画面设置"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("核心画质硬件寄存器与 Android Settings 调优"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .children(if let Some(msg) = &self.status_message {
                        Some(badge(
                            msg.clone(),
                            if self.is_error {
                                ThemeColors::STATUS_ERROR_BG()
                            } else {
                                ThemeColors::STATUS_SUCCESS_BG()
                            },
                            if self.is_error {
                                ThemeColors::STATUS_ERROR()
                            } else {
                                ThemeColors::STATUS_SUCCESS()
                            },
                        ))
                    } else {
                        None
                    })
                    .children(if is_fetching {
                        Some(badge(
                            "正在同步硬件...",
                            ThemeColors::BG_BUTTON(),
                            ThemeColors::ACCENT_CYAN(),
                        ))
                    } else {
                        None
                    })
                    .child(
                        div()
                            .id("btn_refresh_picture_parameters")
                            .child(action_button(
                                "刷新参数",
                                false,
                                !is_connected || is_fetching,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.fetch_parameters(cx);
                            })),
                    ),
            );

        // 1. 真实硬件场景状态卡片
        let scene_card = card_container().child(
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
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("当前硬件场景:"),
                        )
                        .child(badge(
                            scene_text,
                            if is_scene_known {
                                ThemeColors::ACCENT_BG_SUBTLE()
                            } else {
                                ThemeColors::BG_BUTTON()
                            },
                            if is_scene_known {
                                ThemeColors::ACCENT_CYAN()
                            } else {
                                ThemeColors::TEXT_MUTED()
                            },
                        ))
                        .children(if let Some(target_mode) = self.pending_mode {
                            Some(badge(
                                format!(
                                    "待确认: {}{}",
                                    get_picture_scene_name(target_mode),
                                    if self.mode_awaiting_confirm {
                                        " (等待回读匹配)"
                                    } else {
                                        " (命令在途)"
                                    }
                                ),
                                ThemeColors::STATUS_WARNING_BG(),
                                ThemeColors::STATUS_WARNING(),
                            ))
                        } else {
                            None
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(ThemeColors::TEXT_MUTED()))
                        .child(if is_connected {
                            "设置已从显示器读取"
                        } else {
                            "设备未连接 · 页面控件已禁用"
                        }),
                ),
        );

        // 2. 画面模式三组按钮 (标准 14、游戏 10、电影 9)
        let active_mode_display = self.pending_mode.or(self.confirmed_mode);
        let mode_buttons = [
            (14, "标准模式 (14)"),
            (10, "游戏模式 (10)"),
            (9, "电影模式 (9)"),
        ];

        let mode_card = card_container()
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
                            .child("基础画面模式预设"),
                    )
                    .children(if self.pending_mode.is_some() {
                        Some(badge(
                            "切换待确认",
                            ThemeColors::STATUS_WARNING_BG(),
                            ThemeColors::STATUS_WARNING(),
                        ))
                    } else {
                        None
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .children(mode_buttons.into_iter().map(|(mode_val, label_str)| {
                        let is_selected = active_mode_display == Some(mode_val);
                        let is_pending_this = self.pending_mode == Some(mode_val);
                        let full_label = if is_pending_this {
                            format!("{} [待确认]", label_str)
                        } else {
                            label_str.to_string()
                        };

                        div()
                            .id(("btn_picture_mode", mode_val as usize))
                            .child(option_button(full_label, is_selected, !is_connected))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_mode(mode_val, cx);
                            }))
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(ThemeColors::TEXT_MUTED()))
                    .child(if let Some(m) = self.confirmed_mode {
                        if !matches!(m, 14 | 10 | 9) {
                            format!(
                                "提示：当前显示器处于扩展模式 [{}]，未激活标准三档预设。",
                                get_picture_scene_name(m)
                            )
                        } else {
                            "支持在日常办公标准、低延迟游戏与高动态电影预设间直接切换。".to_string()
                        }
                    } else {
                        "模式状态未知，请先刷新或连接设备。".to_string()
                    }),
            );

        // 3. 自动光感与精密控光卡片
        let cur_light_sensor = self.pending_light_sensor.or(self.confirmed_light_sensor);
        let cur_local_dimming = self.pending_local_dimming.or(self.confirmed_local_dimming);

        let dimming_levels = [(0, "0 关"), (1, "1 低"), (2, "2 中"), (3, "3 高")];

        let dimming_and_sensor_card = card_container()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                    .child("自动光感与 MiniLED 分区控光"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    // 自动光感行
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
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child("自动光感 (环境光自适应)"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                            .child(match cur_light_sensor {
                                                Some(true) => "当前状态: 已开启",
                                                Some(false) => "当前状态: 已关闭",
                                                None => "当前状态: 未知",
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .children(if self.pending_light_sensor.is_some() {
                                        Some(badge(
                                            "待确认",
                                            ThemeColors::STATUS_WARNING_BG(),
                                            ThemeColors::STATUS_WARNING(),
                                        ))
                                    } else {
                                        None
                                    })
                                    .child(
                                        div()
                                            .id("btn_picture_light_sensor")
                                            .child(option_button(
                                                match cur_light_sensor {
                                                    Some(true) => "光感: 开启",
                                                    Some(false) => "光感: 关闭",
                                                    None => "光感: 未知 (点击切换)",
                                                },
                                                cur_light_sensor == Some(true),
                                                !is_connected,
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let cur = this
                                                    .pending_light_sensor
                                                    .or(this.confirmed_light_sensor)
                                                    .unwrap_or(false);
                                                this.set_light_sensor(!cur, cx);
                                            })),
                                    ),
                            ),
                    )
                    // 精密控光 (0/1/2/3) 行
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
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
                                            .child("精密控光 (Local Dimming 分区背光)"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_2()
                                            .children(if self.pending_local_dimming.is_some() {
                                                Some(badge(
                                                    "控光待确认",
                                                    ThemeColors::STATUS_WARNING_BG(),
                                                    ThemeColors::STATUS_WARNING(),
                                                ))
                                            } else {
                                                None
                                            })
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                                    .child(match cur_local_dimming {
                                                        Some(v) => format!("当前挡位: {} 档", v),
                                                        None => "当前挡位: 未知".to_string(),
                                                    }),
                                            ),
                                    ),
                            )
                            .child(div().flex().flex_row().gap_2().children(
                                dimming_levels.into_iter().map(|(lvl, lvl_label)| {
                                    let is_selected = cur_local_dimming == Some(lvl);
                                    let is_pending_this = self.pending_local_dimming == Some(lvl);
                                    let btn_label = if is_pending_this {
                                        format!("{} [待确认]", lvl_label)
                                    } else {
                                        lvl_label.to_string()
                                    };

                                    div()
                                        .id(("btn_picture_local_dimming", lvl as usize))
                                        .child(option_button(btn_label, is_selected, !is_connected))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_local_dimming(lvl, cx);
                                        }))
                                }),
                            )),
                    ),
            );

        // 4. 6 个核心滑块卡片 (背光 1..100, 黑色级别/对比度/饱和度/色调/锐度 0..100)
        let render_slider_row = |slider: &SliderParamState| -> Div {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(if slider.pending.is_some() {
                    Some(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .px_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                    .child(format!("{} 参数写操作已发出", slider.label)),
                            )
                            .child(badge(
                                if slider.awaiting_confirm {
                                    "已下发 · 待硬件回读匹配"
                                } else {
                                    "命令在途 · 待确认"
                                },
                                ThemeColors::STATUS_WARNING_BG(),
                                ThemeColors::STATUS_WARNING(),
                            )),
                    )
                } else {
                    None
                })
                .child(slider.entity.clone())
        };

        let sliders_card = card_container()
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
                            .child("基础画面滑块调节"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_MUTED()))
                            .child("背光范围 1..100，其余画质参数范围 0..100"),
                    ),
            )
            .child(render_slider_row(&self.backlight))
            .child(render_slider_row(&self.black_level))
            .child(render_slider_row(&self.contrast))
            .child(render_slider_row(&self.saturation))
            .child(render_slider_row(&self.hue))
            .child(render_slider_row(&self.sharpness));

        // 5. 高级画质阶段说明（明确声明高级控件为另一阶段，拒绝虚假完整）
        // 主容器
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(header)
            .child(scene_card)
            .child(mode_card)
            .child(dimming_and_sensor_card)
            .child(sliders_card)
            .child(self.advanced.clone())
    }
}

pub fn render_picture_page(picture_page: Entity<PicturePage>) -> Div {
    div().w_full().child(picture_page)
}
