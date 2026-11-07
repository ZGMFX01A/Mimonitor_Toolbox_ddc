use gpui::prelude::*;
use gpui::{div, px, rgb, Context, Div, Entity, IntoElement, Render, SharedString, Window};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::device::game::*;
use crate::device::picture::{get_picture_scene_name, is_game_picture_mode};
use crate::message::{AppCommand, MessageBus};
use crate::ui::components::{action_button, badge, card_container};
use crate::ui::theme::ThemeColors;

/// 确认弹窗动作类型
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    SwitchGameModeAndSetFrontSight(u32),
    SwitchGameModeAndSetDynamicFrontSight(bool),
    SwitchGameModeAndSetScope(u32),
    SwitchGameModeAndSetNightVision(bool),
    EnableFrontSightAndDynamic {
        sight: u32,
        dynamic: bool,
        switch_to_game: bool,
    },
    EnableScopeAndNightVision {
        scope: u32,
        night_vision: bool,
        switch_to_game: bool,
    },
}

/// 确认弹窗状态数据
#[derive(Debug, Clone)]
pub struct ConfirmDialogState {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub action: ConfirmAction,
}

/// 游戏模式设置独立页面 Entity
pub struct GamePage {
    pub bus: Arc<MessageBus>,
    pub config: AppConfig,
    pub params: GameParameters,
    pub confirm_dialog: Option<ConfirmDialogState>,
    pub is_refreshing: bool,
    pub last_mode: Option<i32>,
    pub connected: bool,
    pub generation: u64,
    pub error: Option<String>,
}

impl GamePage {
    pub fn new(bus: Arc<MessageBus>, config: AppConfig, _cx: &mut Context<Self>) -> Self {
        Self {
            bus,
            config,
            params: GameParameters::default(),
            confirm_dialog: None,
            is_refreshing: false,
            last_mode: None,
            connected: false,
            generation: 0,
            error: None,
        }
    }

    /// 后台回读最新游戏参数后更新 UI 状态
    pub fn update_params(&mut self, new_params: GameParameters, cx: &mut Context<Self>) {
        let previous_mode = self.last_mode;
        let current_mode = new_params.picture_mode;
        self.last_mode = current_mode;

        // 准星游戏模式仅在游戏模式联动逻辑 (crosshair_game_mode_only)
        if self.config.crosshair_game_mode_only {
            let was_game = previous_mode.map(is_game_picture_mode).unwrap_or(false);
            let is_game = current_mode.map(is_game_picture_mode).unwrap_or(false);

            if previous_mode.is_some() && !was_game && is_game {
                // 回到游戏模式：若当前准星为 0 且存在记忆，自动恢复
                if new_params.front_sight_index == Some(0) {
                    if let Some(mem_sight) = self.config.crosshair_memory {
                        if mem_sight > 0 && mem_sight <= 5 {
                            let gen = self.bus.active_generation.load(Ordering::Relaxed);
                            let _ = self.bus.cmd_sender.send(AppCommand::SetGameFrontSight {
                                index: mem_sight as u32,
                                is_game_mode: true,
                                generation: gen,
                            });
                        }
                    }
                }
            } else if current_mode.is_some() && was_game && !is_game {
                // 离开游戏模式：若当前准星非 0，自动记录并隐藏
                let cur_sight = new_params.front_sight_index.unwrap_or(0);
                if cur_sight != 0 {
                    self.config.crosshair_memory = Some(cur_sight as i32);
                    self.save_current_config();
                    let gen = self.bus.active_generation.load(Ordering::Relaxed);
                    let _ = self.bus.cmd_sender.send(AppCommand::SetGameFrontSight {
                        index: 0,
                        is_game_mode: false,
                        generation: gen,
                    });
                }
            }
        }

        self.params = new_params;
        self.is_refreshing = false;
        cx.notify();
    }

