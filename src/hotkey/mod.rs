pub mod osd;
pub mod types;

pub use osd::{AdjustCommitPayload, AdjustSession, AdjustStateAggregator, OsdNotification};
pub use types::{
    AdjustDirection, AdjustHotkeyConfig, AdjustParam, CycleAction, HotkeyBindingConfig,
    HotkeyEvent, HotkeyKey, HotkeyModifier, HotkeyRegistrationReport, KeyCombination,
    RegistrationStatus, SingleRegistrationResult,
};

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_HOTKEY, WM_USER,
};

const WM_HOTKEY_WORKER_COMMAND: u32 = WM_USER + 201;

/// 内部待注册项规划
#[derive(Debug, Clone)]
pub struct PlannedRegistrationItem {
    pub target_name: String,
    pub modifier_str: String,
    pub key_str: String,
    pub event: Option<HotkeyEvent>,
    pub combo: Option<KeyCombination>,
    pub pre_error: Option<RegistrationStatus>,
}

/// 发送给专用消息线程的命令
enum WorkerCommand {
    Register {
        items: Vec<PlannedRegistrationItem>,
        reply_tx: SyncSender<HotkeyRegistrationReport>,
    },
    UnregisterAll {
        reply_tx: SyncSender<()>,
    },
    Stop {
        reply_tx: SyncSender<()>,
    },
}

/// 专用线程控制句柄
struct WorkerHandle {
    thread_id: u32,
    cmd_tx: Sender<WorkerCommand>,
    join_handle: Option<JoinHandle<()>>,
}

/// Win32 全局快捷键注册与监听器
pub struct GlobalHotkeyManager {
    worker: Mutex<Option<WorkerHandle>>,
    event_rx: Mutex<Receiver<HotkeyEvent>>,
    event_tx: Sender<HotkeyEvent>,
    last_report: Mutex<Option<HotkeyRegistrationReport>>,
    cached_bindings: Mutex<(
        HashMap<String, HotkeyBindingConfig>,
        Vec<AdjustHotkeyConfig>,
    )>,
}

impl std::fmt::Debug for GlobalHotkeyManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalHotkeyManager")
            .field("has_worker", &self.worker.lock().is_some())
            .field("last_report", &*self.last_report.lock())
            .finish()
    }
}

impl Default for GlobalHotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalHotkeyManager {
    /// 创建全局热键管理器并启动专用 Win32 消息循环线程
    pub fn new() -> Self {
        let (event_tx, event_rx) = channel::<HotkeyEvent>();
        let manager = Self {
            worker: Mutex::new(None),
            event_rx: Mutex::new(event_rx),
            event_tx,
            last_report: Mutex::new(None),
            cached_bindings: Mutex::new((HashMap::new(), Vec::new())),
        };
        manager.ensure_worker_started();
        manager
    }

    /// 确保 Win32 专用消息队列线程已启动
    fn ensure_worker_started(&self) {
        let mut worker_guard = self.worker.lock();
        if worker_guard.is_some() {
            return;
        }

        let (cmd_tx, cmd_rx) = channel::<WorkerCommand>();
        let (ready_tx, ready_rx) = sync_channel::<u32>(1);
        let event_tx_clone = self.event_tx.clone();

        let join_handle = thread::spawn(move || {
            // 确保 Windows 为该线程建立消息队列
            unsafe {
                let mut dummy_msg = MSG::default();
                let _ = PeekMessageW(&mut dummy_msg, None, 0, 0, PM_NOREMOVE);
                let tid = GetCurrentThreadId();
                if ready_tx.send(tid).is_err() { return; }
            }

            run_worker_message_loop(cmd_rx, event_tx_clone);
        });

        if let Ok(tid) = ready_rx.recv_timeout(Duration::from_secs(2)) {
            *worker_guard = Some(WorkerHandle {
                thread_id: tid,
                cmd_tx,
                join_handle: Some(join_handle),
            });
        }
    }

    /// 非阻塞接收热键触发事件（供宿主事件循环/UI轮询消费）
    pub fn try_recv_event(&self) -> Option<HotkeyEvent> {
        self.event_rx.lock().try_recv().ok()
    }

