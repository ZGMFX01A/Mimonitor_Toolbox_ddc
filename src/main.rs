// GUI launches (including login startup) must never allocate a console, even
// when a locally built debug executable is registered for autostart.
#![windows_subsystem = "windows"]

#[cfg(all(not(debug_assertions), target_env = "msvc", not(target_feature = "crt-static")))]
compile_error!("Release builds require the static CRT configured in .cargo/config.toml");

pub mod adb;
pub mod config;
pub mod ddc;
pub mod device;
pub mod hotkey;
mod instance;
pub mod logger;
pub mod message;
pub mod presets;
mod resources;
pub mod tray;
pub mod ui;

use gpui::prelude::*;
use gpui::{
    div, point, px, rgb, size, AnyWindowHandle, App, Application, Bounds, Context, Entity,
    InteractiveElement, IntoElement, Pixels, Point, Render,
    StatefulInteractiveElement, Subscription, Window, WindowBounds, WindowOptions,
};
use parking_lot::Mutex;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::adb::{AdbDeviceState, DiscoveredDevice};
use crate::config::AppConfig;
use crate::ddc::DdcMonitorInfo;
use crate::device::light::{ScreenLightMode, ScreenLightState};
use crate::message::{AppCommand, AppEvent, MessageBus};
use crate::tray::{SystemTrayManager, TrayEvent};
use crate::ui::components::{bind_text_input_keys, TextInput};
use crate::ui::pages::picture::PicturePage;
use crate::ui::pages::preset::PresetPageView;
use crate::ui::pages::*;
use crate::ui::theme::ThemeColors;
use crate::ui::tray_popup::TrayBrightnessPopupView;
use crate::ui::tray_value_popup::TrayValuePopupView;

fn monitor_ip(input: &str) -> Result<String, &'static str> {
    let raw = input.trim();
    // Accept the former saved :5555 form without exposing a port field.
    let raw = raw.strip_suffix(":5555").unwrap_or(raw);
    raw.parse::<std::net::Ipv4Addr>().map(|ip| ip.to_string())
        .map_err(|_| "请输入显示器 IP，例如 192.168.31.200")
}

fn single_scan_target(devices: &[DiscoveredDevice]) -> Option<String> {
    let mut eligible = devices.iter().filter(|d| d.is_mitv || adb::is_mitv_model(&d.model));
    let first = eligible.next()?;
    eligible.next().is_none().then(|| first.ip.clone())
}

#[cfg(test)]
mod connection_ux_tests {
    use super::*;
    #[test]
    fn ip_input_uses_fixed_device_port_and_accepts_legacy_saved_form() {
        for input in ["192.168.31.200", " 192.168.31.200:5555 "] {
            let ip = monitor_ip(input).unwrap();
            assert_eq!(ip, "192.168.31.200");
            assert_eq!(adb::AdbDevice::new(ip).serial(), "192.168.31.200:5555");
        }
        assert!(monitor_ip("192.168.31.200:6000").is_err());
        assert!(monitor_ip("999.1.1.1").is_err());
        assert!(monitor_ip("").is_err());
    }
    #[test]
    fn scanning_auto_selects_only_an_unambiguous_monitor() {
        let device = |ip: &str, model: &str| DiscoveredDevice {
            ip: ip.into(), model: model.into(), is_mitv: adb::is_mitv_model(model),
        };
        let first = device("192.168.31.200", "MiTV-MFFU1");
        let other = device("192.168.31.201", "Redmi");
        assert_eq!(single_scan_target(&[first.clone()]), Some(first.ip.clone()));
        assert_eq!(single_scan_target(&[first.clone(), device("192.168.31.10", "Android Phone")]), Some(first.ip.clone()));
        assert_eq!(single_scan_target(&[first, other]), None);
        assert_eq!(single_scan_target(&[device("192.168.31.10", "Android Phone")]), None);
        assert_eq!(single_scan_target(&[]), None);
    }
}

/// 全局共享状态实体 (由后台消息总线驱动，驱动主窗口与托盘面板同步重绘)
pub struct AppState {
    pub bus: Arc<MessageBus>,
    pub tray_manager: Arc<SystemTrayManager>,
    pub config: AppConfig,
    pub active_tab: String,
    pub adb_ip: String,
    pub adb_state: AdbDeviceState,
    pub adb_model: String,
    pub adb_backlight: Option<u32>,
    pub is_adb_connecting: bool,
    pub is_adb_scanning: bool,
    pub discovered_devices: Vec<DiscoveredDevice>,
    pub ddc_monitors: Vec<DdcMonitorInfo>,
    pub display_summary: String,
    pub ddc_refresh_count: u64,
    pub toast: Option<(String, bool)>,
    pub device_busy: bool,
    pub tray_value_results: std::collections::BTreeMap<u64, (u64, Option<String>)>,
    pub text_input: Entity<TextInput>,
    pub source_page: Entity<crate::ui::pages::source::SourcePage>,
    pub remote_page: Entity<crate::ui::pages::remote::RemotePage>,
    pub light_state: ScreenLightState,
    pub picture_page: Entity<PicturePage>,
    pub game_page: Entity<crate::ui::pages::game::GamePage>,
    pub preset_view: Option<Entity<PresetPageView>>,
    pub tools_page: Entity<crate::ui::pages::tools_panel::ToolsPage>,
    pub tray_page: Entity<crate::ui::pages::tray::TrayPage>,
    pub auto_task_page: Entity<crate::ui::pages::auto_task::AutoTaskPage>,
}

impl AppState {
    pub fn new(
        bus: Arc<MessageBus>,
        tray_manager: Arc<SystemTrayManager>,
        config: AppConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved_ip = monitor_ip(&config.saved_ip).unwrap_or_default();
        let initial_input = if saved_ip.trim().is_empty() {
            "".to_string()
        } else {
            saved_ip.clone()
        };

        let text_input =
            cx.new(|cx| TextInput::new(cx, "例如 192.168.31.200", initial_input.clone()));
        let source_page = cx.new(|cx| crate::ui::pages::source::SourcePage::new(bus.clone(), cx));
        let remote_page = cx.new(|cx| crate::ui::pages::remote::RemotePage::new(bus.clone(), cx));

        let picture_page = cx.new(|cx| PicturePage::new(bus.clone(), cx));

        let game_page =
            cx.new(|cx| crate::ui::pages::game::GamePage::new(bus.clone(), config.clone(), cx));

        let tools_page = cx.new(|cx| {
            crate::ui::pages::tools_panel::ToolsPage::new(bus.clone(), config.clone(), cx)
        });
        let tray_page =
            cx.new(|_| crate::ui::pages::tray::TrayPage::new(bus.clone(), config.clone()));
        let auto_task_page = cx.new(|cx| {
            crate::ui::pages::auto_task::AutoTaskPage::new(bus.clone(), config.clone(), cx)
        });
        Self {
            bus,
            tray_manager,
            config,
            active_tab: "主页 & 连接".to_string(),
            adb_ip: initial_input,
            adb_state: AdbDeviceState::NotFound,
            adb_model: String::new(),
            adb_backlight: None,
            is_adb_connecting: false,
            is_adb_scanning: false,
            discovered_devices: Vec::new(),
            ddc_monitors: Vec::new(),
            display_summary: "统一多屏亮度管理".into(),
            ddc_refresh_count: 0,
            device_busy: false,
            tray_value_results: Default::default(),
            toast: if crate::config::migration_blocked() {
                Some((
                    "旧配置备份失败，已进入只读模式；请检查备份目录权限后重启".into(),
                    true,
                ))
            } else {
                None
            },
            text_input,
            source_page,
            remote_page,
            light_state: ScreenLightState::default(),
            picture_page,
            game_page,
            preset_view: None,
            tools_page,
            auto_task_page,
            tray_page,
        }
    }

    fn connect_ip(&mut self, input: &str, cx: &mut Context<Self>) {
        if self.is_adb_connecting { return; }
        let ip = match monitor_ip(input) {
            Ok(ip) => ip,
            Err(message) => {
                self.toast = Some((message.into(), true));
                cx.notify();
                return;
            }
        };
        if self.is_adb_scanning {
            self.bus.cancel_scan();
            self.is_adb_scanning = false;
        }
        let bus = self.bus.clone();
        let generation = bus.active_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.is_adb_connecting = true;
        self.adb_state = AdbDeviceState::NotFound;
        self.adb_backlight = None;
        self.adb_ip = ip.clone();
        self.config.saved_ip = ip.clone();
        self.text_input.update(cx, |input, cx| input.set_text(ip.clone(), cx));
        self.picture_page.update(cx, |page, cx| page.on_adb_state_changed(&AdbDeviceState::NotFound, generation, cx));
        self.game_page.update(cx, |page, cx| page.handle_connection_state(false, generation, cx));
        self.tools_page.update(cx, |page, cx| page.set_connected(false, generation, None, cx));
        self.source_page.update(cx, |page, cx| page.set_connected(false, generation, cx));
        self.remote_page.update(cx, |page, cx| page.set_connected(false, generation, cx));
        self.light_state = ScreenLightState::default();
        self.toast = None;
        let ip_to_save = ip.clone();
        let save_bus = bus.clone();
        std::thread::spawn(move || {
            if let Err(error) = crate::config::update_settings(|cfg| cfg.saved_ip = ip_to_save) {
                let _ = save_bus.event_sender.send(AppEvent::ToastNotification {
                    message: format!("保存连接地址失败：{error}"), is_error: true,
                });
            }
        });
        if bus.cmd_sender.send(AppCommand::ConnectAdb { ip, generation }).is_err() {
            self.is_adb_connecting = false;
            self.toast = Some(("连接失败：后台已停止".into(), true));
        }
        cx.notify();
    }
}

/// GPUI 主窗口视图 (支持中文统一主题、可缩放可滚动、左侧 10 原页面入口)
pub struct MainWindow {
    app_state: Entity<AppState>,
    _subscription: Subscription,
}

