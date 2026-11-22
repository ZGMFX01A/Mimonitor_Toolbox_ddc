use crate::message::AppCommand;
use crate::ui::components::{action_button, badge, card_container, ContinuousSlider};
use crate::ui::theme::ThemeColors;
use crate::AppState;
use gpui::prelude::*;
use gpui::{
    div, rgb, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render, Subscription, Window,
};
use std::collections::HashMap;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy)]
struct BacklightSubmission {
    generation: u64,
    request_id: u64,
    target: u32,
}

fn backlight_display_value(
    pending: &mut Option<BacklightSubmission>,
    generation: u64,
    connected: bool,
    confirmed: Option<u32>,
    results: &std::collections::BTreeMap<u64, (u64, Option<String>)>,
) -> Option<u32> {
    if let Some(write) = pending {
        if !connected || write.generation != generation
            || results.get(&write.request_id).is_some_and(|(epoch, _)| *epoch == generation)
        {
            *pending = None;
        }
    }
    pending.as_ref().map(|write| write.target).or(confirmed)
}

/// 多显示器统一亮度悬浮面板托盘弹窗视图 (严格零同步 IO，由全局共享 AppState 订阅驱动)
pub struct TrayBrightnessPopupView {
    pub app_state: Entity<AppState>,
    _subscription: Subscription,
    pub xiaomi_slider: Entity<ContinuousSlider>,
    pub ddc_sliders: HashMap<String, Entity<ContinuousSlider>>,
    pub is_refreshing: bool,
    seen_refresh_count: u64,
    pub is_closing: bool,
    pub status_msg: Option<String>,
    pub on_open_toolbox: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
    pub on_close: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
    pub focus_handle: FocusHandle,
    pending_backlight: Option<BacklightSubmission>,
}

impl TrayBrightnessPopupView {
    pub fn new(
        app_state: Entity<AppState>,
        cx: &mut Context<Self>,
        on_open_toolbox: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
        on_close: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let weak_view = cx.entity().downgrade();
        let xiaomi_slider = cx.new(|_| {
            ContinuousSlider::new(
                0,
                true,
                Some("ADB 离线 / 未连接".into()),
                move |val, _, cx| {
                    let _ = weak_view.update(cx, |view, cx| view.on_backlight_change(val, cx));
                },
            )
        });

        // 订阅 AppState 变更，实现与主窗口和后台事件的强一致性响应
        let _subscription = cx.observe(&app_state, |this, state, cx| {
            this.sync_from_app_state(&state, cx);
        });

        let mut view = Self {
            app_state: app_state.clone(),
            _subscription,
            xiaomi_slider,
            ddc_sliders: HashMap::new(),
            is_refreshing: false,
            seen_refresh_count: 0,
            is_closing: false,
            status_msg: None,
            on_open_toolbox,
            on_close,
            focus_handle,
            pending_backlight: None,
        };

        // 初始同步共享状态
        view.sync_from_app_state(&app_state, cx);

        // 构造时不执行同步 IO，向后台总线发送异步刷新命令
        view.refresh_monitors(cx);

        view
    }

