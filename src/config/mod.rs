pub mod model;

pub use model::{AppConfig, AutoTaskItem, CloseBehavior, LocalDimmingMemory, PresetItem};

use parking_lot::{Mutex, RwLock};
use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
static MIGRATION_BLOCKED: AtomicBool = AtomicBool::new(false);
pub fn migration_blocked() -> bool {
    MIGRATION_BLOCKED.load(Ordering::Acquire)
}

static CONFIG_INSTANCE: RwLock<Option<Arc<AppConfig>>> = RwLock::new(None);
static CONFIG_SAVE_LOCK: Mutex<()> = Mutex::new(());
static CONFIG_UPDATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    BackupFailed(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "配置 IO 失败: {}", e),
            ConfigError::Serialize(e) => write!(f, "配置序列化失败: {}", e),
            ConfigError::BackupFailed(msg) => write!(f, "配置文件备份失败: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self {
        ConfigError::Serialize(e)
    }
}

/// 获取用户根数据目录
fn get_user_local_appdata() -> PathBuf {
    env::var("LOCALAPPDATA")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::temp_dir())
}

/// 旧版 Python 配置文件路径 (`%LOCALAPPDATA%/.gpro_controller/config.json`)
pub fn get_legacy_settings_path() -> PathBuf {
    get_user_local_appdata()
        .join(".gpro_controller")
        .join("config.json")
}

/// 独立 Rust 版本专用配置文件路径 (`%LOCALAPPDATA%/.mimonitor_toolbox_ddc/config.json`)
pub fn get_rust_settings_path() -> PathBuf {
    get_user_local_appdata()
        .join(".mimonitor_toolbox_ddc")
        .join("config.json")
}

/// 获取全局配置文件保存路径（支持 MIMONITOR_SETTINGS_PATH 显式覆盖，便于测试与便携模式）
pub fn get_settings_path() -> PathBuf {
    if let Ok(override_path) = env::var("MIMONITOR_SETTINGS_PATH") {
        let path = PathBuf::from(override_path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        return path;
    }
    get_rust_settings_path()
}

/// 获取跨平台应用数据存储目录
pub fn get_app_data_dir() -> PathBuf {
    if let Ok(override_path) = env::var("MIMONITOR_SETTINGS_PATH") {
        if let Some(parent) = PathBuf::from(override_path).parent() {
            return parent.to_path_buf();
        }
    }
    let dir = get_user_local_appdata().join(".mimonitor_toolbox_ddc");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// 安全备份指定配置文件为 `.bak`，绝不修改、删除或污染源文件
pub fn backup_existing_config(path: &Path) -> Result<Option<PathBuf>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }

    let bak_path = path.with_extension("bak");
    fs::copy(path, &bak_path).map_err(|e| {
        ConfigError::BackupFailed(format!("无法备份旧配置到 {}: {}", bak_path.display(), e))
    })?;

    Ok(Some(bak_path))
}

/// 从指定路径解析配置（带脏字段容错与未知字段保留）
pub fn load_settings_from_path(path: &Path) -> AppConfig {
    if !path.exists() {
        return AppConfig::default();
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return AppConfig::default(),
    };

    // 1. 尝试直接反序列化
    if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
        return cfg;
    }

    // 2. 字段级容错反序列化（例如个别字段为脏类型时，保留其余正常字段）
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&content)
    {
        let mut fallback = AppConfig::default();
        let mut extra = std::collections::HashMap::new();

        for (k, v) in map {
            match k.as_str() {
                "close_behavior" => {
                    if let Ok(cb) = serde_json::from_value::<CloseBehavior>(v) {
                        fallback.close_behavior = cb;
                    }
                }
                "never_ask_close" => {
                    if let Some(b) = v.as_bool() {
                        fallback.never_ask_close = b;
                    }
                }
                "saved_ip" => {
                    if let Some(s) = v.as_str() {
                        fallback.saved_ip = s.to_string();
                    }
                }
                "scan_force_devices" => {
                    if let Ok(list) = serde_json::from_value::<Vec<String>>(v) {
                        fallback.scan_force_devices = list;
                    }
                }
                "scan_block_devices" => {
                    if let Ok(list) = serde_json::from_value::<Vec<String>>(v) {
                        fallback.scan_block_devices = list;
                    }
                }
                "scan_force_subnets" => {
                    if let Ok(list) = serde_json::from_value::<Vec<String>>(v) {
                        fallback.scan_force_subnets = list;
                    }
                }
                "hdr_sdr_local_dimming_enabled" => {
                    if let Some(b) = v.as_bool() {
                        fallback.hdr_sdr_local_dimming_enabled = b;
                    }
                }
                "hdr_target_display_id" => {
                    if let Some(s) = v.as_str() {
                        fallback.hdr_target_display_id = s.to_string();
                    }
                }
                "local_dimming_memory" => {
                    if let Ok(mem) = serde_json::from_value::<LocalDimmingMemory>(v) {
                        fallback.local_dimming_memory = mem;
                    }
                }
                "local_dimming_toggle_last_value" => {
                    if let Some(n) = v.as_i64() {
                        fallback.local_dimming_toggle_last_value = n as i32;
                    }
                }
                "freesync_mode_memory_enabled" => {
                    if let Some(b) = v.as_bool() {
                        fallback.freesync_mode_memory_enabled = b;
                    }
                }
                "freesync_previous_mode" => {
                    fallback.freesync_previous_mode = v.as_i64().map(|n| n as i32);
                }
                "crosshair_game_mode_only" => {
                    if let Some(b) = v.as_bool() {
                        fallback.crosshair_game_mode_only = b;
                    }
                }
                "crosshair_memory" => {
                    fallback.crosshair_memory = v.as_i64().map(|n| n as i32);
                }
                "tray_items" => {
                    if let Ok(items) = serde_json::from_value::<Vec<String>>(v) {
                        fallback.tray_items = items;
                    }
                }
                "hotkey_countdown_enabled" => {
                    if let Some(b) = v.as_bool() {
                        fallback.hotkey_countdown_enabled = b;
                    }
                }
                "hotkey_countdown_seconds" => {
                    if let Some(f) = v.as_f64() {
                        fallback.hotkey_countdown_seconds = f;
                    }
                }
                "presets" => {
                    if let Ok(presets) = serde_json::from_value::<Vec<PresetItem>>(v) {
                        fallback.presets = presets;
                    }
                }
                "auto_tasks" => {
                    if let Ok(tasks) = serde_json::from_value::<Vec<AutoTaskItem>>(v) {
                        fallback.auto_tasks = tasks;
                    }
                }
                "preset_snapshot" => {
                    if let Ok(snap) = serde_json::from_value::<
                        Option<std::collections::HashMap<String, serde_json::Value>>,
                    >(v)
                    {
                        fallback.preset_snapshot = snap;
                    }
                }
                "active_preset_id" => {
                    fallback.active_preset_id = v.as_str().map(|s| s.to_string());
                }
                _ => {
                    extra.insert(k, v);
                }
            }
        }
        fallback.extra = extra;
        return fallback;
    }

    AppConfig::default()
}

