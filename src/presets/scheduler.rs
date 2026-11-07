use crate::adb::{AdbDevice, AdbError};
use crate::config::AutoTaskItem;
use crate::presets::recovery::{
    chrono_like_now, DeviceIdentity, PendingRestoreRecord, SchedulerRecovery, SessionMetadata,
};
use crate::presets::{
    apply_preset_values, capture_hardware_snapshot, query_hardware_serial, ApplyResult,
    BASELINE_PRESET_ID,
};
use std::collections::HashMap;

/// 将 "HH:MM" 格式解析为当天分钟数 (0 - 1439)
pub fn parse_hhmm(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.len() != 5 || !trimmed.is_char_boundary(2) {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    if h < 24 && m < 60 {
        Some(h * 60 + m)
    } else {
        None
    }
}

/// 将分钟数格式化为 "HH:MM"
pub fn format_hhmm(minutes: u32) -> String {
    let m = minutes % (24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// 判断指定时刻是否落在 [start, end) 时间区间内 (支持跨午夜)
pub fn is_in_period(now_minutes: u32, start_minutes: u32, end_minutes: u32) -> bool {
    if start_minutes == end_minutes {
        return true;
    }
    if start_minutes < end_minutes {
        now_minutes >= start_minutes && now_minutes < end_minutes
    } else {
        // 跨午夜区间 (例如 23:00 - 06:00)
        now_minutes >= start_minutes || now_minutes < end_minutes
    }
}

/// 验证自动任务配置的完整性与合法性
pub fn valid_task(task: &AutoTaskItem) -> bool {
    if task.id.is_empty() || task.preset_id.is_empty() {
        return false;
    }
    match (parse_hhmm(&task.start), parse_hhmm(&task.end)) {
        (Some(s), Some(e)) => s != e,
        _ => false,
    }
}

/// 查询当前命中并处于激活状态的自动任务 (重叠时间段时以列表后者为准)
pub fn find_active_task<'a>(
    tasks: &'a [AutoTaskItem],
    now_minutes: u32,
) -> Option<&'a AutoTaskItem> {
    let mut active = None;
    for task in tasks {
        if !valid_task(task) {
            continue;
        }
        if let (Some(s), Some(e)) = (parse_hhmm(&task.start), parse_hhmm(&task.end)) {
            if is_in_period(now_minutes, s, e) {
                active = Some(task);
            }
        }
    }
    active
}

/// 显式硬件执行接口 (便于生产对接与单元测试注入，解耦调度状态与底层 ADB)
pub trait PresetDeviceExecutor {
    fn is_connected(&self) -> bool;
    fn device_identity(&self) -> Option<DeviceIdentity>;
    fn capture_snapshot(&self) -> Result<HashMap<String, serde_json::Value>, AdbError>;
    fn apply_preset(&self, values: &HashMap<String, serde_json::Value>) -> ApplyResult;
    fn checkpoint(
        &self,
        _scheduler: &AutoTaskScheduler,
        _tasks: &[AutoTaskItem],
    ) -> Result<(), String> {
        Ok(())
    }
}

/// 生产级 ADB 硬件执行器
pub struct AdbPresetExecutor<'a> {
    pub adb: &'a AdbDevice,
    pub identity: DeviceIdentity,
}

impl<'a> AdbPresetExecutor<'a> {
    pub fn new(adb: &'a AdbDevice) -> Self {
        let hardware_serial = query_hardware_serial(adb);
        Self {
            adb,
            identity: DeviceIdentity::new(adb.serial(), hardware_serial),
        }
    }
}

