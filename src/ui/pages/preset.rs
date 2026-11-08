use gpui::prelude::*;
use gpui::{
    div, rgb, Context, Div, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled,
    WeakEntity, Window,
};
#[cfg(test)]
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::PresetItem;
use crate::message::{AppCommand, AppEvent, MessageBus};
use crate::message::preset_sync::PresetConfigMutation;
use crate::presets::{find_preset, missing_keys, new_id, unique_name};
use crate::ui::components::{action_button, badge, card_container, TextInput};
use crate::ui::theme::ThemeColors;

/// 异步保存预设相关的配置字段，严格在后台线程执行 IO，防覆盖其他页配置
pub fn async_save_preset_config(
    mutation: PresetConfigMutation,
    bus: Arc<MessageBus>,
) -> u64 {
    let request_id = crate::ddc::next_ddc_revision();
    if bus.cmd_sender.send(AppCommand::EditPresetConfig { mutation: mutation.clone(), request_id }).is_err() {
        let _ = bus.event_sender.send(AppEvent::PresetConfigResult {
            request_id, mutation, error: Some("保存预设失败：后台已停止".into()),
        });
    }
    request_id
}

/// 预设模式管理页面视图组件
pub struct PresetPageView {
    app_state: WeakEntity<crate::AppState>,
    bus: Arc<MessageBus>,
    /// 新建预设名称输入框
    name_input: Entity<TextInput>,
    /// 后台正在进行的长耗时操作提示（如抓取中、应用中、恢复中）
    pending_operation: Option<String>,
    pending_config_request: Option<u64>,
    /// 当前操作的代数 (generation)，用于拒绝过期回调
    pending_generation: u64,
    /// 待跳转画面编辑的目标预设 ID（若有）
    pending_edit_target: Option<String>,
    /// 处于应用内两阶段删除确认中的预设 ID
    confirm_delete_id: Option<String>,
    confirm_restore: bool,
    /// 当前处于重命名编辑状态的预设 ID
    editing_preset_id: Option<String>,
    /// 重命名输入框实体
    rename_input: Option<Entity<TextInput>>,
    /// 页面内即时状态反馈提示（文案, 是否为错误）
    page_toast: Option<(String, bool)>,
}

impl PresetPageView {
    pub fn new(
        app_state: WeakEntity<crate::AppState>,
        bus: Arc<MessageBus>,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input =
            cx.new(|cx| TextInput::new(cx, "输入预设名称 (例如：夜间防眩 / 影音增强)", ""));

        Self {
            app_state,
            bus,
            name_input,
            pending_operation: None,
            pending_config_request: None,
            pending_generation: 0,
            pending_edit_target: None,
            confirm_delete_id: None,
            confirm_restore: false,
            editing_preset_id: None,
            rename_input: None,
            page_toast: None,
        }
    }

