use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

/// 软件关闭时的行为选项 (兼容旧版 "exit" 与 "tray", 以及 "quit")
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseBehavior {
    #[default]
    Tray,
    Exit,
}

impl Serialize for CloseBehavior {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            CloseBehavior::Tray => serializer.serialize_str("tray"),
            CloseBehavior::Exit => serializer.serialize_str("exit"),
        }
    }
}

impl<'de> Deserialize<'de> for CloseBehavior {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "exit" | "quit" => Ok(CloseBehavior::Exit),
            "tray" => Ok(CloseBehavior::Tray),
            _ => Ok(CloseBehavior::Tray),
        }
    }
}

/// 预设条目数据模型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PresetItem {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub values: HashMap<String, serde_json::Value>,
}

/// 定时自动任务条目数据模型 (兼容 Python 原版中 snapshot 可能嵌套 values 的结构)
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct AutoTaskItem {
    pub id: String,
    pub start: String,
    pub end: String,
    pub preset_id: String,
    pub snapshot: Option<HashMap<String, serde_json::Value>>,
}

impl<'de> Deserialize<'de> for AutoTaskItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawTask {
            #[serde(default)]
            id: String,
            #[serde(default)]
            start: String,
            #[serde(default)]
            end: String,
            #[serde(default)]
            preset_id: String,
            #[serde(default)]
            snapshot: Option<serde_json::Value>,
        }

        let raw = RawTask::deserialize(deserializer)?;

        // 容错解析 snapshot: 可能是 {"values": {...}, "at": "..."}，也可能是直接的 Map
        let parsed_snapshot = match raw.snapshot {
            Some(serde_json::Value::Object(map)) => {
                if let Some(serde_json::Value::Object(sub_values)) = map.get("values") {
                    let mut res = HashMap::new();
                    for (k, v) in sub_values {
                        res.insert(k.clone(), v.clone());
                    }
                    Some(res)
                } else {
                    let mut res = HashMap::new();
                    for (k, v) in map {
                        res.insert(k, v);
                    }
                    Some(res)
                }
            }
            _ => None,
        };

        Ok(AutoTaskItem {
            id: raw.id,
            start: raw.start,
            end: raw.end,
            preset_id: raw.preset_id,
            snapshot: parsed_snapshot,
        })
    }
}

/// 分区背光（精密控光）在 SDR/HDR 下的状态记忆
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LocalDimmingMemory {
    #[serde(default)]
    pub sdr: Option<i32>,
    #[serde(default)]
    pub hdr: Option<i32>,
}

/// 应用程序全局配置模型（与 Python 原版 config.json 结构兼容，保留未知字段）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub close_behavior: CloseBehavior,
    #[serde(default)]
    pub never_ask_close: bool,
    #[serde(default)]
    pub saved_ip: String,
    #[serde(default)]
    pub scan_force_devices: Vec<String>,
    #[serde(default)]
    pub scan_block_devices: Vec<String>,
    #[serde(default)]
    pub scan_force_subnets: Vec<String>,
    #[serde(default)]
    pub hdr_sdr_local_dimming_enabled: bool,
    #[serde(default)]
    pub hdr_target_display_id: String,
    #[serde(default)]
    pub local_dimming_memory: LocalDimmingMemory,
    #[serde(default = "default_local_dimming_last_value")]
    pub local_dimming_toggle_last_value: i32,
    #[serde(default)]
    pub freesync_mode_memory_enabled: bool,
    #[serde(default)]
    pub freesync_previous_mode: Option<i32>,
    #[serde(default = "default_true")]
    pub crosshair_game_mode_only: bool,
    #[serde(default)]
    pub crosshair_memory: Option<i32>,
    #[serde(default = "default_tray_items")]
    pub tray_items: Vec<String>,
    #[serde(default = "default_true")]
    pub hotkey_countdown_enabled: bool,
    #[serde(default = "default_countdown_seconds")]
    pub hotkey_countdown_seconds: f64,
    #[serde(default)]
    pub presets: Vec<PresetItem>,
    #[serde(default)]
    pub auto_tasks: Vec<AutoTaskItem>,
    #[serde(default)]
    pub preset_snapshot: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub active_preset_id: Option<String>,

    /// 保留未知扩展字段，防止旧版或第三方注入的字段在重新序列化时丢失
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn default_true() -> bool {
    true
}

fn default_local_dimming_last_value() -> i32 {
    3
}

fn default_countdown_seconds() -> f64 {
    0.8
}

fn default_tray_items() -> Vec<String> {
    vec![
        "picture_mode".to_string(),
        "local_dimming".to_string(),
        "backlight".to_string(),
    ]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            close_behavior: CloseBehavior::Tray,
            never_ask_close: false,
            saved_ip: String::new(),
            scan_force_devices: Vec::new(),
            scan_block_devices: Vec::new(),
            scan_force_subnets: Vec::new(),
            hdr_sdr_local_dimming_enabled: false,
            hdr_target_display_id: String::new(),
            local_dimming_memory: LocalDimmingMemory::default(),
            local_dimming_toggle_last_value: 3,
            freesync_mode_memory_enabled: false,
            freesync_previous_mode: None,
            crosshair_game_mode_only: true,
            crosshair_memory: None,
            tray_items: default_tray_items(),
            hotkey_countdown_enabled: true,
            hotkey_countdown_seconds: 0.8,
            presets: Vec::new(),
            auto_tasks: Vec::new(),
            preset_snapshot: None,
            active_preset_id: None,
            extra: HashMap::new(),
        }
    }
}
