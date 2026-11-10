use crate::config::AppConfig;
use crate::message::AppCommand;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 手动画面修改到回读更新预设的去抖延时（毫秒）
pub const PRESET_SYNC_DEBOUNCE_MILLIS: u64 = 1200;

/// 挂起的预设自动同步请求
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSync {
    pub preset_id: String,
    pub generation: u64,
    pub deadline: Instant,
}

/// 预设自动同步去抖协调器
#[derive(Debug, Default)]
pub struct PresetSyncDebouncer {
    pending: Option<PendingSync>,
}

impl PresetSyncDebouncer {
    pub fn new() -> Self {
        Self { pending: None }
    }

    /// 记录一次画面参数修改。
    /// 若无活动预设（None），则不 arm 且清理旧请求；
    /// 若已有挂起，无论旧目标为何均重置为当前目标及新 deadline（now + 1200ms）。
    pub fn note(&mut self, active_preset_id: Option<&str>, generation: u64, now: Instant) {
        let Some(preset_id) = active_preset_id else {
            self.pending = None;
            return;
        };
        self.pending = Some(PendingSync {
            preset_id: preset_id.to_string(),
            generation,
            deadline: now + Duration::from_millis(PRESET_SYNC_DEBOUNCE_MILLIS),
        });
    }

    /// 清空挂起的自动同步请求
    pub fn clear(&mut self) {
        self.pending = None;
    }

    /// 是否有挂起的请求
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// 获取当前挂起项的只读引用
    pub fn pending(&self) -> Option<&PendingSync> {
        self.pending.as_ref()
    }

    /// 检查并取出已到期的同步目标预设 ID。
    /// 校验规则：
    /// 1. 当前代数与挂起代数必须完全一致；
    /// 2. 当前活动预设 ID 必须与挂起目标完全一致；
    /// 3. 当前时刻已达到或超过 deadline。
    /// 若代数或活动预设改变，挂起请求被立即丢弃并清空；若未到期则保持挂起并返回 None。
    pub fn take_due(
        &mut self,
        current_generation: u64,
        current_active_id: Option<&str>,
        now: Instant,
    ) -> Option<String> {
        let pending = self.pending.as_ref()?;

        // 会话代数变化，丢弃旧 pending
        if pending.generation != current_generation {
            self.pending = None;
            return None;
        }

        // 当前活动预设与挂起的目标不一致（切换或取消激活），丢弃旧 pending
        if Some(pending.preset_id.as_str()) != current_active_id {
            self.pending = None;
            return None;
        }

        // 未到期
        if now < pending.deadline {
            return None;
        }

        // 到期取出
        self.pending.take().map(|p| p.preset_id)
    }
}

/// 判断当前命令是否为手动画面修改（影响预设的硬件参数）
pub fn is_manual_picture_change(cmd: &AppCommand) -> bool {
    match cmd {
        AppCommand::SetBacklight { .. }
        | AppCommand::SetPictureMode { .. }
        | AppCommand::SetPictureSlider { .. }
        | AppCommand::SetLightSensor { .. }
        | AppCommand::SetLocalDimming { .. } => true,
        AppCommand::ToolAction { action, .. } => matches!(
            action,
            crate::message::actions::ToolAction::Advanced { .. }
                | crate::message::actions::ToolAction::Gains { .. }
                | crate::message::actions::ToolAction::ResetPicture
        ),
        AppCommand::HotkeyCommit { param, .. } | AppCommand::TrayValueCommit { param, .. } => {
            !matches!(param, crate::hotkey::AdjustParam::AtmosphereIllumination)
        }
        AppCommand::SetGameFreeSync { .. } => true,
        _ => false,
    }
}