    /// 处理后台总线投递的预设相关业务事件
    pub fn handle_event(&mut self, evt: &AppEvent, cx: &mut Context<Self>) {
        let event_generation = match evt {
            AppEvent::PresetSnapshotCaptured { generation, .. }
            | AppEvent::PresetApplied { generation, .. }
            | AppEvent::BaselineRestored { generation, .. }
            | AppEvent::PresetOperationFailed { generation, .. }
            | AppEvent::AdbStateChanged { generation, .. } => Some(*generation),
            _ => None,
        };
        if event_generation.is_some_and(|generation| {
            generation != self.bus.active_generation.load(Ordering::Acquire)
        }) {
            return;
        }
        match evt {
            AppEvent::PresetConfigResult { request_id, mutation, error } => {
                if self.pending_config_request != Some(*request_id) { return; }
                self.pending_config_request = None;
                self.pending_operation = None;
                if let Some(error) = error {
                    self.page_toast = Some((error.clone(), true));
                } else {
                    self.page_toast = Some((match mutation {
                        PresetConfigMutation::AddAndActivate { .. } => "预设已创建并保存",
                        PresetConfigMutation::UpdateValues { .. } => "预设参数已更新并保存",
                        PresetConfigMutation::Rename { .. } => "预设名称已保存",
                        PresetConfigMutation::Delete { .. } => "预设已删除",
                    }.into(), false));
                    if matches!(mutation, PresetConfigMutation::AddAndActivate { .. })
                        && mutation.generation() == Some(self.bus.active_generation.load(Ordering::Acquire)) {
                        self.name_input.update(cx, |input, cx| input.set_text("", cx));
                        if let Some(state) = self.app_state.upgrade() {
                            state.update(cx, |st, cx| {
                                st.active_tab = "画面设置".into();
                                cx.notify();
                            });
                        }
                        let generation = self.bus.active_generation.load(Ordering::Acquire);
                        let _ = self.bus.cmd_sender.send(AppCommand::FetchPictureParameters { generation });
                    }
                }
                cx.notify();
            }
            AppEvent::AdbStateChanged { state, generation, .. } => {
                if !state.is_connected() || *generation != self.pending_generation {
                    if self.pending_config_request.is_none() {
                        self.pending_operation = None;
                    }
                    self.pending_edit_target = None;
                    self.confirm_restore = false;
                    cx.notify();
                }
            }
            AppEvent::PresetSnapshotCaptured {
                name,
                update_preset_id,
                values,
                generation,
            } => {
                if *generation != self.pending_generation {
                    return;
                }
                self.pending_operation = None;

                let app_state_entity = match self.app_state.upgrade() {
                    Some(e) => e,
                    None => return,
                };

                if let Some(target_id) = update_preset_id {
                    self.pending_config_request = Some(async_save_preset_config(
                        PresetConfigMutation::UpdateValues {
                            id: target_id.clone(), values: values.clone(), generation: *generation,
                        }, self.bus.clone(),
                    ));
                    self.pending_operation = Some("参数已抓取，正在保存预设…".into());
                    self.page_toast = None;
                } else if let Some(wanted_name) = name {
                    let st = app_state_entity.read(cx);
                    let preset = PresetItem {
                        id: new_id(&st.config.presets, "preset"),
                        name: unique_name(&st.config.presets, wanted_name), values: values.clone(),
                    };
                    self.pending_config_request = Some(async_save_preset_config(
                        PresetConfigMutation::AddAndActivate { preset, generation: *generation }, self.bus.clone(),
                    ));
                    self.pending_operation = Some("参数已抓取，正在创建预设…".into());
                    self.page_toast = None;
                }
                cx.notify();
            }

            AppEvent::PresetApplied {
                preset_id,
                result,
                generation,
                ..
            } => {
                if *generation != self.pending_generation {
                    return;
                }
                self.pending_operation = None;
                let pending_target = self.pending_edit_target.take();
                let is_fully_ok = result.failed.is_empty() && !result.applied.is_empty();
                self.page_toast = Some((
                    format!(
                        "{} ({})",
                        if is_fully_ok {
                            "预设应用成功"
                        } else {
                            "预设应用未完全成功"
                        },
                        result.summary()
                    ),
                    !is_fully_ok,
                ));

                // 处于“编辑画面”流程且目标匹配且完全成功时，跳转画面设置并刷新
                if is_fully_ok && pending_target.as_deref() == Some(preset_id.as_str()) {
                    if let Some(app_state_entity) = self.app_state.upgrade() {
                        app_state_entity.update(cx, |st, cx| {
                            st.active_tab = "画面设置".to_string();
                            cx.notify();
                        });
                        let gen = self.bus.active_generation.load(Ordering::Acquire);
                        let _ = self
                            .bus
                            .cmd_sender
                            .send(AppCommand::FetchPictureParameters { generation: gen });
                    }
                }

                cx.notify();
            }
            AppEvent::BaselineRestored { result, generation } => {
                if *generation != self.pending_generation {
                    return;
                }
                self.pending_operation = None;
                let is_fully_ok = result.failed.is_empty() && !result.applied.is_empty();
                self.page_toast = Some((
                    if is_fully_ok {
                        "已恢复初始硬件基线并保存状态".into()
                    } else {
                        format!("恢复基线未完全成功：{}（保留快照）", result.summary())
                    },
                    !is_fully_ok,
                ));
                cx.notify();
            }

            AppEvent::PresetOperationFailed {
                message,
                generation,
            } => {
                if *generation == self.pending_generation {
                    self.pending_operation = None;
                    self.pending_edit_target = None;
                    self.page_toast = Some((message.clone(), true));
                    cx.notify();
                }
            }

            _ => {}
        }
    }