impl<'a> PresetDeviceExecutor for AdbPresetExecutor<'a> {
    fn checkpoint(
        &self,
        scheduler: &AutoTaskScheduler,
        tasks: &[AutoTaskItem],
    ) -> Result<(), String> {
        let recovery = scheduler.export_recovery_to_value();
        crate::config::update_settings(|config| {
            config.auto_tasks = tasks.to_vec();
            config.extra.insert("scheduler_recovery".into(), recovery);
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn is_connected(&self) -> bool {
        self.adb.get_state().is_connected()
    }

    fn device_identity(&self) -> Option<DeviceIdentity> {
        Some(self.identity.clone())
    }

    fn capture_snapshot(&self) -> Result<HashMap<String, serde_json::Value>, AdbError> {
        capture_hardware_snapshot(self.adb)
    }

    fn apply_preset(&self, values: &HashMap<String, serde_json::Value>) -> ApplyResult {
        apply_preset_values(self.adb, values)
    }
}

/// 自动定时任务运行时状态机调度器
#[derive(Debug, Default)]
pub struct AutoTaskScheduler {
    pub blocked_task_ids: Vec<String>,
    pub active_task_id: Option<String>,
    pub waiting_task_id: Option<String>,
    /// 记录进入自动任务链前的最底层硬件初始基线，防止 A->B 切换时二次捕获被污染的脏状态
    pub initial_baseline: Option<HashMap<String, serde_json::Value>>,
    /// 兼容旧版内存字段 (task_id, snapshot)
    pub pending_restore: Option<(String, HashMap<String, serde_json::Value>)>,

    /// 结构化持久化元数据与恢复记录
    pub session: Option<SessionMetadata>,
    pub pending_restore_record: Option<PendingRestoreRecord>,
    pub orphaned_restores: Vec<PendingRestoreRecord>,

    /// 有界退避状态 (防止每秒 apply 失败刷屏)
    pub backoff_failure_counts: HashMap<String, u32>,
    pub backoff_until_ticks: HashMap<String, u64>,
    pub current_tick: u64,
}

impl AutoTaskScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查指定操作标识是否正处于有界退避等待中
    pub fn is_in_backoff(&self, key: &str) -> bool {
        if let Some(&until) = self.backoff_until_ticks.get(key) {
            self.current_tick < until
        } else {
            false
        }
    }

    /// 记录操作失败并计算有界指数退避步长 (3 -> 6 -> 12 -> 30 ticks)
    pub fn record_failure(&mut self, key: &str) {
        let count = self
            .backoff_failure_counts
            .entry(key.to_string())
            .or_insert(0);
        *count = count.saturating_add(1);
        let wait_ticks = match *count {
            1 => 3,
            2 => 6,
            3 => 12,
            _ => 30,
        };
        self.backoff_until_ticks
            .insert(key.to_string(), self.current_tick + wait_ticks);
    }

    /// 清除操作失败计数与退避状态
    pub fn reset_failure(&mut self, key: &str) {
        self.backoff_failure_counts.remove(key);
        self.backoff_until_ticks.remove(key);
    }

    /// 导出当前调度恢复状态为 SchedulerRecovery 模型
    pub fn export_recovery_state(&self) -> SchedulerRecovery {
        SchedulerRecovery {
            blocked_task_ids: self.blocked_task_ids.clone(),
            session: self.session.clone(),
            initial_baseline: self.initial_baseline.clone(),
            active_task_id: self.active_task_id.clone(),
            waiting_task_id: self.waiting_task_id.clone(),
            pending_restore: self.pending_restore_record.clone(),
            orphaned_restores: self.orphaned_restores.clone(),
        }
    }

    /// 从 SchedulerRecovery 模型导入恢复状态
    pub fn import_recovery_state(&mut self, recovery: SchedulerRecovery) {
        self.blocked_task_ids = recovery.blocked_task_ids;
        self.session = recovery.session;
        self.initial_baseline = recovery.initial_baseline;
        self.active_task_id = recovery.active_task_id;
        self.waiting_task_id = recovery.waiting_task_id;
        if let Some(record) = recovery.pending_restore {
            self.pending_restore = Some((record.task_id.clone(), record.snapshot.clone()));
            self.pending_restore_record = Some(record);
        } else {
            self.pending_restore = None;
            self.pending_restore_record = None;
        }
        self.orphaned_restores = recovery.orphaned_restores;
    }

    /// 导出为 JSON Value 供主 UI 写入 config.extra["scheduler_recovery"]
    pub fn export_recovery_to_value(&self) -> serde_json::Value {
        self.export_recovery_state().to_extra_value()
    }

    /// 从 config.extra["scheduler_recovery"] 导入恢复数据
    pub fn import_recovery_from_value(&mut self, val: &serde_json::Value) -> Result<(), String> {
        let rec = SchedulerRecovery::from_extra_value(val)?;
        self.import_recovery_state(rec);
        Ok(())
    }

    /// 用户在 UI 显式确认恢复指定任务的基线
    pub fn confirm_restore_for_task(
        &mut self,
        task_id: &str,
        current_device: Option<DeviceIdentity>,
    ) {
        if let Some(ref mut rec) = self.pending_restore_record {
            if rec.task_id == task_id {
                rec.user_confirmed = true;
                rec.confirmed_device = current_device.clone();
                rec.requires_user_confirmation = false;
            }
        }
        for rec in &mut self.orphaned_restores {
            if rec.task_id == task_id {
                rec.user_confirmed = true;
                rec.confirmed_device = current_device.clone();
                rec.requires_user_confirmation = false;
            }
        }
    }

    /// 当用户在 UI 删除某个定时任务时，安全转存其未还原的硬件基线，杜绝基线丢失
    pub fn on_task_deleted(
        &mut self,
        task_id: &str,
        task_snapshot: Option<HashMap<String, serde_json::Value>>,
    ) {
        let mut baseline_candidate = None;

        // 1. 若当前活跃任务正是被删任务，提取最初干净基线
        if self.active_task_id.as_deref() == Some(task_id) {
            self.active_task_id = None;
            if let Some(b) = self.initial_baseline.take() {
                baseline_candidate = Some(b);
            }
        }

        // 2. 若暂挂记录正是该任务
        if let Some(rec) = self.pending_restore_record.take() {
            if rec.task_id == task_id {
                baseline_candidate = Some(rec.snapshot);
                self.pending_restore = None;
            } else {
                self.pending_restore_record = Some(rec);
            }
        }

        // 3. 传入的任务自身快照兜底
        if baseline_candidate.is_none() {
            baseline_candidate = task_snapshot;
        }

        // 4. 将基线转入孤立恢复池中持久化保存
        if let Some(snap) = baseline_candidate {
            let record = PendingRestoreRecord {
                task_id: task_id.to_string(),
                snapshot: snap,
                source_device: self.session.as_ref().and_then(|s| s.source_device.clone()),
                created_at: chrono_like_now(),
                requires_user_confirmation: true, // 任务已删，还原前提示用户确认
                user_confirmed: false,
                confirmed_device: None,
                reason: format!(
                    "任务 [{}] 已被用户删除，硬件基线安全转存至孤立恢复池",
                    task_id
                ),
            };
            self.orphaned_restores.push(record);
            crate::logger::log_preset(&format!(
                "任务 [{}] 删除，其硬件基线已安全转移至孤立恢复池",
                task_id
            ));
        }
    }

    /// 基于显式硬件执行接口执行轮询调度 (核心纯状态规划逻辑)
    pub fn poll_and_check_with_executor(
        &mut self,
        now_minutes: u32,
        tasks: &mut [AutoTaskItem],
        preset_map: &HashMap<String, HashMap<String, serde_json::Value>>,
        preset_snapshot: Option<&HashMap<String, serde_json::Value>>,
        is_device_connected: bool,
        executor: &dyn PresetDeviceExecutor,
    ) {
        self.current_tick = self.current_tick.wrapping_add(1);

        let current_identity = executor.device_identity();

        // 0. 若存在此前暂挂的基线还原，在设备已连接就绪时优先补执行
        if is_device_connected {
            let mut record_opt = self.pending_restore_record.clone();
            // 兼容直接从旧 task.snapshot 初始化的场景
            if record_opt.is_none() {
                if let Some((p_id, p_snap)) = self.pending_restore.clone() {
                    record_opt = Some(PendingRestoreRecord::from_legacy_snapshot(
                        p_id,
                        p_snap,
                        "旧版配置暂挂记录",
                    ));
                }
            }

            if let Some(mut record) = record_opt {
                let backoff_key = format!("restore_{}", record.task_id);
                if !self.is_in_backoff(&backoff_key) {
                    // 安全设备校验：未知来源旧 snapshot 或序列号不匹配时，不得直接自动写不确定设备！
                    if !(record.user_confirmed
                        && record.confirmed_device.as_ref() == current_identity.as_ref()
                        && current_identity.is_some())
                    {
                        let is_same = match (&current_identity, &record.source_device) {
                            (Some(cur_dev), Some(src_dev)) => {
                                src_dev.is_definitely_same_device(cur_dev)
                            }
                            _ => false,
                        };
                        if is_same {
                            record.requires_user_confirmation = false;
                        } else {
                            record.requires_user_confirmation = true;
                        }
                    }

                    if record.requires_user_confirmation {
                        crate::logger::log_preset(&format!(
                            "暂挂任务 [{}] 来源设备与当前设备不确定匹配或为未知来源，需用户手动确认后方可还原",
                            record.task_id
                        ));
                        self.pending_restore_record = Some(record);
                    } else {
                        record.user_confirmed = false;
                        record.confirmed_device = None;
                        let res = executor.apply_preset(&record.snapshot);
                        // 关键质量门：全必要项成功才判定为真正成功！
                        if res.ok() && !res.applied.is_empty() {
                            crate::logger::log_preset(&format!(
                                "成功补执行暂挂任务 [{}] 的基线还原: {}",
                                record.task_id,
                                res.summary()
                            ));
                            self.pending_restore = None;
                            self.pending_restore_record = None;
                            self.reset_failure(&backoff_key);
                            if let Some(t) = tasks.iter_mut().find(|t| t.id == record.task_id) {
                                t.snapshot = None;
                            }
                        } else {
                            crate::logger::log_preset(&format!(
                                "补执行暂挂任务 [{}] 失败: {}，保留暂挂快照",
                                record.task_id,
                                res.summary()
                            ));
                            self.record_failure(&backoff_key);
                            record.reason = format!("补执行基线还原失败: {}", res.summary());
                            self.pending_restore_record = Some(record.clone());
                            self.pending_restore = Some((record.task_id, record.snapshot));
                        }
                    }
                }
            }
        }

        // Unresolved recovery must never be replaced by a new task's baseline.
        if self.pending_restore_record.is_some() || !self.orphaned_restores.is_empty() {
            return;
        }
        let active_match =
            find_active_task(tasks, now_minutes).map(|t| (t.id.clone(), t.preset_id.clone()));

        // 1. 若时段结束（切出到无任务状态）
        if active_match.is_none() {
            if let Some(prev_id) = self.active_task_id.take() {
                crate::logger::log_preset(&format!(
                    "自动任务 [{}] 时段结束，准备还原初始基线设置",
                    prev_id
                ));

                let baseline_to_restore = self.initial_baseline.take().or_else(|| {
                    tasks
                        .iter()
                        .find(|t| t.id == prev_id)
                        .and_then(|t| t.snapshot.clone())
                });

                if let Some(snapshot) = baseline_to_restore {
                    let source_device = self
                        .session
                        .as_ref()
                        .and_then(|session| session.source_device.clone());
                    let same_device = source_device
                        .as_ref()
                        .zip(current_identity.as_ref())
                        .is_some_and(|(a, b)| a.is_definitely_same_device(b));
                    let mut restore_success = false;
                    let backoff_key = format!("restore_{}", prev_id);

                    if is_device_connected && same_device && !self.is_in_backoff(&backoff_key) {
                        let res = executor.apply_preset(&snapshot);
                        if res.ok() && !res.applied.is_empty() {
                            restore_success = true;
                        } else {
                            crate::logger::log_preset(&format!(
                                "自动任务 [{}] 基线还原下发失败: {}",
                                prev_id,
                                res.summary()
                            ));
                            self.record_failure(&backoff_key);
                        }
                    }

                    if restore_success {
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == prev_id) {
                            t.snapshot = None;
                        }
                        self.pending_restore = None;
                        self.pending_restore_record = None;
                        self.reset_failure(&backoff_key);
                    } else {
                        // 设备未连接或执行失败：保留挂起快照，绝不可丢失硬件恢复数据！
                        crate::logger::log_preset(&format!(
                            "自动任务 [{}] 基线还原暂挂（设备未连接或下发失败），安全暂挂恢复数据",
                            prev_id
                        ));
                        let record = PendingRestoreRecord {
                            task_id: prev_id.clone(),
                            snapshot: snapshot.clone(),
                            source_device,
                            created_at: chrono_like_now(),
                            requires_user_confirmation: !same_device,
                            user_confirmed: false,
                            confirmed_device: None,
                            reason: "时段结束未完成还原，保留待补执行".to_string(),
                        };
                        self.pending_restore = Some((prev_id.clone(), snapshot.clone()));
                        self.pending_restore_record = Some(record);
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == prev_id) {
                            t.snapshot = Some(snapshot);
                        }
                    }
                }
            }
            self.waiting_task_id = None;
            return;
        }

        let (new_task_id, target_preset_id) = active_match.unwrap();
        if self.blocked_task_ids.contains(&new_task_id) {
            return;
        }

        // 2. 状态未变，无需重复处理
        if self.active_task_id.as_deref() == Some(&new_task_id) {
            return;
        }

        // 3. 设备未连接时进入待命状态，不放弃任务，等连上后再看是否仍在时段内
        if !is_device_connected {
            if self.waiting_task_id.as_deref() != Some(&new_task_id) {
                self.waiting_task_id = Some(new_task_id.clone());
                crate::logger::log_preset(&format!(
                    "自动任务 [{}] 时段已到，但显示器未连接，等待连上后补执行",
                    new_task_id
                ));
            }
            return;
        }

        self.waiting_task_id = None;

        // 4. 解析目标预设：若是 BASELINE_PRESET_ID，使用无预设快照；否则查找预设字典
        let target_values = if target_preset_id == BASELINE_PRESET_ID {
            match preset_snapshot {
                Some(snap) if !snap.is_empty() => snap.clone(),
                _ => {
                    crate::logger::log_preset(&format!(
                        "自动任务 [{}] 要求改用「无预设」，但无可用基线快照，跳过本次执行",
                        new_task_id
                    ));
                    return;
                }
            }
        } else {
            match preset_map.get(&target_preset_id) {
                Some(v) => v.clone(),
                None => {
                    crate::logger::log_preset(&format!(
                        "自动任务 [{}] 引用的预设 [{}] 不存在，跳过执行，不记录虚假快照",
                        new_task_id, target_preset_id
                    ));
                    return;
                }
            }
        };

        // 检查激活退避
        let activate_key = format!("activate_{}", new_task_id);
        if self.is_in_backoff(&activate_key) {
            return;
        }

        // 5. 处理 A -> B 任务衔接：
        // 若之前已有活跃任务 A，其 initial_baseline 必须继承给任务 B，绝不能清空基线导致退出时无法还原
        let current_baseline = if let Some(init) = self.initial_baseline.clone() {
            let source_device = self
                .session
                .as_ref()
                .and_then(|session| session.source_device.clone());
            let same_device = source_device
                .as_ref()
                .zip(current_identity.as_ref())
                .is_some_and(|(a, b)| a.is_definitely_same_device(b));
            if !same_device {
                let task_id = self
                    .active_task_id
                    .clone()
                    .unwrap_or_else(|| new_task_id.clone());
                self.pending_restore = Some((task_id.clone(), init.clone()));
                self.pending_restore_record = Some(PendingRestoreRecord {
                    task_id,
                    snapshot: init,
                    source_device,
                    created_at: chrono_like_now(),
                    requires_user_confirmation: true,
                    user_confirmed: false,
                    confirmed_device: None,
                    reason: "设备身份变化，保留原设备基线并停止任务链".into(),
                });
                return;
            }
            init
        } else {
            // 首次进入自动任务链，从硬件捕获最初始的基线快照
            match executor.capture_snapshot() {
                Ok(snap) => {
                    self.initial_baseline = Some(snap.clone());
                    self.session = Some(SessionMetadata {
                        source_device: current_identity.clone(),
                        captured_at: chrono_like_now(),
                        app_version: None,
                    });
                    snap
                }
                Err(e) => {
                    crate::logger::log_preset(&format!(
                        "自动任务 [{}] 入场抓取硬件快照失败: {}，取消执行",
                        new_task_id, e
                    ));
                    self.record_failure(&activate_key);
                    return;
                }
            }
        };

        // 6. 执行预设下发
        crate::logger::log_preset(&format!(
            "正在激活自动任务 [{}] (预设: {})",
            new_task_id, target_preset_id
        ));
        // Persist the original hardware state before the first write, including partial-write recovery.
        self.pending_restore_record = Some(PendingRestoreRecord {
            task_id: new_task_id.clone(),
            snapshot: current_baseline.clone(),
            source_device: self.session.as_ref().and_then(|s| s.source_device.clone()),
            created_at: chrono_like_now(),
            requires_user_confirmation: false,
            user_confirmed: false,
            confirmed_device: None,
            reason: "任务正在应用，异常退出后恢复原始基线".into(),
        });
        self.pending_restore = Some((new_task_id.clone(), current_baseline.clone()));
        if let Err(error) = executor.checkpoint(self, tasks) {
            crate::logger::log_preset(&format!("基线保存失败，未写入硬件：{error}"));
            self.record_failure(&activate_key);
            return;
        }
        let apply_result = executor.apply_preset(&target_values);

        // 关键质量门：全必要项成功才算成功！若存在 failed 项，绝不可标记为活跃并假成功！
        if !apply_result.ok() || apply_result.applied.is_empty() {
            crate::logger::log_preset(&format!(
                "自动任务 [{}] 下发未完全成功 ({}): 取消标记为活跃",
                new_task_id,
                apply_result.summary()
            ));
            self.record_failure(&activate_key);
            self.blocked_task_ids.push(new_task_id.clone());
            return;
        }

        self.reset_failure(&activate_key);
        self.pending_restore = None;
        self.pending_restore_record = None;

        // 下发成功，更新当前任务自身快照为最初进入时的基线
        if let Some(t) = tasks.iter_mut().find(|t| t.id == new_task_id) {
            t.snapshot = Some(current_baseline);
        }

        // 清理上一个任务 A 的 snapshot 状态
        if let Some(prev_id) = self.active_task_id.take() {
            if prev_id != new_task_id {
                if let Some(t) = tasks.iter_mut().find(|t| t.id == prev_id) {
                    t.snapshot = None;
                }
            }
        }

        self.active_task_id = Some(new_task_id);
    }