/// 原子化保存配置到指定路径，并确保失败时清理临时文件
pub fn save_settings_to_path(path: &Path, config: &AppConfig) -> Result<(), ConfigError> {
    let _lock = CONFIG_SAVE_LOCK.lock();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json_bytes = serde_json::to_vec_pretty(config)?;

    // 写入临时文件后 rename，保证原子替换
    let temp_name = format!("config.json.tmp.{}", uuid::Uuid::new_v4().simple());
    let temp_path = path.with_file_name(temp_name);

    let write_res = (|| -> Result<(), std::io::Error> {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(&json_bytes)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(ConfigError::Io(e));
    }

    if let Err(e) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(ConfigError::Io(e));
    }

    Ok(())
}

/// 加载配置（具备旧版本配置自动无损迁移与备份机制）
pub fn load_settings() -> Arc<AppConfig> {
    {
        let reader = CONFIG_INSTANCE.read();
        if let Some(cfg) = reader.as_ref() {
            return cfg.clone();
        }
    }

    // 若显式指定了环境变量，直接使用
    if env::var("MIMONITOR_SETTINGS_PATH").is_ok() {
        let path = get_settings_path();
        let cfg = Arc::new(load_settings_from_path(&path));
        let mut writer = CONFIG_INSTANCE.write();
        *writer = Some(cfg.clone());
        return cfg;
    }

    let rust_path = get_rust_settings_path();
    let legacy_path = get_legacy_settings_path();

    let cfg = if rust_path.exists() {
        load_settings_from_path(&rust_path)
    } else if legacy_path.exists() {
        crate::logger::log_info(&format!(
            "检测到旧版配置: {}，正在创建备份并迁移到独立 Rust 配置...",
            legacy_path.display()
        ));
        // 在旧配置旁创建备份，绝不改坏旧文件
        if let Err(error) = backup_existing_config(&legacy_path) {
            crate::logger::log_info(&format!("迁移已停止：{error}；原配置保持原样"));
            MIGRATION_BLOCKED.store(true, Ordering::Release);
            let cfg = Arc::new(load_settings_from_path(&legacy_path));
            *CONFIG_INSTANCE.write() = Some(cfg.clone());
            return cfg;
        }
        let loaded = load_settings_from_path(&legacy_path);
        // 持久化到独立 Rust 路径
        if let Err(e) = save_settings_to_path(&rust_path, &loaded) {
            crate::logger::log_info(&format!("写入独立 Rust 配置失败: {}", e));
        } else {
            crate::logger::log_info(&format!(
                "旧配置已安全无损迁移到独立路径: {}",
                rust_path.display()
            ));
        }
        loaded
    } else {
        AppConfig::default()
    };

    let arc = Arc::new(cfg);
    let mut writer = CONFIG_INSTANCE.write();
    *writer = Some(arc.clone());
    arc
}