    fn on_backlight_change(&mut self, value: u32, cx: &mut Context<Self>) {
        let state = self.app_state.read(cx);
        if !state.adb_state.is_connected() { return; }
        let bus = state.bus.clone();
        let generation = bus.active_generation.load(Ordering::Acquire);
        if self.pending_backlight.is_some_and(|write| write.generation == generation && write.target == value)
            || (self.pending_backlight.is_none() && state.adb_backlight == Some(value)) {
            return;
        }
        let request_id = crate::ddc::next_ddc_revision();
        self.pending_backlight = Some(BacklightSubmission { generation, request_id, target: value });
        if bus.cmd_sender.send(AppCommand::TrayValueCommit {
            param: crate::hotkey::AdjustParam::Backlight,
            value: value as i32,
            generation,
            request_id,
        }).is_err() {
            self.pending_backlight = None;
            self.status_msg = Some("亮度提交失败：后台已停止".into());
            let state = self.app_state.clone();
            let view = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = view.update(cx, |this, cx| this.sync_from_app_state(&state, cx));
            });
        }
        cx.notify();
    }

    /// 从共享 AppState 同步所有显示器状态与滑块
    pub fn sync_from_app_state(&mut self, state: &Entity<AppState>, cx: &mut Context<Self>) {
        let count = state.read(cx).ddc_refresh_count;
        if count > self.seen_refresh_count {
            self.seen_refresh_count = count;
            self.is_refreshing = false;
            self.status_msg = None;
        }
        let (connected, backlight, monitors, bus) = {
            let st = state.read(cx);
            let generation = st.bus.active_generation.load(Ordering::Acquire);
            if let Some(write) = self.pending_backlight {
                if let Some((epoch, error)) = st.tray_value_results.get(&write.request_id) {
                    if *epoch == generation {
                        self.status_msg = error.as_ref().map(|error| format!("亮度设置失败：{error}"));
                    }
                }
            }
            let backlight = backlight_display_value(&mut self.pending_backlight,
                generation, st.adb_state.is_connected(), st.adb_backlight, &st.tray_value_results);
            (
                st.adb_state.is_connected(),
                backlight,
                st.ddc_monitors.clone(),
                st.bus.clone(),
            )
        };

        let (disabled, reason) = if !connected {
            (true, Some("ADB 离线 / 未连接".to_string()))
        } else if backlight.is_none() {
            (true, Some("正在读取硬件亮度...".to_string()))
        } else {
            (false, None)
        };

        self.xiaomi_slider.update(cx, |s, cx| {
            s.label = if self.pending_backlight.is_some() { "背光（设置中）" } else { "背光" }.into();
            s.set_disabled(disabled, reason, cx);
            s.set_value_opt(backlight, cx);
        });

        for mon in &monitors {
            let dev_id = mon.device_id.clone();
            let is_supp = mon.is_supported;
            let reason = if !is_supp {
                Some("不支持 DDC/CI 或多屏映射歧义".to_string())
            } else {
                None
            };

            if let Some(slider) = self.ddc_sliders.get(&mon.device_id) {
                let coalescer = crate::ddc::get_ddc_device_coalescer(&mon.device_id);
                let cur_rev = coalescer.last_revision.load(Ordering::Relaxed);
                if mon.write_revision >= cur_rev
                    || !coalescer.should_suppress_read_update(
                        mon.write_revision,
                        std::time::Duration::from_millis(1500),
                    )
                {
                    let pct = mon.current_percent;
                    slider.update(cx, |s, cx| {
                        s.set_disabled(!is_supp, reason, cx);
                        s.set_value_opt(if is_supp { Some(pct) } else { None }, cx);
                    });
                }
            } else {
                let bus_clone = bus.clone();
                let slider = cx.new(|_| {
                    ContinuousSlider::new_param(
                        "亮度",
                        if is_supp {
                            Some(mon.current_percent)
                        } else {
                            None
                        },
                        0,
                        100,
                        1,
                        "%",
                        !is_supp,
                        reason,
                        move |pct, _, _| {
                            let revision = crate::ddc::next_ddc_revision();
                            let _ = bus_clone.cmd_sender.send(AppCommand::SetDdcBrightness {
                                device_id: dev_id.clone(),
                                percent: pct,
                                revision,
                            });
                        },
                    )
                });
                self.ddc_sliders.insert(mon.device_id.clone(), slider);
            }
        }
        cx.notify();
    }

    /// 点击“刷新显示器”
    pub fn refresh_monitors(&mut self, cx: &mut Context<Self>) {
        self.is_refreshing = true;
        self.status_msg = Some("正在后台异步刷新显示器...".to_string());
        let state = self.app_state.read(cx);
        let bus = state.bus.clone();
        let _ = bus.cmd_sender.send(AppCommand::RefreshMonitors);
        if state.adb_state.is_connected() {
            let _ = bus.cmd_sender.send(AppCommand::FetchBacklight {
                generation: bus.active_generation.load(Ordering::Acquire),
            });
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(request_id: u64, target: u32) -> Option<BacklightSubmission> {
        Some(BacklightSubmission { generation: 1, request_id, target })
    }

    #[test]
    fn backlight_keeps_target_during_old_reads_and_unrelated_notifications() {
        let mut pending = write(10, 1);
        let mut results = std::collections::BTreeMap::new();
        assert_eq!(backlight_display_value(&mut pending, 1, true, Some(15), &results), Some(1));
        results.insert(9, (1, None));
        results.insert(8, (1, Some("older command failed".into())));
        assert_eq!(backlight_display_value(&mut pending, 1, true, Some(12), &results), Some(1));
        results.insert(10, (1, None));
        assert_eq!(backlight_display_value(&mut pending, 1, true, Some(1), &results), Some(1));
        assert!(pending.is_none());
        assert_eq!(backlight_display_value(&mut pending, 1, true, Some(25), &results), Some(25));
    }

    #[test]
    fn backlight_rolls_back_only_for_its_own_failure_or_a_session_change() {
        let mut pending = write(11, 30);
        let mut results = std::collections::BTreeMap::new();
        results.insert(11, (0, Some("previous session".into())));
        assert_eq!(backlight_display_value(&mut pending, 1, true, Some(15), &results), Some(30));
        results.insert(11, (1, Some("write failed".into())));
        assert_eq!(backlight_display_value(&mut pending, 1, true, Some(15), &results), Some(15));
        assert!(pending.is_none());
        pending = write(12, 40);
        assert_eq!(backlight_display_value(&mut pending, 2, true, None, &results), None);
        assert!(pending.is_none());
        pending = write(13, 50);
        assert_eq!(backlight_display_value(&mut pending, 1, false, None, &results), None);
        assert!(pending.is_none());
    }
}

impl Render for TrayBrightnessPopupView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (is_connected, model, ddc_monitors) = {
            let st = self.app_state.read(cx);
            (
                st.adb_state.is_connected(),
                st.adb_model.clone(),
                st.ddc_monitors.clone(),
            )
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

        div()
            .id("tray_brightness_popup_root")
            .key_context("TrayBrightnessPopup")
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
                            .child("显示器亮度"),
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
            // 可滚动内容区域（紧凑间距，两台设备一屏可见，同时支持更多屏幕向下滚动）
            .child(
                div()
                    .id("tray_popup_scroll_area")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    // 小米设备亮度卡片
                    .child(
                        card_container()
                            .p_3()
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
                                            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                            .child(xiaomi_model_display),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                            .child("无线直连"),
                                    ),
                            )
                            .child(self.xiaomi_slider.clone()),
                    )
                    // 外接普通 DDC/CI 显示器亮度卡片列表
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                    .child("外接显示器 (DDC/CI)"),
                            )
                            .children(if ddc_monitors.is_empty() {
                                vec![div()
                                    .p_2()
                                    .bg(rgb(ThemeColors::BG_CARD()))
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(ThemeColors::BORDER()))
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                    .child(if self.is_refreshing {
                                        "正在检测外接显示器..."
                                    } else {
                                        "未检测到支持 DDC/CI 的外接显示器"
                                    })
                                    .into_any_element()]
                            } else {
                                ddc_monitors
                                    .iter()
                                    .map(|mon| {
                                        let slider = self.ddc_sliders.get(&mon.device_id).cloned();
                                        let is_supp = mon.is_supported;

                                        card_container()
                                            .p_3()
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
                                                            .text_color(rgb(if is_supp {
                                                                ThemeColors::TEXT_PRIMARY()
                                                            } else {
                                                                ThemeColors::TEXT_DISABLED()
                                                            }))
                                                            .child(mon.label.clone()),
                                                    )
                                                    .child(badge(
                                                        if is_supp {
                                                            "DDC/CI"
                                                        } else {
                                                            "不可控"
                                                        },
                                                        if is_supp {
                                                            ThemeColors::ACCENT_BG_SUBTLE()
                                                        } else {
                                                            ThemeColors::BG_DISABLED()
                                                        },
                                                        if is_supp {
                                                            ThemeColors::ACCENT_CYAN()
                                                        } else {
                                                            ThemeColors::TEXT_DISABLED()
                                                        },
                                                    )),
                                            )
                                            .children(slider)
                                            .into_any_element()
                                    })
                                    .collect()
                            }),
                    ),
            )
            // 底部操作栏: 刷新显示器 / 打开工具箱
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
                            .id("btn_tray_refresh")
                            .child(action_button(
                                if self.is_refreshing {
                                    "刷新中..."
                                } else {
                                    "刷新显示器"
                                },
                                false,
                                self.is_refreshing,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_monitors(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("btn_tray_open_toolbox")
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
            )
    }
}
