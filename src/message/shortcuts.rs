use super::{AppCommand, AppEvent, MessageBus};
use crate::{
    adb::AdbDevice,
    device::{game as g, light as l, picture as p, source as s},
    hotkey::{AdjustDirection, AdjustParam, CycleAction, GlobalHotkeyManager, HotkeyEvent},
};
use std::time::{Duration, Instant};

struct Pending {
    key: String,
    value: i32,
    command: AppCommand,
    deadline: Instant,
    generation: u64,
}
pub(super) struct Shortcuts {
    manager: GlobalHotkeyManager,
    configured: Option<serde_json::Value>,
    pending: Option<Pending>,
}
impl Shortcuts {
    pub fn new() -> Self {
        Self {
            manager: GlobalHotkeyManager::new(),
            configured: None,
            pending: None,
        }
    }
    pub fn cancel(&mut self, bus: &MessageBus) {
        self.pending = None;
        let _ = bus.event_sender.send(AppEvent::HotkeyOsd { message: None });
    }
    pub fn tick(&mut self, device: Option<&AdbDevice>, bus: &MessageBus) {
        let cfg = crate::config::load_settings();
        let signature =
            serde_json::json!([cfg.extra.get("hotkeys"), cfg.extra.get("adjust_hotkeys")]);
        if self.configured.as_ref() != Some(&signature) {
            let report = self.manager.configure_from_extra(&cfg.extra);
            self.configured = Some(signature);
            if report.failed_count() > 0 {
                let text = report
                    .results
                    .iter()
                    .filter(|r| {
                        !matches!(
                            r.status,
                            crate::hotkey::RegistrationStatus::Success { .. }
                                | crate::hotkey::RegistrationStatus::SkippedUnbound
                        )
                    })
                    .map(|r| format!("{}：{:?}", r.target_name, r.status))
                    .collect::<Vec<_>>()
                    .join("；");
                let _ = bus.event_sender.send(AppEvent::ToastNotification {
                    message: format!("部分快捷键未注册：{text}"),
                    is_error: true,
                });
            }
        }
        let generation = bus
            .active_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.generation != generation)
            || device.is_none()
        {
            if self.pending.is_some() {
                self.cancel(bus);
            }
        }
        while let Some(event) = self.manager.try_recv_event() {
            if device.is_none() {
                continue;
            }
            let _ = bus
                .cmd_sender
                .send(AppCommand::Hotkey { event, generation });
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| Instant::now() >= p.deadline)
        {
            if let Some(pending) = self.pending.take() {
                let _ = bus.cmd_sender.send(pending.command);
                let _ = bus.event_sender.send(AppEvent::HotkeyOsd { message: None });
            }
        }
    }
    pub fn stage(
        &mut self,
        dev: &AdbDevice,
        event: &HotkeyEvent,
        bus: &MessageBus,
    ) -> Result<(), String> {
        let generation = bus
            .active_generation
            .load(std::sync::atomic::Ordering::Acquire);
        let key = format!("{event:?}");
        // Group increases and decreases for the same parameter into one pending value.
        let key = match event {
            HotkeyEvent::Adjust { param, .. } => format!("adjust:{param:?}"),
            _ => key,
        };
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.key != key)
        {
            if let Some(previous) = self.pending.take() {
                let _ = bus.cmd_sender.send(previous.command);
            }
        }
        let prior = self
            .pending
            .as_ref()
            .filter(|p| p.key == key)
            .map(|p| p.value);
        let (label, value, command) = dev.transaction(|| -> Result<_, String> {
            let fail = |e: crate::adb::AdbError| e.to_string();
            match event {
                HotkeyEvent::Adjust {
                    param,
                    direction,
                    step,
                } => {
                    if *step == 0 || *step > 1000 {
                        return Err("快捷键步长无效".into());
                    }

                    let baseline = if let Some(value) = prior {
                        value
                    } else if *param == AdjustParam::AtmosphereIllumination {
                        let state = l::read_screen_light_state(dev);
                        let params = state.to_parameters().ok_or("未能读取完整屏幕灯参数")?;
                        let v = params.illumination as i32 + 1;
                        v
                    } else {
                        let state = p::query_picture_parameters(dev).map_err(fail)?;
                        let v = match param {
                            AdjustParam::Backlight => state.backlight,
                            AdjustParam::BlackLevel => state.black_level,
                            AdjustParam::Contrast => state.contrast,
                            AdjustParam::Saturation => state.saturation,
                            AdjustParam::Hue => state.hue,
                            AdjustParam::Sharpness => state.sharpness,
                            AdjustParam::RedGain => state.red_gain,
                            AdjustParam::GreenGain => state.green_gain,
                            AdjustParam::BlueGain => state.blue_gain,
                            _ => None,
                        }
                        .ok_or("参数未读取，未执行快捷键")?;
                        v as i32
                    };
                    let delta = if *direction == AdjustDirection::Increase {
                        *step as i32
                    } else {
                        -(*step as i32)
                    };
                    let value = baseline
                        .saturating_add(delta)
                        .clamp(param.min(), param.max());
                    let command = AppCommand::HotkeyCommit {
                        param: *param,
                        value,
                        generation,
                    };
                    Ok((param.label().to_string(), value, command))
                }
                HotkeyEvent::Cycle(action) => {
                    let state = p::query_picture_parameters(dev).map_err(fail)?;
                    let (current, options): (i32, &[i32]) = match action {
                        CycleAction::PictureModeCycle => {
                            (state.mode.ok_or("模式未读取")?, &[14, 10, 9])
                        }
                        CycleAction::LocalDimmingCycle | CycleAction::LocalDimmingToggleOff => (
                            state.local_dimming.ok_or("精密控光未读取")? as i32,
                            &[0, 1, 2, 3],
                        ),
                        CycleAction::ColorSpaceCycle => (
                            state.color_space.ok_or("色域未读取")? as i32,
                            &[0, 3, 6, 4, 5, 7],
                        ),
                        CycleAction::ColorTempCycle => {
                            (state.color_temp.ok_or("色温未读取")?, &[0, 1, 2, 8, 3])
                        }
                        CycleAction::ResponseTimeCycle => (
                            state.response_time.ok_or("响应时间未读取")? as i32,
                            &[1, 2, 3],
                        ),
                        CycleAction::FreeSyncToggle => {
                            let value = g::query_game_parameters(dev)
                                .map_err(fail)?
                                .freesync
                                .ok_or("FreeSync 未读取")?;
                            (i32::from(value), &[0, 1])
                        }
                        CycleAction::InputSourceCycle => (
                            s::get_current_source(dev)
                                .map_err(fail)?
                                .ok_or("信号源未读取")?
                                .id() as i32,
                            &[23, 24, 29, 30],
                        ),
                    };
                    let current = prior.unwrap_or(current);
                    let value = if *action == CycleAction::LocalDimmingToggleOff {
                        if current == 0 {
                            crate::config::load_settings()
                                .local_dimming_toggle_last_value
                                .clamp(1, 3)
                        } else {
                            let _ = crate::config::update_settings(|cfg| {
                                cfg.local_dimming_toggle_last_value = current
                            });
                            0
                        }
                    } else {
                        let index = options
                            .iter()
                            .position(|v| *v == current)
                            .ok_or("当前值不在支持的循环范围内")?;
                        options[(index + 1) % options.len()]
                    };
                    let id = match action {
                        CycleAction::PictureModeCycle => "picture_mode",
                        CycleAction::LocalDimmingCycle | CycleAction::LocalDimmingToggleOff => {
                            "local_dimming"
                        }
                        CycleAction::ColorSpaceCycle => "color_space",
                        CycleAction::ColorTempCycle => "color_temp",
                        CycleAction::ResponseTimeCycle => "response_time",
                        CycleAction::FreeSyncToggle => "freesync",
                        CycleAction::InputSourceCycle => "input_source",
                    };
                    Ok((
                        action.label().to_string(),
                        value,
                        crate::tray::catalog::Choice::Value(id, value).command(generation),
                    ))
                }
            }
        })?;
        let cfg = crate::config::load_settings();
        let seconds = if cfg.hotkey_countdown_enabled {
            cfg.hotkey_countdown_seconds.clamp(0.1, 5.0)
        } else {
            0.0
        };
        let _ = bus.event_sender.send(AppEvent::HotkeyOsd {
            message: Some(format!("{label}　{value}\n{seconds:.1} 秒后应用")),
        });
        self.pending = Some(Pending {
            key,
            value,
            command,
            deadline: Instant::now() + Duration::from_secs_f64(seconds),
            generation,
        });
        Ok(())
    }
}
