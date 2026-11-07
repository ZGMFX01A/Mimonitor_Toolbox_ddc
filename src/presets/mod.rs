pub mod recovery;
pub mod scheduler;

pub use recovery::{
    chrono_like_now, DeviceIdentity, PendingRestoreRecord, SchedulerRecovery, SessionMetadata,
};
pub use scheduler::{
    find_active_task, format_hhmm, is_in_period, parse_hhmm, valid_task, AutoTaskScheduler,
    PresetDeviceExecutor,
};

use crate::adb::{AdbDevice, AdbError};
use crate::config::PresetItem;
use crate::device::game::{query_freesync_state, set_freesync};
use crate::device::jni::{jni_batch_get_or_single, jni_set, refresh_pq};
use crate::device::picture::{
    hdr_tone_mapping_mtk_to_ui, is_hdr_tone_mapping_picture_mode, mtk_to_xiaomi_color_temp,
    set_backlight, set_color_gains, set_color_space, set_color_temp, set_hdr_tone_mapping,
    set_light_sensor, set_local_dimming, set_picture_mode, set_plain_setting,
    CUSTOM_COLOR_TEMP_VALUE,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const BASELINE_PRESET_ID: &str = "__baseline__";

/// 画面预设抓取时覆盖的 Android Global Setting 键 (严格对齐原版)
pub const PICTURE_PRESET_SETTINGS_KEYS: &[&str] = &[
    "picture_mode",
    "tv_picture_light_sensor",
    "picture_backlight",
    "xiaomi_picture_backlight",
    "picture_brightness",
    "picture_contrast",
    "picture_saturation",
    "picture_hue",
    "picture_sharpness",
    "picture_color_temperature",
    "picture_red_gain",
    "picture_green_gain",
    "picture_blue_gain",
    "picture_local_dimming",
    "tv_picture_video_local_dimming",
    "picture_hdr_tone_mapping",
    "settings_display_hdr_color_tone",
    "picture_dynamic_definition",
    "picture_response_time",
    "tv_picture_advanced_video_color_space",
    "tv_picture_video_color_space",
];

/// 画面预设抓取时覆盖的 JNI 键 (代表硬件底层真实生效寄存器)
pub const PICTURE_PRESET_JNI_KEYS: &[&str] = &[
    "g_disp__disp_back_light",
    "g_video__vid_gamut_mapping_mode",
    "g_video__clr_temp",
    "g_video__vid_local_dimming",
    "g_video__light_sensor_switch",
];

/// 预设应用结果汇总 (逐项追踪成功、跳过与失败)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplyResult {
    pub applied: Vec<String>,
    pub failed: Vec<(String, String)>,  // (标签, 错误原因)
    pub skipped: Vec<(String, String)>, // (标签, 跳过原因)
}

impl ApplyResult {
    /// 只有在毫无任何失败项的情况下才判定为整体成功
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }

    pub fn summary(&self) -> String {
        let mut parts = vec![format!("成功 {} 项", self.applied.len())];
        if !self.skipped.is_empty() {
            parts.push(format!("跳过 {} 项", self.skipped.len()));
        }
        if !self.failed.is_empty() {
            let failed_names: Vec<&str> = self.failed.iter().map(|(n, _)| n.as_str()).collect();
            parts.push(format!(
                "失败 {} 项: {}",
                self.failed.len(),
                failed_names.join("、")
            ));
        }
        parts.join("，")
    }
}

/// 验证显示器预设字段在转为硬件 u32 前的合法范围与枚举
pub fn validate_preset_field(key: &str, val: i32) -> Result<(), String> {
    match key {
        "freesync" => {
            if val == 0 || val == 1 {
                Ok(())
            } else {
                Err(format!("FreeSync 取值 {} 非法，必须为 0 或 1", val))
            }
        }
        "tv_picture_light_sensor" => {
            if val == 0 || val == 1 {
                Ok(())
            } else {
                Err(format!("自动光感取值 {} 非法，必须为 0 或 1", val))
            }
        }
        "picture_backlight" => {
            if (1..=100).contains(&val) {
                Ok(())
            } else {
                Err(format!("背光数值 {} 超出有效范围 1..100", val))
            }
        }
        "picture_brightness" => {
            if (0..=100).contains(&val) {
                Ok(())
            } else {
                Err(format!("黑色级别数值 {} 超出有效范围 0..100", val))
            }
        }
        "picture_contrast" => {
            if (0..=100).contains(&val) {
                Ok(())
            } else {
                Err(format!("对比度数值 {} 超出有效范围 0..100", val))
            }
        }
        "picture_saturation" => {
            if (0..=100).contains(&val) {
                Ok(())
            } else {
                Err(format!("饱和度数值 {} 超出有效范围 0..100", val))
            }
        }
        "picture_hue" => {
            if (0..=100).contains(&val) {
                Ok(())
            } else {
                Err(format!("色调数值 {} 超出有效范围 0..100", val))
            }
        }
        "picture_sharpness" => {
            if (0..=100).contains(&val) {
                Ok(())
            } else {
                Err(format!("锐度数值 {} 超出有效范围 0..100", val))
            }
        }
        "picture_red_gain" | "picture_green_gain" | "picture_blue_gain" => {
            if (524..=1524).contains(&val) {
                Ok(())
            } else {
                Err(format!("{} 数值 {} 超出有效范围 524..1524", key, val))
            }
        }
        "picture_local_dimming" => {
            if (0..=3).contains(&val) {
                Ok(())
            } else {
                Err(format!("精密控光数值 {} 超出有效范围 0..3", val))
            }
        }
        "picture_dynamic_definition" => {
            if (0..=3).contains(&val) {
                Ok(())
            } else {
                Err(format!("动态清晰度数值 {} 超出有效范围 0..3", val))
            }
        }
        "picture_response_time" => {
            if (1..=3).contains(&val) {
                Ok(())
            } else {
                Err(format!("灰阶响应时间数值 {} 超出有效范围 1..3", val))
            }
        }
        "picture_color_temperature" => {
            if crate::device::picture::xiaomi_to_mtk_color_temp(val).is_some() {
                Ok(())
            } else {
                Err(format!("色温数值 {} 不在有效枚举中", val))
            }
        }
        "tv_picture_advanced_video_color_space" => {
            if matches!(val, 0 | 3 | 4 | 5 | 6 | 7) {
                Ok(())
            } else {
                Err(format!(
                    "色域数值 {} 不在有效枚举 [0, 3, 4, 5, 6, 7] 中",
                    val
                ))
            }
        }
        _ => Ok(()),
    }
}