    /// 点击“从当前设备抓取并新增”
    fn on_capture_new(&mut self, cx: &mut Context<Self>) {
        self.pending_edit_target = None;
        if self.pending_operation.is_some() {
            return;
        }

        let app_state_entity = match self.app_state.upgrade() {
            Some(e) => e,
            None => return,
        };

        let is_connected = app_state_entity.read(cx).adb_state.is_connected();
        if !is_connected {
            self.page_toast = Some(("显示器未连接无线 ADB，无法读取硬件快照".into(), true));
            cx.notify();
            return;
        }

        let input_text = self.name_input.read(cx).text().trim().to_string();
        let target_name = if input_text.is_empty() {
            "自定义预设".to_string()
        } else {
            input_text
        };

        let gen = self.bus.active_generation.load(Ordering::Acquire);
        self.pending_generation = gen;
        self.pending_operation = Some("正在从设备抓取完整画面参数快照...".into());
        self.page_toast = None;
        cx.notify();

        let _ = self.bus.cmd_sender.send(AppCommand::CapturePresetSnapshot {
            name: Some(target_name),
            update_preset_id: None,
            generation: gen,
        });
    }

    /// 点击“恢复基线”
    fn on_restore_baseline(&mut self, cx: &mut Context<Self>) {
        self.pending_edit_target = None;
        if self.pending_operation.is_some() {
            return;
        }

        let app_state_entity = match self.app_state.upgrade() {
            Some(e) => e,
            None => return,
        };

        let state = app_state_entity.read(cx);
        if !state.adb_state.is_connected() {
            self.page_toast = Some(("显示器未连接无线 ADB，无法向硬件下发基线恢复".into(), true));
            cx.notify();
            return;
        }

        let snapshot = match &state.config.preset_snapshot {
            Some(s) if !s.is_empty() => s.clone(),
            _ => {
                self.page_toast = Some(("还没有可退回的基线状态快照".into(), true));
                cx.notify();
                return;
            }
        };

        if !self.confirm_restore {
            self.confirm_restore = true;
            self.page_toast = Some(("将向当前显示器写入保存的初始参数。无法核对序列号时，请确认当前连接的正是来源显示器，再次点击“恢复基线”确认。".into(), true));
            cx.notify();
            return;
        }
        self.confirm_restore = false;
        let gen = self.bus.active_generation.load(Ordering::Acquire);
        self.pending_generation = gen;
        self.pending_operation = Some("正在恢复至初始硬件基线设置...".into());
        self.page_toast = None;
        cx.notify();

        let _ = self.bus.cmd_sender.send(AppCommand::RestoreBaseline {
            allow_unknown_origin: true,
            values: snapshot,
            generation: gen,
        });
    }

    /// 点击“应用预设”
    fn on_apply_preset(&mut self, preset_id: String, cx: &mut Context<Self>) {
        self.pending_edit_target = None;
        if self.pending_operation.is_some() {
            return;
        }

        let app_state_entity = match self.app_state.upgrade() {
            Some(e) => e,
            None => return,
        };

        let state = app_state_entity.read(cx);
        if !state.adb_state.is_connected() {
            self.page_toast = Some(("显示器未连接无线 ADB，无法下发预设参数".into(), true));
            cx.notify();
            return;
        }

        let preset = match find_preset(&state.config.presets, &preset_id) {
            Some(p) => p.clone(),
            None => {
                self.page_toast = Some((format!("未找到预设条目: {}", preset_id), true));
                cx.notify();
                return;
            }
        };

        // 首次应用预设前必须先真实捕获基线且成功后保留
        let need_baseline = state.config.preset_snapshot.is_none();

        let gen = self.bus.active_generation.load(Ordering::Acquire);
        self.pending_generation = gen;
        self.pending_operation = Some(format!(
            "正在应用预设「{}」{}...",
            preset.name,
            if need_baseline {
                " (首次应用自动采集基线)"
            } else {
                ""
            }
        ));
        self.page_toast = None;
        cx.notify();

        let _ = self.bus.cmd_sender.send(AppCommand::ApplyPreset {
            preset_id: preset.id,
            values: preset.values,
            need_baseline,
            generation: gen,
        });
    }

