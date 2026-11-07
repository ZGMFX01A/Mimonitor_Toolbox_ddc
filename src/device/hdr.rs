use crate::ddc::RawDisplayItem;
use windows::{
    core::Interface,
    Win32::Graphics::Dxgi::{
        Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, CreateDXGIFactory1, IDXGIFactory1,
        IDXGIOutput6,
    },
};
pub fn resolve<'a>(displays: &'a [RawDisplayItem], id: &str) -> Option<&'a RawDisplayItem> {
    if !id.is_empty() {
        let matches = displays
            .iter()
            .filter(|d| d.device_id == id && crate::ddc::is_xiaomi_device_id(&d.device_id))
            .collect::<Vec<_>>();
        return if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        };
    }
    let xiaomi = displays
        .iter()
        .filter(|d| crate::ddc::is_xiaomi_device_id(&d.device_id))
        .collect::<Vec<_>>();
    if xiaomi.len() == 1 {
        Some(xiaomi[0])
    } else {
        None
    }
}
pub fn query(device_name: &str) -> Option<bool> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut states = Vec::new();
        for index in 0..32 {
            let Ok(adapter) = factory.EnumAdapters1(index) else {
                break;
            };
            for output_index in 0..64 {
                let Ok(output) = adapter.EnumOutputs(output_index) else {
                    break;
                };
                let Ok(output) = output.cast::<IDXGIOutput6>() else {
                    continue;
                };
                let Ok(desc) = output.GetDesc1() else {
                    continue;
                };
                let count = desc
                    .DeviceName
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(desc.DeviceName.len());
                if desc.AttachedToDesktop.as_bool()
                    && String::from_utf16_lossy(&desc.DeviceName[..count]) == device_name
                {
                    states.push(desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020);
                }
            }
        }
        if states.len() == 1 {
            states.first().copied()
        } else {
            None
        }
    }
}
pub struct HdrMemory {
    last: Option<(String, bool)>,
    generation: u64,
    last_poll: std::time::Instant,
}
impl HdrMemory {
    pub fn new() -> Self {
        Self {
            last: None,
            generation: 0,
            last_poll: std::time::Instant::now(),
        }
    }
    pub fn tick(&mut self, dev: Option<&crate::adb::AdbDevice>, bus: &crate::message::MessageBus) {
        if self.last_poll.elapsed() < std::time::Duration::from_secs(3) {
            return;
        }
        self.last_poll = std::time::Instant::now();
        let cfg = crate::config::load_settings();
        if cfg.extra.get("theme").and_then(|v| v.as_str()) == Some("auto")
            && crate::ui::theme::set_preference("auto")
        {
            let _ = bus
                .event_sender
                .send(crate::message::AppEvent::ConfigUpdated((*cfg).clone()));
        }
        let generation = bus
            .active_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if self.generation != generation {
            self.last = None;
            self.generation = generation;
        }
        if !cfg.hdr_sdr_local_dimming_enabled || dev.is_none() {
            self.last = None;
            return;
        }
        let recovery: Option<crate::presets::recovery::SchedulerRecovery> = cfg
            .extra
            .get("scheduler_recovery")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        if cfg.active_preset_id.is_some()
            || recovery.as_ref().is_some_and(|r| {
                r.active_task_id.is_some()
                    || r.pending_restore.is_some()
                    || !r.orphaned_restores.is_empty()
            })
        {
            self.last = None;
            return;
        }
        let dev = dev.unwrap();
        let displays = crate::ddc::list_raw_windows_displays();
        let Some(target) = resolve(&displays, &cfg.hdr_target_display_id) else {
            self.last = None;
            return;
        };
        let Some(hdr) = query(&target.device_name) else {
            self.last = None;
            return;
        };
        let identity = (target.device_id.clone(), hdr);
        if let Some((id, previous)) = &self.last {
            if id == &identity.0 && *previous != hdr {
                let value = if hdr {
                    cfg.local_dimming_memory.hdr
                } else {
                    cfg.local_dimming_memory.sdr
                };
                if let Some(value) = value.filter(|v| (0..=3).contains(v)) {
                    match crate::device::picture::set_local_dimming(dev, value as u32) {
                        Ok(()) => {
                            let _ = bus.cmd_sender.send(
                                crate::message::AppCommand::FetchPictureParameters { generation },
                            );
                        }
                        Err(error) => {
                            let _ = bus.event_sender.send(
                                crate::message::AppEvent::ToastNotification {
                                    message: format!("HDR 精密控光联动失败：{error}"),
                                    is_error: true,
                                },
                            );
                            return;
                        }
                    }
                }
            }
        }
        // Only remember confirmed hardware values for the current mode. Unknown values never replace memory.
        if self.last.as_ref().is_none_or(|last| last == &identity) {
            if let Ok(value) = crate::device::jni::jni_get(dev, "g_video__vid_local_dimming") {
                if (0..=3).contains(&value) {
                    let old = if hdr {
                        cfg.local_dimming_memory.hdr
                    } else {
                        cfg.local_dimming_memory.sdr
                    };
                    if old != Some(value) {
                        if let Ok(cfg) = crate::config::update_settings(|cfg| {
                            if hdr {
                                cfg.local_dimming_memory.hdr = Some(value)
                            } else {
                                cfg.local_dimming_memory.sdr = Some(value)
                            }
                        }) {
                            let _ = bus
                                .event_sender
                                .send(crate::message::AppEvent::ConfigUpdated((*cfg).clone()));
                        }
                    }
                }
            }
        }
        self.last = Some(identity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_monitor_never_controls_xiaomi_dimming() {
        let ordinary = RawDisplayItem {
            device_name: "DISPLAY1".into(),
            device_id: "MONITOR\\YHY1600\\1".into(),
            device_string: "YHY1600".into(),
            label: "YHY1600".into(),
        };
        let displays = vec![ordinary.clone()];
        assert!(resolve(&displays, "").is_none());
        assert!(resolve(&displays, &ordinary.device_id).is_none());
        let xiaomi = RawDisplayItem {
            device_id: "MONITOR\\XMI300A\\2".into(),
            ..ordinary
        };
        let displays = vec![xiaomi.clone()];
        assert_eq!(resolve(&displays, ""), Some(&xiaomi));
    }
}