impl MainWindow {
    pub fn new(app_state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&app_state, |_this, _state, cx| {
            cx.notify();
        });
        Self {
            app_state,
            _subscription,
        }
    }

    fn on_connect(&mut self, cx: &mut Context<Self>) {
        let ip = self.app_state.read(cx).text_input.read(cx).text().to_string();
        self.app_state.update(cx, |st, cx| st.connect_ip(&ip, cx));
    }

    fn on_cancel_connect(&mut self, cx: &mut Context<Self>) {
        let bus = self.app_state.read(cx).bus.clone();
        bus.cancel_connection();
        self.app_state.update(cx, |st, cx| {
            st.is_adb_connecting = false;
            cx.notify();
        });
    }

    fn on_disconnect(&mut self, cx: &mut Context<Self>) {
        let bus = self.app_state.read(cx).bus.clone();
        let _ = bus.cmd_sender.send(AppCommand::DisconnectAdb);
        self.app_state.update(cx, |st, cx| {
            st.is_adb_connecting = false;
            st.light_state = ScreenLightState::default();
            cx.notify();
        });
    }

    fn on_scan(&mut self, cx: &mut Context<Self>) {
        let bus = self.app_state.read(cx).bus.clone();
        let scan_gen = bus.current_scan_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.app_state.update(cx, |st, cx| {
            st.is_adb_scanning = true;
            st.discovered_devices.clear();
            st.toast = None;
            cx.notify();
        });
        let _ = bus.cmd_sender.send(AppCommand::ScanNetworkDevices {
            generation: scan_gen,
        });
    }

    fn on_cancel_scan(&mut self, cx: &mut Context<Self>) {
        let bus = self.app_state.read(cx).bus.clone();
        bus.cancel_scan();
        self.app_state.update(cx, |st, cx| {
            st.is_adb_scanning = false;
            cx.notify();
        });
    }

    fn on_select_discovered_ip(&mut self, selected_ip: &str, cx: &mut Context<Self>) {
        self.app_state.update(cx, |st, cx| {
            if st.adb_state.is_connected() && st.adb_ip == selected_ip { return; }
            st.connect_ip(selected_ip, cx);
        });
    }

    fn on_refresh_light(&mut self, cx: &mut Context<Self>) {
        let state = self.app_state.read(cx);
        if !state.adb_state.is_connected() {
            return;
        }
        let bus = state.bus.clone();
        let gen = bus.active_generation.load(Ordering::SeqCst);
        let rev = self.app_state.update(cx, |st, cx| {
            st.light_state.is_reading = true;
            st.light_state.last_error = None;
            let rev = st.light_state.last_revision + 1;
            st.light_state.last_revision = rev;
            cx.notify();
            rev
        });
        let _ = bus.cmd_sender.send(AppCommand::FetchScreenLight {
            generation: gen,
            revision: rev,
        });
    }

    fn on_set_light_mode(&mut self, mode: ScreenLightMode, cx: &mut Context<Self>) {
        let state = self.app_state.read(cx);
        if !state.adb_state.is_connected() {
            return;
        }
        let Some(mut params) = state.light_state.to_parameters() else {
            self.app_state.update(cx, |st, cx| {
                st.toast = Some(("存在未知硬件参数，禁止写入以防覆盖".into(), true));
                cx.notify();
            });
            return;
        };
        params.mode = mode;
        let bus = state.bus.clone();
        let gen = bus.active_generation.load(Ordering::SeqCst);
        let rev = self.app_state.update(cx, |st, cx| {
            st.light_state.is_writing = true;
            st.light_state.mode = Some(mode);
            st.light_state.last_error = None;
            let rev = st.light_state.last_revision + 1;
            st.light_state.last_revision = rev;
            cx.notify();
            rev
        });
        let _ = bus.cmd_sender.send(AppCommand::SetScreenLight {
            params,
            generation: gen,
            revision: rev,
        });
    }

    fn on_set_light_illumination(&mut self, val: u32, cx: &mut Context<Self>) {
        let state = self.app_state.read(cx);
        if !state.adb_state.is_connected() {
            return;
        }
        let Some(mut params) = state.light_state.to_parameters() else {
            self.app_state.update(cx, |st, cx| {
                st.toast = Some(("存在未知硬件参数，禁止写入以防覆盖".into(), true));
                cx.notify();
            });
            return;
        };
        let hw_ill = if val > 14 {
            val.saturating_sub(1).min(14)
        } else {
            val.min(14)
        };
        params.illumination = hw_ill;
        let bus = state.bus.clone();
        let gen = bus.active_generation.load(Ordering::SeqCst);
        let rev = self.app_state.update(cx, |st, cx| {
            st.light_state.is_writing = true;
            st.light_state.illumination = Some(hw_ill);
            st.light_state.last_error = None;
            let rev = st.light_state.last_revision + 1;
            st.light_state.last_revision = rev;
            cx.notify();
            rev
        });
        let _ = bus.cmd_sender.send(AppCommand::SetScreenLight {
            params,
            generation: gen,
            revision: rev,
        });
    }

    fn on_set_light_color_temp(&mut self, temp: u32, cx: &mut Context<Self>) {
        let state = self.app_state.read(cx);
        if !state.adb_state.is_connected() {
            return;
        }
        let Some(mut params) = state.light_state.to_parameters() else {
            self.app_state.update(cx, |st, cx| {
                st.toast = Some(("存在未知硬件参数，禁止写入以防覆盖".into(), true));
                cx.notify();
            });
            return;
        };
        let valid_temp = temp.min(2);
        params.mode = ScreenLightMode::Lighting;
        params.color_temp = valid_temp;
        let bus = state.bus.clone();
        let gen = bus.active_generation.load(Ordering::SeqCst);
        let rev = self.app_state.update(cx, |st, cx| {
            st.light_state.is_writing = true;
            st.light_state.mode = Some(ScreenLightMode::Lighting);
            st.light_state.color_temp = Some(valid_temp);
            st.light_state.last_error = None;
            let rev = st.light_state.last_revision + 1;
            st.light_state.last_revision = rev;
            cx.notify();
            rev
        });
        let _ = bus.cmd_sender.send(AppCommand::SetScreenLight {
            params,
            generation: gen,
            revision: rev,
        });
    }

    fn on_set_light_color_value(&mut self, color: u32, cx: &mut Context<Self>) {
        let state = self.app_state.read(cx);
        if !state.adb_state.is_connected() {
            return;
        }
        let Some(mut params) = state.light_state.to_parameters() else {
            self.app_state.update(cx, |st, cx| {
                st.toast = Some(("存在未知硬件参数，禁止写入以防覆盖".into(), true));
                cx.notify();
            });
            return;
        };
        let valid_color = color.min(4);
        params.mode = ScreenLightMode::Solid;
        params.color_value = valid_color;
        let bus = state.bus.clone();
        let gen = bus.active_generation.load(Ordering::SeqCst);
        let rev = self.app_state.update(cx, |st, cx| {
            st.light_state.is_writing = true;
            st.light_state.mode = Some(ScreenLightMode::Solid);
            st.light_state.color_value = Some(valid_color);
            st.light_state.last_error = None;
            let rev = st.light_state.last_revision + 1;
            st.light_state.last_revision = rev;
            cx.notify();
            rev
        });
        let _ = bus.cmd_sender.send(AppCommand::SetScreenLight {
            params,
            generation: gen,
            revision: rev,
        });
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (
            active_tab,
            adb_state,
            adb_model,
            adb_backlight,
            is_connecting,
            is_scanning,
            discovered,
            toast,
            text_input,
            adb_ip,
            ddc_avail_count,
            source_page,
            remote_page,
            light_state,
            picture_page,
            active_preset_name,
        ) = {
            let state = self.app_state.read(cx);
            let active_preset_name = state
                .config
                .active_preset_id
                .as_ref()
                .and_then(|id| {
                    state
                        .config
                        .presets
                        .iter()
                        .find(|p| p.id == *id)
                        .map(|p| p.name.clone())
                });
            (
                state.active_tab.clone(),
                state.adb_state.clone(),
                state.adb_model.clone(),
                state.adb_backlight,
                state.is_adb_connecting,
                state.is_adb_scanning,
                state.discovered_devices.clone(),
                state.toast.clone(),
                state.text_input.clone(),
                state.adb_ip.clone(),
                state.ddc_monitors.iter().filter(|m| m.is_supported).count(),
                state.source_page.clone(),
                state.remote_page.clone(),
                state.light_state.clone(),
                state.picture_page.clone(),
                active_preset_name,
            )
        };
        let app_state_entity = self.app_state.clone();

        let nav_items: [(&str, &str, Option<&str>); 10] = [
            ("主页 & 连接", "主页连接", Some("设备控制")),
            ("画面设置", "画面设置", None),
            ("游戏模式", "游戏模式", None),
            ("信号源切换", "信号源", None),
            ("屏幕灯", "屏幕灯", None),
            ("托盘菜单", "托盘菜单", Some("自动化与设置")),
            ("预设模式", "预设模式", None),
            ("自动任务", "自动任务", None),
            ("工具与设置", "工具设置", None),
            ("遥控器", "遥控器", None),
        ];

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(ThemeColors::BG_ROOT()))
            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
            // 顶部导航栏：合并进应用无边框体验，紧凑内边距
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_4()
                    .py_2()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(ThemeColors::BORDER()))
                    .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .window_control_area(gpui::WindowControlArea::Drag)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(7.0))
                                            .h(px(7.0))
                                            .rounded_full()
                                            .bg(rgb(ThemeColors::ACCENT_CYAN())),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                            .child("Mimonitor 工具箱"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                    .truncate()
                                    .child(self.app_state.read(cx).display_summary.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_shrink_0()
                            .child(
                                div()
                                    .id("open-brightness-panel")
                                    .border_1()
                                    .border_color(rgb(ThemeColors::ACCENT_CYAN()))
                                    .bg(rgb(ThemeColors::ACCENT_CYAN()))
                                    .text_color(rgb(ThemeColors::TEXT_ON_ACCENT()))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_3()
                                    .h(px(28.0))
                                    .rounded_md()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .hover(|s| s.bg(rgb(ThemeColors::ACCENT_HOVER())).border_color(rgb(ThemeColors::ACCENT_HOVER())))
                                    .child("☀ 亮度面板")
                                    .on_click({
                                        let bus = self.app_state.read(cx).bus.clone();
                                        move |_, _, _| {
                                            let _ = bus.event_sender.send(AppEvent::OpenBrightness);
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .id("window-minimize")
                                            .w(px(32.0))
                                            .h(px(28.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_md()
                                            .text_xs()
                                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                            .hover(|s| s.bg(rgb(ThemeColors::BG_HOVER())).text_color(rgb(ThemeColors::TEXT_PRIMARY())))
                                            .child("—")
                                            .on_click(|_, window, _| window.minimize_window()),
                                    )
                                    .child(
                                        div()
                                            .id("window-maximize")
                                            .w(px(32.0))
                                            .h(px(28.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_md()
                                            .text_xs()
                                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                            .hover(|s| s.bg(rgb(ThemeColors::BG_HOVER())).text_color(rgb(ThemeColors::TEXT_PRIMARY())))
                                            .child("□")
                                            .window_control_area(gpui::WindowControlArea::Max),
                                    )
                                    .child(
                                        div()
                                            .id("window-close")
                                            .w(px(32.0))
                                            .h(px(28.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_md()
                                            .text_xs()
                                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                                            .hover(|s| s.bg(rgb(0xC42B1C)).text_color(rgb(0xffffff)))
                                            .child("×")
                                            .window_control_area(gpui::WindowControlArea::Close),
                                    ),
                            ),
                    ),
            )
            .children(toast.as_ref().map(|(message, error)| {
                let app_state_for_close_toast = app_state_entity.clone();
                let is_err = *error;
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(if is_err {
                        ThemeColors::STATUS_ERROR()
                    } else {
                        ThemeColors::STATUS_SUCCESS()
                    }))
                    .bg(rgb(if is_err {
                        ThemeColors::STATUS_ERROR_BG()
                    } else {
                        ThemeColors::STATUS_SUCCESS_BG()
                    }))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(if is_err {
                                        ThemeColors::STATUS_ERROR()
                                    } else {
                                        ThemeColors::STATUS_SUCCESS()
                                    }))
                                    .child(if is_err { "⚠ 提示" } else { "✓ 成功" }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
                                    .flex_1()
                                    .min_w_0()
                                    .child(message.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id("btn_close_toast")
                            .cursor_pointer()
                            .px_1p5()
                            .py(px(2.0))
                            .rounded_md()
                            .text_xs()
                            .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                            .hover(|s| s.bg(rgb(ThemeColors::BG_HOVER())).text_color(rgb(ThemeColors::TEXT_PRIMARY())))
                            .child("✕")
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                app_state_for_close_toast.update(cx, |st, cx| {
                                    st.toast = None;
                                    cx.notify();
                                });
                            })),
                    )
            }))
            // 中间主体：左侧 10 原页面入口导航栏 + 右侧内容区
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    // 左侧导航侧边栏 (宽160px，自适应纵向滚动)
                    .child(
                        div()
                            .id("sidebar-scroll")
                            .overflow_y_scroll()
                            .min_h_0()
                            .w(px(160.0))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .p_2()
                            .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                            .border_r_1()
                            .border_color(rgb(ThemeColors::BORDER()))
                            .child(div().flex().flex_col().gap_0p5().children(
                                nav_items.into_iter().enumerate().map(|(idx, (tab_key, tab_label, group_header))| {
                                    let is_active = tab_key == active_tab;
                                    let name_str = tab_key.to_string();
                                    let app_state_for_click = app_state_entity.clone();

                                    div()
                                        .flex()
                                        .flex_col()
                                        .children(group_header.map(|header| {
                                            div()
                                                .pt_2()
                                                .pb_1()
                                                .px_2()
                                                .text_size(px(11.0))
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                                .child(header)
                                        }))
                                        .child(
                                            div()
                                                .id(("nav_item", idx))
                                                .px_3()
                                                .py_1p5()
                                                .rounded_md()
                                                .text_xs()
                                                .font_weight(if is_active {
                                                    gpui::FontWeight::BOLD
                                                } else {
                                                    gpui::FontWeight::NORMAL
                                                })
                                                .bg(if is_active {
                                                    rgb(ThemeColors::ACCENT_BG_SUBTLE())
                                                } else {
                                                    rgb(crate::ui::theme::ThemeColors::BG_CARD())
                                                })
                                                .border_1()
                                                .border_color(if is_active {
                                                    rgb(ThemeColors::ACCENT_CYAN())
                                                } else {
                                                    rgb(crate::ui::theme::ThemeColors::BG_CARD())
                                                })
                                                .text_color(if is_active {
                                                    rgb(ThemeColors::ACCENT_CYAN())
                                                } else {
                                                    rgb(ThemeColors::TEXT_SECONDARY())
                                                })
                                                .cursor_pointer()
                                                .hover(|s| s.bg(rgb(ThemeColors::BG_HOVER())).text_color(rgb(ThemeColors::TEXT_PRIMARY())))
                                                .child(tab_label)
                                                .on_click(cx.listener(move |_this, _, _, cx| {
                                                    app_state_for_click.update(cx, |st, cx| {
                                                        st.active_tab = name_str.clone();
                                                        cx.notify();
                                                    });
                                                })),
                                        )
                                }),
                            ))
                            .child(
                                div()
                                    .px_2()
                                    .py_2()
                                    .border_t_1()
                                    .border_color(rgb(ThemeColors::BORDER()))
                                    .text_size(px(11.0))
                                    .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                    .child("本地控制 · 独立通道"),
                            ),
                    )
                    // 右侧内容区 (可滚动、响应式)
                    .child(
                        div()
                            .id("main_content_scroll_area")
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_y_scroll()
                            .p_5()
                            .children(active_preset_name.as_ref().map(|name| {
                                div()
                                    .mb_4()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .justify_between()
                                    .items_center()
                                    .gap_2()
                                    .p_3()
                                    .rounded_lg()
                                    .bg(rgb(crate::ui::theme::ThemeColors::ACCENT_BG_SUBTLE()))
                                    .border_1()
                                    .border_color(rgb(crate::ui::theme::ThemeColors::ACCENT_CYAN()))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_2()
                                            .min_w_0()
                                            .child(crate::ui::components::badge(
                                                "当前预设",
                                                crate::ui::theme::ThemeColors::ACCENT_CYAN(),
                                                crate::ui::theme::ThemeColors::TEXT_ON_ACCENT(),
                                            ))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(crate::ui::theme::ThemeColors::TEXT_PRIMARY()))
                                                    .child(format!("正在编辑：{} · 画面调整将自动保存", name)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("btn_nav_manage_presets")
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .text_xs()
                                            .border_1()
                                            .border_color(rgb(crate::ui::theme::ThemeColors::ACCENT_CYAN()))
                                            .text_color(rgb(crate::ui::theme::ThemeColors::ACCENT_CYAN()))
                                            .hover(|s| s.bg(rgb(crate::ui::theme::ThemeColors::BG_HOVER())))
                                            .child("管理预设")
                                            .on_click({
                                                let app_state_for_nav = app_state_entity.clone();
                                                move |_, _, cx| {
                                                    app_state_for_nav.update(cx, |st, cx| {
                                                        st.active_tab = "预设模式".to_string();
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                            }))
                            .child(match active_tab.as_str() {
                                "主页 & 连接" => render_home_page(
                                    text_input.clone(),
                                    &adb_state,
                                    &adb_model,
                                    adb_backlight,
                                    is_connecting,
                                    is_scanning,
                                    &discovered,
                                    &adb_ip,
                                    cx,
                                    Self::on_connect,
                                    Self::on_cancel_connect,
                                    Self::on_disconnect,
                                    Self::on_scan,
                                    Self::on_cancel_scan,
                                    Self::on_select_discovered_ip,
                                ),
                                "画面设置" => render_picture_page(picture_page),
                                "游戏模式" => {
                                    render_game_page(self.app_state.read(cx).game_page.clone())
                                }
                                "信号源切换" => render_source_page(source_page),
                                "屏幕灯" => render_light_page(
                                    &light_state,
                                    &adb_state,
                                    cx,
                                    Self::on_refresh_light,
                                    Self::on_set_light_mode,
                                    Self::on_set_light_illumination,
                                    Self::on_set_light_color_temp,
                                    Self::on_set_light_color_value,
                                ),
                                "托盘菜单" => {
                                    render_tray_page(self.app_state.read(cx).tray_page.clone())
                                }
                                "预设模式" => {
                                    if let Some(pv) = self.app_state.read(cx).preset_view.clone() {
                                        div().w_full().child(pv)
                                    } else {
                                        render_preset_page()
                                    }
                                }
                                "自动任务" => render_auto_task_page(
                                    self.app_state.read(cx).auto_task_page.clone(),
                                ),
                                "工具与设置" => {
                                    render_tools_page(self.app_state.read(cx).tools_page.clone())
                                }
                                "遥控器" => render_remote_page(remote_page),
                                _ => div().child("未知页面"),
                            })
                            .when(self.app_state.read(cx).device_busy, |area| {
                                area.child(
                                    div()
                                        .id("device-operation-blocker")
                                        .absolute()
                                        .inset_0()
                                        .bg(gpui::rgba(0x13161BCC))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(crate::ui::components::badge(
                                            "正在执行设备操作，请稍候…",
                                            ThemeColors::ACCENT_BG_SUBTLE(),
                                            ThemeColors::ACCENT_CYAN(),
                                        ))
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(|_, _, cx| cx.stop_propagation()),
                                )
                            }),
                    ),
            )
            // 底部状态栏
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .justify_between()
                    .items_center()
                    .gap_x_4()
                    .gap_y_1()
                    .px_4()
                    .py_2()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(ThemeColors::BORDER()))
                    .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
                    .text_xs()
                    .text_color(rgb(ThemeColors::TEXT_SECONDARY()))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1p5()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .w(px(6.0))
                                            .h(px(6.0))
                                            .rounded_full()
                                            .flex_shrink_0()
                                            .bg(rgb(if adb_state.is_connected() {
                                                ThemeColors::STATUS_SUCCESS()
                                            } else {
                                                ThemeColors::TEXT_MUTED()
                                            })),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .child(format!(
                                                "ADB: {}{}",
                                                adb_state.display_label(),
                                                if !adb_ip.is_empty() {
                                                    format!(" ({})", adb_ip)
                                                } else {
                                                    "".to_string()
                                                }
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .text_color(rgb(ThemeColors::BORDER()))
                                    .child("|"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .child(format!("DDC/CI 普通屏: {} 台可用", ddc_avail_count)),
                            ),
                    )
                    .child(
                        div()
                            .id("repository-link")
                            .cursor_pointer()
                            .flex_shrink_0()
                            .text_color(rgb(ThemeColors::ACCENT_CYAN()))
                            .hover(|s| s.text_color(rgb(ThemeColors::ACCENT_HOVER())))
                            .child("GitHub · Mimonitor_Toolbox_ddc")
                            .on_click(|_, _, cx| {
                                cx.open_url("https://github.com/ZGMFX01A/Mimonitor_Toolbox_ddc")
                            }),
                    ),
            )
    }
}

fn run_diagnose_cli(target_ip: &str) {
    println!("============================================================");
    println!("     Mimonitor Toolbox DDC - 硬件与环境只读诊断报告         ");
    println!("============================================================");

    let settings_path = config::get_settings_path();
    println!("1. 配置文件路径: {}", settings_path.display());
    println!(
        "   状态: {}",
        if settings_path.exists() {
            "已存在"
        } else {
            "尚未生成 (使用默认配置)"
        }
    );

    let adb_port = adb::get_adb_server_port();
    let adb_alive = adb::is_adb_server_alive(Duration::from_millis(200));
    let bundled_adb = adb::get_bundled_adb_path();
    println!("2. ADB 运行环境:");
    println!("   分发路径: {}", bundled_adb.display());
    println!("   隔离端口: {}", adb_port);
    println!(
        "   监听就绪: {}",
        if adb_alive {
            "正在运行"
        } else {
            "未启动 (按需冷启动)"
        }
    );

    println!("3. 系统活动 Windows 显示器:");
    let displays = ddc::list_raw_windows_displays();
    let gdi_map = ddc::enumerate_gdi_monitors();

    if displays.is_empty() {
        println!("   [警告] 未检测到任何已连接桌面的活动显示器！");
    } else {
        for (idx, disp) in displays.iter().enumerate() {
            let is_xiaomi = ddc::is_xiaomi_device_id(&disp.device_id)
                || ddc::is_xiaomi_device_id(&disp.device_string);
            println!(
                "   [{}] 设备名: {}, 标识: {}, 标签: [{}]",
                idx + 1,
                disp.device_name,
                disp.device_id,
                disp.label
            );

            if is_xiaomi {
                println!("       -> 小米 / Redmi 系列显示器 (绝对排除在 DDC/CI 之外，由 ADB 协议控制背光)");
            } else {
                println!("       -> 普通显示器，正在进行 Win32 DDC/CI 只读检测...");
                if let Some(&h_mon) = gdi_map.get(&disp.device_name) {
                    let mut num_phys: u32 = 0;
                    let count_ok = unsafe {
                        ddc::GetNumberOfPhysicalMonitorsFromHMONITOR(h_mon, &mut num_phys)
                    };
                    if count_ok != 0 && num_phys == 1 {
                        let mut phys = vec![
                            ddc::PHYSICAL_MONITOR {
                                h_physical_monitor: std::ptr::null_mut(),
                                sz_physical_monitor_description: [0; 128],
                            };
                            1
                        ];
                        let get_ok = unsafe {
                            ddc::GetPhysicalMonitorsFromHMONITOR(h_mon, 1, phys.as_mut_ptr())
                        };
                        if get_ok != 0 {
                            let guard = ddc::PhysicalMonitorsGuard::new(1, phys);
                            if let Some(h_p) = guard.handle(0) {
                                if let Some((min_b, cur_b, max_b, is_vcp)) =
                                    ddc::read_physical_monitor_brightness(h_p)
                                {
                                    let pct = ddc::normalize_brightness(cur_b, min_b, max_b);
                                    println!(
                                        "          [DDC 支持: 是] 范围: {}-{}, 当前值: {}, 归一化: {}%, 协议: {}",
                                        min_b,
                                        max_b,
                                        cur_b,
                                        pct,
                                        if is_vcp { "VCP 0x10 回退" } else { "High-level API" }
                                    );
                                } else {
                                    println!(
                                        "          [DDC 支持: 否] 显示器响应超时或未开启 DDC/CI"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\n4. 目标设备 ADB 只读探测 (IP: {}):", target_ip);
    let dev = adb::AdbDevice::new(target_ip);
    match dev.connect() {
        Ok(_) => {
            println!("   连接就绪: 成功");
            let model = dev.get_model().unwrap_or_default();
            println!("   设备型号: {}", model);
            if let Ok(bk) = device::picture::get_backlight(&dev) {
                println!("   硬件背光: {}%", bk);
            }
        }
        Err(e) => {
            println!("   连接探测结果: {}", e);
        }
    }
    println!("============================================================");
}

/// 执行 CLI 模式下机器可读的结构化 JSON 诊断输出
fn run_diagnose_json_cli() {
    let settings_path = config::get_settings_path();
    let config_data = config::load_settings();
    let adb_port = adb::get_adb_server_port();
    let adb_alive = adb::is_adb_server_alive(Duration::from_millis(200));
    let bundled_adb = adb::get_bundled_adb_path();

    let displays = ddc::list_raw_windows_displays();
    let gdi_map = ddc::enumerate_gdi_monitors();
    let mut ddc_monitors = Vec::new();

    for disp in &displays {
        let is_xiaomi = ddc::is_xiaomi_device_id(&disp.device_id)
            || ddc::is_xiaomi_device_id(&disp.device_string);
        let mut ddc_supported = false;
        let mut min_b = 0u32;
        let mut max_b = 100u32;
        let mut cur_b = 0u32;
        let mut pct = 0u32;

        if !is_xiaomi {
            if let Some(&h_mon) = gdi_map.get(&disp.device_name) {
                let mut num_phys: u32 = 0;
                let count_ok =
                    unsafe { ddc::GetNumberOfPhysicalMonitorsFromHMONITOR(h_mon, &mut num_phys) };
                if count_ok != 0 && num_phys == 1 {
                    let mut phys = vec![
                        ddc::PHYSICAL_MONITOR {
                            h_physical_monitor: std::ptr::null_mut(),
                            sz_physical_monitor_description: [0; 128],
                        };
                        1
                    ];
                    let get_ok = unsafe {
                        ddc::GetPhysicalMonitorsFromHMONITOR(h_mon, 1, phys.as_mut_ptr())
                    };
                    if get_ok != 0 {
                        let guard = ddc::PhysicalMonitorsGuard::new(1, phys);
                        if let Some(h_p) = guard.handle(0) {
                            if let Some((b_min, b_cur, b_max, _)) =
                                ddc::read_physical_monitor_brightness(h_p)
                            {
                                ddc_supported = true;
                                min_b = b_min;
                                max_b = b_max;
                                cur_b = b_cur;
                                pct = ddc::normalize_brightness(cur_b, min_b, max_b);
                            }
                        }
                    }
                }
            }
        }

        ddc_monitors.push(serde_json::json!({
            "device_name": disp.device_name,
            "device_id": disp.device_id,
            "label": disp.label,
            "physical_size": ddc::edid::read_display_size(&disp.device_id),
            "is_xiaomi": is_xiaomi,
            "ddc_supported": ddc_supported,
            "brightness_min": min_b,
            "brightness_max": max_b,
            "brightness_current": cur_b,
            "brightness_percent": pct,
        }));
    }

    let mut adb_device_probe = serde_json::json!({
        "target_ip": config_data.saved_ip,
        "connected": false,
    });
    if !config_data.saved_ip.trim().is_empty() {
        let dev = adb::AdbDevice::new(&config_data.saved_ip);
        if dev.connect().is_ok() {
            let state = dev.get_state();
            let model = dev.get_model().unwrap_or_default();
            let bk = device::picture::get_backlight(&dev).ok();
            adb_device_probe = serde_json::json!({
                "target_ip": config_data.saved_ip,
                "connected": state.is_connected(),
                "state": format!("{:?}", state),
                "model": model,
                "backlight": bk,
            });
        }
    }

    let report = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "config": {
            "path": settings_path.display().to_string(),
            "exists": settings_path.exists(),
            "saved_ip": config_data.saved_ip,
            "presets_count": config_data.presets.len(),
            "auto_tasks_count": config_data.auto_tasks.len(),
        },
        "adb_environment": {
            "bundled_adb_path": bundled_adb.display().to_string(),
            "server_port": adb_port,
            "server_alive": adb_alive,
        },
        "displays": ddc_monitors,
        "adb_device_probe": adb_device_probe,
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
}

/// 执行 CLI 模式下真实硬件可恢复亮度验证（安全沙箱：先读原值 -> 微调 +1 -> 校验 -> finally 严格恢复）
fn run_adb_verify_brightness_cli(target_ip: &str) -> Result<(), String> {
    println!("============================================================");
    println!("     Mimonitor Toolbox DDC - 真实硬件可恢复亮度验证         ");
    println!("============================================================");
    println!("目标设备 IP: {}", target_ip);
    println!("验证规范: 保存原值 -> 微调(+1) -> 回读校验 -> 严格恢复原值");

    let dev = adb::AdbDevice::new(target_ip);
    dev.connect()
        .map_err(|e| format!("无法连接到 ADB 设备: {}", e))?;

    let state = dev.get_state();
    if !state.is_connected() {
        return Err(format!("设备未处于连接状态: {:?}", state));
    }

    // 步骤 1：精确读取当前硬件面板背光原值
    let orig_backlight = device::picture::get_backlight(&dev)
        .map_err(|e| format!("无法读取当前背光原值，中止调整以保护设备: {}", e))?;
    println!(
        "\n[步骤 1] 成功读取当前设备背光基线原值: {}",
        orig_backlight
    );

    // 步骤 2：安全微调测试值 (+1 微调)
    let test_backlight = if orig_backlight < 100 {
        orig_backlight + 1
    } else {
        orig_backlight - 1
    };
    println!(
        "[步骤 2] 执行临时安全微调目标值: {} -> {}",
        orig_backlight, test_backlight
    );

    // 内部作用域隔离执行微调与回读验证
    let test_result: Result<(), String> = (|| {
        device::picture::set_backlight(&dev, test_backlight)
            .map_err(|e| format!("下发临时微调背光失败: {}", e))?;
        std::thread::sleep(Duration::from_millis(400));
        let read_test = device::picture::get_backlight(&dev)
            .map_err(|e| format!("临时微调后回读异常: {}", e))?;
        println!("[步骤 3] 临时微调后设备硬件回读值: {}", read_test);
        if read_test != test_backlight {
            return Err(format!(
                "写入回读不符：目标 {}，实际 {}",
                test_backlight, read_test
            ));
        }
        Ok(())
    })();

    // 步骤 4：无条件执行 finally 恢复原值并回读
    println!("\n[步骤 4] 严格执行 finally 恢复原值: {}", orig_backlight);
    let restore_err = device::picture::set_backlight(&dev, orig_backlight).err();
    std::thread::sleep(Duration::from_millis(400));
    let final_read = device::picture::get_backlight(&dev).ok();
    if let Some(r) = final_read {
        println!("[步骤 5] 最终设备硬件回读确认值: {}", r);
    }

    if let Some(err) = restore_err {
        return Err(format!("恢复原背光值失败: {}", err));
    }

    if final_read != Some(orig_backlight) {
        return Err(format!(
            "原亮度未确认恢复：预期 {}，实际 {:?}",
            orig_backlight, final_read
        ));
    }
    test_result?;

    println!("\n============================================================");
    println!(
        "  硬件亮度验证完美通过！已安全且无残留地恢复设备原始背光 ({})",
        orig_backlight
    );
    println!("============================================================");
    Ok(())
}

fn run_ddc_verify_brightness_cli() -> Result<(), String> {
    let monitors = ddc::enumerate_ddc_monitors();
    let supported: Vec<_> = monitors.into_iter().filter(|m| m.is_supported).collect();
    if supported.is_empty() {
        return Err("没有身份可确认且支持亮度控制的普通显示器".into());
    }
    for monitor in supported {
        let original = monitor.current_percent;
        if ddc::denormalize_brightness(original, monitor.min_brightness, monitor.max_brightness)
            != monitor.current_brightness
        {
            return Err(format!(
                "{} 原始寄存器值无法无损转换成百分比，跳过写入",
                monitor.label
            ));
        }
        let target = if original < 100 {
            original + 1
        } else {
            original - 1
        };
        println!(
            "DDC {} [{}] 原值 {}%，范围 {}..{}，临时目标 {}%",
            monitor.label,
            monitor.device_id,
            original,
            monitor.min_brightness,
            monitor.max_brightness,
            target
        );
        let read = || -> Result<u32, String> {
            let found: Vec<_> = ddc::enumerate_ddc_monitors()
                .into_iter()
                .filter(|m| m.device_id == monitor.device_id && m.is_supported)
                .collect();
            if found.len() != 1 {
                return Err("回读时设备身份不唯一或设备已断开".into());
            }
            Ok(found[0].current_percent)
        };
        let trial = ddc::set_ddc_brightness(&monitor.device_id, target).and_then(|_| {
            let actual = read()?;
            let expected = ddc::normalize_brightness(
                ddc::denormalize_brightness(target, monitor.min_brightness, monitor.max_brightness),
                monitor.min_brightness,
                monitor.max_brightness,
            );
            println!("DDC 临时写入回读 {}%", actual);
            if actual != expected {
                return Err(format!("临时回读不符，预期 {expected}%，实际 {actual}%"));
            }
            Ok(())
        });
        let restore = ddc::set_ddc_brightness(&monitor.device_id, original).and_then(|_| {
            let actual = read()?;
            println!("DDC 恢复回读 {}%", actual);
            if actual != original {
                return Err(format!("恢复回读不符，预期 {original}%，实际 {actual}%"));
            }
            Ok(())
        });
        restore.map_err(|error| format!("恢复原亮度未确认：{error}"))?;
        trial?;
    }
    println!("DDC 亮度写入、回读及原亮度恢复均通过");
    Ok(())
}

/// 获取屏幕右下角靠近系统托盘的工作区坐标
fn native_window(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    let handle = raw_window_handle::HasWindowHandle::window_handle(window).ok()?;
    if let raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() {
        Some(windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _))
    } else {
        None
    }
}

fn get_tray_popup_origin_with_size(width: f32, height: f32) -> Point<Pixels> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::{
            Foundation::POINT,
            Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            },
            UI::{
                HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
                WindowsAndMessaging::GetCursorPos,
            },
        };
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let (mut dx, mut dy) = (96, 96);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let scale = (dx.max(96) as f32) / 96.0;
            return point(
                px((info.rcWork.right as f32 / scale - width - 12.0).max(info.rcWork.left as f32 / scale)),
                px((info.rcWork.bottom as f32 / scale - height - 12.0).max(info.rcWork.top as f32 / scale)),
            );
        }
    }
    point(px(1000.0), px(500.0))
}

fn get_tray_popup_origin() -> Point<Pixels> {
    get_tray_popup_origin_with_size(320.0, 440.0)
}

/// 永久隐藏的 Anchor View，用于在主窗口关闭到托盘时保持 GPUI 运行时存活
struct AnchorView;

impl Render for AnchorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn main() {
    let _adb_lifetime = adb::client::AdbLifetime;
    let args: Vec<String> = std::env::args().collect();

    // 默认诊断 IP 优先读取已迁移配置
    let default_ip = {
        let cfg = config::load_settings();
        if !cfg.saved_ip.trim().is_empty() {
            cfg.saved_ip.clone()
        } else {
            "192.168.5.19".to_string()
        }
    };

    // 检查只读硬件诊断参数
    if args.iter().any(|arg| arg == "--diagnose") {
        let ip = args
            .iter()
            .position(|arg| arg == "--diagnose")
            .and_then(|p| args.get(p + 1))
            .filter(|s| !s.starts_with("--"))
            .cloned()
            .unwrap_or(default_ip);
        run_diagnose_cli(&ip);
        return;
    }

    // 检查 JSON 诊断参数
    if args.iter().any(|arg| arg == "--diagnose-json") {
        run_diagnose_json_cli();
        return;
    }

    // 检查真实硬件可恢复亮度验证参数 (--adb-verify-brightness [ip])
    if let Some(pos) = args.iter().position(|arg| arg == "--adb-verify-brightness") {
        let ip = if pos + 1 < args.len() && !args[pos + 1].starts_with("--") {
            args[pos + 1].clone()
        } else {
            default_ip
        };
        if let Err(e) = run_adb_verify_brightness_cli(&ip) {
            eprintln!("[失败] {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "--ddc-verify-brightness") {
        if let Err(error) = run_ddc_verify_brightness_cli() {
            eprintln!("[失败] {error}");
            std::process::exit(1);
        }
        return;
    }
    let start_in_tray = args.iter().any(|arg| arg == "--tray");
    let is_open_brightness = args.iter().any(|arg| arg == "--open-brightness");
    let instance = match instance::Instance::acquire(start_in_tray, is_open_brightness) {
        Ok(Some(instance)) => Arc::new(instance),
        Ok(None) => return,
        Err(error) => {
            eprintln!("无法建立单实例通信：{error}");
            return;
        }
    };

    // 1. 初始化跨平台日志
    logger::log_info("启动 Mimonitor Toolbox DDC (纯 Rust + 原生 GPUI)...");

    // 2. 加载全局配置
    let config = {
        let mut cfg = (*config::load_settings()).clone();
        let ip = monitor_ip(&cfg.saved_ip).unwrap_or_default();
        if !ip.is_empty() && ip != cfg.saved_ip {
            if let Err(error) = config::update_settings(|current| current.saved_ip = ip.clone()) {
                logger::log_info(&format!("连接地址迁移未保存：{error}"));
            }
        }
        cfg.saved_ip = ip;
        Arc::new(cfg)
    };
    logger::log_info(&format!(
        "加载配置文件成功，已保存 IP: '{}', 预设数量: {}, 自动任务数量: {}",
        config.saved_ip,
        config.presets.len(),
        config.auto_tasks.len()
    ));

    crate::ui::theme::set_preference(
        config
            .extra
            .get("theme")
            .and_then(|v| v.as_str())
            .unwrap_or("dark"),
    );
    // 3. 初始化消息总线与系统托盘管理器
    let bus = Arc::new(MessageBus::new());
    logger::log_info("后台工作队列已就绪");
    let tray_manager = Arc::new(SystemTrayManager::new());
    tray_manager.start();

    // 4. 启动 GPUI 应用程序
    Application::new().run(move |cx: &mut App| {
        // 创建永久隐藏的 anchor 窗口，保持 GPUI 事件循环存活（防止主窗口关闭到托盘或弹窗失焦移除时作为最后窗口触发 PostQuitMessage）
        let _ = cx.open_window(
            WindowOptions {
                show: false,
                focus: false,
                // GPUI's normal Windows windows use WS_EX_APPWINDOW. The
                // keep-alive window must never become a taskbar/Alt-Tab entry.
                kind: gpui::WindowKind::PopUp,
                titlebar: None,
                is_resizable: false,
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| AnchorView),
        );

        // 绑定全局文本快捷键 (Ctrl+A, Ctrl+C, Ctrl+V 等)
        logger::log_info("GPUI 运行时已进入");
        bind_text_input_keys(cx);

        // 创建全局应用状态实体与预设管理页面实体
        let app_state =
            cx.new(|cx| AppState::new(bus.clone(), tray_manager.clone(), (*config).clone(), cx));
        logger::log_info("应用页面已创建");
        let preset_view = cx.new(|cx| PresetPageView::new(app_state.downgrade(), bus.clone(), cx));
        app_state.update(cx, |st, cx| {
            st.preset_view = Some(preset_view);
            cx.notify();
        });

        let main_window_handle: Arc<Mutex<Option<AnyWindowHandle>>> = Arc::new(Mutex::new(None));
        let osd_handle: Arc<Mutex<Option<gpui::WindowHandle<crate::ui::osd::Osd>>>> =
            Arc::new(Mutex::new(None));
        let tray_popup_handle: Arc<Mutex<Option<AnyWindowHandle>>> = Arc::new(Mutex::new(None));
        let popup_focus_state: Arc<Mutex<(bool, std::time::Instant)>> =
            Arc::new(Mutex::new((false, std::time::Instant::now())));
        let value_popup_handle: Arc<Mutex<Option<AnyWindowHandle>>> = Arc::new(Mutex::new(None));
        let value_popup_focus_state: Arc<Mutex<(bool, std::time::Instant)>> =
            Arc::new(Mutex::new((false, std::time::Instant::now())));

        let main_handle_for_open = main_window_handle.clone();
        let app_state_for_open = app_state.clone();
        let bus_for_open = bus.clone();
        let tray_for_open = tray_manager.clone();

        // 封装打开主窗口函数 (单例防死锁，释放锁后打开，支持关闭后重新打开)
        let open_main_window = move |cx: &mut App| {
            let existing_handle = {
                let handle_guard = main_handle_for_open.lock();
                *handle_guard
            };

            if let Some(handle) = existing_handle {
                let activated = handle.update(cx, |_, window, cx| {
                    window.activate_window();
                    cx.activate(true);
                });
                if activated.is_ok() {
                    return;
                }
                *main_handle_for_open.lock() = None;
            }

            let bounds = Bounds::centered(None, size(px(860.0), px(600.0)), cx);
            let state_entity = app_state_for_open.clone();
            let handle_storage = main_handle_for_open.clone();
            let bus_close = bus_for_open.clone();
            let tray_close = tray_for_open.clone();

            let win = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some("Mimonitor 工具箱".into()),
                            appears_transparent: true,
                            traffic_light_position: None,
                        }),
                        ..Default::default()
                    },
                    |window, cx| {
                        if let Some(hwnd) = native_window(window) {
                            crate::tray::set_window_icon(hwnd);
                        }
                        let handle_close = handle_storage.clone();
                        let state_close = state_entity.clone();
                        let bus_for_exit = bus_close.clone();
                        let tray_for_exit = tray_close.clone();

                        window.on_window_should_close(cx, move |window, cx| {
                            let behavior = state_close.read(cx).config.close_behavior;
                            match behavior {
                                crate::config::CloseBehavior::Tray => {
                                    // Hide synchronously: GPUI destroys the HWND
                                    // on its executor after this close callback.
                                    if let Some(hwnd) = native_window(window) {
                                        unsafe {
                                            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
                                            let _ = ShowWindow(hwnd, SW_HIDE);
                                        }
                                    }
                                    *handle_close.lock() = None;
                                    window.remove_window();
                                    false
                                }
                                crate::config::CloseBehavior::Exit => {
                                    *handle_close.lock() = None;
                                    bus_for_exit.shutdown();
                                    tray_for_exit.shutdown();
                                    cx.quit();
                                    true
                                }
                            }
                        });

                        cx.new(|cx| MainWindow::new(state_entity, cx))
                    },
                )
                .expect("初始化 GPUI 主窗口失败");

            *handle_storage.lock() = Some(win.into());
        };

        let open_main_window_for_tray = Arc::new(Mutex::new(open_main_window.clone()));

        // 封装切换/打开托盘弹窗函数
        let tray_popup_storage = tray_popup_handle.clone();
        let app_state_for_popup = app_state.clone();
        let open_main_for_popup = open_main_window_for_tray.clone();
        let popup_focus_tracker = popup_focus_state.clone();
        let main_for_popup_owner = main_window_handle.clone();
        let value_popup_storage_for_brightness = value_popup_handle.clone();

        let toggle_tray_popup = move |cx: &mut App| {
            if let Some(existing_val) = value_popup_storage_for_brightness.lock().take() {
                let _ = existing_val.update(cx, |_, window, _| {
                    window.remove_window();
                });
            }

            let existing = {
                let mut popup_guard = tray_popup_storage.lock();
                popup_guard.take()
            };

            if let Some(existing_handle) = existing {
                let _ = existing_handle.update(cx, |_, window, _| {
                    window.remove_window();
                });
                return;
            }

            let origin = get_tray_popup_origin();
            let bounds = Bounds::new(origin, size(px(320.0), px(440.0)));
            let app_state_clone = app_state_for_popup.clone();
            let open_main_cb = open_main_for_popup.clone();
            let popup_storage_close = tray_popup_storage.clone();
            let owner = (*main_for_popup_owner.lock()).and_then(|handle| {
                handle
                    .update(cx, |_, window, _| native_window(window))
                    .ok()
                    .flatten()
            });

            let win = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("统一亮度面板".into()),
                        appears_transparent: true,
                        traffic_light_position: None,
                    }),
                    kind: gpui::WindowKind::PopUp,
                    is_resizable: false,
                    ..Default::default()
                },
                |window, cx| {
                    if let Some(hwnd) = native_window(window) {
                        logger::log_info(&format!("统一亮度弹窗 HWND={}", hwnd.0 as isize));
                        unsafe {
                            use windows::Win32::UI::WindowsAndMessaging::*;
                            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                            SetWindowLongPtrW(hwnd, GWL_STYLE, style | WS_POPUP.0 as isize);
                            if let Some(owner) = owner {
                                SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner.0 as isize);
                            }
                            let _ = SetWindowPos(
                                hwnd,
                                Some(HWND_TOPMOST),
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
                            );
                        }
                    }
                    let popup_storage = popup_storage_close.clone();
                    window.on_window_should_close(cx, move |_window, _cx| {
                        *popup_storage.lock() = None;
                        true
                    });

                    let popup_storage_for_close = popup_storage_close.clone();
                    let popup_storage_for_toolbox = popup_storage_close.clone();

                    let view = cx.new(|cx| {
                        TrayBrightnessPopupView::new(
                            app_state_clone,
                            cx,
                            Some(Box::new(move |window, cx| {
                                *popup_storage_for_toolbox.lock() = None;
                                window.remove_window();
                                let cb = open_main_cb.lock();
                                cb(cx);
                            })),
                            Some(Box::new(move |window, _cx| {
                                *popup_storage_for_close.lock() = None;
                                window.remove_window();
                            })),
                        )
                    });

                    window.focus(&view.read(cx).focus_handle);
                    view
                },
            );

            if let Ok(w) = win {
                logger::log_info("已创建统一亮度弹窗");
                let _ = w.update(cx, |_, window, _| window.activate_window());
                *tray_popup_storage.lock() = Some(w.into());
                *popup_focus_tracker.lock() = (false, std::time::Instant::now());
            }
        };

        // 封装切换/打开通用数值浮窗函数
        let value_popup_storage = value_popup_handle.clone();
        let app_state_for_value = app_state.clone();
        let open_main_for_value = open_main_window_for_tray.clone();
        let value_popup_focus_tracker = value_popup_focus_state.clone();
        let main_for_value_owner = main_window_handle.clone();
        let tray_popup_for_value = tray_popup_handle.clone();

        let open_value_popup = move |param: crate::hotkey::AdjustParam, cx: &mut App| {
            // 打开数值浮窗前，关闭已有亮度窗，避免互相抢焦点
            if let Some(existing_brightness) = tray_popup_for_value.lock().take() {
                let _ = existing_brightness.update(cx, |_, window, _| {
                    window.remove_window();
                });
            }

            // 同类窗口重新打开时关闭旧窗口
            let existing = {
                let mut popup_guard = value_popup_storage.lock();
                popup_guard.take()
            };

            if let Some(existing_handle) = existing {
                let _ = existing_handle.update(cx, |_, window, _| {
                    window.remove_window();
                });
            }

            let origin = get_tray_popup_origin_with_size(360.0, 360.0);
            let bounds = Bounds::new(origin, size(px(360.0), px(360.0)));
            let app_state_clone = app_state_for_value.clone();
            let open_main_cb = open_main_for_value.clone();
            let popup_storage_close = value_popup_storage.clone();
            let owner = (*main_for_value_owner.lock()).and_then(|handle| {
                handle
                    .update(cx, |_, window, _| native_window(window))
                    .ok()
                    .flatten()
            });

            let param_title = format!("{}调节", param.label());
            let win = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some(param_title.into()),
                        appears_transparent: true,
                        traffic_light_position: None,
                    }),
                    kind: gpui::WindowKind::PopUp,
                    is_resizable: false,
                    ..Default::default()
                },
                |window, cx| {
                    if let Some(hwnd) = native_window(window) {
                        logger::log_info(&format!("数值调节浮窗 HWND={}", hwnd.0 as isize));
                        unsafe {
                            use windows::Win32::UI::WindowsAndMessaging::*;
                            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                            SetWindowLongPtrW(hwnd, GWL_STYLE, style | WS_POPUP.0 as isize);
                            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                            SetWindowLongPtrW(
                                hwnd,
                                GWL_EXSTYLE,
                                ex_style | WS_EX_TOOLWINDOW.0 as isize,
                            );
                            if let Some(owner) = owner {
                                SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner.0 as isize);
                            }
                            let _ = SetWindowPos(
                                hwnd,
                                Some(HWND_TOPMOST),
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
                            );
                        }
                    }
                    let popup_storage = popup_storage_close.clone();
                    window.on_window_should_close(cx, move |_window, _cx| {
                        *popup_storage.lock() = None;
                        true
                    });

                    let popup_storage_for_close = popup_storage_close.clone();
                    let popup_storage_for_toolbox = popup_storage_close.clone();

                    let view = cx.new(|cx| {
                        TrayValuePopupView::new(
                            app_state_clone,
                            param,
                            cx,
                            Some(Box::new(move |window, cx| {
                                *popup_storage_for_toolbox.lock() = None;
                                window.remove_window();
                                let cb = open_main_cb.lock();
                                cb(cx);
                            })),
                            Some(Box::new(move |window, _cx| {
                                *popup_storage_for_close.lock() = None;
                                window.remove_window();
                            })),
                        )
                    });

                    window.focus(&view.read(cx).focus_handle);
                    view
                },
            );

            if let Ok(w) = win {
                logger::log_info("已创建数值调节浮窗");
                let _ = w.update(cx, |_, window, _| window.activate_window());
                *value_popup_storage.lock() = Some(w.into());
                *value_popup_focus_tracker.lock() = (false, std::time::Instant::now());
            }
        };

        // 如果启动参数包含 --open-brightness，直接打开托盘亮度面板
        if is_open_brightness {
            toggle_tray_popup(cx);
        } else if !start_in_tray {
            open_main_window(cx);
        }
        if !config.saved_ip.trim().is_empty() {
            let generation = bus.active_generation.load(Ordering::Acquire);
            app_state.update(cx, |state, cx| {
                state.is_adb_connecting = true;
                cx.notify();
            });
            let _ = bus.cmd_sender.send(AppCommand::ConnectAdb {
                ip: config.saved_ip.clone(),
                generation,
            });
        }

        // 后台总线事件与托盘事件桥接泵 (每 40ms 轮询一次事件队列并安全驱动 GPUI 主线程更新)
        let bus_pump = bus.clone();
        let tray_pump = tray_manager.clone();
        let app_state_pump = app_state.clone();
        let toggle_popup_for_pump = Arc::new(Mutex::new(toggle_tray_popup));
        let open_value_popup_for_pump = Arc::new(Mutex::new(open_value_popup));
        let open_main_for_pump = open_main_window_for_tray.clone();

        let async_cx = cx.to_async();
        let bg_exec = cx.background_executor().clone();

        cx.foreground_executor()
            .spawn(async move {
                loop {
                    if instance.take_main() {
                        let _ = async_cx.update(|cx| (open_main_for_pump.lock())(cx));
                    }
                    if instance.take_brightness() {
                        let _ = async_cx.update(|cx| (toggle_popup_for_pump.lock())(cx));
                    }
                    // 0. 托盘面板失焦自动关闭检查（失焦关闭并清空 storage，下次点击立即可重新打开）
                    let popup_handle_opt = {
                        let guard = tray_popup_handle.lock();
                        *guard
                    };
                    if let Some(popup_h) = popup_handle_opt {
                        let _ = async_cx.update(|cx| {
                            let active_res =
                                popup_h.update(cx, |_, window, _| window.is_window_active());
                            match active_res {
                                Ok(is_active) => {
                                    let mut focus_state = popup_focus_state.lock();
                                    if is_active {
                                        focus_state.0 = true;
                                    } else if focus_state.1.elapsed() > Duration::from_millis(700) {
                                        logger::log_info("统一亮度弹窗失焦关闭");
                                        let _ = popup_h.update(cx, |_, window, _| {
                                            window.remove_window();
                                        });
                                        *tray_popup_handle.lock() = None;
                                    }
                                }
                                Err(_) => {
                                    *tray_popup_handle.lock() = None;
                                }
                            }
                        });
                    }

                    let value_popup_handle_opt = {
                        let guard = value_popup_handle.lock();
                        *guard
                    };
                    if let Some(val_popup_h) = value_popup_handle_opt {
                        let _ = async_cx.update(|cx| {
                            let active_res =
                                val_popup_h.update(cx, |_, window, _| window.is_window_active());
                            match active_res {
                                Ok(is_active) => {
                                    let mut focus_state = value_popup_focus_state.lock();
                                    if is_active {
                                        focus_state.0 = true;
                                    } else if focus_state.1.elapsed() > Duration::from_millis(700) {
                                        logger::log_info("数值调节浮窗失焦关闭");
                                        let _ = val_popup_h.update(cx, |_, window, _| {
                                            window.remove_window();
                                        });
                                        *value_popup_handle.lock() = None;
                                    }
                                }
                                Err(_) => {
                                    *value_popup_handle.lock() = None;
                                }
                            }
                        });
                    }

                    // 1. 处理后台业务事件
                    for _ in 0..64 {
                        let Some(evt) = bus_pump.try_recv_event() else { break; };
                        if matches!(evt, AppEvent::OpenBrightness) {
                            let _ = async_cx.update(|cx| (toggle_popup_for_pump.lock())(cx));
                            continue;
                        }
                        if let AppEvent::HotkeyOsd { message } = &evt {
                            let _ = async_cx.update(|cx| {
                                let existing = *osd_handle.lock();
                                if let Some(message) = message {
                                    if let Some(handle) = existing {
                                        if handle
                                            .update(cx, |view, _, cx| {
                                                view.message = message.clone();
                                                cx.notify();
                                            })
                                            .is_ok()
                                        {
                                            return;
                                        }
                                    }
                                    let origin = get_tray_popup_origin_with_size(320.0, 130.0);
                                    let bus = bus_pump.clone();
                                    let message = message.clone();
                                    if let Ok(handle) = cx.open_window(
                                        WindowOptions {
                                            window_bounds: Some(WindowBounds::Windowed(
                                                Bounds::new(origin, size(px(320.0), px(130.0))),
                                            )),
                                            kind: gpui::WindowKind::PopUp,
                                            titlebar: None,
                                            is_resizable: false,
                                            focus: false,
                                            ..Default::default()
                                        },
                                        |_, cx| cx.new(|_| crate::ui::osd::Osd::new(message, bus)),
                                    ) {
                                        *osd_handle.lock() = Some(handle);
                                    }
                                } else {
                                    if let Some(handle) = existing {
                                        let _ = handle
                                            .update(cx, |_, window, _| window.remove_window());
                                    }
                                    *osd_handle.lock() = None;
                                }
                            });
                            continue;
                        }
                        if matches!(evt, AppEvent::AdbStateChanged { .. }) {
                            let _ = async_cx.update(|cx| {
                                let view = app_state_pump.read(cx).preset_view.clone();
                                if let Some(view) = view {
                                    view.update(cx, |view, cx| view.handle_event(&evt, cx));
                                }
                            });
                        }
                        if matches!(
                            evt,
                            AppEvent::PresetSnapshotCaptured { .. }
                                | AppEvent::PresetConfigResult { .. }
                                | AppEvent::PresetApplied { .. }
                                | AppEvent::BaselineRestored { .. }
                                | AppEvent::PresetOperationFailed { .. }
                        ) {
                            let _ = async_cx.update(|cx| {
                                let view = app_state_pump.read(cx).preset_view.clone();
                                if let Some(view) = view {
                                    view.update(cx, |view, cx| view.handle_event(&evt, cx));
                                }
                            });
                            continue;
                        }
                        let _ = async_cx.update(|cx| {
                            app_state_pump.update(cx, |st, cx| match evt {
                                AppEvent::TrayValueResult { request_id, generation, error } => {
                                    if generation == bus_pump.active_generation.load(Ordering::Acquire) {
                                        st.tray_value_results.retain(|_, (epoch, _)| *epoch == generation);
                                        st.tray_value_results.insert(request_id, (generation, error));
                                        while st.tray_value_results.len() > 128 {
                                            st.tray_value_results.pop_first();
                                        }
                                        cx.notify();
                                    }
                                }
                                AppEvent::GuardianStatusUpdated { status, generation } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Acquire)
                                    {
                                        st.tools_page
                                            .update(cx, |view, cx| view.set_guardian(status, cx));
                                    }
                                }
                                AppEvent::DeviceActivity {
                                    generation,
                                    active,
                                    message,
                                    is_error,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Acquire)
                                    {
                                        st.device_busy = active;
                                        if !message.is_empty() {
                                            st.toast = Some((message, is_error));
                                        }
                                    }
                                    cx.notify();
                                }
                                AppEvent::OpenBrightness => {}
                                AppEvent::AdbStateChanged {
                                    ip,
                                    state,
                                    model,
                                    generation,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                    {
                                        st.device_busy = false;
                                        let is_connected = state.is_connected();
                                        if let adb::AdbDeviceState::Error(message) = &state {
                                            st.toast =
                                                Some((format!("ADB 连接失败：{message}"), true));
                                        }
                                        st.tools_page.update(cx, |view, cx| {
                                            view.set_connected(is_connected, generation, Some(&ip), cx)
                                        });
                                        st.picture_page.update(cx, |page, cx| {
                                            page.advanced.update(cx, |view, cx| {
                                                view.set_connected(is_connected, generation, cx)
                                            })
                                        });
                                        st.source_page.update(cx, |sp, cx| {
                                            sp.set_connected(is_connected, generation, cx);
                                        });
                                        st.remote_page.update(cx, |rp, cx| {
                                            rp.set_connected(is_connected, generation, cx);
                                        });
                                        st.adb_ip = ip;
                                        st.adb_state = state.clone();
                                        st.adb_model = model;
                                        st.is_adb_connecting = false;
                                        if state.is_connected() {
                                            st.light_state.is_reading = true;
                                            let rev = st.light_state.last_revision + 1;
                                            st.light_state.last_revision = rev;
                                            let _ = bus_pump.cmd_sender.send(
                                                AppCommand::FetchScreenLight {
                                                    generation,
                                                    revision: rev,
                                                },
                                            );
                                        } else {
                                            st.light_state = ScreenLightState::default();
                                        }
                                        st.picture_page.update(cx, |page, cx| {
                                            page.on_adb_state_changed(&state, generation, cx);
                                        });
                                        let is_conn = st.adb_state.is_connected();
                                        st.game_page.update(cx, |page, cx| {
                                            page.handle_connection_state(is_conn, generation, cx);
                                        });
                                        cx.notify();
                                    }
                                }
                                AppEvent::SourceUpdated {
                                    source,
                                    raw_value,
                                    generation,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                    {
                                        st.source_page.update(cx, |sp, cx| {
                                            sp.on_source_updated(
                                                source, &raw_value, generation, cx,
                                            );
                                        });
                                    }
                                }
                                AppEvent::SourceSwitchSuccess { source, generation } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                    {
                                        st.source_page.update(cx, |sp, cx| {
                                            sp.on_switch_success(source, generation, cx);
                                        });
                                    }
                                }
                                AppEvent::SourceSwitchFailed {
                                    source,
                                    error,
                                    generation,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                    {
                                        st.source_page.update(cx, |sp, cx| {
                                            sp.on_switch_failed(source, &error, generation, cx);
                                        });
                                    }
                                }
                                AppEvent::SourceReadFailed { error, generation } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                    {
                                        st.source_page.update(cx, |sp, cx| {
                                            sp.on_read_failed(&error, generation, cx);
                                        });
                                    }
                                }
                                AppEvent::RemoteKeySuccess { key, generation } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                    {
                                        st.remote_page.update(cx, |rp, cx| {
                                            rp.on_key_success(key, generation, cx);
                                        });
                                    }
                                }
                                AppEvent::RemoteKeyFailed {
                                    key,
                                    error,
                                    generation,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                    {
                                        st.remote_page.update(cx, |rp, cx| {
                                            rp.on_key_failed(key, &error, generation, cx);
                                        });
                                    }
                                }
                                AppEvent::BacklightUpdated {
                                    value,
                                    generation,
                                    revision,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                        && revision
                                            >= bus_pump.light_revision.load(Ordering::Acquire)
                                    {
                                        st.adb_backlight = value;
                                        st.picture_page.update(cx, |page, cx| {
                                            page.on_backlight_updated(value, generation, cx);
                                        });
                                        cx.notify();
                                    }
                                }
                                AppEvent::AdbOperationFailed {
                                    message,
                                    generation,
                                } => {
                                    if generation
                                        != bus_pump.active_generation.load(Ordering::Acquire)
                                    {
                                        return;
                                    }
                                    st.toast = Some((message.clone(), true));
                                    st.is_adb_connecting = false;
                                    st.picture_page.update(cx, |page, cx| {
                                        page.on_adb_operation_failed(&message, generation, cx);
                                    });
                                    st.game_page.update(cx, |page, cx| {
                                        page.handle_operation_failed(&message, generation, cx);
                                    });
                                    cx.notify();
                                }
                                AppEvent::WindowsDisplaysDiscovered(displays) => {
                                    st.display_summary = if displays.is_empty() {
                                        "未发现显示器 · 可使用 IP 连接".into()
                                    } else {
                                        displays
                                            .iter()
                                            .map(|d| d.label.clone())
                                            .collect::<Vec<_>>()
                                            .join(" / ")
                                    };
                                    cx.notify();
                                    st.tools_page
                                        .update(cx, |view, cx| view.set_displays(displays, cx));
                                }
                                AppEvent::DdcMonitorsDiscovered(monitors) => {
                                    st.ddc_refresh_count += 1;
                                    let mut updated_monitors = Vec::new();
                                    for mut mon in monitors {
                                        let coalescer =
                                            crate::ddc::get_ddc_device_coalescer(&mon.device_id);
                                        let cur_rev =
                                            coalescer.last_revision.load(Ordering::Relaxed);
                                        if cur_rev > mon.write_revision
                                            || coalescer.should_suppress_read_update(
                                                mon.write_revision,
                                                Duration::from_millis(1500),
                                            )
                                        {
                                            if let Some((value, _)) =
                                                *coalescer.pending_target.lock()
                                            {
                                                mon.current_percent = value;
                                            } else if let Some(previous) = st
                                                .ddc_monitors
                                                .iter()
                                                .find(|m| m.device_id == mon.device_id)
                                            {
                                                mon.current_percent = previous.current_percent;
                                            } else if let Some(value) =
                                                *coalescer.last_written_pct.lock()
                                            {
                                                mon.current_percent = value;
                                            }
                                        }
                                        updated_monitors.push(mon);
                                    }
                                    st.ddc_monitors = updated_monitors;
                                    cx.notify();
                                }
                                AppEvent::DdcBrightnessChanged {
                                    device_id,
                                    percent,
                                    revision,
                                } => {
                                    let coalescer =
                                        crate::ddc::get_ddc_device_coalescer(&device_id);
                                    if revision >= coalescer.last_revision.load(Ordering::Relaxed) {
                                        if let Some(pos) = st
                                            .ddc_monitors
                                            .iter()
                                            .position(|m| m.device_id == device_id)
                                        {
                                            st.ddc_monitors[pos].current_percent = percent;
                                            st.ddc_monitors[pos].write_revision = revision;
                                            cx.notify();
                                        }
                                    }
                                }
                                AppEvent::DdcOperationFailed { device_id, error } => {
                                    let _ = bus_pump.cmd_sender.send(AppCommand::RefreshMonitors);
                                    st.toast = Some((
                                        format!("DDC [{}] 操作失败: {}", device_id, error),
                                        true,
                                    ));
                                    cx.notify();
                                }
                                AppEvent::ScanDeviceDiscovered { device, generation } => {
                                    if generation
                                        == bus_pump.current_scan_generation.load(Ordering::Relaxed)
                                    {
                                        if !st.discovered_devices.iter().any(|d| d.ip == device.ip)
                                        {
                                            st.discovered_devices.push(device);
                                            cx.notify();
                                        }
                                    }
                                }
                                AppEvent::ScanCompleted {
                                    found_count,
                                    generation,
                                } => {
                                    if generation
                                        == bus_pump.current_scan_generation.load(Ordering::Relaxed)
                                    {
                                        let completed_by_user_scan = st.is_adb_scanning;
                                        st.is_adb_scanning = false;
                                        if completed_by_user_scan && !st.adb_state.is_connected() && !st.is_adb_connecting {
                                            if let Some(ip) = single_scan_target(&st.discovered_devices) {
                                                st.connect_ip(&ip, cx);
                                            } else {
                                                st.toast = Some((if found_count == 0 {
                                                    "未发现显示器，请检查显示器与电脑是否连接同一网络".into()
                                                } else {
                                                    format!("发现 {found_count} 台设备，点击要连接的显示器")
                                                }, found_count == 0));
                                            }
                                        }
                                        cx.notify();
                                    }
                                }
                                AppEvent::ToastNotification { message, is_error } => {
                                    st.toast = Some((message, is_error));
                                    cx.notify();
                                }
                                AppEvent::HotkeyOsd { .. } => {}
                                AppEvent::ConfigUpdated(config) => {
                                    crate::ui::theme::set_preference(
                                        config
                                            .extra
                                            .get("theme")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("dark"),
                                    );
                                    cx.refresh_windows();
                                    st.tray_page
                                        .update(cx, |view, cx| view.sync(config.clone(), cx));
                                    st.auto_task_page
                                        .update(cx, |view, cx| view.sync(config.clone(), cx));
                                    st.tools_page
                                        .update(cx, |view, cx| view.set_config(config.clone(), cx));
                                    st.game_page.update(cx, |view, cx| {
                                        view.config = config.clone();
                                        cx.notify();
                                    });
                                    st.config = config;
                                    cx.notify();
                                }
                                AppEvent::ToolResult {
                                    report,
                                    is_error,
                                    generation,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Acquire)
                                    {
                                        st.tools_page.update(cx, |view, cx| {
                                            view.receive(report.clone(), cx)
                                        });
                                        st.picture_page.update(cx, |page, cx| {
                                            page.advanced.update(cx, |view, cx| view.completed(cx))
                                        });
                                        st.toast = Some((report, is_error));
                                        cx.notify();
                                    }
                                }
                                AppEvent::ScreenLightUpdated {
                                    state,
                                    generation,
                                    revision,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                        && revision >= st.light_state.last_revision
                                    {
                                        let prev_status = st.light_state.last_status.take();
                                        st.light_state = state;
                                        st.light_state.is_reading = false;
                                        st.light_state.is_writing = false;
                                        st.light_state.last_revision = revision;
                                        if st.light_state.last_status.is_none() {
                                            st.light_state.last_status = prev_status;
                                        }
                                        cx.notify();
                                    }
                                }
                                AppEvent::ScreenLightCommitted {
                                    params,
                                    generation,
                                    revision,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                        && revision >= st.light_state.last_revision
                                    {
                                        st.light_state.is_writing = false;
                                        st.light_state.mode = Some(params.mode);
                                        st.light_state.illumination = Some(params.illumination);
                                        st.light_state.color_temp = Some(params.color_temp);
                                        st.light_state.color_value = Some(params.color_value);
                                        st.light_state.last_revision = revision;
                                        st.light_state.last_status =
                                            Some("设置已提交，请观察灯光效果".into());
                                        st.light_state.last_error = None;
                                        cx.notify();
                                    }
                                }
                                AppEvent::ScreenLightOperationFailed {
                                    message,
                                    generation,
                                    revision,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                        && revision >= st.light_state.last_revision
                                    {
                                        st.light_state.is_reading = false;
                                        st.light_state.is_writing = false;
                                        st.light_state.last_revision = revision;
                                        st.light_state.last_error = Some(message.clone());
                                        st.toast = Some((message, true));
                                        cx.notify();
                                    }
                                }
                                AppEvent::PictureParametersFetched { state, generation } => {
                                    st.picture_page.update(cx, |page, cx| {
                                        page.on_parameters_fetched(*state, generation, cx);
                                    });
                                    cx.notify();
                                }
                                AppEvent::PictureFetchFailed {
                                    message,
                                    generation,
                                } => {
                                    st.picture_page.update(cx, |page, cx| {
                                        page.on_fetch_failed(message, generation, cx);
                                    });
                                    cx.notify();
                                }
                                AppEvent::PictureSliderUpdated {
                                    param,
                                    value,
                                    generation,
                                    revision,
                                } => {
                                    st.picture_page.update(cx, |page, cx| {
                                        page.on_slider_updated(
                                            param, value, generation, revision, cx,
                                        );
                                    });
                                    cx.notify();
                                }
                                AppEvent::PictureModeUpdated {
                                    mode,
                                    generation,
                                    revision,
                                } => {
                                    st.picture_page.update(cx, |page, cx| {
                                        page.on_mode_updated(mode, generation, revision, cx);
                                    });
                                    cx.notify();
                                }
                                AppEvent::LightSensorUpdated {
                                    enabled,
                                    generation,
                                    revision,
                                } => {
                                    st.picture_page.update(cx, |page, cx| {
                                        page.on_light_sensor_updated(
                                            enabled, generation, revision, cx,
                                        );
                                    });
                                    cx.notify();
                                }
                                AppEvent::LocalDimmingUpdated {
                                    value,
                                    generation,
                                    revision,
                                } => {
                                    st.picture_page.update(cx, |page, cx| {
                                        page.on_local_dimming_updated(
                                            value, generation, revision, cx,
                                        );
                                    });
                                    cx.notify();
                                }
                                AppEvent::GameParametersUpdated {
                                    params,
                                    generation,
                                    revision,
                                } => {
                                    if generation
                                        == bus_pump.active_generation.load(Ordering::Relaxed)
                                        && revision
                                            >= crate::message::GAME_REVISION.load(Ordering::Acquire)
                                    {
                                        st.game_page.update(cx, |page, cx| {
                                            page.update_params(params, cx);
                                        });
                                    }
                                }
                                AppEvent::PresetSnapshotCaptured { .. }
                                | AppEvent::PresetConfigResult { .. }
                                | AppEvent::PresetApplied { .. }
                                | AppEvent::BaselineRestored { .. }
                                | AppEvent::PresetOperationFailed { .. } => {
                                    if let Some(pv) = st.preset_view.clone() {
                                        pv.update(cx, |view, cx| {
                                            view.handle_event(&evt, cx);
                                        });
                                    }
                                }
                            });
                        });
                    }

                    // 2. 处理托盘事件 (安全桥接分发至 GPUI 线程)
                    while let Some(tray_evt) = tray_pump.try_recv_event() {
                        let _ = async_cx.update(|cx| match tray_evt {
                            TrayEvent::OpenMainWindow => {
                                let cb = open_main_for_pump.lock();
                                cb(cx);
                            }
                            TrayEvent::ToggleBrightnessPanel => {
                                let cb = toggle_popup_for_pump.lock();
                                cb(cx);
                            }
                            TrayEvent::OpenValueSlider(param) => {
                                let cb = open_value_popup_for_pump.lock();
                                cb(param, cx);
                            }
                            TrayEvent::RefreshMonitors => {
                                let _ = bus_pump.cmd_sender.send(AppCommand::RefreshMonitors);
                            }
                            TrayEvent::Custom(choice) => {
                                let generation = bus_pump.active_generation.load(Ordering::Acquire);
                                let _ = bus_pump.cmd_sender.send(choice.command(generation));
                            }
                            TrayEvent::Quit => {
                                bus_pump.shutdown();
                                tray_pump.shutdown();
                                cx.quit();
                            }
                        });
                    }

                    // 休眠 40ms 维持低负载高平滑事件响应
                    bg_exec.timer(Duration::from_millis(40)).await;
                }
            })
            .detach();
    });
}