pub fn as_int(values: &HashMap<String, serde_json::Value>, key: &str) -> Result<i32, String> {
    match values.get(key) {
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| format!("键 {} 数值溢出", key)),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() || trimmed == "null" || trimmed == "N/A" {
                Err(format!("键 {} 为空或无效", key))
            } else {
                trimmed
                    .parse::<i32>()
                    .map_err(|_| format!("键 {} 无法解析为整数: {}", key, s))
            }
        }
        _ => Err(format!("缺少键 {} 的有效值", key)),
    }
}

/// 查询设备硬件真实序列号 (非 ADB endpoint)
pub fn query_hardware_serial(adb: &AdbDevice) -> Option<String> {
    for prop in &["ro.serialno", "ro.boot.serialno", "noa.hardware.sn"] {
        if let Ok(out) = adb.shell(&format!("getprop {}", prop)) {
            let trimmed = out.trim();
            if !trimmed.is_empty()
                && trimmed != "unknown"
                && !trimmed.to_lowercase().contains("error")
                && !trimmed.to_lowercase().contains("failed")
            {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// 抓取当前显示器硬件的完整画面快照
///
/// 纪律要求：
/// 1. 批量读取 Global Settings；
/// 2. 批量读取 JNI 并将底层寄存器真实状态覆盖 Settings（正确执行 MTK 到小米色温映射）；
/// 3. 读取 FreeSync 状态；
/// 4. 必需项不完整或设备通信异常不得 Ok 假快照，返回 Err；
/// 5. 正确区分可选项不支持（如非 HDR 模式缺少 HDR 色调映射、非自定义色温缺少色增益）与核心必需项失败。
pub fn capture_hardware_snapshot(
    adb: &AdbDevice,
) -> Result<HashMap<String, serde_json::Value>, AdbError> {
    adb.transaction(|| -> Result<HashMap<String, serde_json::Value>, AdbError> {
        let mut map = HashMap::new();

        // 1. 批量读取 Global Settings
        let keys_str = PICTURE_PRESET_SETTINGS_KEYS.join(" ");
        let cmd = format!(
            "for k in {}; do echo $k=$(settings get global $k); done",
            keys_str
        );
        let out = adb.shell(&cmd)?;

        for line in out.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim();
                let raw = v.trim();
                if raw.is_empty() || raw == "null" || raw == "N/A" {
                    continue;
                }
                if let Ok(num) = raw.parse::<i64>() {
                    map.insert(key.to_string(), serde_json::Value::Number(num.into()));
                } else {
                    map.insert(key.to_string(), serde_json::Value::String(raw.to_string()));
                }
            }
        }

        // 2. 批量读取 JNI 真实硬件寄存器状态并覆盖 Settings (严格对齐原 Python apply_picture_jni_overrides)
        // 核心硬件寄存器读取失败必须直接传播 Err，禁止仅退化为 Settings 快照
        let jni_vals = jni_batch_get_or_single(adb, PICTURE_PRESET_JNI_KEYS)?;
        if let Some(missing) = PICTURE_PRESET_JNI_KEYS
            .iter()
            .find(|key| !jni_vals.contains_key(**key))
        {
            return Err(crate::adb::AdbError::CommandFailed(format!(
                "核心硬件参数未读取：{missing}；未保存不完整预设"
            )));
        }

        // 背光：JNI 是真实生效值
        if let Some(&b) = jni_vals.get("g_disp__disp_back_light") {
            if b >= 0 {
                map.insert(
                    "picture_backlight".to_string(),
                    serde_json::Value::Number(b.into()),
                );
                map.insert(
                    "xiaomi_picture_backlight".to_string(),
                    serde_json::Value::Number(b.into()),
                );
            }
        }
        // 色域：MTK 值和 settings 值同域，覆盖主键与旧 OSD 键
        if let Some(&gamut) = jni_vals.get("g_video__vid_gamut_mapping_mode") {
            map.insert(
                "tv_picture_advanced_video_color_space".to_string(),
                serde_json::Value::Number(gamut.into()),
            );
            map.insert(
                "tv_picture_video_color_space".to_string(),
                serde_json::Value::Number(gamut.into()),
            );
        }
        // 色温：MTK 枚举需转换为小米 UI 枚举
        if let Some(&clr) = jni_vals.get("g_video__clr_temp") {
            if let Some(xi) = mtk_to_xiaomi_color_temp(clr) {
                map.insert(
                    "picture_color_temperature".to_string(),
                    serde_json::Value::Number(xi.into()),
                );
            }
        }
        // 精密控光：覆盖官方主键与旧 OSD 键
        if let Some(&dim) = jni_vals.get("g_video__vid_local_dimming") {
            map.insert(
                "picture_local_dimming".to_string(),
                serde_json::Value::Number(dim.into()),
            );
            map.insert(
                "tv_picture_video_local_dimming".to_string(),
                serde_json::Value::Number(dim.into()),
            );
        }
        // 自动光感：以 MTK 状态为准
        if let Some(&sensor) = jni_vals.get("g_video__light_sensor_switch") {
            map.insert(
                "tv_picture_light_sensor".to_string(),
                serde_json::Value::Number(sensor.into()),
            );
        }

        // 3. 读取 FreeSync 状态 (按输入源选键读取)
        // 核心硬件状态读取失败必须直接传播 Err
        let fsync = query_freesync_state(adb)?;
        map.insert(
            "freesync".to_string(),
            serde_json::Value::Number((if fsync { 1 } else { 0 }).into()),
        );

        // 4. 必需项完整性校验：核心必要参数若缺失，绝不可判定为成功快照！
        let core_keys = &[
            "picture_mode",
            "tv_picture_light_sensor",
            "picture_backlight",
            "picture_brightness",
            "picture_contrast",
            "picture_saturation",
            "picture_hue",
            "picture_sharpness",
            "picture_color_temperature",
            "freesync",
        ];

        let missing_core: Vec<&str> = core_keys
            .iter()
            .copied()
            .filter(|&k| !map.contains_key(k))
            .collect();

        if !missing_core.is_empty() {
            return Err(AdbError::CommandFailed(format!(
                "快照读取不完整，缺少核心画面参数: {}",
                missing_core.join("、")
            )));
        }

        Ok(map)
    })
}

/// 设置动态清晰度 (私有硬件 setter，避免改动 device 模块)
fn set_dynamic_definition(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    jni_set(adb, "g_video__vid_insert_black", val as i32, 3)?;
    adb.put_setting("picture_dynamic_definition", &val.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置灰阶响应时间 (私有硬件 setter，避免改动 device 模块)
fn set_response_time(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    jni_set(adb, "g_video__vid_od_response_time", val as i32, 3)?;
    adb.put_setting("picture_response_time", &val.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 按规定顺序将快照参数集合应用到设备
///
/// 纪律要求：
/// 1. 严格遵循写入顺序：FreeSync 最先、画面模式紧随其后；
/// 2. 逐项追踪所有写入步骤与 refresh_pq，绝不允许 `let _=` 掩盖失败；
/// 3. 单项中途失败整项判 failed，绝不在假成功后写入 applied；
/// 4. 画面模式缺失或写入失败时，禁止继续写模式相关参数到旧模式，明确记录 skipped 原因；
/// 5. 整体放置在 adb.transaction 中排他执行；
/// 6. 所有硬件 u32 转换前强验证取值范围与枚举，损坏负数或越界项严禁强转为 u32::MAX。
pub fn apply_preset_values(
    adb: &AdbDevice,
    values: &HashMap<String, serde_json::Value>,
) -> ApplyResult {
    adb.transaction(|| {
        let mut result = ApplyResult::default();

        // 1. FreeSync (必须排在最前写入，开着会把设备锁在游戏模式冲掉后续模式)
        if values.contains_key("freesync") {
            match as_int(values, "freesync") {
                Ok(wanted_fsync) if wanted_fsync == 0 || wanted_fsync == 1 => {
                    let enable = wanted_fsync == 1;
                    match set_freesync(adb, enable, None) {
                        Ok(_) => result.applied.push("FreeSync".to_string()),
                        Err(e) => result.failed.push(("FreeSync".to_string(), e.to_string())),
                    }
                }
                Ok(other) => {
                    result.failed.push((
                        "FreeSync".to_string(),
                        format!("FreeSync 取值 {} 非法，必须为 0 或 1", other),
                    ));
                }
                Err(e) => {
                    result.failed.push(("FreeSync".to_string(), e));
                }
            }
        } else {
            result.skipped.push((
                "FreeSync".to_string(),
                "预设未记录 FreeSync 状态".to_string(),
            ));
        }

        // 2. 画面模式 (第二位写入，因为写模式会重置设备为该模式默认值，其余参数必须在之后写)
        let mut target_mode: Option<i32> = None;
        if values.contains_key("picture_mode") {
            match as_int(values, "picture_mode") {
                Ok(mode) if mode >= 0 => match set_picture_mode(adb, mode) {
                    Ok(_) => {
                        target_mode = Some(mode);
                        result.applied.push("画面模式".to_string());
                    }
                    Err(e) => {
                        result.failed.push(("画面模式".to_string(), e.to_string()));
                    }
                },
                Ok(mode) => {
                    result.failed.push((
                        "画面模式".to_string(),
                        format!("画面模式数值 {} 非法，不能为负数", mode),
                    ));
                }
                Err(e) => {
                    result.failed.push((
                        "画面模式".to_string(),
                        format!("预设缺少有效 picture_mode: {}", e),
                    ));
                }
            }
        } else {
            result
                .failed
                .push(("画面模式".to_string(), "预设缺少 picture_mode".to_string()));
        }

        // 模式写失败不能继续写模式相关参数到当前旧模式；记录明确 skipped 原因
        if target_mode.is_none() {
            let mode_err_reason =
                "画面模式未确定或下发失败，跳过模式相关参数写入以防污染当前模式".to_string();

            result
                .skipped
                .push(("自动光感".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("背光".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("黑色级别".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("对比度".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("饱和度".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("色调".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("锐度".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("色温".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("色增益".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("精密控光".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("HDR 色调映射".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("动态清晰度".to_string(), mode_err_reason.clone()));
            result
                .skipped
                .push(("灰阶响应时间".to_string(), mode_err_reason.clone()));
            result.skipped.push(("色域".to_string(), mode_err_reason));

            return result;
        }

        let mode = target_mode.unwrap();

        // 3. 自动光感 (0..=1)
        if values.contains_key("tv_picture_light_sensor") {
            match as_int(values, "tv_picture_light_sensor") {
                Ok(sensor) if sensor == 0 || sensor == 1 => {
                    match set_light_sensor(adb, sensor == 1) {
                        Ok(_) => result.applied.push("自动光感".to_string()),
                        Err(e) => result.failed.push(("自动光感".to_string(), e.to_string())),
                    }
                }
                Ok(sensor) => {
                    result.failed.push((
                        "自动光感".to_string(),
                        format!("自动光感取值 {} 非法，必须为 0 或 1", sensor),
                    ));
                }
                Err(e) => {
                    result.failed.push(("自动光感".to_string(), e));
                }
            }
        } else {
            result
                .skipped
                .push(("自动光感".to_string(), "预设未记录自动光感".to_string()));
        }

        // 4. 背光 (1..=100)
        if values.contains_key("picture_backlight") {
            match as_int(values, "picture_backlight") {
                Ok(b) if (1..=100).contains(&b) => match set_backlight(adb, b as u32) {
                    Ok(_) => result.applied.push("背光".to_string()),
                    Err(e) => result.failed.push(("背光".to_string(), e.to_string())),
                },
                Ok(b) => {
                    result.failed.push((
                        "背光".to_string(),
                        format!("背光数值 {} 超出有效范围 1..100", b),
                    ));
                }
                Err(e) => {
                    result.failed.push(("背光".to_string(), e));
                }
            }
        } else {
            result
                .skipped
                .push(("背光".to_string(), "预设未记录背光".to_string()));
        }

        // 5. 纯 Setting 基础参数 (0..=100)
        for (label, key) in [
            ("黑色级别", "picture_brightness"),
            ("对比度", "picture_contrast"),
            ("饱和度", "picture_saturation"),
            ("色调", "picture_hue"),
            ("锐度", "picture_sharpness"),
        ] {
            if values.contains_key(key) {
                match as_int(values, key) {
                    Ok(v) if (0..=100).contains(&v) => {
                        match set_plain_setting(adb, key, v as u32) {
                            Ok(_) => result.applied.push(label.to_string()),
                            Err(e) => result.failed.push((label.to_string(), e.to_string())),
                        }
                    }
                    Ok(v) => {
                        result.failed.push((
                            label.to_string(),
                            format!("{}数值 {} 超出有效范围 0..100", label, v),
                        ));
                    }
                    Err(e) => {
                        result.failed.push((label.to_string(), e));
                    }
                }
            } else {
                result
                    .skipped
                    .push((label.to_string(), format!("预设未记录 {}", label)));
            }
        }

        // 6. 色温 (有效表 xiaomi_to_mtk_color_temp)
        let mut has_color_temp = false;
        let mut color_temp_val = 0;
        if values.contains_key("picture_color_temperature") {
            match as_int(values, "picture_color_temperature") {
                Ok(ct) if crate::device::picture::xiaomi_to_mtk_color_temp(ct).is_some() => {
                    match set_color_temp(adb, ct) {
                        Ok(_) => {
                            has_color_temp = true;
                            color_temp_val = ct;
                            result.applied.push("色温".to_string());
                        }
                        Err(e) => {
                            result.failed.push(("色温".to_string(), e.to_string()));
                        }
                    }
                }
                Ok(ct) => {
                    result.failed.push((
                        "色温".to_string(),
                        format!("色温数值 {} 不在有效枚举中", ct),
                    ));
                }
                Err(e) => {
                    result.failed.push(("色温".to_string(), e));
                }
            }
        } else {
            result
                .skipped
                .push(("色温".to_string(), "预设未记录色温".to_string()));
        }

        // 7. 色增益 (RGB: 524..=1524，必须色温生效且为自定义 3 时才下发)
        if has_color_temp && color_temp_val == CUSTOM_COLOR_TEMP_VALUE {
            let r_res = as_int(values, "picture_red_gain");
            let g_res = as_int(values, "picture_green_gain");
            let b_res = as_int(values, "picture_blue_gain");

            match (r_res, g_res, b_res) {
                (Ok(r), Ok(g), Ok(b)) => {
                    if (524..=1524).contains(&r)
                        && (524..=1524).contains(&g)
                        && (524..=1524).contains(&b)
                    {
                        match set_color_gains(adb, r as u32, g as u32, b as u32) {
                            Ok(_) => result.applied.push("色增益".to_string()),
                            Err(e) => result.failed.push(("色增益".to_string(), e.to_string())),
                        }
                    } else {
                        result.failed.push((
                            "色增益".to_string(),
                            format!(
                                "色增益数值超出有效范围 524..1524: R={}, G={}, B={}",
                                r, g, b
                            ),
                        ));
                    }
                }
                _ => {
                    result.failed.push((
                        "色增益".to_string(),
                        "色温为自定义但色增益参数不完整或损坏".to_string(),
                    ));
                }
            }
        } else {
            result
                .skipped
                .push(("色增益".to_string(), "色温非自定义，跳过色增益".to_string()));
        }

        // 8. 精密控光 (0..=3)
        if values.contains_key("picture_local_dimming") {
            match as_int(values, "picture_local_dimming") {
                Ok(ld) if (0..=3).contains(&ld) => match set_local_dimming(adb, ld as u32) {
                    Ok(_) => result.applied.push("精密控光".to_string()),
                    Err(e) => result.failed.push(("精密控光".to_string(), e.to_string())),
                },
                Ok(ld) => {
                    result.failed.push((
                        "精密控光".to_string(),
                        format!("精密控光数值 {} 超出有效范围 0..3", ld),
                    ));
                }
                Err(e) => {
                    result.failed.push(("精密控光".to_string(), e));
                }
            }
        } else {
            result
                .skipped
                .push(("精密控光".to_string(), "预设未记录精密控光".to_string()));
        }

        // 9. HDR 色调映射 (仅在画面模式成功且为 HDR/Dolby 模式下有效)
        if is_hdr_tone_mapping_picture_mode(mode) {
            let ui_tone = as_int(values, "settings_display_hdr_color_tone")
                .ok()
                .or_else(|| {
                    as_int(values, "picture_hdr_tone_mapping")
                        .ok()
                        .and_then(|mtk| hdr_tone_mapping_mtk_to_ui(mtk).map(|u| u as i32))
                });

            if let Some(ui_val) = ui_tone {
                if (0..=3).contains(&ui_val) {
                    match set_hdr_tone_mapping(adb, ui_val as u32) {
                        Ok(_) => result.applied.push("HDR 色调映射".to_string()),
                        Err(e) => result
                            .failed
                            .push(("HDR 色调映射".to_string(), e.to_string())),
                    }
                } else {
                    result.failed.push((
                        "HDR 色调映射".to_string(),
                        format!("HDR 色调映射数值 {} 超出有效范围 0..3", ui_val),
                    ));
                }
            } else {
                result.failed.push((
                    "HDR 色调映射".to_string(),
                    "HDR 模式下缺少有效色调映射参数".to_string(),
                ));
            }
        } else {
            result.skipped.push((
                "HDR 色调映射".to_string(),
                "非 HDR 模式，跳过映射".to_string(),
            ));
        }

        // 10. 动态清晰度 (0..=3)
        if values.contains_key("picture_dynamic_definition") {
            match as_int(values, "picture_dynamic_definition") {
                Ok(dd) if (0..=3).contains(&dd) => match set_dynamic_definition(adb, dd as u32) {
                    Ok(_) => result.applied.push("动态清晰度".to_string()),
                    Err(e) => result
                        .failed
                        .push(("动态清晰度".to_string(), e.to_string())),
                },
                Ok(dd) => {
                    result.failed.push((
                        "动态清晰度".to_string(),
                        format!("动态清晰度数值 {} 超出有效范围 0..3", dd),
                    ));
                }
                Err(e) => {
                    result.failed.push(("动态清晰度".to_string(), e));
                }
            }
        } else {
            result
                .skipped
                .push(("动态清晰度".to_string(), "预设未记录动态清晰度".to_string()));
        }

        // 11. 灰阶响应时间 (1..=3)
        if values.contains_key("picture_response_time") {
            match as_int(values, "picture_response_time") {
                Ok(rt) if (1..=3).contains(&rt) => match set_response_time(adb, rt as u32) {
                    Ok(_) => result.applied.push("灰阶响应时间".to_string()),
                    Err(e) => result
                        .failed
                        .push(("灰阶响应时间".to_string(), e.to_string())),
                },
                Ok(rt) => {
                    result.failed.push((
                        "灰阶响应时间".to_string(),
                        format!("灰阶响应时间数值 {} 超出有效范围 1..3", rt),
                    ));
                }
                Err(e) => {
                    result.failed.push(("灰阶响应时间".to_string(), e));
                }
            }
        } else {
            result.skipped.push((
                "灰阶响应时间".to_string(),
                "预设未记录灰阶响应时间".to_string(),
            ));
        }

        // 12. 色域映射 (0 | 3 | 4 | 5 | 6 | 7)
        if values.contains_key("tv_picture_advanced_video_color_space") {
            match as_int(values, "tv_picture_advanced_video_color_space") {
                Ok(cs) if matches!(cs, 0 | 3 | 4 | 5 | 6 | 7) => {
                    match set_color_space(adb, cs as u32) {
                        Ok(_) => result.applied.push("色域".to_string()),
                        Err(e) => result.failed.push(("色域".to_string(), e.to_string())),
                    }
                }
                Ok(cs) => {
                    result.failed.push((
                        "色域".to_string(),
                        format!("色域数值 {} 不在有效枚举 [0, 3, 4, 5, 6, 7] 中", cs),
                    ));
                }
                Err(e) => {
                    result.failed.push(("色域".to_string(), e));
                }
            }
        } else {
            result
                .skipped
                .push(("色域".to_string(), "预设未记录色域".to_string()));
        }

        result
    })
}

/// 检查预设中缺失的关键字段
pub fn missing_keys(values: &HashMap<String, serde_json::Value>) -> Vec<&'static str> {
    let needed = &[
        "picture_mode",
        "tv_picture_light_sensor",
        "picture_backlight",
        "picture_brightness",
        "picture_contrast",
        "picture_saturation",
        "picture_hue",
        "picture_sharpness",
        "picture_color_temperature",
        "picture_red_gain",
        "picture_green_gain",
        "picture_blue_gain",
        "picture_local_dimming",
        "picture_dynamic_definition",
        "picture_response_time",
        "tv_picture_advanced_video_color_space",
        "settings_display_hdr_color_tone",
    ];

    needed
        .iter()
        .copied()
        .filter(|&k| !values.contains_key(k))
        .collect()
}

/// 在现有预设中按 ID 查找
pub fn find_preset<'a>(presets: &'a [PresetItem], id: &str) -> Option<&'a PresetItem> {
    presets.iter().find(|p| p.id == id)
}

/// 生成唯一短 ID
pub fn new_id(existing: &[PresetItem], prefix: &str) -> String {
    let mut index = 1;
    loop {
        let candidate = format!("{}{}", prefix, index);
        if !existing.iter().any(|p| p.id == candidate) {
            return candidate;
        }
        index += 1;
    }
}

/// 预设重名时自动递增后缀
pub fn unique_name(existing: &[PresetItem], wanted: &str) -> String {
    let names: Vec<&str> = existing.iter().map(|p| p.name.as_str()).collect();
    if !names.contains(&wanted) {
        return wanted.to_string();
    }
    let mut index = 2;
    loop {
        let candidate = format!("{} ({})", wanted, index);
        if !names.contains(&candidate.as_str()) {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_keys_detection() {
        let mut map = HashMap::new();
        map.insert("picture_mode".into(), serde_json::json!(14));
        map.insert("picture_backlight".into(), serde_json::json!(80));

        let missing = missing_keys(&map);
        assert!(missing.contains(&"tv_picture_light_sensor"));
        assert!(missing.contains(&"picture_contrast"));
        assert!(!missing.contains(&"picture_mode"));
        assert!(!missing.contains(&"picture_backlight"));
    }

    #[test]
    fn test_unique_name_generation() {
        let existing = vec![
            PresetItem {
                id: "p1".into(),
                name: "办公".into(),
                values: HashMap::new(),
            },
            PresetItem {
                id: "p2".into(),
                name: "办公 (2)".into(),
                values: HashMap::new(),
            },
        ];

        assert_eq!(unique_name(&existing, "游戏"), "游戏");
        assert_eq!(unique_name(&existing, "办公"), "办公 (3)");
    }

    #[test]
    fn test_apply_result_summary() {
        let mut res = ApplyResult::default();
        res.applied.push("背光".into());
        res.applied.push("对比度".into());
        res.skipped.push(("色增益".into(), "色温非自定义".into()));
        res.failed.push(("FreeSync".into(), "握手超时".into()));

        assert!(!res.ok());
        let summary = res.summary();
        assert!(summary.contains("成功 2 项"));
        assert!(summary.contains("跳过 1 项"));
        assert!(summary.contains("失败 1 项: FreeSync"));
    }

    #[test]
    fn test_as_int_parsing() {
        let mut values = HashMap::new();
        values.insert("num".into(), serde_json::json!(42));
        values.insert("str_num".into(), serde_json::json!("100"));
        values.insert("invalid".into(), serde_json::json!("abc"));
        values.insert("null_val".into(), serde_json::json!("null"));

        assert_eq!(as_int(&values, "num"), Ok(42));
        assert_eq!(as_int(&values, "str_num"), Ok(100));
        assert!(as_int(&values, "invalid").is_err());
        assert!(as_int(&values, "null_val").is_err());
        assert!(as_int(&values, "missing").is_err());
    }

    #[test]
    fn test_color_temp_and_hdr_mapping() {
        use crate::device::picture::{hdr_tone_mapping_ui_to_mtk, xiaomi_to_mtk_color_temp};

        // 色温映射对照 (小米 -> MTK)
        assert_eq!(xiaomi_to_mtk_color_temp(0), Some(1)); // 标准
        assert_eq!(xiaomi_to_mtk_color_temp(1), Some(2)); // 冷色
        assert_eq!(xiaomi_to_mtk_color_temp(2), Some(3)); // 暖色
        assert_eq!(xiaomi_to_mtk_color_temp(3), Some(0)); // 自定义

        // 反向色温映射 (MTK -> 小米)
        assert_eq!(mtk_to_xiaomi_color_temp(1), Some(0));
        assert_eq!(mtk_to_xiaomi_color_temp(2), Some(1));
        assert_eq!(mtk_to_xiaomi_color_temp(3), Some(2));
        assert_eq!(mtk_to_xiaomi_color_temp(0), Some(3));

        // HDR Tone Mapping (UI -> MTK)
        assert_eq!(hdr_tone_mapping_ui_to_mtk(0), Some(5)); // 增强
        assert_eq!(hdr_tone_mapping_ui_to_mtk(1), Some(0)); // 自然
        assert_eq!(hdr_tone_mapping_ui_to_mtk(2), Some(2)); // 真实
        assert_eq!(hdr_tone_mapping_ui_to_mtk(3), Some(1)); // 柔和

        // HDR Tone Mapping (MTK -> UI)
        assert_eq!(hdr_tone_mapping_mtk_to_ui(5), Some(0));
        assert_eq!(hdr_tone_mapping_mtk_to_ui(0), Some(1));
        assert_eq!(hdr_tone_mapping_mtk_to_ui(2), Some(2));
        assert_eq!(hdr_tone_mapping_mtk_to_ui(1), Some(3));
    }

    #[test]
    fn test_hdr_mode_qualification() {
        // SDR 标准模式不是 HDR 模式
        assert!(!is_hdr_tone_mapping_picture_mode(14));
        assert!(!is_hdr_tone_mapping_picture_mode(10));
        assert!(!is_hdr_tone_mapping_picture_mode(9));

        // HDR 及 Dolby 模式判定为支持 HDR tone mapping
        assert!(is_hdr_tone_mapping_picture_mode(11));
        assert!(is_hdr_tone_mapping_picture_mode(15));
        assert!(is_hdr_tone_mapping_picture_mode(22));
        assert!(is_hdr_tone_mapping_picture_mode(29));
    }

    #[test]
    fn test_as_int_overflow_protection() {
        let mut values = HashMap::new();
        values.insert("huge_pos".into(), serde_json::json!(3_000_000_000_i64));
        values.insert("huge_max".into(), serde_json::json!(i64::MAX));
        values.insert("huge_min".into(), serde_json::json!(i64::MIN));
        values.insert("normal_neg".into(), serde_json::json!(-1));

        // 溢出值必须返回 Err，严禁截断为 i32 导致假合法或符号反转
        assert!(as_int(&values, "huge_pos").is_err());
        assert!(as_int(&values, "huge_max").is_err());
        assert!(as_int(&values, "huge_min").is_err());
        assert_eq!(as_int(&values, "normal_neg"), Ok(-1));
    }

    #[test]
    fn test_validate_preset_field_ranges() {
        // 背光 1..=100
        assert!(validate_preset_field("picture_backlight", 1).is_ok());
        assert!(validate_preset_field("picture_backlight", 100).is_ok());
        assert!(validate_preset_field("picture_backlight", 0).is_err());
        assert!(validate_preset_field("picture_backlight", -10).is_err());
        assert!(validate_preset_field("picture_backlight", 101).is_err());

        // 基础 0..=100
        for key in &[
            "picture_brightness",
            "picture_contrast",
            "picture_saturation",
            "picture_hue",
            "picture_sharpness",
        ] {
            assert!(validate_preset_field(key, 0).is_ok());
            assert!(validate_preset_field(key, 100).is_ok());
            assert!(validate_preset_field(key, -1).is_err());
            assert!(validate_preset_field(key, 101).is_err());
        }

        // RGB 524..=1524
        for key in &[
            "picture_red_gain",
            "picture_green_gain",
            "picture_blue_gain",
        ] {
            assert!(validate_preset_field(key, 524).is_ok());
            assert!(validate_preset_field(key, 1524).is_ok());
            assert!(validate_preset_field(key, 523).is_err());
            assert!(validate_preset_field(key, 1525).is_err());
            assert!(validate_preset_field(key, -1).is_err());
        }

        // 局部调光 0..=3
        assert!(validate_preset_field("picture_local_dimming", 0).is_ok());
        assert!(validate_preset_field("picture_local_dimming", 3).is_ok());
        assert!(validate_preset_field("picture_local_dimming", -1).is_err());
        assert!(validate_preset_field("picture_local_dimming", 4).is_err());

        // 动态清晰 0..=3
        assert!(validate_preset_field("picture_dynamic_definition", 0).is_ok());
        assert!(validate_preset_field("picture_dynamic_definition", 3).is_ok());
        assert!(validate_preset_field("picture_dynamic_definition", -1).is_err());
        assert!(validate_preset_field("picture_dynamic_definition", 4).is_err());

        // 响应 1..=3
        assert!(validate_preset_field("picture_response_time", 1).is_ok());
        assert!(validate_preset_field("picture_response_time", 3).is_ok());
        assert!(validate_preset_field("picture_response_time", 0).is_err());
        assert!(validate_preset_field("picture_response_time", 4).is_err());
        assert!(validate_preset_field("picture_response_time", -1).is_err());

        // 色温有效表 (0, 1, 2, 3, 4, 5, 8)
        for valid in &[0, 1, 2, 3, 4, 5, 8] {
            assert!(validate_preset_field("picture_color_temperature", *valid).is_ok());
        }
        assert!(validate_preset_field("picture_color_temperature", -1).is_err());
        assert!(validate_preset_field("picture_color_temperature", 6).is_err());
        assert!(validate_preset_field("picture_color_temperature", 99).is_err());

        // 色域 (0, 3, 4, 5, 6, 7)
        for valid in &[0, 3, 4, 5, 6, 7] {
            assert!(validate_preset_field("tv_picture_advanced_video_color_space", *valid).is_ok());
        }
        assert!(validate_preset_field("tv_picture_advanced_video_color_space", 1).is_err());
        assert!(validate_preset_field("tv_picture_advanced_video_color_space", 2).is_err());
        assert!(validate_preset_field("tv_picture_advanced_video_color_space", -1).is_err());

        // FreeSync 与 sensor 0/1
        assert!(validate_preset_field("freesync", 0).is_ok());
        assert!(validate_preset_field("freesync", 1).is_ok());
        assert!(validate_preset_field("freesync", -1).is_err());
        assert!(validate_preset_field("freesync", 2).is_err());

        assert!(validate_preset_field("tv_picture_light_sensor", 0).is_ok());
        assert!(validate_preset_field("tv_picture_light_sensor", 1).is_ok());
        assert!(validate_preset_field("tv_picture_light_sensor", -1).is_err());
        assert!(validate_preset_field("tv_picture_light_sensor", 2).is_err());
    }

    #[test]
    fn test_apply_preset_picture_mode_negative_stops_execution() {
        let dev = AdbDevice::new("127.0.0.1:65534");
        let mut values = HashMap::new();
        values.insert("picture_mode".into(), serde_json::json!(-1));
        values.insert("picture_backlight".into(), serde_json::json!(50));
        values.insert("picture_contrast".into(), serde_json::json!(50));

        let res = apply_preset_values(&dev, &values);
        assert!(!res.ok());
        assert!(res
            .failed
            .iter()
            .any(|(k, msg)| k == "画面模式" && msg.contains("非法")));
        // 模式未生效，背光与对比度等模式相关参数必须被明确 skipped，不能写入
        assert!(res
            .skipped
            .iter()
            .any(|(k, msg)| k == "背光" && msg.contains("防污染当前模式")));
        assert!(res
            .skipped
            .iter()
            .any(|(k, msg)| k == "对比度" && msg.contains("防污染当前模式")));
        assert!(res.applied.is_empty());
    }

    #[test]
    fn test_apply_preset_missing_mode_fails_and_skips_dependents() {
        let dev = AdbDevice::new("127.0.0.1:65534");
        let mut values = HashMap::new();
        values.insert("picture_backlight".into(), serde_json::json!(50));

        let res = apply_preset_values(&dev, &values);
        assert!(!res.ok());
        assert!(res.failed.iter().any(|(k, _)| k == "画面模式"));
        assert!(res
            .skipped
            .iter()
            .any(|(k, msg)| k == "背光" && msg.contains("防污染当前模式")));
        assert!(res.applied.is_empty());
    }

    #[test]
    fn test_apply_preset_invalid_freesync_fails_immediately() {
        let dev = AdbDevice::new("127.0.0.1:65534");
        let mut values = HashMap::new();
        values.insert("freesync".into(), serde_json::json!(-1));
        values.insert("picture_mode".into(), serde_json::json!(-1));

        let res = apply_preset_values(&dev, &values);
        assert!(!res.ok());
        assert!(res
            .failed
            .iter()
            .any(|(k, msg)| k == "FreeSync" && msg.contains("非法")));
    }
}