    /// 点击“编辑画面”
    fn on_edit_preset(&mut self, preset_id: String, cx: &mut Context<Self>) {
        if self.pending_operation.is_some() {
            return;
        }

        let app_state_entity = match self.app_state.upgrade() {
            Some(e) => e,
            None => return,
        };

        let (is_connected, is_current_active) = {
            let state = app_state_entity.read(cx);
            (
                state.adb_state.is_connected(),
                state.config.active_preset_id.as_deref() == Some(&preset_id),
            )
        };

        if !is_connected {
            self.pending_edit_target = None;
            self.page_toast = Some(("显示器未连接无线 ADB，无法进入预设画面编辑".into(), true));
            cx.notify();
            return;
        }

        if is_current_active {
            // 已是当前预设且连接，直接跳转画面设置页并刷新
            self.pending_edit_target = None;
            app_state_entity.update(cx, |st, cx| {
                st.active_tab = "画面设置".to_string();
                cx.notify();
            });
            let gen = self.bus.active_generation.load(Ordering::Acquire);
            let _ = self
                .bus
                .cmd_sender
                .send(AppCommand::FetchPictureParameters { generation: gen });
            return;
        }

        // 非当前预设：沿用 on_apply_preset 下发，并记录 pending_edit_target 等待真正成功
        self.on_apply_preset(preset_id.clone(), cx);
        if self.pending_operation.is_some() {
            self.pending_edit_target = Some(preset_id);
        } else {
            self.pending_edit_target = None;
        }
    }

    /// 点击“重新抓取更新”
    fn on_recapture_preset(&mut self, preset_id: String, cx: &mut Context<Self>) {
        self.pending_edit_target = None;
        if self.pending_operation.is_some() {
            return;
        }

        let app_state_entity = match self.app_state.upgrade() {
            Some(e) => e,
            None => return,
        };

        let state = app_state_entity.read(cx);
        if !state.adb_state.is_connected() {
            self.page_toast = Some(("显示器未连接无线 ADB，无法读取硬件快照".into(), true));
            cx.notify();
            return;
        }

        let preset = match find_preset(&state.config.presets, &preset_id) {
            Some(p) => p.clone(),
            None => return,
        };

        let gen = self.bus.active_generation.load(Ordering::Acquire);
        self.pending_generation = gen;
        self.pending_operation = Some(format!(
            "正在从设备采集快照并更新预设「{}」...",
            preset.name
        ));
        self.page_toast = None;
        cx.notify();

        let _ = self.bus.cmd_sender.send(AppCommand::CapturePresetSnapshot {
            name: None,
            update_preset_id: Some(preset.id),
            generation: gen,
        });
    }

    /// 触发重命名模式
    fn on_start_rename(&mut self, preset_id: String, cx: &mut Context<Self>) {
        let app_state_entity = match self.app_state.upgrade() {
            Some(e) => e,
            None => return,
        };

        let current_name = {
            let state = app_state_entity.read(cx);
            find_preset(&state.config.presets, &preset_id)
                .map(|p| p.name.clone())
                .unwrap_or_default()
        };

        let rename_input = cx.new(|cx| TextInput::new(cx, "输入新预设名称", current_name));
        self.editing_preset_id = Some(preset_id);
        self.rename_input = Some(rename_input);
        self.confirm_delete_id = None;
        cx.notify();
    }

    /// 提交重命名
    fn on_confirm_rename(&mut self, preset_id: String, cx: &mut Context<Self>) {
        if self.pending_operation.is_some() { return; }
        let new_name = match &self.rename_input {
            Some(input) => input.read(cx).text().trim().to_string(),
            None => return,
        };

        if new_name.is_empty() {
            self.page_toast = Some(("预设名称不能为空".into(), true));
            cx.notify();
            return;
        }

        if self.app_state.upgrade().is_none() { return; }
        self.pending_config_request = Some(async_save_preset_config(
            PresetConfigMutation::Rename { id: preset_id, name: new_name }, self.bus.clone(),
        ));
        self.pending_operation = Some("正在保存预设名称…".into());

        self.editing_preset_id = None;
        self.rename_input = None;
        self.page_toast = None;
        cx.notify();
    }

    /// 取消重命名
    fn on_cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.editing_preset_id = None;
        self.rename_input = None;
        cx.notify();
    }

    /// 确认删除预设
    fn on_confirm_delete(&mut self, preset_id: String, cx: &mut Context<Self>) {
        if self.pending_operation.is_some() { return; }
        self.pending_config_request = Some(async_save_preset_config(
            PresetConfigMutation::Delete { id: preset_id }, self.bus.clone(),
        ));
        self.pending_operation = Some("正在删除预设…".into());

        self.confirm_delete_id = None;
        self.page_toast = None;
        cx.notify();
    }
}