    /// 标记刷新中并向总线请求最新硬件参数
    pub fn trigger_refresh(&mut self, cx: &mut Context<Self>) {
        self.is_refreshing = true;
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::FetchGameParameters { generation: gen });
        cx.notify();
    }

    /// 判断当前是否处于已确认的游戏模式
    pub fn is_game_mode_active(&self) -> bool {
        self.params
            .picture_mode
            .map(is_game_picture_mode)
            .unwrap_or(false)
    }

    /// 异步安全持久化当前配置
    fn save_current_config(&self) {
        let cfg = self.config.clone();
        let bus = self.bus.clone();
        std::thread::spawn(move || {
            if let Err(error) = crate::config::update_settings(|current| {
                current.crosshair_game_mode_only = cfg.crosshair_game_mode_only;
                current.crosshair_memory = cfg.crosshair_memory;
                current.freesync_mode_memory_enabled = cfg.freesync_mode_memory_enabled;
                current.freesync_previous_mode = cfg.freesync_previous_mode;
            }) {
                let _ = bus
                    .event_sender
                    .send(crate::message::AppEvent::ToastNotification {
                        message: error.to_string(),
                        is_error: true,
                    });
            }
        });
    }

    pub fn handle_connection_state(
        &mut self,
        connected: bool,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if !connected || self.generation != generation {
            self.params = GameParameters::default();
            self.last_mode = None;
            self.confirm_dialog = None;
            self.error = None;
        }
        self.connected = connected;
        self.generation = generation;
        if connected {
            self.trigger_refresh(cx);
        }
        cx.notify();
    }

    pub fn handle_operation_failed(
        &mut self,
        message: &str,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        self.is_refreshing = false;
        self.error = Some(message.to_string());
        cx.notify();
    }

    /// 关闭确认弹窗
    pub fn dismiss_confirm_dialog(&mut self, cx: &mut Context<Self>) {
        self.confirm_dialog = None;
        cx.notify();
    }

    /// 接受确认弹窗并执行下发
    pub fn accept_confirm_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(dlg) = self.confirm_dialog.take() {
            let gen = self.bus.active_generation.load(Ordering::Relaxed);
            match dlg.action {
                ConfirmAction::SwitchGameModeAndSetFrontSight(idx) => {
                    if idx > 0 {
                        self.config.crosshair_memory = Some(idx as i32);
                        self.save_current_config();
                    }
                    let _ = self
                        .bus
                        .cmd_sender
                        .send(AppCommand::SwitchGameModeAndApply {
                            switch_to_game: true,
                            front_sight: Some(idx),
                            dynamic_front_sight: None,
                            sniper_scope: None,
                            sniper_night_vision: None,
                            generation: gen,
                        });
                }
                ConfirmAction::SwitchGameModeAndSetDynamicFrontSight(enabled) => {
                    let _ = self
                        .bus
                        .cmd_sender
                        .send(AppCommand::SwitchGameModeAndApply {
                            switch_to_game: true,
                            front_sight: None,
                            dynamic_front_sight: Some(enabled),
                            sniper_scope: None,
                            sniper_night_vision: None,
                            generation: gen,
                        });
                }
                ConfirmAction::SwitchGameModeAndSetScope(code) => {
                    let _ = self
                        .bus
                        .cmd_sender
                        .send(AppCommand::SwitchGameModeAndApply {
                            switch_to_game: true,
                            front_sight: None,
                            dynamic_front_sight: None,
                            sniper_scope: Some(code),
                            sniper_night_vision: None,
                            generation: gen,
                        });
                }
                ConfirmAction::SwitchGameModeAndSetNightVision(enabled) => {
                    let _ = self
                        .bus
                        .cmd_sender
                        .send(AppCommand::SwitchGameModeAndApply {
                            switch_to_game: true,
                            front_sight: None,
                            dynamic_front_sight: None,
                            sniper_scope: None,
                            sniper_night_vision: Some(enabled),
                            generation: gen,
                        });
                }
                ConfirmAction::EnableFrontSightAndDynamic {
                    sight,
                    dynamic,
                    switch_to_game,
                } => {
                    self.config.crosshair_memory = Some(sight as i32);
                    self.save_current_config();
                    let _ = self
                        .bus
                        .cmd_sender
                        .send(AppCommand::SwitchGameModeAndApply {
                            switch_to_game,
                            front_sight: Some(sight),
                            dynamic_front_sight: Some(dynamic),
                            sniper_scope: None,
                            sniper_night_vision: None,
                            generation: gen,
                        });
                }
                ConfirmAction::EnableScopeAndNightVision {
                    scope,
                    night_vision,
                    switch_to_game,
                } => {
                    let _ = self
                        .bus
                        .cmd_sender
                        .send(AppCommand::SwitchGameModeAndApply {
                            switch_to_game,
                            front_sight: None,
                            dynamic_front_sight: None,
                            sniper_scope: Some(scope),
                            sniper_night_vision: Some(night_vision),
                            generation: gen,
                        });
                }
            }
        }
        cx.notify();
    }

    /// 用户选择准星样式
    pub fn select_front_sight(&mut self, index: u32, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        if index == 0 {
            // 用户主动关准星，清空记忆
            self.config.crosshair_memory = None;
            self.save_current_config();
            let gen = self.bus.active_generation.load(Ordering::Relaxed);
            let _ = self.bus.cmd_sender.send(AppCommand::SetGameFrontSight {
                index: 0,
                is_game_mode: self.is_game_mode_active(),
                generation: gen,
            });
            cx.notify();
            return;
        }

        // 开启准星：若当前非游戏模式，必须应用内确认切换游戏模式
        if !self.is_game_mode_active() {
            self.confirm_dialog = Some(ConfirmDialogState {
                title: "需要游戏模式".into(),
                message: format!(
                    "当前不是游戏模式，准星（样式 {}）只能在游戏模式下生效。\n\n是否切换到游戏模式并继续？",
                    index
                ),
                confirm_label: "切换到游戏模式".into(),
                action: ConfirmAction::SwitchGameModeAndSetFrontSight(index),
            });
            cx.notify();
            return;
        }

        // 正常在游戏模式下修改准星：更新记忆，触发 14->500ms->10 重载
        self.config.crosshair_memory = Some(index as i32);
        self.save_current_config();
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self.bus.cmd_sender.send(AppCommand::SetGameFrontSight {
            index,
            is_game_mode: true,
            generation: gen,
        });
        cx.notify();
    }

    /// 用户选择动态准星开关
    pub fn select_dynamic_front_sight(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        if !enabled {
            let gen = self.bus.active_generation.load(Ordering::Relaxed);
            let _ = self
                .bus
                .cmd_sender
                .send(AppCommand::SetGameDynamicFrontSight {
                    enabled: false,
                    generation: gen,
                });
            cx.notify();
            return;
        }

        // 开启动态准星：前置依赖检查
        let current_sight = self.params.front_sight_index.unwrap_or(0);
        if current_sight == 0 {
            // 缺准星：提示确认同时开启前置
            let switch_game = !self.is_game_mode_active();
            self.confirm_dialog = Some(ConfirmDialogState {
                title: "需要开启前置功能".into(),
                message: "动态准星需要先启用准星才能生效。\n\n是否同时启用准星 1？".into(),
                confirm_label: "同时启用准星 1".into(),
                action: ConfirmAction::EnableFrontSightAndDynamic {
                    sight: 1,
                    dynamic: true,
                    switch_to_game: switch_game,
                },
            });
            cx.notify();
            return;
        }

        if !self.is_game_mode_active() {
            self.confirm_dialog = Some(ConfirmDialogState {
                title: "需要游戏模式".into(),
                message:
                    "当前不是游戏模式，动态准星只能在游戏模式下生效。\n\n是否切换到游戏模式并继续？"
                        .into(),
                confirm_label: "切换到游戏模式".into(),
                action: ConfirmAction::SwitchGameModeAndSetDynamicFrontSight(true),
            });
            cx.notify();
            return;
        }

        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::SetGameDynamicFrontSight {
                enabled: true,
                generation: gen,
            });
        cx.notify();
    }

    /// 用户选择狙击镜倍率代码 (0, 1, 3, 5, 7, 10)
    pub fn select_sniper_scope(&mut self, code: u32, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        if code == 0 {
            let gen = self.bus.active_generation.load(Ordering::Relaxed);
            let _ = self.bus.cmd_sender.send(AppCommand::SetGameSniperScope {
                code: 0,
                generation: gen,
            });
            cx.notify();
            return;
        }

        if !self.is_game_mode_active() {
            self.confirm_dialog = Some(ConfirmDialogState {
                title: "需要游戏模式".into(),
                message:
                    "当前不是游戏模式，狙击镜只能在游戏模式下生效。\n\n是否切换到游戏模式并继续？"
                        .into(),
                confirm_label: "切换到游戏模式".into(),
                action: ConfirmAction::SwitchGameModeAndSetScope(code),
            });
            cx.notify();
            return;
        }

        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self.bus.cmd_sender.send(AppCommand::SetGameSniperScope {
            code,
            generation: gen,
        });
        cx.notify();
    }

    /// 用户选择狙击夜视开关
    pub fn select_sniper_night_vision(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        if !enabled {
            let gen = self.bus.active_generation.load(Ordering::Relaxed);
            let _ = self
                .bus
                .cmd_sender
                .send(AppCommand::SetGameSniperNightVision {
                    enabled: false,
                    generation: gen,
                });
            cx.notify();
            return;
        }

        // 开启夜视：前置依赖检查 (狙击镜倍率非 0)
        let current_scope = self.params.sniper_scope.unwrap_or(0);
        if current_scope == 0 {
            let switch_game = !self.is_game_mode_active();
            self.confirm_dialog = Some(ConfirmDialogState {
                title: "需要开启前置功能".into(),
                message: "狙击镜夜视需要先启用狙击镜 1.1x才能生效。\n\n是否同时启用狙击镜 1.1x？"
                    .into(),
                confirm_label: "同时启用 1.1x".into(),
                action: ConfirmAction::EnableScopeAndNightVision {
                    scope: 1,
                    night_vision: true,
                    switch_to_game: switch_game,
                },
            });
            cx.notify();
            return;
        }

        if !self.is_game_mode_active() {
            self.confirm_dialog = Some(ConfirmDialogState {
                title: "需要游戏模式".into(),
                message:
                    "当前不是游戏模式，狙击夜视只能在游戏模式下生效。\n\n是否切换到游戏模式并继续？"
                        .into(),
                confirm_label: "切换到游戏模式".into(),
                action: ConfirmAction::SwitchGameModeAndSetNightVision(true),
            });
            cx.notify();
            return;
        }

        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self
            .bus
            .cmd_sender
            .send(AppCommand::SetGameSniperNightVision {
                enabled: true,
                generation: gen,
            });
        cx.notify();
    }

    /// 用户选择 FreeSync Premium Pro
    pub fn select_freesync(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let mut restore_mode = None;

        if self.config.freesync_mode_memory_enabled && !is_freesync_memory_suspended() {
            if enabled {
                // 从关到开时记录原模式（如果当前不是游戏模式）
                if self.params.freesync != Some(true) {
                    if let Some(cur_mode) = self.params.picture_mode {
                        if !is_game_picture_mode(cur_mode) {
                            self.config.freesync_previous_mode = Some(cur_mode);
                            self.save_current_config();
                        }
                    }
                }
            } else if self.params.freesync == Some(true) {
                // 关闭 FreeSync：获取之前记录的模式作为 restore_mode
                restore_mode = self.config.freesync_previous_mode;
            }
        }

        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self.bus.cmd_sender.send(AppCommand::SetGameFreeSync {
            enabled,
            restore_mode,
            generation: gen,
        });
        cx.notify();
    }

    /// 切换 FreeSync 模式记忆开关配置
    pub fn toggle_freesync_mode_memory(&mut self, cx: &mut Context<Self>) {
        self.config.freesync_mode_memory_enabled = !self.config.freesync_mode_memory_enabled;
        self.save_current_config();
        cx.notify();
    }

    /// 切换准星仅游戏模式显示配置
    pub fn toggle_crosshair_game_mode_only(&mut self, cx: &mut Context<Self>) {
        self.config.crosshair_game_mode_only = !self.config.crosshair_game_mode_only;
        self.save_current_config();
        cx.notify();
    }

    /// 用户选择 320Hz 竞技刷新率模式
    pub fn select_320hz(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self.bus.cmd_sender.send(AppCommand::SetGame320Hz {
            enabled,
            generation: gen,
        });
        cx.notify();
    }

    /// 用户选择 FPS 计数器
    pub fn select_fps_counter(&mut self, value: u32, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self.bus.cmd_sender.send(AppCommand::SetGameFpsCounter {
            value,
            generation: gen,
        });
        cx.notify();
    }

    /// 用户选择秒表
    pub fn select_stopwatch(&mut self, value: u32, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self.bus.cmd_sender.send(AppCommand::SetGameStopwatch {
            value,
            generation: gen,
        });
        cx.notify();
    }

    /// 用户选择定时器
    pub fn select_timer(&mut self, seconds: u32, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let gen = self.bus.active_generation.load(Ordering::Relaxed);
        let _ = self.bus.cmd_sender.send(AppCommand::SetGameTimer {
            seconds,
            generation: gen,
        });
        cx.notify();
    }
}

