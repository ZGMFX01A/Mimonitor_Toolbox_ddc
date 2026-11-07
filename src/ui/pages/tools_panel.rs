use crate::{
    config::{AppConfig, CloseBehavior},
    message::{actions::ToolAction, AppCommand, AppEvent, MessageBus},
    ui::components::{card_container, option_button, TextInput},
};
use gpui::prelude::*;
use gpui::{div, Context, Div, Entity, Render, Window};
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
    pub busy: bool,
    shell_input: Entity<TextInput>,
    apk_input: Entity<TextInput>,
    scan_inputs: Vec<Entity<TextInput>>,
    hotkey_form: Entity<super::hotkeys::HotkeyForm>,
    seconds: Entity<TextInput>,
}
pub fn save_patch(bus: Arc<MessageBus>, patch: HashMap<String, serde_json::Value>) {
    std::thread::spawn(move || match crate::config::patch_settings(patch) {
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
        }
    }
    pub fn set_connected(&mut self, connected: bool, generation: u64, cx: &mut Context<Self>) {
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
        let (guardian_label, color) = match &self.guardian {
            Some(status) if status.ok => (
                "状态：正常运行 · ADB 保活已启用".to_string(),
                crate::ui::theme::ThemeColors::STATUS_SUCCESS(),
            ),
            Some(status) => (
                format!("状态：未就绪 · {}", status.missing_reasons().join("、")),
                crate::ui::theme::ThemeColors::STATUS_WARNING(),
            ),
            None => (
                if self.connected {
                    if self.busy {
                        "状态：检测中…"
                    } else {
                        "状态：未知，请检测状态"
                    }
                } else {
                    "状态：未连接"
                }
                .into(),
                crate::ui::theme::ThemeColors::TEXT_SECONDARY(),
            ),
        };
        let mut guardian = card_container()
            .child(div().text_lg().child("ADB 保活守护"))
            .child(
                div()
                    .text_sm()
                    .child("重启、待机或唤醒后自动恢复无线 ADB，并保持 5555 端口可用。"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(color))
                    .child(guardian_label),
            );
        let mut guardian_actions = div().flex().flex_wrap().gap_2();
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
        let mut device = card_container().child(div().text_lg().child("设备工具"));
        let mut row = div().flex().flex_wrap().gap_2();
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
        device = device
            .child(row)
            .child(div().text_sm().child(self.report.clone()));
        if let Some(action) = self.confirmation.clone() {
            device = device
                .child(div().text_sm().child(match action {
                    ToolAction::DeployGuardian => {
                        "将安装随包 AdbGuardian，并配置显示器端保活服务。"
                    }
                    ToolAction::Reboot => "将重启显示器，连接和画面会暂时中断。",
                    ToolAction::InstallApk(_) => "将向当前显示器安装所选 APK。",
                    ToolAction::Shell(_) => "将在当前显示器执行此 Shell 命令，请确认内容。",
                    _ => "将修改显示器 UI 分辨率并立即重启，连接和画面会暂时中断。",
                }))
                .child(
                    div()
                        .flex()
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
                );
        }
        let mut extras = card_container()
            .child(div().text_lg().child("ADB 终端与应用安装"))
            .child(self.shell_input.clone())
            .child(
                option_button("执行 Shell 命令", false, disabled)
                    .id("run-shell-command")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !disabled {
                            let command = this.shell_input.read(cx).text().trim().to_string();
                            if !command.is_empty() {
                                this.confirmation = Some(ToolAction::Shell(command));
                                cx.notify();
                            }
                        }
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .child("Shell 命令直接在显示器上运行；修改前请确认命令内容。"),
            )
            .child(self.apk_input.clone())
            .child(
                option_button("选择 APK", false, false)
                    .id("pick-apk")
                    .on_click(cx.listener(|_, _, _, cx| {
                        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("选择 APK 软件包".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = receiver.await {
                                if let Some(path) = paths.first() {
                                    let path = path.to_string_lossy().to_string();
                                    let _ = this.update(cx, |this, cx| {
                                        this.apk_input
                                            .update(cx, |input, cx| input.set_text(path, cx))
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
            );
        for (index, label, shell) in [(0, "打开 ADB CMD", false), (1, "进入 ADB Shell", true)] {
            extras=extras.child(option_button(label,false,shell&&disabled).id(("adb-terminal",index as usize)).on_click(cx.listener(move|this,_,_,cx|{
                if shell&&disabled{return;}
                use std::os::windows::process::CommandExt;
                let adb=crate::adb::get_bundled_adb_path();let directory=adb.parent().unwrap_or_else(||std::path::Path::new("."));
                let serial=crate::adb::format_adb_serial(&this.config.saved_ip,5555);
                let text=if shell{r#""%MIMONITOR_ADB_BIN%" -P %MIMONITOR_ADB_PORT% -s %MIMONITOR_ADB_SERIAL% shell"#}else{r#"doskey adb=adb.exe -P %MIMONITOR_ADB_PORT% $*"#};
                if let Err(error)=std::process::Command::new("cmd.exe").args(["/k",text]).current_dir(directory).env("MIMONITOR_ADB_BIN",&adb).env("MIMONITOR_ADB_PORT",crate::adb::get_adb_server_port().to_string()).env("MIMONITOR_ADB_SERIAL",serial).creation_flags(0x10).spawn(){this.report=format!("无法打开终端：{error}");cx.notify();}
            })));
        }
        let mut settings = card_container().child(div().text_lg().child("应用设置"));
        for (label, behavior) in [
            ("关闭到托盘", CloseBehavior::Tray),
            ("关闭即退出", CloseBehavior::Exit),
        ] {
            let active = self.config.close_behavior == behavior;
            settings = settings.child(
                option_button(label, active, false)
                    .id(gpui::SharedString::from(format!("close-{label}")))
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.save("close_behavior", serde_json::to_value(behavior).unwrap());
                    })),
            );
        }
        let autostart = self
            .config
            .extra
            .get("autostart")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        settings = settings.child(
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
                std::thread::spawn(move || {
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
        let mut themes = div().flex().gap_2();
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
        settings = settings.child(themes);
        settings = settings.child(
            div()
                .text_sm()
                .child("扫描范围（留空则使用物理网卡所在网段）"),
        );
        for (i, label) in ["强制扫描 IP", "排除 IP", "额外 CIDR 网段"]
            .iter()
            .enumerate()
        {
            settings = settings
                .child(div().text_xs().child(*label))
                .child(self.scan_inputs[i].clone());
        }
        settings = settings.child(
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
        );
        let countdown = self.config.hotkey_countdown_enabled;
        let keys = card_container()
            .child(self.hotkey_form.clone())
            .child(div().text_sm().child("松手后等待秒数（0.1–5）"))
            .child(self.seconds.clone())
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
                option_button("保存等待时间", true, false)
                    .id("save-countdown")
                    .on_click(cx.listener(|this, _, _, cx| {
                        match this.seconds.read(cx).text().parse::<f64>() {
                            Ok(seconds)
                                if seconds.is_finite() && (0.1..=5.0).contains(&seconds) =>
                            {
                                this.save("hotkey_countdown_seconds", serde_json::json!(seconds))
                            }
                            _ => {
                                this.report = "等待时间范围为 0.1–5 秒".into();
                                cx.notify();
                            }
                        }
                    })),
            );
        let hdr_enabled = self.config.hdr_sdr_local_dimming_enabled;
        let mut hdr = card_container()
            .child(div().text_lg().child("HDR / SDR 精密控光记忆"))
            .child(
                div()
                    .text_sm()
                    .child("仅用于小米 / Redmi 显示器，按其 Windows HDR 状态恢复控光档位。"),
            )
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
            .child(div().text_sm().child(format!(
                        "SDR 记忆：{}　HDR 记忆：{}",
                        self.config
                            .local_dimming_memory
                            .sdr
                            .map(|v| v.to_string())
                            .unwrap_or("未记录".into()),
                        self.config
                            .local_dimming_memory
                            .hdr
                            .map(|v| v.to_string())
                            .unwrap_or("未记录".into())
                    )));
        let mut choices = div().flex().flex_wrap().gap_2();
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
        hdr = hdr.child(choices);
        let logs = card_container()
            .child(div().text_lg().child("运行日志"))
            .child(
                option_button("导出日志", false, false)
                    .id("export-log")
                    .on_click(cx.listener(|this, _, _, _| {
                        let bus = this.bus.clone();
                        std::thread::spawn(move || {
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
            );
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child("工具与设置"))
            .child(guardian)
            .child(device)
            .child(extras)
            .child(settings)
            .child(hdr)
            .child(keys)
            .child(logs)
    }
}
pub fn render_tools_page(page: Entity<ToolsPage>) -> Div {
    div().child(page)
}