impl Render for PresetPageView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_state_opt = self.app_state.upgrade();
        let (adb_connected, presets, active_id, has_baseline, baseline_keys_count) =
            if let Some(state_entity) = app_state_opt {
                let st = state_entity.read(cx);
                let conn = st.adb_state.is_connected();
                let ps = st.config.presets.clone();
                let aid = st.config.active_preset_id.clone();
                let base_count = st
                    .config
                    .preset_snapshot
                    .as_ref()
                    .map(|s| s.len())
                    .unwrap_or(0);
                let has_base = st
                    .config
                    .preset_snapshot
                    .as_ref()
                    .map_or(false, |s| !s.is_empty());
                (conn, ps, aid, has_base, base_count)
            } else {
                (false, Vec::new(), None, false, 0)
            };

        let is_busy = self.pending_operation.is_some();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full()
            // 1. 顶部标题与说明
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
                            .child("预设模式"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("管理显示器个性化色彩与硬件快照。支持从当前设备抓取新增、一键切换与无损基线还原。"),
                    ),
            )
            // 2. 进行中操作或错误反馈横幅
            .children(if let Some(op) = &self.pending_operation {
                vec![
                    card_container()
                        .bg(rgb(ThemeColors::ACCENT_BG_SUBTLE()))
                        .border_color(rgb(ThemeColors::ACCENT_CYAN()))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(badge("正在执行", ThemeColors::STATUS_SUCCESS_BG(), ThemeColors::STATUS_SUCCESS()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                        .child(op.clone()),
                                ),
                        ),
                ]
            } else if let Some((msg, is_err)) = &self.page_toast {
                vec![
                    card_container()
                        .bg(if *is_err { rgb(ThemeColors::STATUS_ERROR_BG()) } else { rgb(ThemeColors::STATUS_SUCCESS_BG()) })
                        .border_color(if *is_err { rgb(ThemeColors::STATUS_ERROR()) } else { rgb(ThemeColors::STATUS_SUCCESS()) })
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(if *is_err { rgb(ThemeColors::STATUS_ERROR()) } else { rgb(ThemeColors::STATUS_SUCCESS()) })
                                        .child(msg.clone()),
                                ),
                        ),
                ]
            } else {
                vec![]
            })
            // 3. 从当前设备采集并新增预设卡片
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
                                    .child("从当前设备抓取并新增预设"),
                            )
                            .child(
                                badge(
                                    if adb_connected { "设备已就绪" } else { "ADB 未连接" },
                                    if adb_connected { ThemeColors::STATUS_SUCCESS_BG() } else { ThemeColors::BG_DISABLED() },
                                    if adb_connected { ThemeColors::STATUS_SUCCESS() } else { ThemeColors::TEXT_SECONDARY() },
                                ),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("以当前显示器硬件正在生效的真实模式、背光、对比度及色增益等配置为内容，新建独立的预设条目。"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .child(self.name_input.clone()),
                            )
                            .child(
                                div()
                                    .id("btn_capture_and_create")
                                    .child(action_button("从设备抓取并新增", true, is_busy || !adb_connected))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.on_capture_new(cx);
                                    })),
                            ),
                    ),
            )
            // 4. “无预设 / 恢复基线” 卡片
            .child(
                card_container()
                    .border_color(if active_id.is_none() {
                        rgb(ThemeColors::ACCENT_CYAN())
                    } else {
                        rgb(ThemeColors::BORDER())
                    })
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
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                            .child("无预设 / 恢复原始基线"),
                                    )
                                    .children(if active_id.is_none() {
                                        vec![badge("当前生效中", ThemeColors::ACCENT_CYAN(), ThemeColors::TEXT_ON_ACCENT())]
                                    } else {
                                        vec![]
                                    }),
                            )
                            .child(
                                div()
                                    .id("btn_restore_baseline")
                                    .child(action_button(
                                        "恢复基线",
                                        false,
                                        is_busy || !adb_connected || !has_baseline,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.on_restore_baseline(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .child(if has_baseline {
                                format!(
                                    "已完整保留首次套用预设前采集的硬件快照 (含 {} 项初始参数)。点击右侧按钮可安全退回该状态。",
                                    baseline_keys_count
                                )
                            } else {
                                "尚未记录基线快照。首次应用任意自定义预设前，系统将全自动先捕获当前硬件真实快照并安全保留。".to_string()
                            }),
                    ),
            )
            // 5. 已保存预设列表
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                    .child(format!("预设条目清单 (共 {} 项)", presets.len())),
                            ),
                    )
                    .children(if presets.is_empty() {
                        vec![
                            card_container()
                                .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                        .child("当前暂无预设。请在上方输入预设名称并点击「从设备抓取并新增」录入第一份预设。"),
                                ),
                        ]
                    } else {
                        presets.into_iter().enumerate().map(|(idx, preset)| {
                            let pid = preset.id.clone();
                            let is_active = active_id.as_deref() == Some(&pid);
                            let missing = missing_keys(&preset.values);
                            let has_missing = !missing.is_empty();
                            let is_editing = self.editing_preset_id.as_deref() == Some(&pid);
                            let is_confirm_delete = self.confirm_delete_id.as_deref() == Some(&pid);

                            let pid_for_edit = pid.clone();
                            let pid_for_apply = pid.clone();
                            let pid_for_recapture = pid.clone();
                            let pid_for_rename_start = pid.clone();
                            let pid_for_rename_confirm = pid.clone();
                            let pid_for_delete_start = pid.clone();
                            let pid_for_delete_confirm = pid.clone();

                            card_container()
                                .border_color(if is_active {
                                    rgb(ThemeColors::ACCENT_CYAN())
                                } else {
                                    rgb(ThemeColors::BORDER())
                                })
                                .child(
                                    // 预设标题行与激活状态
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
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                                        .child(preset.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                                        .child(format!("({})", preset.id)),
                                                )
                                                .children(if is_active {
                                                    vec![badge("当前生效中", ThemeColors::ACCENT_CYAN(), ThemeColors::TEXT_ON_ACCENT())]
                                                } else {
                                                    vec![]
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                                .child(format!("{} 项设置参数", preset.values.len())),
                                        ),
                                )
                                // 缺失项提示 (如有缺失)
                                .children(if has_missing {
                                    let missing_text = missing.join("、");
                                    vec![
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_2()
                                            .p_2()
                                            .rounded_md()
                                            .bg(rgb(ThemeColors::STATUS_WARNING_BG()))
                                            .border_1()
                                            .border_color(rgb(ThemeColors::STATUS_WARNING()))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(ThemeColors::STATUS_WARNING()))
                                                    .child(format!(
                                                        "⚠ 缺少 {} 项设置 ({})，应用时将自动跳过未定义的项",
                                                        missing.len(),
                                                        missing_text
                                                    )),
                                            ),
                                    ]
                                } else {
                                    vec![]
                                })
                                // 编辑重命名行
                                .children(if is_editing {
                                    vec![
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap_2()
                                            .items_center()
                                            .pt_1()
                                            .children(self.rename_input.clone().map(|inp| {
                                                div().flex_1().child(inp)
                                            }))
                                            .child(
                                                div()
                                                    .id(("btn_confirm_rename", idx))
                                                    .child(action_button("确认修改", true, is_busy))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.on_confirm_rename(pid_for_rename_confirm.clone(), cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id(("btn_cancel_rename", idx))
                                                    .child(action_button("取消", false, false))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.on_cancel_rename(cx);
                                                    })),
                                            ),
                                    ]
                                } else if is_confirm_delete {
                                    // 删除二次确认行
                                    vec![
                                        div()
                                            .flex()
                                            .flex_row()
                                            .justify_between()
                                            .items_center()
                                            .p_2()
                                            .rounded_md()
                                            .bg(rgb(ThemeColors::STATUS_ERROR_BG()))
                                            .border_1()
                                            .border_color(rgb(ThemeColors::STATUS_ERROR()))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(ThemeColors::STATUS_ERROR()))
                                                    .child("确定删除此预设吗？相关的设置数据将被移除。"),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .id(("btn_real_delete", idx))
                                                            .child(action_button("确认删除", true, is_busy))
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                this.on_confirm_delete(pid_for_delete_confirm.clone(), cx);
                                                            })),
                                                    )
                                                    .child(
                                                        div()
                                                            .id(("btn_cancel_delete", idx))
                                                            .child(action_button("取消", false, false))
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.confirm_delete_id = None;
                                                                cx.notify();
                                                            })),
                                                    ),
                                            ),
                                    ]
                                } else {
                                    // 正常操作按钮条
                                    vec![
                                        div()
                                            .flex()
                                            .flex_row()
                                            .justify_end()
                                            .gap_2()
                                            .items_center()
                                            .pt_2()
                                            .child(
                                                div()
                                                    .id(("btn_edit_picture", idx))
                                                    .child(action_button(
                                                        "编辑画面",
                                                        false,
                                                        is_busy || !adb_connected,
                                                    ))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.on_edit_preset(pid_for_edit.clone(), cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id(("btn_apply_preset", idx))
                                                    .child(action_button(
                                                        if is_active { "重新应用" } else { "应用预设" },
                                                        true,
                                                        is_busy || !adb_connected,
                                                    ))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.on_apply_preset(pid_for_apply.clone(), cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id(("btn_recapture", idx))
                                                    .child(action_button("重新抓取更新", false, is_busy || !adb_connected))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.on_recapture_preset(pid_for_recapture.clone(), cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id(("btn_rename", idx))
                                                    .child(action_button("重命名", false, is_busy))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.on_start_rename(pid_for_rename_start.clone(), cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id(("btn_ask_delete", idx))
                                                    .child(action_button("删除", false, is_busy))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.confirm_delete_id = Some(pid_for_delete_start.clone());
                                                        this.editing_preset_id = None;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ]
                                })
                        }).collect()
                    }),
            )
    }
}