/// 辅助渲染统一风格的单选按钮组
fn render_option_group<T: Copy + PartialEq + 'static>(
    title: &'static str,
    options: &'static [(T, &'static str)],
    current_val: Option<T>,
    on_select: impl Fn(T, &mut GamePage, &mut Context<GamePage>) + 'static + Copy,
    cx: &mut Context<GamePage>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                .child(title),
        )
        .child(div().flex().flex_row().flex_wrap().gap_2().children(
            options.iter().enumerate().map(|(idx, &(val, label))| {
                let is_active = current_val.map(|c| c == val).unwrap_or(false);
                let element_id: SharedString = format!("opt_{}_{}", title, idx).into();
                div()
                    .id(element_id)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .cursor_pointer()
                    .font_weight(if is_active {
                        gpui::FontWeight::BOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .bg(if is_active {
                        rgb(ThemeColors::ACCENT_CYAN())
                    } else {
                        rgb(ThemeColors::BG_CARD())
                    })
                    .border_1()
                    .border_color(if is_active {
                        rgb(ThemeColors::ACCENT_HOVER())
                    } else {
                        rgb(ThemeColors::BORDER())
                    })
                    .text_color(if is_active {
                        rgb(ThemeColors::TEXT_ON_ACCENT())
                    } else {
                        rgb(ThemeColors::TEXT_PRIMARY())
                    })
                    .hover(|s| {
                        if is_active {
                            s.bg(rgb(ThemeColors::ACCENT_HOVER()))
                        } else {
                            s.bg(rgb(ThemeColors::BG_HOVER()))
                        }
                    })
                    .child(label)
                    .on_click(cx.listener(move |page, _, _, cx| {
                        if page.connected && current_val.is_some() {
                            on_select(val, page, cx);
                        }
                    }))
            }),
        ))
}

impl Render for GamePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_game_mode = self.is_game_mode_active();
        let picture_mode_known = self.params.picture_mode.is_some();

        // 状态提示文本与颜色 (对照 pages.py 410-412 与 display_features.py 1740-1753)
        let (hint_text, hint_color) = if !picture_mode_known {
            (
                "当前画面模式未知；高亮值尚未确认是否生效。",
                ThemeColors::STATUS_WARNING(),
            )
        } else if is_game_mode {
            (
                "当前为游戏模式；下方高亮为当前生效值。",
                ThemeColors::STATUS_SUCCESS(),
            )
        } else {
            (
                "当前不是游戏模式；下方高亮为记忆值，功能当前未生效。",
                ThemeColors::STATUS_WARNING(),
            )
        };

        // FreeSync 状态文案
        let freesync_memory_label = {
            let enabled = self.config.freesync_mode_memory_enabled;
            let is_fs_on = self.params.freesync == Some(true);
            let prefix = if enabled { "已开启" } else { "已关闭" };
            if is_fs_on {
                let prev_mode_str = self
                    .config
                    .freesync_previous_mode
                    .map(|m| format!("{}（{}）", get_picture_scene_name(m), m))
                    .unwrap_or_else(|| "--".to_string());
                format!(
                    "FreeSync 模式记忆：{}，已记录开启前模式：{}（关闭后切回）",
                    prefix, prev_mode_str
                )
            } else {
                let cur_mode_str = self
                    .params
                    .picture_mode
                    .map(|m| format!("{}（{}）", get_picture_scene_name(m), m))
                    .unwrap_or_else(|| "--".to_string());
                format!(
                    "FreeSync 模式记忆：{}，开启时将记录当前模式：{}",
                    prefix, cur_mode_str
                )
            }
        };

        // 准星状态文案
        let crosshair_memory_label = {
            let enabled = self.config.crosshair_game_mode_only;
            let mem = self.config.crosshair_memory.unwrap_or(0);
            let prefix = if enabled { "已开启" } else { "已关闭" };
            if mem > 0 {
                format!(
                    "准星游戏联动：{}，已记忆样式：{}（离开游戏隐藏，回到游戏恢复）",
                    prefix, mem
                )
            } else {
                format!("准星游戏联动：{}，尚未记录生效样式", prefix)
            }
        };

        let is_refreshing = self.is_refreshing;

        div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full()
            .relative()
            // 顶部标题栏与刷新按钮
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
                                    .child("游戏模式"),
                            )
                            .child(div().text_xs().text_color(rgb(hint_color)).child(hint_text)),
                    )
                    .child(
                        div()
                            .id("btn_refresh_game_data")
                            .child(action_button(
                                if is_refreshing {
                                    "正在刷新..."
                                } else {
                                    "刷新数据"
                                },
                                false,
                                is_refreshing,
                            ))
                            .on_click(cx.listener(|page, _, _, cx| {
                                page.trigger_refresh(cx);
                            })),
                    ),
            )
            // 卡片 1: 刷新率与同步 (FreeSync & 320Hz)
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
                                    .child("同步与刷新率"),
                            )
                            .child(badge(
                                if let Some(src) = &self.params.current_source {
                                    format!("信号源: {}", src)
                                } else {
                                    "信号源: 未知".to_string()
                                },
                                ThemeColors::BG_BUTTON(),
                                ThemeColors::TEXT_SECONDARY(),
                            )),
                    )
                    // FreeSync Premium Pro
                    .child(render_option_group(
                        "FreeSync Premium Pro",
                        &[(false, "关"), (true, "开")],
                        self.params.freesync,
                        |val, page, cx| page.select_freesync(val, cx),
                        cx,
                    ))
                    // FreeSync 模式记忆提示与开关
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .pt_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                    .child(freesync_memory_label),
                            )
                            .child(
                                div()
                                    .id("btn_toggle_freesync_mem")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .cursor_pointer()
                                    .bg(if self.config.freesync_mode_memory_enabled {
                                        rgb(ThemeColors::STATUS_SUCCESS_BG())
                                    } else {
                                        rgb(ThemeColors::BORDER())
                                    })
                                    .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                    .child(if self.config.freesync_mode_memory_enabled {
                                        "记忆: 开启"
                                    } else {
                                        "记忆: 关闭"
                                    })
                                    .on_click(cx.listener(|page, _, _, cx| {
                                        page.toggle_freesync_mode_memory(cx);
                                    })),
                            ),
                    )
                    // 320Hz 竞技模式
                    .child(render_option_group(
                        "320Hz 竞技模式",
                        &[(false, "关"), (true, "开")],
                        self.params.mode_320,
                        |val, page, cx| page.select_320hz(val, cx),
                        cx,
                    )),
            )
            // 卡片 2: 准星与狙击镜瞄准辅助
            .child(
                card_container()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                            .child("准星与瞄准辅助"),
                    )
                    // 准星 (0..5)
                    .child(render_option_group(
                        "准星样式",
                        &FRONT_SIGHT_OPTIONS,
                        self.params.front_sight_index,
                        |val, page, cx| page.select_front_sight(val, cx),
                        cx,
                    ))
                    // 准星模式联动提示与开关
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .pt_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                    .child(crosshair_memory_label),
                            )
                            .child(
                                div()
                                    .id("btn_toggle_crosshair_mem")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .cursor_pointer()
                                    .bg(if self.config.crosshair_game_mode_only {
                                        rgb(ThemeColors::STATUS_SUCCESS_BG())
                                    } else {
                                        rgb(ThemeColors::BORDER())
                                    })
                                    .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                    .child(if self.config.crosshair_game_mode_only {
                                        "联动: 仅游戏显示"
                                    } else {
                                        "联动: 关闭"
                                    })
                                    .on_click(cx.listener(|page, _, _, cx| {
                                        page.toggle_crosshair_game_mode_only(cx);
                                    })),
                            ),
                    )
                    // 动态准星 (关/开)
                    .child(render_option_group(
                        "动态准星 (需先启用准星)",
                        &[(false, "关"), (true, "开")],
                        self.params.dynamic_front_sight,
                        |val, page, cx| page.select_dynamic_front_sight(val, cx),
                        cx,
                    ))
                    // 狙击镜 (代码 0, 1, 3, 5, 7, 10，禁止 clamp 0..3 错误)
                    .child(render_option_group(
                        "狙击镜放大倍率",
                        &SNIPER_SCOPE_OPTIONS,
                        self.params.sniper_scope,
                        |val, page, cx| page.select_sniper_scope(val, cx),
                        cx,
                    ))
                    // 狙击镜夜视 (关/开)
                    .child(render_option_group(
                        "狙击镜夜视 (需先启用狙击镜)",
                        &[(false, "关"), (true, "开")],
                        self.params.sniper_night_vision,
                        |val, page, cx| page.select_sniper_night_vision(val, cx),
                        cx,
                    )),
            )
            // 卡片 4: 游戏辅助工具 (FPS / 秒表 / 计时器)
            .child(
                card_container()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                            .child("面板辅助工具"),
                    )
                    // FPS 计数器
                    .child(render_option_group(
                        "FPS 计数器",
                        &FPS_COUNTER_OPTIONS,
                        self.params.fps_counter,
                        |val, page, cx| page.select_fps_counter(val, cx),
                        cx,
                    ))
                    // 秒表
                    .child(render_option_group(
                        "秒表",
                        &STOPWATCH_OPTIONS,
                        self.params.stopwatch,
                        |val, page, cx| page.select_stopwatch(val, cx),
                        cx,
                    ))
                    // 定时器
                    .child(render_option_group(
                        "定时器",
                        &TIMER_OPTIONS,
                        self.params.timer,
                        |val, page, cx| page.select_timer(val, cx),
                        cx,
                    )),
            )
            // 应用内安全前置/模式确认弹窗浮层
            .children(self.confirm_dialog.as_ref().map(|dlg| {
                let title = dlg.title.clone();
                let message = dlg.message.clone();
                let confirm_label = dlg.confirm_label.clone();

                div()
                    .absolute()
                    .inset_0()
                    .bg(rgb(0x000000))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(460.0))
                            .p_6()
                            .bg(rgb(ThemeColors::BG_CARD()))
                            .border_1()
                            .border_color(rgb(ThemeColors::ACCENT_CYAN()))
                            .rounded_xl()
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                    .child(message),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_end()
                                    .gap_3()
                                    .pt_2()
                                    .child(
                                        div()
                                            .id("btn_confirm_dialog_cancel")
                                            .child(action_button("取消", false, false))
                                            .on_click(cx.listener(|page, _, _, cx| {
                                                page.dismiss_confirm_dialog(cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("btn_confirm_dialog_ok")
                                            .child(action_button(confirm_label, true, false))
                                            .on_click(cx.listener(|page, _, _, cx| {
                                                page.accept_confirm_dialog(cx);
                                            })),
                                    ),
                            ),
                    )
            }))
    }
}

/// 保持向后兼容的渲染函数，供页面路由器或直接包含时使用
pub fn render_game_page(game_page: Entity<GamePage>) -> Div {
    div().flex().flex_col().w_full().child(game_page)
}