    /// 获取最后一次配置注册的报告
    pub fn last_report(&self) -> Option<HotkeyRegistrationReport> {
        self.last_report.lock().clone()
    }

    /// 从原始配置字典 extra["hotkeys"] 和 extra["adjust_hotkeys"] 加载并注册
    pub fn configure_from_extra(
        &self,
        extra: &HashMap<String, serde_json::Value>,
    ) -> HotkeyRegistrationReport {
        let mut hotkeys = HashMap::new();
        if let Some(serde_json::Value::Object(map)) = extra.get("hotkeys") {
            for (k, v) in map {
                if let Ok(cfg) = serde_json::from_value::<HotkeyBindingConfig>(v.clone()) {
                    hotkeys.insert(k.clone(), cfg);
                }
            }
        }

        let mut adjust = Vec::new();
        if let Some(serde_json::Value::Array(arr)) = extra.get("adjust_hotkeys") {
            for v in arr {
                if let Ok(cfg) = serde_json::from_value::<AdjustHotkeyConfig>(v.clone()) {
                    adjust.push(cfg);
                }
            }
        }

        self.configure(&hotkeys, &adjust)
    }

    /// 配置并重载全部快捷键绑定，逐项验证并返回注册结果报告
    pub fn configure(
        &self,
        hotkeys: &HashMap<String, HotkeyBindingConfig>,
        adjust: &[AdjustHotkeyConfig],
    ) -> HotkeyRegistrationReport {
        *self.cached_bindings.lock() = (hotkeys.clone(), adjust.to_vec());
        self.register_bindings(hotkeys, adjust)
    }

