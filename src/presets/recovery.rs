use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 设备标识信息（至少包含 ADB endpoint 和硬件真实序列号）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub adb_endpoint: String,
    pub hardware_serial: Option<String>,
}

impl DeviceIdentity {
    pub fn new(adb_endpoint: impl Into<String>, hardware_serial: Option<String>) -> Self {
        Self {
            adb_endpoint: adb_endpoint.into(),
            hardware_serial: hardware_serial.and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() || trimmed == "unknown" {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }),
        }
    }

    /// 校验是否可以安全且确定地判定为同台硬件设备
    ///
    /// 纪律要求：
    /// 1. 双方必须都有非空的 hardware_serial 且完全相等；
    /// 2. 只要任何一方 hardware_serial 不可用 (None)，绝不可假确认同设备，必须要求用户确认！
    pub fn is_definitely_same_device(&self, other: &DeviceIdentity) -> bool {
        match (&self.hardware_serial, &other.hardware_serial) {
            (Some(s1), Some(s2)) if !s1.is_empty() && !s2.is_empty() => s1 == s2,
            _ => false,
        }
    }
}

/// 调度器会话级元数据
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionMetadata {
    pub source_device: Option<DeviceIdentity>,
    pub captured_at: String,
    #[serde(default)]
    pub app_version: Option<String>,
}

/// 暂挂还原基线记录（带设备身份与安全拦截标志）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingRestoreRecord {
    pub task_id: String,
    pub snapshot: HashMap<String, serde_json::Value>,
    pub source_device: Option<DeviceIdentity>,
    pub created_at: String,
    #[serde(default)]
    pub requires_user_confirmation: bool,
    #[serde(default)]
    pub user_confirmed: bool,
    #[serde(default)]
    pub confirmed_device: Option<DeviceIdentity>,
    #[serde(default)]
    pub reason: String,
}

impl PendingRestoreRecord {
    /// 从旧版 AutoTaskItem.snapshot 迁移创建（未知来源，必须要求用户确认）
    pub fn from_legacy_snapshot(
        task_id: impl Into<String>,
        snapshot: HashMap<String, serde_json::Value>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            snapshot,
            source_device: None,
            created_at: chrono_like_now(),
            requires_user_confirmation: true,
            user_confirmed: false,
            confirmed_device: None,
            reason: reason.into(),
        }
    }
}

/// 调度器持久化恢复状态 (序列化存储至 config.extra["scheduler_recovery"])
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SchedulerRecovery {
    #[serde(default)]
    pub blocked_task_ids: Vec<String>,
    #[serde(default)]
    pub session: Option<SessionMetadata>,
    #[serde(default)]
    pub initial_baseline: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub active_task_id: Option<String>,
    #[serde(default)]
    pub waiting_task_id: Option<String>,
    #[serde(default)]
    pub pending_restore: Option<PendingRestoreRecord>,
    #[serde(default)]
    pub orphaned_restores: Vec<PendingRestoreRecord>,
}

impl SchedulerRecovery {
    /// 导出为 JSON Value 供主 UI 写入 config.extra["scheduler_recovery"]
    pub fn to_extra_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// 从 config.extra["scheduler_recovery"] 导入
    pub fn from_extra_value(val: &serde_json::Value) -> Result<Self, String> {
        if val.is_null() {
            return Ok(Self::default());
        }
        serde_json::from_value(val.clone()).map_err(|e| format!("解析调度器恢复数据失败: {}", e))
    }
}

/// 简易时间戳生成器（避免强依赖外部时间库，格式对齐原 Python）
pub fn chrono_like_now() -> String {
    // 使用 std::time 返回粗略秒级时间戳或由调用方注入
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("timestamp:{}", now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_identity_matching() {
        let dev1 = DeviceIdentity::new("192.168.1.50:5555", Some("SN_ABC_123".into()));
        let dev2 = DeviceIdentity::new("192.168.1.50:5555", Some("SN_ABC_123".into()));
        assert!(dev1.is_definitely_same_device(&dev2));

        // 相同 IP 但序列号不同（例如 DHCP 重新分配给不同显示器）：绝不能假确认！
        let dev3 = DeviceIdentity::new("192.168.1.50:5555", Some("SN_DIFF_999".into()));
        assert!(!dev1.is_definitely_same_device(&dev3));

        // 序列号不可用时 (None)：质量门规定「serial不可用时不能假确认同设备」
        let dev_no_sn1 = DeviceIdentity::new("192.168.1.50:5555", None);
        let dev_no_sn2 = DeviceIdentity::new("192.168.1.50:5555", None);
        assert!(!dev_no_sn1.is_definitely_same_device(&dev_no_sn2));
        assert!(!dev1.is_definitely_same_device(&dev_no_sn1));
    }

    #[test]
    fn test_legacy_snapshot_requires_user_confirmation() {
        let mut snap = HashMap::new();
        snap.insert("picture_mode".into(), serde_json::json!(14));

        let record = PendingRestoreRecord::from_legacy_snapshot("task_old", snap, "旧版本导入");
        assert_eq!(record.task_id, "task_old");
        assert!(record.requires_user_confirmation);
        assert_eq!(record.source_device, None);
    }

    #[test]
    fn test_scheduler_recovery_serialization_roundtrip() {
        let mut recovery = SchedulerRecovery::default();
        recovery.active_task_id = Some("task_1".into());
        let mut baseline = HashMap::new();
        baseline.insert("picture_mode".into(), serde_json::json!(10));
        recovery.initial_baseline = Some(baseline);

        let json_val = recovery.to_extra_value();
        let loaded = SchedulerRecovery::from_extra_value(&json_val).unwrap();
        assert_eq!(recovery, loaded);
    }
}
