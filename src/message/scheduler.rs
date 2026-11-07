use super::{AppCommand, AppEvent, MessageBus};
use crate::{
    adb::{AdbDevice, AdbError},
    config,
    presets::{
        recovery::DeviceIdentity,
        scheduler::{valid_task, AdbPresetExecutor, AutoTaskScheduler, PresetDeviceExecutor},
        ApplyResult,
    },
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

struct Offline;
impl PresetDeviceExecutor for Offline {
    fn is_connected(&self) -> bool {
        false
    }
    fn device_identity(&self) -> Option<DeviceIdentity> {
        None
    }
    fn capture_snapshot(&self) -> Result<HashMap<String, serde_json::Value>, AdbError> {
        Err(AdbError::NotFound)
    }
    fn apply_preset(&self, _: &HashMap<String, serde_json::Value>) -> ApplyResult {
        ApplyResult::default()
    }
}
pub(super) struct SchedulerEngine {
    state: AutoTaskScheduler,
    last: Instant,
    generation: u64,
    identity: Option<DeviceIdentity>,
}
impl SchedulerEngine {
    pub fn new() -> Self {
        let mut state = AutoTaskScheduler::new();
        let cfg = config::load_settings();
        if let Some(value) = cfg.extra.get("scheduler_recovery") {
            if let Err(error) = state.import_recovery_from_value(value) {
                crate::logger::log_preset(&error);
            }
        }
        for task in &cfg.auto_tasks {
            if let Some(snapshot) = task.snapshot.as_ref().filter(|s| !s.is_empty()) {
                let already_recorded = state.active_task_id.as_ref() == Some(&task.id)
                    || state
                        .pending_restore_record
                        .as_ref()
                        .is_some_and(|r| r.task_id == task.id)
                    || state.orphaned_restores.iter().any(|r| r.task_id == task.id);
                if !already_recorded {
                    state.orphaned_restores.push(
                        crate::presets::recovery::PendingRestoreRecord::from_legacy_snapshot(
                            task.id.clone(),
                            snapshot.clone(),
                            "旧版任务快照：请确认当前设备后还原",
                        ),
                    );
                }
            }
        }
        // A previous run may have stopped after applying a task. Recover its original baseline first.
        if state.pending_restore_record.is_none() {
            if let Some(snapshot) = state.initial_baseline.take() {
                let task_id = state
                    .active_task_id
                    .take()
                    .unwrap_or_else(|| "上次会话".into());
                state.pending_restore = Some((task_id.clone(), snapshot.clone()));
                state.pending_restore_record =
                    Some(crate::presets::recovery::PendingRestoreRecord {
                        task_id,
                        snapshot,
                        source_device: state.session.as_ref().and_then(|s| s.source_device.clone()),
                        created_at: crate::presets::recovery::chrono_like_now(),
                        requires_user_confirmation: true,
                        user_confirmed: false,
                        confirmed_device: None,
                        reason: "上次会话未完成还原".into(),
                    });
            }
        }
        state.active_task_id = None;
        for record in state
            .pending_restore_record
            .iter_mut()
            .chain(state.orphaned_restores.iter_mut())
        {
            record.user_confirmed = false;
            record.confirmed_device = None;
        }
        Self {
            state,
            last: Instant::now() - Duration::from_secs(2),
            generation: 0,
            identity: None,
        }
    }
    pub fn edit(
        &mut self,
        command: &AppCommand,
        bus: &MessageBus,
        device: Option<&AdbDevice>,
    ) -> bool {
        let cfg = config::load_settings();
        let mut tasks = cfg.auto_tasks.clone();
        match command {
            AppCommand::SaveAutoTask(task) => {
                self.state.blocked_task_ids.retain(|id| id != &task.id);
                if !valid_task(task)
                    || !cfg.presets.iter().any(|p| p.id == task.preset_id)
                        && task.preset_id != crate::presets::BASELINE_PRESET_ID
                {
                    let _ = bus.event_sender.send(AppEvent::ToastNotification {
                        message: "请选择有效预设，起止时间须为 HH:MM 且不能相同".into(),
                        is_error: true,
                    });
                    return true;
                }
                if let Some(old) = tasks.iter_mut().find(|old| old.id == task.id) {
                    let snapshot = old.snapshot.clone();
                    *old = task.clone();
                    old.snapshot = snapshot;
                } else {
                    tasks.push(task.clone());
                }
            }
            AppCommand::DeleteAutoTask(id) => {
                let old = tasks.iter().find(|t| &t.id == id);
                self.state
                    .on_task_deleted(id, old.and_then(|t| t.snapshot.clone()));
                tasks.retain(|t| &t.id != id);
            }
            AppCommand::ConfirmTaskRestore(id) => {
                let Some(device) = device else {
                    let _ = bus.event_sender.send(AppEvent::ToastNotification {
                        message: "请先连接原显示器，再确认还原".into(),
                        is_error: true,
                    });
                    return true;
                };
                if self
                    .state
                    .pending_restore_record
                    .as_ref()
                    .is_none_or(|r| &r.task_id == id)
                {
                    if let Some(index) = self
                        .state
                        .orphaned_restores
                        .iter()
                        .position(|r| &r.task_id == id)
                    {
                        let record = self.state.orphaned_restores.remove(index);
                        self.state.pending_restore =
                            Some((record.task_id.clone(), record.snapshot.clone()));
                        self.state.pending_restore_record = Some(record);
                    }
                    self.state.confirm_restore_for_task(
                        id,
                        Some(DeviceIdentity::new(
                            device.serial(),
                            crate::presets::query_hardware_serial(device),
                        )),
                    );
                } else {
                    let _ = bus.event_sender.send(AppEvent::ToastNotification {
                        message: "请先处理当前待还原记录".into(),
                        is_error: true,
                    });
                }
            }
            _ => return false,
        }
        self.persist(tasks, bus);
        self.last = Instant::now() - Duration::from_secs(2);
        true
    }
    fn persist(&self, tasks: Vec<config::AutoTaskItem>, bus: &MessageBus) {
        let recovery = self.state.export_recovery_to_value();
        match config::update_settings(|cfg| {
            cfg.auto_tasks = tasks;
            cfg.extra.insert("scheduler_recovery".into(), recovery);
        }) {
            Ok(cfg) => {
                let _ = bus
                    .event_sender
                    .send(AppEvent::ConfigUpdated((*cfg).clone()));
            }
            Err(error) => {
                let _ = bus.event_sender.send(AppEvent::ToastNotification {
                    message: format!("自动任务状态保存失败：{error}"),
                    is_error: true,
                });
            }
        }
    }
    pub fn tick(&mut self, device: Option<&AdbDevice>, bus: &MessageBus) {
        if self.last.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last = Instant::now();
        let cfg = config::load_settings();
        if cfg.auto_tasks.is_empty()
            && self.state.pending_restore.is_none()
            && self.state.orphaned_restores.is_empty()
        {
            return;
        }
        let generation = bus
            .active_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if generation != self.generation {
            self.identity = None;
            self.generation = generation;
        }
        let mut tasks = cfg.auto_tasks.clone();
        let before = (tasks.clone(), self.state.export_recovery_to_value());
        let presets = cfg
            .presets
            .iter()
            .map(|p| (p.id.clone(), p.values.clone()))
            .collect();
        use chrono::Timelike;
        let time = chrono::Local::now();
        let minute = time.hour() * 60 + time.minute();
        if let Some(adb) = device {
            if self.identity.is_none() {
                self.identity = Some(DeviceIdentity::new(
                    adb.serial(),
                    crate::presets::query_hardware_serial(adb),
                ));
            }
            let executor = AdbPresetExecutor {
                adb,
                identity: self.identity.clone().unwrap(),
            };
            let snapshot_origin: Option<DeviceIdentity> = cfg
                .extra
                .get("preset_snapshot_device")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let safe_snapshot = if snapshot_origin
                .as_ref()
                .is_some_and(|source| source.is_definitely_same_device(&executor.identity))
            {
                cfg.preset_snapshot.as_ref()
            } else {
                None
            };
            self.state.poll_and_check_with_executor(
                minute,
                &mut tasks,
                &presets,
                safe_snapshot,
                true,
                &executor,
            );
        } else {
            self.state.poll_and_check_with_executor(
                minute,
                &mut tasks,
                &presets,
                cfg.preset_snapshot.as_ref(),
                false,
                &Offline,
            );
        }
        if before != (tasks.clone(), self.state.export_recovery_to_value()) {
            self.persist(tasks, bus);
        }
    }
}