    /// 直接注册指定的快捷键绑定列表
    pub fn register_bindings(
        &self,
        hotkeys: &HashMap<String, HotkeyBindingConfig>,
        adjust: &[AdjustHotkeyConfig],
    ) -> HotkeyRegistrationReport {
        let planned = validate_and_plan_bindings(hotkeys, adjust);

        self.ensure_worker_started();
        let worker_guard = self.worker.lock();
        if let Some(ref handle) = *worker_guard {
            let (reply_tx, reply_rx) = sync_channel::<HotkeyRegistrationReport>(1);
            if handle
                .cmd_tx
                .send(WorkerCommand::Register {
                    items: planned,
                    reply_tx,
                })
                .is_ok()
            {
                unsafe {
                    let _ = PostThreadMessageW(
                        handle.thread_id,
                        WM_HOTKEY_WORKER_COMMAND,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                if let Ok(report) = reply_rx.recv_timeout(Duration::from_secs(2)) {
                    *self.last_report.lock() = Some(report.clone());
                    return report;
                }
            }
        }

        // 线程通信异常情况下的保底报告
        let fallback_report = HotkeyRegistrationReport {
            results: vec![SingleRegistrationResult {
                target_name: "worker_communication".to_string(),
                modifier: "".to_string(),
                key: "".to_string(),
                status: RegistrationStatus::SystemOccupied {
                    error_code: 0,
                    error_message: "后台热键消息线程通信超时或未就绪".to_string(),
                },
            }],
        };
        *self.last_report.lock() = Some(fallback_report.clone());
        fallback_report
    }

    /// 注册全部系统快捷键（兼容现有入口）
    pub fn register_all(&self) -> Result<(), String> {
        let (hotkeys, adjust) = self.cached_bindings.lock().clone();
        let report = self.register_bindings(&hotkeys, &adjust);
        if report.failed_count() > 0 {
            Err(format!(
                "部分快捷键注册失败或冲突 (失败数: {})",
                report.failed_count()
            ))
        } else {
            Ok(())
        }
    }

    /// 注销全部系统快捷键（兼容现有入口）
    pub fn unregister_all(&self) {
        let worker_guard = self.worker.lock();
        if let Some(ref handle) = *worker_guard {
            let (reply_tx, reply_rx) = sync_channel::<()>(1);
            if handle
                .cmd_tx
                .send(WorkerCommand::UnregisterAll { reply_tx })
                .is_ok()
            {
                unsafe {
                    let _ = PostThreadMessageW(
                        handle.thread_id,
                        WM_HOTKEY_WORKER_COMMAND,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                let _ = reply_rx.recv_timeout(Duration::from_secs(1));
            }
        }
    }

    /// 有界优雅退出，注销所有热键并回收专用线程
    pub fn shutdown(&self) {
        let mut worker_guard = self.worker.lock();
        if let Some(mut handle) = worker_guard.take() {
            let (reply_tx, reply_rx) = sync_channel::<()>(1);
            if handle.cmd_tx.send(WorkerCommand::Stop { reply_tx }).is_ok() {
                unsafe {
                    let _ = PostThreadMessageW(
                        handle.thread_id,
                        WM_HOTKEY_WORKER_COMMAND,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                let _ = reply_rx.recv_timeout(Duration::from_millis(800));
            }
            if let Some(join) = handle.join_handle.take() {
                let _ = join.join();
            }
        }
    }
}

impl Drop for GlobalHotkeyManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 纯校验与注册规划逻辑（独立无副作用，便于单元测试）
pub fn validate_and_plan_bindings(
    hotkeys: &HashMap<String, HotkeyBindingConfig>,
    adjust: &[AdjustHotkeyConfig],
) -> Vec<PlannedRegistrationItem> {
    let mut planned = Vec::new();
    let mut registered_combos: HashSet<(u32, u32)> = HashSet::new();

    // 1. 规划 8 种固定 Action 快捷键
    for action in &CycleAction::ALL {
        let act_name = action.as_str();
        let default_cfg = HotkeyBindingConfig::default();
        let cfg = hotkeys.get(act_name).unwrap_or(&default_cfg);

        let mod_opt = HotkeyModifier::parse(&cfg.modifier);
        let key_opt = HotkeyKey::parse(&cfg.key);

        let target_name = format!("{}({})", action.label(), act_name);
        let modifier_str = cfg.modifier.clone();
        let key_str = cfg.key.clone();

        match (mod_opt, key_opt) {
            (Some(HotkeyModifier::None), Some(HotkeyKey::None)) => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::SkippedUnbound),
                });
            }
            (None, _) => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str: cfg.modifier.clone(),
                    key_str: cfg.key.clone(),
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(format!(
                        "非法修饰键 '{}'",
                        cfg.modifier
                    ))),
                });
            }
            (_, None) => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str: cfg.modifier.clone(),
                    key_str: cfg.key.clone(),
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(format!(
                        "非法按键 '{}'",
                        cfg.key
                    ))),
                });
            }
            (Some(m), Some(HotkeyKey::None)) if m != HotkeyModifier::None => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str: cfg.modifier.clone(),
                    key_str: cfg.key.clone(),
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(
                        "已指定修饰键但按键为'无'".to_string(),
                    )),
                });
            }
            (Some(modifier), Some(key)) => {
                let combo = KeyCombination { modifier, key };
                let signature = (combo.modifier_flags(), combo.vk_code());
                if registered_combos.contains(&signature) {
                    planned.push(PlannedRegistrationItem {
                        target_name,
                        modifier_str,
                        key_str,
                        event: None,
                        combo: Some(combo),
                        pre_error: Some(RegistrationStatus::DuplicateBinding),
                    });
                } else {
                    registered_combos.insert(signature);
                    planned.push(PlannedRegistrationItem {
                        target_name,
                        modifier_str,
                        key_str,
                        event: Some(HotkeyEvent::Cycle(*action)),
                        combo: Some(combo),
                        pre_error: None,
                    });
                }
            }
        }
    }

    // 2. 规划可调参数快捷键
    for (idx, rule) in adjust.iter().enumerate() {
        let param_opt = AdjustParam::from_str_name(&rule.param);
        let dir_opt = AdjustDirection::from_str_name(&rule.direction);
        let mod_opt = HotkeyModifier::parse(&rule.modifier);
        let key_opt = HotkeyKey::parse(&rule.key);

        let param_label = param_opt.map(|p| p.label()).unwrap_or("未知参数");
        let target_name = format!("可调参数#{}[{}:{}]", idx + 1, param_label, rule.param);
        let modifier_str = rule.modifier.clone();
        let key_str = rule.key.clone();

        // 校验修饰键与按键
        let (modifier, key) = match (mod_opt, key_opt) {
            (Some(HotkeyModifier::None), Some(HotkeyKey::None)) => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::SkippedUnbound),
                });
                continue;
            }
            (None, _) => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(format!(
                        "非法修饰键 '{}'",
                        rule.modifier
                    ))),
                });
                continue;
            }
            (_, None) => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(format!(
                        "非法按键 '{}'",
                        rule.key
                    ))),
                });
                continue;
            }
            (Some(m), Some(HotkeyKey::None)) if m != HotkeyModifier::None => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(
                        "已指定修饰键但按键为'无'".to_string(),
                    )),
                });
                continue;
            }
            (Some(m), Some(k)) => (m, k),
        };

        // 校验参数类型
        let param = match param_opt {
            Some(p) => p,
            None => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(format!(
                        "不支持的可调参数 '{}'",
                        rule.param
                    ))),
                });
                continue;
            }
        };

        // 校验方向
        let direction = match dir_opt {
            Some(d) => d,
            None => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(format!(
                        "非法的调节方向 '{}'",
                        rule.direction
                    ))),
                });
                continue;
            }
        };

        // 校验步进 step：必须为正整数且在有效范围内
        let max_span = (param.max() - param.min()) as u32;
        let step = match rule.step {
            Some(s) if s > 0 && s <= max_span => s,
            Some(s) => {
                planned.push(PlannedRegistrationItem {
                    target_name,
                    modifier_str,
                    key_str,
                    event: None,
                    combo: None,
                    pre_error: Some(RegistrationStatus::InvalidConfig(format!(
                        "步进 step 必须在 1..={} 范围内，当前为 {}",
                        max_span, s
                    ))),
                });
                continue;
            }
            None => param.default_step(),
        };

        let combo = KeyCombination { modifier, key };
        let signature = (combo.modifier_flags(), combo.vk_code());
        if registered_combos.contains(&signature) {
            planned.push(PlannedRegistrationItem {
                target_name,
                modifier_str,
                key_str,
                event: None,
                combo: Some(combo),
                pre_error: Some(RegistrationStatus::DuplicateBinding),
            });
        } else {
            registered_combos.insert(signature);
            planned.push(PlannedRegistrationItem {
                target_name,
                modifier_str,
                key_str,
                event: Some(HotkeyEvent::Adjust {
                    param,
                    direction,
                    step,
                }),
                combo: Some(combo),
                pre_error: None,
            });
        }
    }

    planned
}