/// 保存并更新配置
pub fn save_settings(config: &AppConfig) -> Result<(), ConfigError> {
    if migration_blocked() {
        return Err(ConfigError::BackupFailed(
            "旧配置备份未成功，当前为只读模式；修复备份目录权限后重启".into(),
        ));
    }
    let path = get_settings_path();
    save_settings_to_path(&path, config)?;
    let mut writer = CONFIG_INSTANCE.write();
    *writer = Some(Arc::new(config.clone()));
    Ok(())
}

/// Serialize read-modify-save operations so independent pages preserve one another's fields.
pub fn update_settings(update: impl FnOnce(&mut AppConfig)) -> Result<Arc<AppConfig>, ConfigError> {
    let _guard = CONFIG_UPDATE_LOCK.lock();
    let mut config = (*load_settings()).clone();
    update(&mut config);
    save_settings(&config)?;
    Ok(Arc::new(config))
}

pub fn patch_settings(
    patch: std::collections::HashMap<String, serde_json::Value>,
) -> Result<Arc<AppConfig>, ConfigError> {
    let _guard = CONFIG_UPDATE_LOCK.lock();
    let mut value = serde_json::to_value(&*load_settings())?;
    let object = value
        .as_object_mut()
        .expect("configuration serializes to an object");
    for (key, value) in patch {
        object.insert(key, value);
    }
    let config: AppConfig = serde_json::from_value(value)?;
    save_settings(&config)?;
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_close_behavior_compatibility() {
        // 兼容旧版 "exit"
        let json_exit = r#"{"close_behavior": "exit"}"#;
        let cfg1: AppConfig = serde_json::from_str(json_exit).unwrap();
        assert_eq!(cfg1.close_behavior, CloseBehavior::Exit);

        // 兼容 "quit"
        let json_quit = r#"{"close_behavior": "quit"}"#;
        let cfg2: AppConfig = serde_json::from_str(json_quit).unwrap();
        assert_eq!(cfg2.close_behavior, CloseBehavior::Exit);

        // 兼容 "tray"
        let json_tray = r#"{"close_behavior": "tray"}"#;
        let cfg3: AppConfig = serde_json::from_str(json_tray).unwrap();
        assert_eq!(cfg3.close_behavior, CloseBehavior::Tray);

        // 序列化为 "exit"
        let serialized = serde_json::to_string(&cfg1).unwrap();
        assert!(serialized.contains(r#""close_behavior":"exit""#));
    }

    #[test]
    fn test_corrupted_dirty_field_does_not_break_other_settings() {
        let dir = env::temp_dir().join(format!("test_dirty_cfg_{}", uuid::Uuid::new_v4().simple()));
        let _ = fs::create_dir_all(&dir);
        let cfg_path = dir.join("config.json");

        // saved_ip 损坏为数字，hotkey_countdown_seconds 损坏为字符串，但 presets 和 close_behavior 有效
        let dirty_json = r#"{
            "close_behavior": "exit",
            "saved_ip": 12345,
            "hotkey_countdown_seconds": "invalid_number",
            "presets": [{"id": "p1", "name": "游戏", "values": {}}],
            "custom_unknown_field": "preserved"
        }"#;

        fs::write(&cfg_path, dirty_json).unwrap();
        let loaded = load_settings_from_path(&cfg_path);

        assert_eq!(loaded.close_behavior, CloseBehavior::Exit);
        assert_eq!(loaded.presets.len(), 1);
        assert_eq!(loaded.presets[0].name, "游戏");
        assert_eq!(loaded.saved_ip, ""); // 脏字段安全退回默认值
        assert_eq!(
            loaded
                .extra
                .get("custom_unknown_field")
                .and_then(|v| v.as_str()),
            Some("preserved")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_legacy_migration_leaves_legacy_file_untouched() {
        let test_root =
            env::temp_dir().join(format!("test_migration_{}", uuid::Uuid::new_v4().simple()));
        let legacy_dir = test_root.join(".gpro_controller");
        let rust_dir = test_root.join(".mimonitor_toolbox_ddc");
        let _ = fs::create_dir_all(&legacy_dir);
        let _ = fs::create_dir_all(&rust_dir);

        let legacy_file = legacy_dir.join("config.json");
        let rust_file = rust_dir.join("config.json");

        let original_content = r#"{"saved_ip": "192.168.31.200", "close_behavior": "exit"}"#;
        fs::write(&legacy_file, original_content).unwrap();

        // 备份旧文件
        let bak = backup_existing_config(&legacy_file).unwrap();
        assert!(bak.is_some());
        assert!(legacy_dir.join("config.bak").exists());

        // 读取并持久化到新 Rust 路径
        let loaded = load_settings_from_path(&legacy_file);
        save_settings_to_path(&rust_file, &loaded).unwrap();

        // 确认旧文件内容未被修改
        let read_back_legacy = fs::read_to_string(&legacy_file).unwrap();
        assert_eq!(read_back_legacy, original_content);

        // 确认新文件已成功写入并解析正确
        let read_back_rust = load_settings_from_path(&rust_file);
        assert_eq!(read_back_rust.saved_ip, "192.168.31.200");
        assert_eq!(read_back_rust.close_behavior, CloseBehavior::Exit);

        let _ = fs::remove_dir_all(&test_root);
    }
}

pub mod system;
