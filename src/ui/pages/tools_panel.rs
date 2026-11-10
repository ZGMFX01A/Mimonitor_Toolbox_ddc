use crate::{
    config::{AppConfig, CloseBehavior},
    message::{actions::ToolAction, AppCommand, AppEvent, MessageBus},
    ui::components::{badge, card_container, option_button, TextInput},
    ui::theme::ThemeColors,
};
use gpui::prelude::*;
use gpui::{div, Context, Div, Entity, FontWeight, Render, SharedString, Window};
use std::{collections::HashMap, sync::Arc};

pub struct ToolsPage {
    guardian: Option<crate::device::guardian::GuardianStatus>,
    displays: Vec<crate::ddc::RawDisplayItem>,
    bus: Arc<MessageBus>,
    config: AppConfig,
    connected: bool,
    generation: u64,
    confirmation: Option<ToolAction>,
    pub report: String,
    connected_serial: Option<String>,
    pub busy: bool,
    shell_input: Entity<TextInput>,
    apk_input: Entity<TextInput>,
    scan_inputs: Vec<Entity<TextInput>>,
    hotkey_form: Entity<super::hotkeys::HotkeyForm>,
    seconds: Entity<TextInput>,
    terminal_expanded: bool,
}
pub fn save_patch(bus: Arc<MessageBus>, patch: HashMap<String, serde_json::Value>) {
    crate::lifecycle::spawn_ui_task(move || match crate::config::patch_settings(patch) {
        Ok(config) => {
            let _ = bus
                .event_sender
                .send(AppEvent::ConfigUpdated((*config).clone()));
        }
        Err(error) => {
            let _ = bus.event_sender.send(AppEvent::ToastNotification {
                message: error.to_string(),
                is_error: true,
            });
        }
    });
}
impl ToolsPage {
    pub fn new(bus: Arc<MessageBus>, config: AppConfig, cx: &mut Context<Self>) -> Self {
        let shell_input = cx.new(|cx| TextInput::new(cx, "例如 getprop ro.product.model", ""));
        let apk_input = cx.new(|cx| TextInput::new(cx, "选择或填写 APK 文件路径", ""));
        let scan_inputs = [
            ("强制扫描 IP，逗号分隔", config.scan_force_devices.join(",")),
            ("忽略 IP，逗号分隔", config.scan_block_devices.join(",")),
            (
                "强制扫描网段 CIDR，逗号分隔",
                config.scan_force_subnets.join(","),
            ),
        ]
        .into_iter()
        .map(|(p, v)| cx.new(|cx| TextInput::new(cx, p, v)))
        .collect();
        let hotkey_form = cx.new(|cx| super::hotkeys::HotkeyForm::new(bus.clone(), &config, cx));
        let seconds = cx.new(|cx| {
            TextInput::new(
                cx,
                "倒计时秒数",
                config.hotkey_countdown_seconds.to_string(),
            )
        });
        Self {
            guardian: None,
            displays: Vec::new(),
            bus,
            config,
            connected_serial: None,
            connected: false,
            generation: 0,
            confirmation: None,
            report: "点击刷新读取设备状态".into(),
            busy: false,
            shell_input,
            apk_input,
            scan_inputs,
            hotkey_form,
            seconds,
            terminal_expanded: false,
        }
    }
    pub fn set_connected(&mut self, connected: bool, generation: u64, ip: Option<&str>, cx: &mut Context<Self>) {
        if generation != self.generation || connected != self.connected {
            self.report = if connected {
                "点击刷新读取设备状态"
            } else {
                "请连接显示器后刷新状态"
            }
            .into();
            self.guardian = None;
            self.busy = false;
            self.confirmation = None;
        }
        self.connected_serial = ip.filter(|_| connected).map(|ip| crate::adb::format_adb_serial(ip, 5555));
        self.connected = connected;
        self.generation = generation;
        if connected && self.guardian.is_none() && !self.busy {
            self.request(ToolAction::CheckGuardian, cx);
        }
        cx.notify();
    }
    pub fn set_displays(
        &mut self,
        displays: Vec<crate::ddc::RawDisplayItem>,
        cx: &mut Context<Self>,
    ) {
        self.displays = displays
            .into_iter()
            .filter(|d| crate::ddc::is_xiaomi_device_id(&d.device_id))
            .collect();
        cx.notify();
    }
    pub fn set_config(&mut self, config: AppConfig, cx: &mut Context<Self>) {
        self.config = config;
        cx.notify();
    }
    pub fn set_guardian(
        &mut self,
        status: crate::device::guardian::GuardianStatus,
        cx: &mut Context<Self>,
    ) {
        self.guardian = Some(status);
        cx.notify();
    }
    pub fn receive(&mut self, report: String, cx: &mut Context<Self>) {
        self.report = report;
        self.busy = false;
        cx.notify();
    }
    fn request(&mut self, action: ToolAction, cx: &mut Context<Self>) {
        if !self.connected || self.busy {
            return;
        }
        if matches!(
            action,
            ToolAction::CheckGuardian | ToolAction::StartGuardian | ToolAction::DeployGuardian
        ) {
            self.guardian = None;
        }
        self.busy = true;
        self.report = "正在执行，请稍候…".into();
        let _ = self.bus.cmd_sender.send(AppCommand::ToolAction {
            action,
            generation: self.generation,
        });
        cx.notify();
    }
    fn save(&mut self, key: &str, value: serde_json::Value) {
        save_patch(self.bus.clone(), HashMap::from([(key.to_string(), value)]));
    }
}
impl Render for ToolsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = !self.connected || self.busy;

        // 1. ADB Guardian 卡片状态指示与视觉增强
        let (badge_text, badge_bg, badge_fg, guardian_detail) = match &self.guardian {
            Some(status) if status.ok => (
                "正常运行",
                ThemeColors::STATUS_SUCCESS_BG(),
                ThemeColors::STATUS_SUCCESS(),
                "保活守护服务已就绪，无线 ADB 处于活动状态并保持 5555 端口。".to_string(),
            ),
            Some(status) => (
                "未就绪",
                ThemeColors::STATUS_WARNING_BG(),
                ThemeColors::STATUS_WARNING(),
                format!("未就绪原因：{}", status.missing_reasons().join("、")),
            ),
            None => {
                if self.connected {
                    if self.busy {
                        (
                            "检测中…",
                            ThemeColors::ACCENT_BG_SUBTLE(),
                            ThemeColors::ACCENT_CYAN(),
                            "正在向设备查询守护服务状态…".to_string(),
                        )
                    } else {
                        (
                            "状态未知",
                            ThemeColors::BG_BUTTON(),
                            ThemeColors::TEXT_SECONDARY(),
                            "尚未检测守护服务状态，请点击下方“检测状态”按钮。".to_string(),
                        )
                    }
                } else {
                    (
                        "未连接",
                        ThemeColors::BG_DISABLED(),
                        ThemeColors::TEXT_DISABLED(),
                        "请先连接显示器后再检测或配置保活守护。".to_string(),
                    )
                }
            }
        };

        let mut guardian = card_container()
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                            .child("ADB 保活守护"),
                    )
                    .child(badge(badge_text, badge_bg, badge_fg)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                    .child("重启、待机或唤醒后自动恢复无线 ADB，并保持 5555 端口可用。"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(gpui::rgb(badge_fg))
                    .child(guardian_detail),
            );

        let mut guardian_actions = div()
            .flex()
            .flex_wrap()
            .flex_shrink_0()
            .items_center()
            .min_h(gpui::px(32.0))
            .gap_2();
        for (index, label, action) in [
            (0, "检测状态", ToolAction::CheckGuardian),
            (1, "部署/修复", ToolAction::DeployGuardian),
            (2, "启动保活", ToolAction::StartGuardian),
        ] {
            guardian_actions = guardian_actions.child(
                option_button(label, index == 1, disabled)
                    .id(("guardian-action", index as usize))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !disabled {
                            this.request(action.clone(), cx);
                        }
                    })),
            );
        }
        guardian = guardian.child(guardian_actions);

        // 2. 设备工具卡片与长错误换行展示
        let mut device = card_container().child(
            div()
                .flex_shrink_0()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                .child("设备工具"),
        );
        let mut row = div()
            .flex()
            .flex_wrap()
            .flex_shrink_0()
            .items_center()
            .min_h(gpui::px(32.0))
            .gap_2();
        for (index, label, action) in [
            (0, "刷新状态", ToolAction::ReadStatus),
            (1, "部署 AdbGuardian", ToolAction::DeployGuardian),
            (2, "4K UI", ToolAction::Resolution4k),
            (3, "1080p UI", ToolAction::Resolution1080p),
            (4, "重启显示器", ToolAction::Reboot),
        ] {
            row = row.child(
                option_button(label, false, disabled)
                    .id(("tool", index as usize))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if disabled {
                            return;
                        }
                        if matches!(action, ToolAction::ReadStatus) {
                            this.request(action.clone(), cx);
                        } else {
                            this.confirmation = Some(action.clone());
                            cx.notify();
                        }
                    })),
            );
        }
        device = device.child(row);

        // 状态报告与长错误信息支持自然高度折行，不压缩不溢出
        device = device.child(
            div()
                .flex_shrink_0()
                .p_2()
                .rounded_md()
                .bg(gpui::rgb(ThemeColors::BG_ROOT()))
                .border_1()
                .border_color(gpui::rgb(ThemeColors::BORDER()))
                .text_xs()
                .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                .child(self.report.clone()),
        );

        if let Some(action) = self.confirmation.clone() {
            device = device
                .child(
                    div()
                        .flex_shrink_0()
                        .p_3()
                        .rounded_md()
                        .bg(gpui::rgb(ThemeColors::STATUS_WARNING_BG()))
                        .border_1()
                        .border_color(gpui::rgb(ThemeColors::STATUS_WARNING()))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                                .child(match action {
                                    ToolAction::DeployGuardian => {
                                        "将安装随包 AdbGuardian，并配置显示器端保活服务。"
                                    }
                                    ToolAction::Reboot => "将重启显示器，连接和画面会暂时中断。",
                                    ToolAction::InstallApk(_) => "将向当前显示器安装所选 APK。",
                                    ToolAction::Shell(_) => {
                                        "将在当前显示器执行此 Shell 命令，请确认内容。"
                                    }
                                    _ => {
                                        "将修改显示器 UI 分辨率并立即重启，连接和画面会暂时中断。"
                                    }
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_shrink_0()
                                .gap_2()
                                .child(
                                    option_button("确认执行", true, false)
                                        .id("confirm-tool")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.confirmation = None;
                                            this.request(action.clone(), cx);
                                        })),
                                )
                                .child(
                                    option_button("取消", false, false)
                                        .id("cancel-tool")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirmation = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                );
        }

        // 3. ADB 终端与应用安装（使用 terminal_expanded 折叠，各组均确保非收缩自然高度）
        let toggle_label = if self.terminal_expanded {
            "收起工具"
        } else {
            "展开工具"
        };
        let mut extras = card_container().child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("ADB 终端与应用安装"),
                        )
                        .child(badge(
                            "高级维护",
                            ThemeColors::ACCENT_BG_SUBTLE(),
                            ThemeColors::ACCENT_CYAN(),
                        )),
                )
                .child(
                    option_button(toggle_label, self.terminal_expanded, false)
                        .id("toggle-terminal-expand")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.terminal_expanded = !this.terminal_expanded;
                            cx.notify();
                        })),
                ),
        );

        if self.terminal_expanded {
            // Shell 命令行分组：包含标签、32px 输入框外层容器和 32px 按钮行，全部明确 flex_shrink_0
            extras = extras
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("Shell 命令行（显示器端执行）"),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .h(gpui::px(32.0))
                                .child(self.shell_input.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .flex_shrink_0()
                                .items_center()
                                .min_h(gpui::px(32.0))
                                .gap_2()
                                .child(
                                    option_button("执行 Shell 命令", false, disabled)
                                        .id("run-shell-command")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !disabled {
                                                let command =
                                                    this.shell_input.read(cx).text().trim().to_string();
                                                if !command.is_empty() {
                                                    this.confirmation =
                                                        Some(ToolAction::Shell(command));
                                                    cx.notify();
                                                }
                                            }
                                        })),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(gpui::rgb(ThemeColors::TEXT_MUTED()))
                                        .child("命令直接在显示器上运行；修改前请确认内容。"),
                                ),
                        ),
                )
                // APK 安装分组：包含标签、32px 输入框外层容器和 32px 按钮行，全部明确 flex_shrink_0
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("APK 软件包旁加载"),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .h(gpui::px(32.0))
                                .child(self.apk_input.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .flex_shrink_0()
                                .items_center()
                                .min_h(gpui::px(32.0))
                                .gap_2()
                                .child(
                                    option_button("选择 APK", false, false)
                                        .id("pick-apk")
                                        .on_click(cx.listener(|_, _, _, cx| {
                                            let receiver =
                                                cx.prompt_for_paths(gpui::PathPromptOptions {
                                                    files: true,
                                                    directories: false,
                                                    multiple: false,
                                                    prompt: Some("选择 APK 软件包".into()),
                                                });
                                            cx.spawn(async move |this, cx| {
                                                if let Ok(Ok(Some(paths))) = receiver.await {
                                                    if let Some(path) = paths.first() {
                                                        let path =
                                                            path.to_string_lossy().to_string();
                                                        let _ = this.update(cx, |this, cx| {
                                                            this.apk_input.update(cx, |input, cx| {
                                                                input.set_text(path, cx)
                                                            })
                                                        });
                                                    }
                                                }
                                            })
                                            .detach();
                                        })),
                                )
                                .child(
                                    option_button("安装 APK", false, disabled)
                                        .id("install-apk")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !disabled {
                                                this.confirmation = Some(ToolAction::InstallApk(
                                                    this.apk_input.read(cx).text().to_string(),
                                                ));
                                                cx.notify();
                                            }
                                        })),
                                ),
                        ),
                );

            let mut terminal_row = div()
                .flex()
                .flex_wrap()
                .flex_shrink_0()
                .items_center()
                .min_h(gpui::px(32.0))
                .gap_2();
            for (index, label, shell) in [(0, "打开 ADB CMD", false), (1, "进入 ADB Shell", true)] {
                terminal_row = terminal_row.child(
                    option_button(label, false, shell && disabled)
                        .id(("adb-terminal", index as usize))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if shell && disabled {
                                return;
                            }
                            let adb = crate::adb::get_bundled_adb_path();
                            let serial = this.connected_serial.clone().unwrap_or_default();
                            if shell && serial.is_empty() { return; }
                            if let Err(error) = crate::process_io::open_adb_console(&adb, &serial, shell)
                            {
                                this.report = format!("无法打开终端：{error}");
                                cx.notify();
                            }
                        })),
                );
            }
            extras = extras.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("本地 ADB 调试控制台"),
                    )
                    .child(terminal_row),
            );
        } else {
            extras = extras.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(gpui::rgb(ThemeColors::TEXT_MUTED()))
                    .child("默认收起高级终端与 APK 安装工具，点击右上角“展开工具”使用。"),
            );
        }

        // 4. 应用设置卡片（偏好选项横向可换行紧凑布局，各组明确高度与非收缩）
        let autostart = self
            .config
            .extra
            .get("autostart")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut behavior_options = div()
            .flex()
            .flex_wrap()
            .flex_shrink_0()
            .items_center()
            .min_h(gpui::px(32.0))
            .gap_2();
        for (label, behavior) in [
            ("关闭到托盘", CloseBehavior::Tray),
            ("关闭即退出", CloseBehavior::Exit),
        ] {
            let active = self.config.close_behavior == behavior;
            behavior_options = behavior_options.child(
                option_button(label, active, false)
                    .id(SharedString::from(format!("close-{label}")))
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.save("close_behavior", serde_json::to_value(behavior).unwrap());
                    })),
            );
        }
        behavior_options = behavior_options.child(
            option_button(
                if autostart {
                    "开机启动：开"
                } else {
                    "开机启动：关"
                },
                autostart,
                false,
            )
            .id("autostart")
            .on_click(cx.listener(move |this, _, _, _| {
                let bus = this.bus.clone();
                crate::lifecycle::spawn_ui_task(move || {
                    match crate::config::system::set_autostart(!autostart) {
                        Ok(()) => save_patch(
                            bus,
                            HashMap::from([("autostart".into(), serde_json::json!(!autostart))]),
                        ),
                        Err(error) => {
                            let _ = bus.event_sender.send(AppEvent::ToastNotification {
                                message: error,
                                is_error: true,
                            });
                        }
                    }
                });
            })),
        );

        let mut themes = div()
            .flex()
            .flex_wrap()
            .flex_shrink_0()
            .items_center()
            .min_h(gpui::px(32.0))
            .gap_2();
        for (index, (label, value)) in [
            ("跟随系统", "auto"),
            ("深色模式", "dark"),
            ("浅色模式", "light"),
        ]
        .into_iter()
        .enumerate()
        {
            let active = self
                .config
                .extra
                .get("theme")
                .and_then(|v| v.as_str())
                .unwrap_or("dark")
                == value;
            themes = themes.child(
                option_button(label, active, false)
                    .id(("theme", index))
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.save("theme", serde_json::json!(value))
                    })),
            );
        }

        let mut settings = card_container()
            .child(
                div()
                    .flex_shrink_0()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                    .child("应用偏好与扫描设置"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .gap_1_5()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("常规偏好与启闭"),
                    )
                    .child(behavior_options),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .gap_1_5()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("界面主题"),
                    )
                    .child(themes),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                    .child("设备扫描范围（留空则默认使用物理网卡网段）"),
            );

        for (i, label) in ["强制扫描 IP", "排除 IP", "额外 CIDR 网段"]
            .iter()
            .enumerate()
        {
            settings = settings.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                            .child(*label),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .h(gpui::px(32.0))
                            .child(self.scan_inputs[i].clone()),
                    ),
            );
        }

        settings = settings.child(
            div()
                .flex()
                .flex_shrink_0()
                .justify_end()
                .min_h(gpui::px(32.0))
                .child(
                    option_button("保存扫描范围", true, false)
                        .id("save-scan")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let mut patch = HashMap::new();
                            for (i, key) in [
                                "scan_force_devices",
                                "scan_block_devices",
                                "scan_force_subnets",
                            ]
                            .iter()
                            .enumerate()
                            {
                                let values: Vec<String> = this.scan_inputs[i]
                                    .read(cx)
                                    .text()
                                    .split([',', '，', ';', '\n'])
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                                    .collect();
                                patch.insert(key.to_string(), serde_json::json!(values));
                            }
                            save_patch(this.bus.clone(), patch);
                        })),
                ),
        );

        // 5. HDR / SDR 精密控光记忆卡片（横向紧凑布局，非收缩）
        let hdr_enabled = self.config.hdr_sdr_local_dimming_enabled;
        let mut choices = div()
            .flex()
            .flex_wrap()
            .flex_shrink_0()
            .items_center()
            .min_h(gpui::px(32.0))
            .gap_2();
        choices = choices.child(
            option_button(
                "自动识别",
                self.config.hdr_target_display_id.is_empty(),
                false,
            )
            .id("hdr-auto")
            .on_click(cx.listener(|this, _, _, _| {
                this.save("hdr_target_display_id", serde_json::json!(""))
            })),
        );
        for (index, display) in self.displays.clone().into_iter().enumerate() {
            let id = display.device_id.clone();
            let selected = self.config.hdr_target_display_id == id;
            let label = crate::ddc::format_display_label(
                &display.device_string,
                &display.device_id,
                &display.device_name,
            );
            choices = choices.child(
                option_button(label, selected, false)
                    .id(("hdr-display", index))
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.save("hdr_target_display_id", serde_json::json!(id))
                    })),
            );
        }

        let hdr = card_container()
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                            .child("HDR / SDR 精密控光记忆"),
                    )
                    .child(badge(
                        if hdr_enabled {
                            "联动已启用"
                        } else {
                            "联动已关闭"
                        },
                        if hdr_enabled {
                            ThemeColors::STATUS_SUCCESS_BG()
                        } else {
                            ThemeColors::BG_BUTTON()
                        },
                        if hdr_enabled {
                            ThemeColors::STATUS_SUCCESS()
                        } else {
                            ThemeColors::TEXT_MUTED()
                        },
                    )),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                    .child("仅用于小米 / Redmi 显示器，按其 Windows HDR 状态自动恢复匹配控光档位。"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .min_h(gpui::px(32.0))
                    .gap_2()
                    .child(
                        option_button(
                            if hdr_enabled {
                                "联动：开"
                            } else {
                                "联动：关"
                            },
                            hdr_enabled,
                            false,
                        )
                        .id("hdr-memory-toggle")
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.save(
                                "hdr_sdr_local_dimming_enabled",
                                serde_json::json!(!hdr_enabled),
                            )
                        })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                            .child(format!(
                                "SDR 记忆: {}",
                                self.config
                                    .local_dimming_memory
                                    .sdr
                                    .map(|v| v.to_string())
                                    .unwrap_or("未记录".into())
                            ))
                            .child(
                                div()
                                    .text_color(gpui::rgb(ThemeColors::BORDER()))
                                    .child("|"),
                            )
                            .child(format!(
                                "HDR 记忆: {}",
                                self.config
                                    .local_dimming_memory
                                    .hdr
                                    .map(|v| v.to_string())
                                    .unwrap_or("未记录".into())
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .gap_1_5()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                            .child("生效目标显示器："),
                    )
                    .child(choices),
            );

        // 6. 热键与提交设置卡片（非收缩，自然高度）
        let countdown = self.config.hotkey_countdown_enabled;
        let keys = card_container()
            .child(
                div()
                    .flex_shrink_0()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                    .child("全局快捷键与提交控制"),
            )
            .child(div().flex_shrink_0().child(self.hotkey_form.clone()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .min_h(gpui::px(32.0))
                    .gap_2()
                    .child(
                        option_button(
                            if countdown {
                                "倒计时提交：开"
                            } else {
                                "倒计时提交：关"
                            },
                            countdown,
                            false,
                        )
                        .id("countdown-toggle")
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.save("hotkey_countdown_enabled", serde_json::json!(!countdown))
                        })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                                    .child("松手等待秒数 (0.1–5):"),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(gpui::px(80.0))
                                    .h(gpui::px(32.0))
                                    .child(self.seconds.clone()),
                            )
                            .child(
                                option_button("保存等待时间", true, false)
                                    .id("save-countdown")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        match this.seconds.read(cx).text().trim().parse::<f64>() {
                                            Ok(seconds)
                                                if seconds.is_finite()
                                                    && (0.1..=5.0).contains(&seconds) =>
                                            {
                                                this.save(
                                                    "hotkey_countdown_seconds",
                                                    serde_json::json!(seconds),
                                                )
                                            }
                                            _ => {
                                                let _ = this.bus.event_sender.send(crate::message::AppEvent::ToastNotification {
                                                    message: "等待时间范围为 0.1–5 秒".into(), is_error: true,
                                                });
                                                cx.notify();
                                            }
                                        }
                                    })),
                            ),
                    ),
            );

        // 7. 运行日志卡片
        let logs = card_container().child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                                .child("运行日志"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                                .child("导出调试与排错日志文件以供问题诊断。"),
                        ),
                )
                .child(
                    option_button("导出日志", false, false)
                        .id("export-log")
                        .on_click(cx.listener(|this, _, _, _| {
                            let bus = this.bus.clone();
                            crate::lifecycle::spawn_ui_task(move || {
                                let dir = crate::config::get_app_data_dir().join("exports");
                                let result = std::fs::create_dir_all(&dir).and_then(|_| {
                                    let path = dir.join(format!(
                                        "日志-{}.txt",
                                        chrono::Local::now().format("%Y%m%d-%H%M%S")
                                    ));
                                    crate::logger::export_logs_to_file(&path).map(|_| path)
                                });
                                let (message, is_error) = match result {
                                    Ok(path) => (format!("日志已导出：{}", path.display()), false),
                                    Err(error) => (format!("日志导出失败：{error}"), true),
                                };
                                let _ = bus
                                    .event_sender
                                    .send(AppEvent::ToastNotification { message, is_error });
                            });
                        })),
                ),
        );

        // 外层纵向容器：不设固定高度与裁剪，各卡片自然堆叠计算真实高度，由宿主外层滚动容器承载
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .gap_4()
            .child(
                div()
                    .flex_shrink_0()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                    .child("工具与设置"),
            )
            .child(guardian)
            .child(device)
            .child(extras)
            .child(settings)
            .child(hdr)
            .child(keys)
            .child(logs)
            .child(
                option_button("第三方许可", false, false)
                    .id("third-party-notices")
                    .on_click(cx.listener(|this, _, _, _| {
                        let bus = this.bus.clone();
                        crate::lifecycle::spawn_ui_task(move || {
                            let result = crate::resources::license_directory().and_then(|directory| {
                                std::process::Command::new("explorer.exe").arg(directory)
                                    .stdin(std::process::Stdio::null())
                                    .stdout(std::process::Stdio::null())
                                    .stderr(std::process::Stdio::null())
                                    .spawn()
                                    .map(|_| ()).map_err(|error| error.to_string())
                            });
                            if let Err(error) = result {
                                let _ = bus.event_sender.send(AppEvent::ToastNotification {
                                    message: format!("无法打开第三方许可：{error}"), is_error: true,
                                });
                            }
                        });
                    })),
            )
    }
}
pub fn render_tools_page(page: Entity<ToolsPage>) -> Div {
    div().child(page)
}