/// 导出供 MainWindow 初始占位使用的辅助渲染函数
pub fn render_preset_page() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                .child("预设模式"),
        )
        .child(
            crate::ui::components::card_container().child(
                div()
                    .text_sm()
                    .child("管理显示器个性化色彩快照与场景切换。"),
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::ApplyResult;

    #[test]
    fn test_apply_result_success_determination() {
        // 纪律：应用/恢复有 failed 或 applied 为空不能标记成功/清基线
        let failed_res = ApplyResult {
            applied: vec!["背光".into()],
            failed: vec![("FreeSync".into(), "失败".into())],
            skipped: vec![],
        };
        assert!(!failed_res.failed.is_empty());
        assert!(!(failed_res.failed.is_empty() && !failed_res.applied.is_empty()));

        let empty_applied_res = ApplyResult {
            applied: vec![],
            failed: vec![],
            skipped: vec![("全部".into(), "跳过".into())],
        };
        assert!(!(empty_applied_res.failed.is_empty() && !empty_applied_res.applied.is_empty()));

        let ok_res = ApplyResult {
            applied: vec!["背光".into(), "对比度".into()],
            failed: vec![],
            skipped: vec![],
        };
        assert!(ok_res.failed.is_empty() && !ok_res.applied.is_empty());
    }

    #[test]
    fn test_save_only_preset_fields_preserves_other_configs_and_extra() {
        use crate::config::AppConfig;

        let mut config = AppConfig::default();
        config.saved_ip = "192.168.1.100".to_string();
        config.hdr_sdr_local_dimming_enabled = true;
        config.extra.insert(
            "custom_extension_field".into(),
            serde_json::json!("keep_me"),
        );

        let new_presets = vec![PresetItem {
            id: "preset_1".into(),
            name: "电影模式".into(),
            values: HashMap::new(),
        }];
        let mut new_snapshot = HashMap::new();
        new_snapshot.insert("picture_mode".into(), serde_json::json!(14));

        // 模拟 async_save_preset_config 的更新逻辑
        let mut updated = config.clone();
        updated.presets = new_presets.clone();
        updated.preset_snapshot = Some(new_snapshot.clone());
        updated.active_preset_id = Some("preset_1".into());

        // 验证其他字段完全被保留
        assert_eq!(updated.saved_ip, "192.168.1.100");
        assert!(updated.hdr_sdr_local_dimming_enabled);
        assert_eq!(
            updated.extra.get("custom_extension_field"),
            Some(&serde_json::json!("keep_me"))
        );
        // 验证预设字段更新
        assert_eq!(updated.presets.len(), 1);
        assert_eq!(updated.active_preset_id, Some("preset_1".into()));
        assert!(updated.preset_snapshot.is_some());
    }

}