    /// 轮询并驱动定时任务的状态迁移与基线还原 (向前兼容原 API)
    pub fn poll_and_check(
        &mut self,
        now_minutes: u32,
        tasks: &mut [AutoTaskItem],
        preset_map: &HashMap<String, HashMap<String, serde_json::Value>>,
        preset_snapshot: Option<&HashMap<String, serde_json::Value>>,
        is_device_connected: bool,
        adb: Option<&AdbDevice>,
    ) {
        if let Some(dev) = adb {
            let executor = AdbPresetExecutor::new(dev);
            self.poll_and_check_with_executor(
                now_minutes,
                tasks,
                preset_map,
                preset_snapshot,
                is_device_connected,
                &executor,
            );
        } else {
            crate::logger::log_preset("未提供有效 ADB 控制句柄，跳过硬件预设应用与还原");
            // 设备掉线但仍在定时时段外时，安全保留挂起
            if !is_device_connected && self.active_task_id.is_some() {
                if find_active_task(tasks, now_minutes).is_none() {
                    if let Some(prev_id) = self.active_task_id.take() {
                        if let Some(snap) = self.initial_baseline.take() {
                            self.pending_restore = Some((prev_id.clone(), snap.clone()));
                            self.pending_restore_record = Some(PendingRestoreRecord {
                                task_id: prev_id.clone(),
                                snapshot: snap.clone(),
                                source_device: self
                                    .session
                                    .as_ref()
                                    .and_then(|session| session.source_device.clone()),
                                created_at: chrono_like_now(),
                                requires_user_confirmation: true,
                                user_confirmed: false,
                                confirmed_device: None,
                                reason: "设备离线未连接，暂挂还原".to_string(),
                            });
                            if let Some(t) = tasks.iter_mut().find(|t| t.id == prev_id) {
                                t.snapshot = Some(snap);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn device_change_never_restores_or_chains_to_another_monitor() {
        for end_period in [false, true] {
            let a = MockPresetExecutor::new(true);
            let mut scheduler = AutoTaskScheduler::new();
            let baseline = HashMap::from([("picture_mode".into(), serde_json::json!(14))]);
            scheduler.initial_baseline = Some(baseline.clone());
            scheduler.active_task_id = Some("a".into());
            scheduler.session = Some(SessionMetadata {
                source_device: a.identity.clone(),
                ..Default::default()
            });
            let mut b = MockPresetExecutor::new(true);
            b.identity = Some(DeviceIdentity::new(
                "192.168.1.100:5555",
                Some("ANOTHER_MONITOR".into()),
            ));
            let mut tasks = vec![
                AutoTaskItem {
                    id: "a".into(),
                    start: "08:00".into(),
                    end: "09:00".into(),
                    preset_id: "p".into(),
                    snapshot: Some(baseline.clone()),
                },
                AutoTaskItem {
                    id: "b".into(),
                    start: "09:00".into(),
                    end: "10:00".into(),
                    preset_id: "p".into(),
                    snapshot: None,
                },
            ];
            let presets = HashMap::from([("p".into(), baseline.clone())]);
            scheduler.poll_and_check_with_executor(
                if end_period { 11 * 60 } else { 9 * 60 + 30 },
                &mut tasks,
                &presets,
                None,
                true,
                &b,
            );
            assert!(b.last_applied_values.lock().is_none());
            let record = scheduler.pending_restore_record.as_ref().unwrap();
            assert_eq!(record.snapshot, baseline);
            assert_eq!(record.source_device, a.identity);
            assert!(record.requires_user_confirmation);
        }
    }

    #[test]
    fn unknown_origin_cannot_continue_task_chain() {
        let mut scheduler = AutoTaskScheduler::new();
        let baseline = HashMap::from([("picture_mode".into(), serde_json::json!(14))]);
        scheduler.initial_baseline = Some(baseline.clone());
        let mut tasks = vec![AutoTaskItem {
            id: "a".into(),
            start: "08:00".into(),
            end: "10:00".into(),
            preset_id: "p".into(),
            snapshot: None,
        }];
        let executor = MockPresetExecutor::new(true);
        scheduler.poll_and_check_with_executor(
            9 * 60,
            &mut tasks,
            &HashMap::from([("p".into(), baseline)]),
            None,
            true,
            &executor,
        );
        assert!(executor.last_applied_values.lock().is_none());
        assert!(
            scheduler
                .pending_restore_record
                .unwrap()
                .requires_user_confirmation
        );
    }

    /// 单元测试专用 Mock 硬件执行器
    #[derive(Debug, Clone)]
    pub struct MockPresetExecutor {
        pub connected: bool,
        pub identity: Option<DeviceIdentity>,
        pub snapshot_result: Result<HashMap<String, serde_json::Value>, AdbError>,
        pub apply_result: ApplyResult,
        pub last_applied_values: Arc<Mutex<Option<HashMap<String, serde_json::Value>>>>,
    }

    impl MockPresetExecutor {
        pub fn new(connected: bool) -> Self {
            let mut def_apply = ApplyResult::default();
            def_apply.applied.push("画面模式".to_string());
            Self {
                connected,
                identity: Some(DeviceIdentity::new(
                    "192.168.1.100:5555",
                    Some("MOCK_SERIAL_123".into()),
                )),
                snapshot_result: {
                    let mut m = HashMap::new();
                    m.insert("picture_mode".into(), serde_json::json!(14));
                    Ok(m)
                },
                apply_result: def_apply,
                last_applied_values: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl PresetDeviceExecutor for MockPresetExecutor {
        fn is_connected(&self) -> bool {
            self.connected
        }

        fn device_identity(&self) -> Option<DeviceIdentity> {
            self.identity.clone()
        }

        fn capture_snapshot(&self) -> Result<HashMap<String, serde_json::Value>, AdbError> {
            self.snapshot_result.clone()
        }

        fn apply_preset(&self, values: &HashMap<String, serde_json::Value>) -> ApplyResult {
            *self.last_applied_values.lock() = Some(values.clone());
            self.apply_result.clone()
        }
    }

    #[test]
    fn test_hhmm_parsing_and_formatting() {
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("09:30"), Some(570));
        assert_eq!(parse_hhmm("23:59"), Some(1439));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("invalid"), None);

        assert_eq!(format_hhmm(0), "00:00");
        assert_eq!(format_hhmm(570), "09:30");
        assert_eq!(format_hhmm(1439), "23:59");
    }

    #[test]
    fn test_is_in_period_day_and_midnight() {
        // 普通日间区间 09:00 - 18:00 (540 - 1080)
        assert!(is_in_period(540, 540, 1080));
        assert!(is_in_period(800, 540, 1080));
        assert!(!is_in_period(1080, 540, 1080));
        assert!(!is_in_period(300, 540, 1080));

        // 跨午夜区间 23:00 - 06:00 (1380 - 360)
        assert!(is_in_period(1380, 1380, 360));
        assert!(is_in_period(1400, 1380, 360));
        assert!(is_in_period(100, 1380, 360));
        assert!(is_in_period(359, 1380, 360));
        assert!(!is_in_period(360, 1380, 360));
        assert!(!is_in_period(720, 1380, 360));
    }

    #[test]
    fn test_active_task_overlap_priority() {
        let tasks = vec![
            AutoTaskItem {
                id: "task1".into(),
                start: "08:00".into(),
                end: "18:00".into(),
                preset_id: "preset_day".into(),
                snapshot: None,
            },
            AutoTaskItem {
                id: "task2".into(),
                start: "12:00".into(),
                end: "14:00".into(),
                preset_id: "preset_lunch".into(),
                snapshot: None,
            },
        ];

        let t9 = find_active_task(&tasks, 9 * 60).unwrap();
        assert_eq!(t9.id, "task1");

        let t13 = find_active_task(&tasks, 13 * 60).unwrap();
        assert_eq!(t13.id, "task2");

        assert!(find_active_task(&tasks, 20 * 60).is_none());
    }

    #[test]
    fn test_scheduler_lifecycle_a_to_b_to_none_baseline_preservation() {
        let mut scheduler = AutoTaskScheduler::new();

        let mut tasks = vec![
            AutoTaskItem {
                id: "task_a".into(),
                start: "09:00".into(),
                end: "12:00".into(),
                preset_id: "preset_a".into(),
                snapshot: None,
            },
            AutoTaskItem {
                id: "task_b".into(),
                start: "12:00".into(),
                end: "18:00".into(),
                preset_id: "preset_b".into(),
                snapshot: None,
            },
        ];

        let mut preset_map = HashMap::new();
        let mut p_a = HashMap::new();
        p_a.insert("picture_mode".into(), serde_json::json!(10));
        preset_map.insert("preset_a".into(), p_a);

        let mut p_b = HashMap::new();
        p_b.insert("picture_mode".into(), serde_json::json!(9));
        preset_map.insert("preset_b".into(), p_b);

        // 模拟原始硬件基线：picture_mode = 14 (标准)
        let mut original_baseline = HashMap::new();
        original_baseline.insert("picture_mode".into(), serde_json::json!(14));
        scheduler.initial_baseline = Some(original_baseline.clone());
        scheduler.session = Some(SessionMetadata {
            source_device: MockPresetExecutor::new(true).identity,
            ..Default::default()
        });

        // 1. 09:30 进入任务 A
        scheduler.poll_and_check_with_executor(
            9 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );
        assert_eq!(scheduler.active_task_id.as_deref(), Some("task_a"));
        assert_eq!(tasks[0].snapshot, Some(original_baseline.clone()));

        // 2. 13:00 从任务 A 切入任务 B (A -> B)
        scheduler.poll_and_check_with_executor(
            13 * 60,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );
        assert_eq!(scheduler.active_task_id.as_deref(), Some("task_b"));
        // 关键质量门：任务 B 必须继承最初的基线，绝不能捕获 A 污染后的状态！
        assert_eq!(tasks[1].snapshot, Some(original_baseline.clone()));
        assert_eq!(tasks[0].snapshot, None); // A 已结束，快照移交并清空

        // 3. 20:00 时段结束，切出到无任务状态
        scheduler.poll_and_check_with_executor(
            20 * 60,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );
        assert_eq!(scheduler.active_task_id, None);
        assert_eq!(scheduler.initial_baseline, None);
        assert_eq!(tasks[1].snapshot, None); // 退出后基线已完成还原并安全清空
    }

    #[test]
    fn test_invalid_preset_does_not_save_fake_snapshot() {
        let mut scheduler = AutoTaskScheduler::new();

        let mut tasks = vec![AutoTaskItem {
            id: "task_ghost".into(),
            start: "10:00".into(),
            end: "11:00".into(),
            preset_id: "non_existent_preset".into(),
            snapshot: None,
        }];

        let preset_map = HashMap::new();

        // 10:30 进入任务，但引用的预设不存在
        scheduler.poll_and_check_with_executor(
            10 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );

        // 关键质量门：不得标记为活跃，不得保存虚假快照！
        assert_eq!(scheduler.active_task_id, None);
        assert_eq!(tasks[0].snapshot, None);
    }

    #[test]
    fn test_disconnected_device_sets_waiting_and_recovers_when_connected() {
        let mut scheduler = AutoTaskScheduler::new();

        let mut tasks = vec![AutoTaskItem {
            id: "task_morning".into(),
            start: "07:00".into(),
            end: "09:00".into(),
            preset_id: "preset_work".into(),
            snapshot: None,
        }];

        let mut preset_map = HashMap::new();
        preset_map.insert("preset_work".into(), HashMap::new());

        // 07:30 时段到达，但显示器未连接 (is_device_connected = false)
        scheduler.poll_and_check_with_executor(
            7 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            false,
            &MockPresetExecutor::new(false),
        );

        // 处于等待待命状态
        assert_eq!(scheduler.waiting_task_id.as_deref(), Some("task_morning"));
        assert_eq!(scheduler.active_task_id, None);
        assert_eq!(tasks[0].snapshot, None);

        // 08:00 显示器连上 (is_device_connected = true)
        let mut fake_baseline = HashMap::new();
        fake_baseline.insert("picture_mode".into(), serde_json::json!(14));
        scheduler.initial_baseline = Some(fake_baseline.clone());
        scheduler.session = Some(SessionMetadata {
            source_device: MockPresetExecutor::new(true).identity,
            ..Default::default()
        });

        scheduler.poll_and_check_with_executor(
            8 * 60,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );

        // 自动激活补执行
        assert_eq!(scheduler.waiting_task_id, None);
        assert_eq!(scheduler.active_task_id.as_deref(), Some("task_morning"));
        assert_eq!(tasks[0].snapshot, Some(fake_baseline));
    }

    #[test]
    fn test_disconnected_device_on_period_end_preserves_pending_restore() {
        let mut scheduler = AutoTaskScheduler::new();

        let mut tasks = vec![AutoTaskItem {
            id: "task_night".into(),
            start: "20:00".into(),
            end: "22:00".into(),
            preset_id: "preset_dark".into(),
            snapshot: None,
        }];

        let mut preset_map = HashMap::new();
        preset_map.insert("preset_dark".into(), HashMap::new());

        let mut initial = HashMap::new();
        initial.insert("picture_mode".into(), serde_json::json!(14));
        scheduler.initial_baseline = Some(initial.clone());
        scheduler.session = Some(SessionMetadata {
            source_device: MockPresetExecutor::new(true).identity,
            ..Default::default()
        });

        // 20:30 处于活跃任务中
        scheduler.poll_and_check_with_executor(
            20 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );
        assert_eq!(scheduler.active_task_id.as_deref(), Some("task_night"));

        // 22:30 时段结束，但显示器断连了 (is_device_connected = false)
        scheduler.poll_and_check_with_executor(
            22 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            false,
            &MockPresetExecutor::new(false),
        );

        // 关键断言：活跃任务已退出，但 pending_restore 和 tasks[0].snapshot 必须被保留，绝不丢失！
        assert_eq!(scheduler.active_task_id, None);
        assert!(
            scheduler.pending_restore.is_some(),
            "断连时必须暂挂待还原基线"
        );
        assert_eq!(tasks[0].snapshot, Some(initial));
    }

    #[test]
    fn test_partial_restore_failure_preserves_pending_restore() {
        let mut scheduler = AutoTaskScheduler::new();

        let mut tasks = vec![AutoTaskItem {
            id: "task_fail".into(),
            start: "10:00".into(),
            end: "11:00".into(),
            preset_id: "preset_f".into(),
            snapshot: None,
        }];

        let mut preset_map = HashMap::new();
        preset_map.insert("preset_f".into(), HashMap::new());

        let mut initial = HashMap::new();
        initial.insert("picture_mode".into(), serde_json::json!(14));
        scheduler.initial_baseline = Some(initial.clone());
        scheduler.session = Some(SessionMetadata {
            source_device: MockPresetExecutor::new(true).identity,
            ..Default::default()
        });

        // 进入任务
        scheduler.poll_and_check_with_executor(
            10 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );
        assert_eq!(scheduler.active_task_id.as_deref(), Some("task_fail"));

        // 准备一个下发部分失败的 Mock
        let mut mock_fail = MockPresetExecutor::new(true);
        mock_fail.apply_result = ApplyResult {
            applied: vec!["画面模式".into()],
            failed: vec![("背光".into(), "写入超时".into())],
            skipped: vec![],
        };

        // 11:30 时段结束退出，使用 mock_fail 执行还原
        scheduler.poll_and_check_with_executor(
            11 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            true,
            &mock_fail,
        );

        // 关键质量门：只要有 failed 项，绝不可判定为成功还原！
        assert_eq!(scheduler.active_task_id, None);
        assert!(
            scheduler.pending_restore_record.is_some(),
            "部分失败必须保留暂挂记录"
        );
        assert!(scheduler.pending_restore.is_some(), "兼容字段必须保留");
        assert_eq!(tasks[0].snapshot, Some(initial));
    }

    #[test]
    fn test_skip_all_preserves_pending_restore() {
        let mut scheduler = AutoTaskScheduler::new();

        let mut tasks = vec![AutoTaskItem {
            id: "task_skip".into(),
            start: "10:00".into(),
            end: "11:00".into(),
            preset_id: "preset_s".into(),
            snapshot: None,
        }];

        let mut preset_map = HashMap::new();
        preset_map.insert("preset_s".into(), HashMap::new());

        let mut initial = HashMap::new();
        initial.insert("picture_mode".into(), serde_json::json!(14));
        scheduler.initial_baseline = Some(initial.clone());
        scheduler.session = Some(SessionMetadata {
            source_device: MockPresetExecutor::new(true).identity,
            ..Default::default()
        });

        // 进入任务
        scheduler.poll_and_check_with_executor(
            10 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );
        assert_eq!(scheduler.active_task_id.as_deref(), Some("task_skip"));

        // 准备一个全 skipped 的 Mock (无 failed，但 applied 为空)
        let mut mock_skip = MockPresetExecutor::new(true);
        mock_skip.apply_result = ApplyResult {
            applied: vec![],
            failed: vec![],
            skipped: vec![("背光".into(), "预设未记录背光".into())],
        };

        // 11:30 时段结束退出，使用 mock_skip 执行还原
        scheduler.poll_and_check_with_executor(
            11 * 60 + 30,
            &mut tasks,
            &preset_map,
            None,
            true,
            &mock_skip,
        );

        // 关键纪律：不要把全部 skip 当成成功来清恢复记录！
        assert_eq!(scheduler.active_task_id, None);
        assert!(
            scheduler.pending_restore_record.is_some(),
            "全 skip 绝不可视为成功还原，必须保留暂挂基线"
        );
        assert!(scheduler.pending_restore.is_some(), "兼容字段必须保留");
        assert_eq!(tasks[0].snapshot, Some(initial));
    }

    #[test]
    fn test_task_deletion_preserves_orphaned_restore() {
        let mut scheduler = AutoTaskScheduler::new();
        let mut tasks = vec![AutoTaskItem {
            id: "task_del".into(),
            start: "08:00".into(),
            end: "12:00".into(),
            preset_id: "preset_d".into(),
            snapshot: None,
        }];

        let mut preset_map = HashMap::new();
        preset_map.insert("preset_d".into(), HashMap::new());

        let mut initial = HashMap::new();
        initial.insert("picture_mode".into(), serde_json::json!(14));
        scheduler.initial_baseline = Some(initial.clone());
        scheduler.session = Some(SessionMetadata {
            source_device: MockPresetExecutor::new(true).identity,
            ..Default::default()
        });

        // 进入活跃任务
        scheduler.poll_and_check_with_executor(
            9 * 60,
            &mut tasks,
            &preset_map,
            None,
            true,
            &MockPresetExecutor::new(true),
        );
        assert_eq!(scheduler.active_task_id.as_deref(), Some("task_del"));

        // 用户在 UI 点击删除该任务
        scheduler.on_task_deleted("task_del", tasks[0].snapshot.clone());

        // 关键质量门：活跃任务退出，但基线转入 orphaned_restores 队列，绝不丢失！
        assert_eq!(scheduler.active_task_id, None);
        assert_eq!(scheduler.orphaned_restores.len(), 1);
        assert_eq!(scheduler.orphaned_restores[0].task_id, "task_del");
        assert_eq!(scheduler.orphaned_restores[0].snapshot, initial);
        assert!(scheduler.orphaned_restores[0].requires_user_confirmation);
    }

    #[test]
    fn test_cross_device_mismatch_blocks_auto_restore_until_confirmed() {
        let mut scheduler = AutoTaskScheduler::new();
        let mut tasks = vec![AutoTaskItem {
            id: "task_pending".into(),
            start: "08:00".into(),
            end: "10:00".into(),
            preset_id: "p1".into(),
            snapshot: None,
        }];

        let mut snap = HashMap::new();
        snap.insert("picture_mode".into(), serde_json::json!(14));

        // 记录来源设备为设备 A
        let dev_a = DeviceIdentity::new("192.168.1.100:5555", Some("SERIAL_AAA".into()));
        let record = PendingRestoreRecord {
            task_id: "task_pending".into(),
            snapshot: snap.clone(),
            source_device: Some(dev_a),
            created_at: chrono_like_now(),
            requires_user_confirmation: false,
            user_confirmed: false,
            confirmed_device: None,
            reason: "待还原".into(),
        };
        scheduler.pending_restore_record = Some(record);
        scheduler.pending_restore = Some(("task_pending".into(), snap));

        // 当前连接的是另一台设备 B
        let mut mock_dev_b = MockPresetExecutor::new(true);
        mock_dev_b.identity = Some(DeviceIdentity::new(
            "192.168.1.100:5555",
            Some("SERIAL_BBB".into()),
        ));

        // 尝试轮询自动恢复
        let preset_map = HashMap::new();
        scheduler.poll_and_check_with_executor(
            11 * 60,
            &mut tasks,
            &preset_map,
            None,
            true,
            &mock_dev_b,
        );

        // 关键质量门：设备不匹配，必须拦截自动下发，要求用户确认！
        let rec = scheduler.pending_restore_record.as_ref().unwrap();
        assert!(rec.requires_user_confirmation, "必须要求用户确认");
        assert!(
            mock_dev_b.last_applied_values.lock().is_none(),
            "不得向错误设备下发基线"
        );

        // 用户确认后再次执行
        scheduler.confirm_restore_for_task("task_pending", mock_dev_b.device_identity());
        assert!(
            !scheduler
                .pending_restore_record
                .as_ref()
                .unwrap()
                .requires_user_confirmation
        );

        scheduler.poll_and_check_with_executor(
            11 * 60,
            &mut tasks,
            &preset_map,
            None,
            true,
            &mock_dev_b,
        );
        // 确认后已成功还原并清空
        assert!(scheduler.pending_restore_record.is_none());
        assert!(mock_dev_b.last_applied_values.lock().is_some());
    }

    #[test]
    fn test_backoff_suppresses_repeated_spam() {
        let mut scheduler = AutoTaskScheduler::new();
        let mut tasks = vec![AutoTaskItem {
            id: "task_frequent".into(),
            start: "08:00".into(),
            end: "10:00".into(),
            preset_id: "preset_p".into(),
            snapshot: None,
        }];

        let mut preset_map = HashMap::new();
        let mut p = HashMap::new();
        p.insert("picture_mode".into(), serde_json::json!(10));
        preset_map.insert("preset_p".into(), p);

        let mut mock = MockPresetExecutor::new(true);
        mock.apply_result = ApplyResult {
            applied: vec![],
            failed: vec![("画面模式".into(), "通信超时".into())],
            skipped: vec![],
        };

        // 第一次尝试下发失败
        scheduler.poll_and_check_with_executor(
            8 * 60 + 10,
            &mut tasks,
            &preset_map,
            None,
            true,
            &mock,
        );
        assert!(scheduler.is_in_backoff("activate_task_frequent"));

        // 下一个 tick 轮询，应被退避拦截，不再重复调用 apply
        *mock.last_applied_values.lock() = None;
        scheduler.poll_and_check_with_executor(
            8 * 60 + 11,
            &mut tasks,
            &preset_map,
            None,
            true,
            &mock,
        );
        assert!(
            mock.last_applied_values.lock().is_some(),
            "部分写入后应优先尝试基线还原"
        );
        assert!(scheduler
            .blocked_task_ids
            .contains(&"task_frequent".to_string()));
        *mock.last_applied_values.lock() = None;
        scheduler.poll_and_check_with_executor(
            8 * 60 + 12,
            &mut tasks,
            &preset_map,
            None,
            true,
            &mock,
        );
        assert!(
            mock.last_applied_values.lock().is_none(),
            "还原失败退避期间不得重复尝试"
        );
    }

    #[test]
    fn test_scheduler_restart_recovery_roundtrip_execution() {
        let mut scheduler1 = AutoTaskScheduler::new();
        let mut tasks = vec![AutoTaskItem {
            id: "task_reboot".into(),
            start: "08:00".into(),
            end: "10:00".into(),
            preset_id: "preset_r".into(),
            snapshot: None,
        }];

        let mut initial = HashMap::new();
        initial.insert("picture_mode".into(), serde_json::json!(14));
        scheduler1.initial_baseline = Some(initial.clone());

        let dev_id = DeviceIdentity::new("192.168.1.100:5555", Some("MOCK_SERIAL_123".into()));
        scheduler1.session = Some(SessionMetadata {
            source_device: Some(dev_id.clone()),
            captured_at: chrono_like_now(),
            app_version: Some("0.1.0".into()),
        });
        scheduler1.active_task_id = Some("task_reboot".into());

        // 模拟 10:30 时段结束但设备断连，产生暂挂恢复记录
        scheduler1.poll_and_check_with_executor(
            10 * 60 + 30,
            &mut tasks,
            &HashMap::new(),
            None,
            false,
            &MockPresetExecutor::new(false),
        );
        assert!(scheduler1.pending_restore_record.is_some());

        // 模拟应用退出，导出持久化至 config.extra["scheduler_recovery"]
        let exported_json = scheduler1.export_recovery_to_value();

        // 模拟应用重新启动，创建全新的 scheduler2 并导入恢复
        let mut scheduler2 = AutoTaskScheduler::new();
        assert!(scheduler2.pending_restore_record.is_none());
        scheduler2
            .import_recovery_from_value(&exported_json)
            .unwrap();

        // 校验恢复数据完整还原
        assert_eq!(
            scheduler2.pending_restore_record.as_ref().unwrap().task_id,
            "task_reboot"
        );
        assert_eq!(
            scheduler2.pending_restore_record.as_ref().unwrap().snapshot,
            initial
        );
        assert_eq!(
            scheduler2.pending_restore.as_ref().unwrap().0,
            "task_reboot"
        );

        // 显示器在重启后连上，验证 scheduler2 能够顺利补执行基线恢复
        let mock_online = MockPresetExecutor::new(true);
        scheduler2.poll_and_check_with_executor(
            11 * 60,
            &mut tasks,
            &HashMap::new(),
            None,
            true,
            &mock_online,
        );

        // 恢复成功后安全清空
        assert!(scheduler2.pending_restore_record.is_none());
        assert!(scheduler2.pending_restore.is_none());
        assert_eq!(tasks[0].snapshot, None);
        assert_eq!(
            mock_online.last_applied_values.lock().as_ref(),
            Some(&initial)
        );
    }
    #[test]
    fn failed_confirmed_restore_cannot_follow_device_change() {
        let mut state = AutoTaskScheduler::new();
        let snapshot = HashMap::from([("picture_mode".into(), serde_json::json!(14))]);
        state.pending_restore = Some(("legacy".into(), snapshot.clone()));
        state.pending_restore_record = Some(PendingRestoreRecord::from_legacy_snapshot(
            "legacy", snapshot, "legacy",
        ));
        let mut first = MockPresetExecutor::new(true);
        state.confirm_restore_for_task("legacy", first.device_identity());
        first.apply_result = ApplyResult {
            applied: vec![],
            failed: vec![("mode".into(), "offline".into())],
            skipped: vec![],
        };
        state.poll_and_check_with_executor(0, &mut [], &HashMap::new(), None, true, &first);
        assert!(first.last_applied_values.lock().is_some());
        assert!(
            !state
                .pending_restore_record
                .as_ref()
                .unwrap()
                .user_confirmed
        );
        let mut next = MockPresetExecutor::new(true);
        next.identity = Some(DeviceIdentity::new("other", Some("OTHER_SERIAL".into())));
        for _ in 0..35 {
            state.poll_and_check_with_executor(0, &mut [], &HashMap::new(), None, true, &next);
        }
        assert!(next.last_applied_values.lock().is_none());
        assert!(
            state
                .pending_restore_record
                .as_ref()
                .unwrap()
                .requires_user_confirmation
        );
    }
    #[test]
    fn orphaned_recovery_blocks_new_task_writes() {
        let mut state = AutoTaskScheduler::new();
        state
            .orphaned_restores
            .push(PendingRestoreRecord::from_legacy_snapshot(
                "old",
                HashMap::from([("picture_mode".into(), serde_json::json!(14))]),
                "legacy",
            ));
        let mut tasks = vec![AutoTaskItem {
            id: "new".into(),
            start: "08:00".into(),
            end: "10:00".into(),
            preset_id: "p".into(),
            snapshot: None,
        }];
        let presets = HashMap::from([(
            "p".into(),
            HashMap::from([("picture_mode".into(), serde_json::json!(10))]),
        )]);
        let device = MockPresetExecutor::new(true);
        state.poll_and_check_with_executor(9 * 60, &mut tasks, &presets, None, true, &device);
        assert!(device.last_applied_values.lock().is_none());
        assert!(state.initial_baseline.is_none());
        assert_eq!(state.orphaned_restores.len(), 1);
    }
}