/// 将本次成功读到的字段合并到目标预设，保留未读到的可选参数。
/// 保证：
/// 1. 空 values 绝不保存；
/// 2. 闭包内二次校验当前 active_preset_id 是否仍为目标预设；
/// 3. 仅在目标预设存在时原地更新其 values，绝不覆写整个 presets 列表副本；
/// 4. 绝不修改基线快照（preset_snapshot）及其它配置项。
pub fn apply_preset_values_to_config(
    cfg: &mut AppConfig,
    target_preset_id: &str,
    new_values: HashMap<String, serde_json::Value>,
) -> bool {
    if new_values.is_empty() {
        return false;
    }
    if cfg.active_preset_id.as_deref() != Some(target_preset_id) {
        return false;
    }
    if let Some(target) = cfg.presets.iter_mut().find(|p| p.id == target_preset_id) {
        target.values.extend(new_values);
        true
    } else {
        false
    }
}

/// Field-level edits are queued with device work, so stale UI snapshots cannot
/// replace a newer automatically captured preset or resurrect a deleted entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PresetConfigMutation {
    AddAndActivate { preset: crate::config::PresetItem, generation: u64 },
    UpdateValues { id: String, values: HashMap<String, serde_json::Value>, generation: u64 },
    Rename { id: String, name: String },
    Delete { id: String },
}
impl PresetConfigMutation {
    pub fn generation(&self) -> Option<u64> {
        match self {
            Self::AddAndActivate { generation, .. } | Self::UpdateValues { generation, .. } => Some(*generation),
            _ => None,
        }
    }
    pub fn apply(&self, cfg: &mut AppConfig) -> Result<(), String> {
        match self {
            Self::AddAndActivate { preset, .. } => {
                if cfg.presets.iter().any(|item| item.id == preset.id) {
                    return Err("预设已存在，请刷新后重试".into());
                }
                let mut preset = preset.clone();
                preset.name = crate::presets::unique_name(&cfg.presets, &preset.name);
                cfg.active_preset_id = Some(preset.id.clone());
                cfg.presets.push(preset);
            }
            Self::UpdateValues { id, values, .. } => {
                if values.is_empty() { return Err("未读取到有效预设参数".into()); }
                let target = cfg.presets.iter_mut().find(|item| item.id == *id).ok_or("预设已删除")?;
                target.values.extend(values.clone());
            }
            Self::Rename { id, name } => {
                let others = cfg.presets.iter().filter(|item| item.id != *id).cloned().collect::<Vec<_>>();
                let name = crate::presets::unique_name(&others, name);
                let target = cfg.presets.iter_mut().find(|item| item.id == *id).ok_or("预设已删除")?;
                target.name = name;
            }
            Self::Delete { id } => {
                cfg.presets.retain(|item| item.id != *id);
                if cfg.active_preset_id.as_deref() == Some(id) { cfg.active_preset_id = None; }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, PresetItem};
    use serde_json::json;

    #[test]
    fn autosave_preserves_unread_optional_values() {
        let mut cfg = AppConfig {
            active_preset_id: Some("p".into()),
            presets: vec![PresetItem { id: "p".into(), name: "P".into(), values: HashMap::from([
                ("picture_backlight".into(), json!(30)),
                ("optional_rgb".into(), json!(1000)),
            ]) }],
            ..Default::default()
        };
        assert!(apply_preset_values_to_config(&mut cfg, "p", HashMap::from([
            ("picture_backlight".into(), json!(40)),
        ])));
        assert_eq!(cfg.presets[0].values.get("picture_backlight"), Some(&json!(40)));
        assert_eq!(cfg.presets[0].values.get("optional_rgb"), Some(&json!(1000)));
    }

    #[test]
    fn rename_from_stale_ui_preserves_autosaved_values_and_baseline() {
        let mut cfg = AppConfig {
            active_preset_id: Some("p".into()),
            preset_snapshot: Some(HashMap::from([("baseline".into(), json!(10))])),
            presets: vec![PresetItem { id: "p".into(), name: "P".into(), values: HashMap::from([
                ("picture_backlight".into(), json!(30)),
            ]) }],
            ..Default::default()
        };
        let rename = PresetConfigMutation::Rename { id: "p".into(), name: "改名".into() };
        apply_preset_values_to_config(&mut cfg, "p", HashMap::from([("picture_backlight".into(), json!(65))]));
        rename.apply(&mut cfg).unwrap();
        assert_eq!(cfg.presets[0].values["picture_backlight"], json!(65));
        assert_eq!(cfg.presets[0].name, "改名");
        assert_eq!(cfg.preset_snapshot.as_ref().unwrap()["baseline"], json!(10));
        PresetConfigMutation::Delete { id: "p".into() }.apply(&mut cfg).unwrap();
        assert!(rename.apply(&mut cfg).is_err());
        assert!(cfg.presets.is_empty());
        assert!(cfg.active_preset_id.is_none());
    }

    #[test]
    fn test_debounce_coalescing() {
        let mut debouncer = PresetSyncDebouncer::new();
        let t0 = Instant::now();

        // 第一次触发
        debouncer.note(Some("preset_a"), 1, t0);
        assert!(debouncer.is_pending());
        assert_eq!(
            debouncer.pending().unwrap().deadline,
            t0 + Duration::from_millis(PRESET_SYNC_DEBOUNCE_MILLIS)
        );

        // 500ms 后再次触发，重置 deadline
        let t1 = t0 + Duration::from_millis(500);
        debouncer.note(Some("preset_a"), 1, t1);
        let expected_deadline = t1 + Duration::from_millis(PRESET_SYNC_DEBOUNCE_MILLIS);
        assert_eq!(debouncer.pending().unwrap().deadline, expected_deadline);

        // 原 1200ms (t0 + 1200) 处检查，由于重置应未到期
        assert_eq!(
            debouncer.take_due(1, Some("preset_a"), t0 + Duration::from_millis(1200)),
            None
        );
        assert!(debouncer.is_pending());

        // 新 deadline (t1 + 1200 = t0 + 1700) 处检查，成功取出
        assert_eq!(
            debouncer.take_due(1, Some("preset_a"), expected_deadline),
            Some("preset_a".to_string())
        );
        // 取出后不再 pending
        assert!(!debouncer.is_pending());
    }

    #[test]
    fn test_no_active_no_record() {
        let mut debouncer = PresetSyncDebouncer::new();
        let t0 = Instant::now();

        // active 为 None，不产生 pending
        debouncer.note(None, 1, t0);
        assert!(!debouncer.is_pending());
        assert_eq!(
            debouncer.take_due(1, None, t0 + Duration::from_millis(2000)),
            None
        );

        // 若原先有 pending，传入 None 会清空
        debouncer.note(Some("preset_a"), 1, t0);
        assert!(debouncer.is_pending());
        debouncer.note(None, 1, t0 + Duration::from_millis(100));
        assert!(!debouncer.is_pending());
    }

    #[test]
    fn test_generation_change_discarded() {
        let mut debouncer = PresetSyncDebouncer::new();
        let t0 = Instant::now();
        debouncer.note(Some("preset_a"), 1, t0);

        // 代数变为 2，即使到期也丢弃
        let t_due = t0 + Duration::from_millis(1500);
        assert_eq!(debouncer.take_due(2, Some("preset_a"), t_due), None);
        assert!(!debouncer.is_pending());
    }

    #[test]
    fn test_target_change_discarded() {
        let mut debouncer = PresetSyncDebouncer::new();
        let t0 = Instant::now();
        debouncer.note(Some("preset_a"), 1, t0);

        let t_due = t0 + Duration::from_millis(1500);
        // 活动预设变为 preset_b，丢弃旧 pending
        assert_eq!(debouncer.take_due(1, Some("preset_b"), t_due), None);
        assert!(!debouncer.is_pending());

        // 活动预设变为 None 同样丢弃
        debouncer.note(Some("preset_a"), 1, t0);
        assert_eq!(debouncer.take_due(1, None, t_due), None);
        assert!(!debouncer.is_pending());
    }

    #[test]
    fn test_apply_preset_values_preserves_other_configs() {
        let mut baseline_snapshot = HashMap::new();
        baseline_snapshot.insert("contrast".to_string(), json!(50));

        let mut p1_values = HashMap::new();
        p1_values.insert("brightness".to_string(), json!(30));

        let mut p2_values = HashMap::new();
        p2_values.insert("brightness".to_string(), json!(80));

        let mut cfg = AppConfig {
            active_preset_id: Some("p1".to_string()),
            preset_snapshot: Some(baseline_snapshot.clone()),
            presets: vec![
                PresetItem {
                    id: "p1".to_string(),
                    name: "标准".to_string(),
                    values: p1_values,
                },
                PresetItem {
                    id: "p2".to_string(),
                    name: "电影".to_string(),
                    values: p2_values.clone(),
                },
            ],
            saved_ip: "192.168.1.100".to_string(),
            ..Default::default()
        };
        cfg.extra.insert("custom_key".to_string(), json!("custom_val"));

        // 1. 空 values 拒绝更新
        assert!(!apply_preset_values_to_config(&mut cfg, "p1", HashMap::new()));

        // 2. target 与 active_preset_id 不一致拒绝更新
        let mut new_values = HashMap::new();
        new_values.insert("brightness".to_string(), json!(45));
        assert!(!apply_preset_values_to_config(&mut cfg, "p2", new_values.clone()));

        // 3. 正常更新目标预设
        assert!(apply_preset_values_to_config(&mut cfg, "p1", new_values.clone()));

        // 校验目标 p1 被更新
        let p1 = cfg.presets.iter().find(|p| p.id == "p1").unwrap();
        assert_eq!(p1.values.get("brightness"), Some(&json!(45)));

        // 校验 p2 保持未变
        let p2 = cfg.presets.iter().find(|p| p.id == "p2").unwrap();
        assert_eq!(p2.values, p2_values);

        // 校验基线快照未被修改
        assert_eq!(cfg.preset_snapshot, Some(baseline_snapshot));

        // 校验其它配置字段未被修改
        assert_eq!(cfg.saved_ip, "192.168.1.100");
        assert_eq!(cfg.extra.get("custom_key"), Some(&json!("custom_val")));
    }

    #[test]
    fn test_is_manual_picture_change() {
        use crate::hotkey::AdjustParam;
        use crate::message::actions::ToolAction;

        assert!(is_manual_picture_change(&AppCommand::SetBacklight {
            value: 50,
            generation: 1
        }));
        assert!(is_manual_picture_change(&AppCommand::SetPictureMode {
            mode: 1,
            generation: 1,
            revision: 1
        }));
        assert!(is_manual_picture_change(&AppCommand::SetPictureSlider {
            param: crate::device::picture::PictureSliderParam::Contrast,
            value: 50,
            generation: 1,
            revision: 1
        }));
        assert!(is_manual_picture_change(&AppCommand::SetLightSensor {
            enabled: true,
            generation: 1,
            revision: 1
        }));
        assert!(is_manual_picture_change(&AppCommand::SetLocalDimming {
            value: 1,
            generation: 1,
            revision: 1
        }));
        assert!(is_manual_picture_change(&AppCommand::SetGameFreeSync {
            enabled: true,
            restore_mode: None,
            generation: 1
        }));
        assert!(is_manual_picture_change(&AppCommand::ToolAction {
            action: ToolAction::ResetPicture,
            generation: 1
        }));
        assert!(is_manual_picture_change(&AppCommand::HotkeyCommit {
            param: AdjustParam::Backlight,
            value: 60,
            generation: 1
        }));

        // 排除项
        assert!(!is_manual_picture_change(&AppCommand::HotkeyCommit {
            param: AdjustParam::AtmosphereIllumination,
            value: 1,
            generation: 1
        }));
        assert!(!is_manual_picture_change(&AppCommand::FetchBacklight {
            generation: 1
        }));
        assert!(!is_manual_picture_change(&AppCommand::FetchPictureParameters {
            generation: 1
        }));
        assert!(!is_manual_picture_change(&AppCommand::RefreshMonitors));
        assert!(!is_manual_picture_change(&AppCommand::DisconnectAdb));
    }
}