/// Win32 专用工作线程函数：负责消息循环、热键注册与注销
fn run_worker_message_loop(cmd_rx: Receiver<WorkerCommand>, event_tx: Sender<HotkeyEvent>) {
    let mut hotkey_id_allocator = 1000i32;
    let mut registered_ids: Vec<i32> = Vec::new();
    let mut hotkey_events: HashMap<i32, HotkeyEvent> = HashMap::new();

    let mut msg = MSG::default();

    loop {
        // 1. 处理所有排队的控制命令
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                WorkerCommand::Register { items, reply_tx } => {
                    // 先注销上一次所有热键
                    for id in registered_ids.drain(..) {
                        unsafe {
                            let _ = UnregisterHotKey(None, id);
                        }
                    }
                    hotkey_events.clear();

                    let mut results = Vec::new();

                    for item in items {
                        if let Some(err_status) = item.pre_error {
                            results.push(SingleRegistrationResult {
                                target_name: item.target_name,
                                modifier: item.modifier_str,
                                key: item.key_str,
                                status: err_status,
                            });
                            continue;
                        }

                        let combo = match item.combo {
                            Some(c) => c,
                            None => {
                                results.push(SingleRegistrationResult {
                                    target_name: item.target_name,
                                    modifier: item.modifier_str,
                                    key: item.key_str,
                                    status: RegistrationStatus::SkippedUnbound,
                                });
                                continue;
                            }
                        };

                        let event = match item.event {
                            Some(e) => e,
                            None => {
                                results.push(SingleRegistrationResult {
                                    target_name: item.target_name,
                                    modifier: item.modifier_str,
                                    key: item.key_str,
                                    status: RegistrationStatus::SkippedUnbound,
                                });
                                continue;
                            }
                        };

                        let id = hotkey_id_allocator;
                        hotkey_id_allocator += 1;

                        let reg_res = unsafe {
                            RegisterHotKey(
                                None,
                                id,
                                HOT_KEY_MODIFIERS(combo.modifier_flags()),
                                combo.vk_code(),
                            )
                        };

                        match reg_res {
                            Ok(()) => {
                                registered_ids.push(id);
                                hotkey_events.insert(id, event);
                                results.push(SingleRegistrationResult {
                                    target_name: item.target_name,
                                    modifier: item.modifier_str,
                                    key: item.key_str,
                                    status: RegistrationStatus::Success { id },
                                });
                            }
                            Err(e) => {
                                results.push(SingleRegistrationResult {
                                    target_name: item.target_name,
                                    modifier: item.modifier_str,
                                    key: item.key_str,
                                    status: RegistrationStatus::SystemOccupied {
                                        error_code: e.code().0 as u32,
                                        error_message: e.message().to_string(),
                                    },
                                });
                            }
                        }
                    }

                    let _ = reply_tx.send(HotkeyRegistrationReport { results });
                }
                WorkerCommand::UnregisterAll { reply_tx } => {
                    for id in registered_ids.drain(..) {
                        unsafe {
                            let _ = UnregisterHotKey(None, id);
                        }
                    }
                    hotkey_events.clear();
                    let _ = reply_tx.send(());
                }
                WorkerCommand::Stop { reply_tx } => {
                    for id in registered_ids.drain(..) {
                        unsafe {
                            let _ = UnregisterHotKey(None, id);
                        }
                    }
                    hotkey_events.clear();
                    let _ = reply_tx.send(());
                    return;
                }
            }
        }

        // 2. 阻塞获取 Windows 线程消息（包括 WM_HOTKEY 与 WM_HOTKEY_WORKER_COMMAND）
        let has_msg = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if !has_msg.as_bool() {
            break;
        }

        if msg.message == WM_HOTKEY {
            let hid = msg.wParam.0 as i32;
            if let Some(event) = hotkey_events.get(&hid) {
                let _ = event_tx.send(event.clone());
            }
        }
    }

    // 线程退出时的兜底清理
    for id in registered_ids {
        unsafe {
            let _ = UnregisterHotKey(None, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_and_plan_all_actions_and_params() {
        let mut hotkeys = HashMap::new();
        hotkeys.insert(
            "picture_mode_cycle".to_string(),
            HotkeyBindingConfig {
                modifier: "Ctrl + Alt".to_string(),
                key: "F1".to_string(),
            },
        );
        hotkeys.insert(
            "freesync_toggle".to_string(),
            HotkeyBindingConfig {
                modifier: "Ctrl + Shift".to_string(),
                key: "P".to_string(),
            },
        );

        let adjust = vec![
            AdjustHotkeyConfig {
                param: "backlight".to_string(),
                direction: "increase".to_string(),
                step: Some(10),
                modifier: "Alt + Shift".to_string(),
                key: "PageUp".to_string(),
            },
            AdjustHotkeyConfig {
                param: "atmosphere_illumination".to_string(),
                direction: "decrease".to_string(),
                step: None, // 应该默认 step 1
                modifier: "Win + Shift".to_string(),
                key: "Down".to_string(),
            },
        ];

        let planned = validate_and_plan_bindings(&hotkeys, &adjust);

        // 8 个 action + 2 个 adjust = 10 项
        assert_eq!(planned.len(), 10);

        // picture_mode_cycle: 有效注册
        let pic_item = planned
            .iter()
            .find(|p| p.target_name.contains("picture_mode_cycle"))
            .unwrap();
        assert!(pic_item.pre_error.is_none());
        assert_eq!(
            pic_item.event,
            Some(HotkeyEvent::Cycle(CycleAction::PictureModeCycle))
        );

        // 未配置的 action (如 local_dimming_cycle): 应跳过
        let dim_item = planned
            .iter()
            .find(|p| p.target_name.contains("local_dimming_cycle"))
            .unwrap();
        assert_eq!(dim_item.pre_error, Some(RegistrationStatus::SkippedUnbound));

        // 可调参数 backlight: 有效注册且 step=10
        let back_item = planned
            .iter()
            .find(|p| p.target_name.contains("backlight"))
            .unwrap();
        assert!(back_item.pre_error.is_none());
        assert_eq!(
            back_item.event,
            Some(HotkeyEvent::Adjust {
                param: AdjustParam::Backlight,
                direction: AdjustDirection::Increase,
                step: 10,
            })
        );

        // 可调参数 atmosphere_illumination: 有效注册且使用默认 step 1
        let light_item = planned
            .iter()
            .find(|p| p.target_name.contains("atmosphere_illumination"))
            .unwrap();
        assert!(light_item.pre_error.is_none());
        assert_eq!(
            light_item.event,
            Some(HotkeyEvent::Adjust {
                param: AdjustParam::AtmosphereIllumination,
                direction: AdjustDirection::Decrease,
                step: 1,
            })
        );
    }

    #[test]
    fn test_duplicate_key_combination_detected() {
        let mut hotkeys = HashMap::new();
        hotkeys.insert(
            "picture_mode_cycle".to_string(),
            HotkeyBindingConfig {
                modifier: "Ctrl + Alt".to_string(),
                key: "A".to_string(),
            },
        );
        hotkeys.insert(
            "input_source_cycle".to_string(),
            HotkeyBindingConfig {
                modifier: "Ctrl + Alt".to_string(),
                key: "A".to_string(), // 重复绑定！
            },
        );

        let planned = validate_and_plan_bindings(&hotkeys, &[]);

        let first = planned
            .iter()
            .find(|p| p.target_name.contains("picture_mode_cycle"))
            .unwrap();
        let dup = planned
            .iter()
            .find(|p| p.target_name.contains("input_source_cycle"))
            .unwrap();

        // 一个成功规划，另一个检测为重复
        assert!(
            (first.pre_error.is_none()
                && dup.pre_error == Some(RegistrationStatus::DuplicateBinding))
                || (dup.pre_error.is_none()
                    && first.pre_error == Some(RegistrationStatus::DuplicateBinding))
        );
    }

    #[test]
    fn test_invalid_key_and_step_handling() {
        let mut hotkeys = HashMap::new();
        hotkeys.insert(
            "color_space_cycle".to_string(),
            HotkeyBindingConfig {
                modifier: "Ctrl + Alt".to_string(),
                key: "InvalidKeyName".to_string(), // 非法按键
            },
        );

        let adjust = vec![
            AdjustHotkeyConfig {
                param: "backlight".to_string(),
                direction: "increase".to_string(),
                step: Some(0), // 非法 step (0 不是有效正整数)
                modifier: "Ctrl + Alt".to_string(),
                key: "B".to_string(),
            },
            AdjustHotkeyConfig {
                param: "backlight".to_string(),
                direction: "increase".to_string(),
                step: Some(99999), // 超出范围
                modifier: "Ctrl + Alt".to_string(),
                key: "C".to_string(),
            },
        ];

        let planned = validate_and_plan_bindings(&hotkeys, &adjust);

        let invalid_key_item = planned
            .iter()
            .find(|p| p.target_name.contains("color_space_cycle"))
            .unwrap();
        assert!(matches!(
            invalid_key_item.pre_error,
            Some(RegistrationStatus::InvalidConfig(_))
        ));

        let zero_step_item = planned.iter().find(|p| p.key_str == "B").unwrap();
        assert!(matches!(
            zero_step_item.pre_error,
            Some(RegistrationStatus::InvalidConfig(_))
        ));

        let overflow_step_item = planned.iter().find(|p| p.key_str == "C").unwrap();
        assert!(matches!(
            overflow_step_item.pre_error,
            Some(RegistrationStatus::InvalidConfig(_))
        ));
    }
}
