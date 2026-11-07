use crate::adb::{AdbDevice, AdbError};
use crate::device::jni::{
    check_and_heal_jar, jni_get, jni_set, jni_set_color_gains, jni_set_hdr_tone_mapping, refresh_pq,
};
use serde::{Deserialize, Serialize};

pub const CUSTOM_COLOR_TEMP_VALUE: i32 = 3;

/// 小米与 MTK 色温枚举映射对照
pub const XIAOMI_TO_MTK_COLOR_TEMP: &[(i32, i32)] = &[
    (0, 1), // 标准
    (1, 2), // 冷色
    (2, 3), // 暖色
    (3, 0), // 自定义
    (4, 4),
    (5, 5),
    (8, 6),
];

pub fn xiaomi_to_mtk_color_temp(val: i32) -> Option<i32> {
    XIAOMI_TO_MTK_COLOR_TEMP
        .iter()
        .find(|&&(x, _)| x == val)
        .map(|&(_, m)| m)
}

pub fn mtk_to_xiaomi_color_temp(val: i32) -> Option<i32> {
    XIAOMI_TO_MTK_COLOR_TEMP
        .iter()
        .find(|&&(_, m)| m == val)
        .map(|&(x, _)| x)
}

/// HDR Tone Mapping UI 到 MTK 枚举映射
pub const HDR_TONE_MAPPING_UI_TO_MTK: &[(u32, i32)] = &[
    (0, 5), // 增强
    (1, 0), // 自然
    (2, 2), // 真实
    (3, 1), // 柔和
];

pub fn hdr_tone_mapping_ui_to_mtk(ui_val: u32) -> Option<i32> {
    HDR_TONE_MAPPING_UI_TO_MTK
        .iter()
        .find(|&&(u, _)| u == ui_val)
        .map(|&(_, m)| m)
}

pub fn hdr_tone_mapping_mtk_to_ui(mtk_val: i32) -> Option<u32> {
    HDR_TONE_MAPPING_UI_TO_MTK
        .iter()
        .find(|&&(_, m)| m == mtk_val)
        .map(|&(u, _)| u)
}

/// 游戏模式枚举集合 (模式 10 及其子预设，加部分特殊 HDR 游戏)
pub fn is_game_picture_mode(mode: i32) -> bool {
    matches!(
        mode,
        10 | 25 | 26 | 27 | 28 | 29 | 4 | 15 | 19 | 30 | 31 | 32 | 33
    )
}

/// HDR Tone Mapping 支持的画面模式集合
pub fn is_hdr_tone_mapping_picture_mode(mode: i32) -> bool {
    matches!(
        mode,
        11 | 12
            | 13
            | 15
            | 16
            | 17
            | 18
            | 19
            | 22
            | 23
            | 29
            | 30
            | 31
            | 32
            | 33
            | 39
            | 40
            | 41
            | 42
            | 43
            | 44
    )
}

/// 获取画面场景的人类可读中文名称
pub fn get_picture_scene_name(mode: i32) -> &'static str {
    match mode {
        9 => "电影",
        10 => "游戏",
        11 => "Dolby Vision 明亮",
        12 => "Dolby Vision 暗场",
        13 => "Dolby Vision 自定义",
        14 => "标准",
        15 => "HDR 游戏",
        16 => "HDR 图片",
        17 => "HDR 电影",
        18 => "Dolby Vision IQ",
        19 => "Dolby Vision 游戏",
        21 => "Filmmaker",
        22 => "HDR 显示器",
        23 => "HDR Filmmaker",
        24 => "SDR 游戏 FPS",
        25 => "SDR 游戏 RPG",
        26 => "SDR 游戏 RTS",
        27 => "SDR 游戏 MOBA",
        28 => "SDR 游戏 SPT",
        29 => "HDR 游戏 FPS",
        30 => "HDR 游戏 RPG",
        31 => "HDR 游戏 RTS",
        32 => "HDR 游戏 MOBA",
        33 => "HDR 游戏 SPT",
        34 => "SDR PC AdobeRGB",
        35 => "SDR PC DCI-P3",
        36 => "SDR PC CG",
        37 => "SDR PC 暗房",
        38 => "SDR PC sRGB",
        39 => "HDR PC AdobeRGB",
        40 => "HDR PC DCI-P3",
        41 => "HDR PC CG",
        42 => "HDR PC 暗房",
        43 => "HDR PC sRGB",
        44 => "HDR Vivid",
        64..=68 => "标准预设",
        _ => "未知场景",
    }
}

/// 画面参数完整状态集合（全部以 Option 区分真实已读与未知值，杜绝伪造 50/0/14）
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PictureParametersState {
    pub mode: Option<i32>,
    pub backlight: Option<u32>,
    pub black_level: Option<u32>,
    pub contrast: Option<u32>,
    pub saturation: Option<u32>,
    pub hue: Option<u32>,
    pub sharpness: Option<u32>,
    pub color_temp: Option<i32>,
    pub red_gain: Option<u32>,
    pub green_gain: Option<u32>,
    pub blue_gain: Option<u32>,
    pub local_dimming: Option<u32>,
    pub color_space: Option<u32>,
    pub hdr_tone_mapping: Option<u32>,
    pub light_sensor: Option<bool>,
    pub dynamic_definition: Option<u32>,
    pub response_time: Option<u32>,
}

/// 画面连续拖拽滑块参数类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PictureSliderParam {
    Backlight,
    BlackLevel,
    Contrast,
    Saturation,
    Hue,
    Sharpness,
    RedGain,
    GreenGain,
    BlueGain,
    DynamicDefinition,
    ResponseTime,
}

/// 画面参数集合
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PictureParameters {
    pub mode: i32,
    pub backlight: u32,
    pub black_level: u32,
    pub contrast: u32,
    pub saturation: u32,
    pub hue: u32,
    pub sharpness: u32,
    pub color_temp: i32,
    pub red_gain: u32,
    pub green_gain: u32,
    pub blue_gain: u32,
    pub local_dimming: u32,
    pub color_space: u32,
    pub hdr_tone_mapping: u32,
    pub light_sensor: bool,
    pub dynamic_definition: u32,
    pub response_time: u32,
}

impl Default for PictureParameters {
    fn default() -> Self {
        Self {
            mode: 14,
            backlight: 50,
            black_level: 50,
            contrast: 50,
            saturation: 50,
            hue: 50,
            sharpness: 50,
            color_temp: 0,
            red_gain: 1024,
            green_gain: 1024,
            blue_gain: 1024,
            local_dimming: 3,
            color_space: 0,
            hdr_tone_mapping: 0,
            light_sensor: false,
            dynamic_definition: 0,
            response_time: 0,
        }
    }
}

/// 切换画面模式
pub fn set_picture_mode(adb: &AdbDevice, mode: i32) -> Result<(), AdbError> {
    adb.put_setting("picture_mode", &mode.to_string())
}

/// 设置背光 (1 - 100)
pub fn set_backlight(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    let clamped = val.clamp(1, 100);
    jni_set(adb, "g_disp__disp_back_light", clamped as i32, 3)?;
    refresh_pq(adb)?;
    adb.put_setting("picture_backlight", &clamped.to_string())?;
    adb.put_setting("xiaomi_picture_backlight", &clamped.to_string())?;
    Ok(())
}

/// 读取当前背光值 (优先读取 JNI 底层硬件寄存器，若失败则回退至 Android Settings 读数)
pub fn get_backlight(adb: &AdbDevice) -> Result<u32, AdbError> {
    if let Ok(b) = jni_get(adb, "g_disp__disp_back_light") {
        if b >= 0 {
            return Ok(b as u32);
        }
    }
    let s = adb.get_setting("picture_backlight")?;
    s.parse::<u32>()
        .map_err(|e| AdbError::CommandFailed(e.to_string()))
}

/// 设置常规纯 Settings 画面参数 (0 - 100)
pub fn set_plain_setting(adb: &AdbDevice, key: &str, val: u32) -> Result<(), AdbError> {
    let clamped = val.clamp(0, 100);
    adb.put_setting(key, &clamped.to_string())
}

/// 设置黑色级别 (0 - 100)
pub fn set_black_level(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    set_plain_setting(adb, "picture_brightness", val)
}

/// 设置对比度 (0 - 100)
pub fn set_contrast(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    set_plain_setting(adb, "picture_contrast", val)
}

/// 设置饱和度 (0 - 100)
pub fn set_saturation(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    set_plain_setting(adb, "picture_saturation", val)
}

/// 设置色调 (0 - 100)
pub fn set_hue(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    set_plain_setting(adb, "picture_hue", val)
}

/// 设置锐度 (0 - 100)
pub fn set_sharpness(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    set_plain_setting(adb, "picture_sharpness", val)
}

/// 根据 PictureSliderParam 统一分发写入
pub fn set_picture_slider_param(
    adb: &AdbDevice,
    param: PictureSliderParam,
    val: u32,
) -> Result<(), AdbError> {
    match param {
        PictureSliderParam::Backlight => set_backlight(adb, val),
        PictureSliderParam::BlackLevel => set_black_level(adb, val),
        PictureSliderParam::Contrast => set_contrast(adb, val),
        PictureSliderParam::Saturation => set_saturation(adb, val),
        PictureSliderParam::Hue => set_hue(adb, val),
        PictureSliderParam::Sharpness => set_sharpness(adb, val),
        PictureSliderParam::RedGain => set_plain_setting(adb, "picture_red_gain", val),
        PictureSliderParam::GreenGain => set_plain_setting(adb, "picture_green_gain", val),
        PictureSliderParam::BlueGain => set_plain_setting(adb, "picture_blue_gain", val),
        PictureSliderParam::DynamicDefinition => set_dynamic_definition(adb, val),
        PictureSliderParam::ResponseTime => set_response_time(adb, val),
    }
}

/// 设置色温
pub fn set_color_temp(adb: &AdbDevice, ui_val: i32) -> Result<(), AdbError> {
    let mtk_val = xiaomi_to_mtk_color_temp(ui_val)
        .ok_or_else(|| AdbError::CommandFailed(format!("未知色温枚举: {}", ui_val)))?;
    jni_set(adb, "g_video__clr_temp", mtk_val, 3)?;
    adb.put_setting("picture_color_temperature", &ui_val.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置 RGB 增益 (524 - 1524，强制切换为自定义色温 3)
pub fn set_color_gains(adb: &AdbDevice, r: u32, g: u32, b: u32) -> Result<(), AdbError> {
    let r_clamped = r.clamp(524, 1524);
    let g_clamped = g.clamp(524, 1524);
    let b_clamped = b.clamp(524, 1524);

    let mtk_custom = xiaomi_to_mtk_color_temp(CUSTOM_COLOR_TEMP_VALUE).unwrap_or(0);
    jni_set(adb, "g_video__clr_temp", mtk_custom, 3)?;
    adb.put_setting(
        "picture_color_temperature",
        &CUSTOM_COLOR_TEMP_VALUE.to_string(),
    )?;

    jni_set_color_gains(adb, r_clamped, g_clamped, b_clamped)?;
    adb.put_setting("picture_red_gain", &r_clamped.to_string())?;
    adb.put_setting("picture_green_gain", &g_clamped.to_string())?;
    adb.put_setting("picture_blue_gain", &b_clamped.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置精密控光 (0: 关, 1: 低, 2: 中, 3: 高)
pub fn set_local_dimming(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    let clamped = val.clamp(0, 3);
    jni_set(adb, "g_video__vid_local_dimming", clamped as i32, 3)?;
    adb.put_setting("picture_local_dimming", &clamped.to_string())?;
    adb.put_setting("tv_picture_video_local_dimming", &clamped.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置色域映射
pub fn set_color_space(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    jni_set(adb, "g_video__vid_gamut_mapping_mode", val as i32, 3)?;
    adb.put_setting("tv_picture_advanced_video_color_space", &val.to_string())?;
    adb.put_setting("tv_picture_video_color_space", &val.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置 HDR Tone Mapping (UI 枚举 0 - 3)
pub fn set_hdr_tone_mapping(adb: &AdbDevice, ui_val: u32) -> Result<(), AdbError> {
    let mtk_val = hdr_tone_mapping_ui_to_mtk(ui_val).ok_or_else(|| {
        AdbError::CommandFailed(format!("未知 HDR Tone Mapping 枚举: {}", ui_val))
    })?;

    check_and_heal_jar(adb)?;
    jni_set_hdr_tone_mapping(adb, mtk_val, 3)?;
    adb.put_setting("picture_hdr_tone_mapping", &mtk_val.to_string())?;
    adb.put_setting("settings_display_hdr_color_tone", &ui_val.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置自动光感
pub fn set_light_sensor(adb: &AdbDevice, enabled: bool) -> Result<(), AdbError> {
    let val = if enabled { 1 } else { 0 };
    // 光感 upd 为 1，不需要发 refresh_pq
    jni_set(adb, "g_video__light_sensor_switch", val, 1)?;
    adb.put_setting("tv_picture_light_sensor", &val.to_string())?;
    Ok(())
}

/// 设置动态清晰度 (0: 关, 1: 低, 2: 中, 3: 高)
pub fn set_dynamic_definition(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    let clamped = val.clamp(0, 3);
    jni_set(adb, "g_video__vid_insert_black", clamped as i32, 3)?;
    adb.put_setting("picture_dynamic_definition", &clamped.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置灰阶响应时间 (1: 普通, 2: 快速, 3: 高速)
pub fn set_response_time(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    let clamped = val.clamp(1, 3);
    jni_set(adb, "g_video__vid_od_response_time", clamped as i32, 3)?;
    adb.put_setting("picture_response_time", &clamped.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 恢复当前画面模式默认设置 (JNI g_fusion_picture__pic_reset_def_bypicmode 0)
pub fn reset_current_mode(adb: &AdbDevice) -> Result<(), AdbError> {
    jni_set(adb, "g_fusion_picture__pic_reset_def_bypicmode", 0, 3)?;
    refresh_pq(adb)?;
    Ok(())
}

/// 批量获取画面页完整参数状态（在单一 ADB 事务内批量执行 Settings 读取和 JNI 读取，再以 JNI 覆盖 Settings）
pub fn fetch_picture_parameters(adb: &AdbDevice) -> Result<PictureParametersState, AdbError> {
    let mut state = PictureParametersState::default();

    adb.transaction(|| -> Result<(), AdbError> {
        // 1. 批量读取 global settings
        let settings_keys = [
            "picture_mode",
            "picture_preset_scenario",
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
            "tv_picture_light_sensor",
        ];

        let keys_str = settings_keys.join(" ");
        let shell_cmd = format!(
            "for k in {}; do echo $k=$(settings get global $k); done",
            keys_str
        );
        if let Ok(out) = adb.shell(&shell_cmd) {
            for line in out.lines() {
                let trimmed = line.trim();
                if let Some((k, v)) = trimmed.split_once('=') {
                    let k = k.trim();
                    let v = v.trim();
                    if v.is_empty() || v == "null" || v == "N/A" {
                        continue;
                    }
                    match k {
                        "picture_mode" => state.mode = v.parse::<i32>().ok(),
                        "picture_backlight" | "xiaomi_picture_backlight" => {
                            if state.backlight.is_none() {
                                state.backlight = v.parse::<u32>().ok();
                            }
                        }
                        "picture_brightness" => state.black_level = v.parse::<u32>().ok(),
                        "picture_contrast" => state.contrast = v.parse::<u32>().ok(),
                        "picture_saturation" => state.saturation = v.parse::<u32>().ok(),
                        "picture_hue" => state.hue = v.parse::<u32>().ok(),
                        "picture_sharpness" => state.sharpness = v.parse::<u32>().ok(),
                        "picture_color_temperature" => state.color_temp = v.parse::<i32>().ok(),
                        "picture_red_gain" => state.red_gain = v.parse::<u32>().ok(),
                        "picture_green_gain" => state.green_gain = v.parse::<u32>().ok(),
                        "picture_blue_gain" => state.blue_gain = v.parse::<u32>().ok(),
                        "picture_local_dimming" | "tv_picture_video_local_dimming" => {
                            if state.local_dimming.is_none() {
                                state.local_dimming = v.parse::<u32>().ok();
                            }
                        }
                        "settings_display_hdr_color_tone" => {
                            state.hdr_tone_mapping = v.parse::<u32>().ok();
                        }
                        "picture_hdr_tone_mapping" => {
                            if state.hdr_tone_mapping.is_none() {
                                if let Ok(mtk_val) = v.parse::<i32>() {
                                    state.hdr_tone_mapping = hdr_tone_mapping_mtk_to_ui(mtk_val);
                                }
                            }
                        }
                        "picture_dynamic_definition" => {
                            state.dynamic_definition = v.parse::<u32>().ok()
                        }
                        "picture_response_time" => state.response_time = v.parse::<u32>().ok(),
                        "tv_picture_advanced_video_color_space"
                        | "tv_picture_video_color_space" => {
                            if state.color_space.is_none() {
                                state.color_space = v.parse::<u32>().ok();
                            }
                        }
                        "tv_picture_light_sensor" => {
                            if let Ok(num) = v.parse::<i32>() {
                                state.light_sensor = Some(num == 1);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // 2. 批量读取 JNI 底层真实硬件寄存器（JNI 才是硬件真实状态，覆盖 settings 读数）
        let jni_keys = [
            "g_disp__disp_back_light",
            "g_video__vid_gamut_mapping_mode",
            "g_video__clr_temp",
            "g_video__vid_local_dimming",
            "g_video__vid_hdr_tone_mapping_mode",
            "g_video__light_sensor_switch",
        ];

        if let Ok(jni_vals) = crate::device::jni::jni_batch_get_or_single(adb, &jni_keys) {
            // 背光
            if let Some(&bk) = jni_vals.get("g_disp__disp_back_light") {
                if bk >= 0 {
                    state.backlight = Some(bk as u32);
                }
            }
            // 色域
            if let Some(&gamut) = jni_vals.get("g_video__vid_gamut_mapping_mode") {
                if gamut >= 0 {
                    state.color_space = Some(gamut as u32);
                }
            }
            // 色温：MTK -> Xiaomi
            if let Some(&mtk_clr) = jni_vals.get("g_video__clr_temp") {
                if let Some(xi_clr) = mtk_to_xiaomi_color_temp(mtk_clr) {
                    state.color_temp = Some(xi_clr);
                }
            }
            // 精密控光
            if let Some(&dim) = jni_vals.get("g_video__vid_local_dimming") {
                if dim >= 0 {
                    state.local_dimming = Some(dim as u32);
                }
            }
            // HDR Tone Mapping: MTK -> UI
            if let Some(&hdr_mode) = jni_vals.get("g_video__vid_hdr_tone_mapping_mode") {
                if let Some(ui_tone) = hdr_tone_mapping_mtk_to_ui(hdr_mode) {
                    state.hdr_tone_mapping = Some(ui_tone);
                }
            }
            // 自动光感
            if let Some(&sensor) = jni_vals.get("g_video__light_sensor_switch") {
                state.light_sensor = Some(sensor == 1);
            }
        }

        Ok(())
    })?;

    Ok(state)
}

/// 查询画面参数（别名与 fetch_picture_parameters 行为完全一致）
pub fn query_picture_parameters(adb: &AdbDevice) -> Result<PictureParametersState, AdbError> {
    fetch_picture_parameters(adb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_picture_parameters_state_initial_unknown() {
        let state = PictureParametersState::default();
        // 初始读数全部为 None，严禁伪造 50、14 等假数据
        assert_eq!(state.mode, None);
        assert_eq!(state.backlight, None);
        assert_eq!(state.black_level, None);
        assert_eq!(state.contrast, None);
        assert_eq!(state.saturation, None);
        assert_eq!(state.hue, None);
        assert_eq!(state.sharpness, None);
        assert_eq!(state.light_sensor, None);
        assert_eq!(state.local_dimming, None);
    }

    #[test]
    fn test_picture_scene_names() {
        assert_eq!(get_picture_scene_name(14), "标准");
        assert_eq!(get_picture_scene_name(10), "游戏");
        assert_eq!(get_picture_scene_name(9), "电影");
        assert_eq!(get_picture_scene_name(15), "HDR 游戏");
        assert_eq!(get_picture_scene_name(11), "Dolby Vision 明亮");
        assert_eq!(get_picture_scene_name(999), "未知场景");
    }

    #[test]
    fn test_color_temp_table_bidirectional_mapping() {
        // 冷0 -> MTK 1
        assert_eq!(xiaomi_to_mtk_color_temp(0), Some(1));
        assert_eq!(mtk_to_xiaomi_color_temp(1), Some(0));

        // 标准1 -> MTK 2
        assert_eq!(xiaomi_to_mtk_color_temp(1), Some(2));
        assert_eq!(mtk_to_xiaomi_color_temp(2), Some(1));

        // 暖色2 -> MTK 3
        assert_eq!(xiaomi_to_mtk_color_temp(2), Some(3));
        assert_eq!(mtk_to_xiaomi_color_temp(3), Some(2));

        // 原色8 -> MTK 6
        assert_eq!(xiaomi_to_mtk_color_temp(8), Some(6));
        assert_eq!(mtk_to_xiaomi_color_temp(6), Some(8));

        // 自定义3 -> MTK 0
        assert_eq!(xiaomi_to_mtk_color_temp(3), Some(0));
        assert_eq!(mtk_to_xiaomi_color_temp(0), Some(3));
    }

    #[test]
    fn test_hdr_tone_mapping_ui_to_mtk() {
        // UI 0 (HGiG) -> MTK 5
        assert_eq!(hdr_tone_mapping_ui_to_mtk(0), Some(5));
        assert_eq!(hdr_tone_mapping_mtk_to_ui(5), Some(0));

        // UI 1 (层次) -> MTK 0
        assert_eq!(hdr_tone_mapping_ui_to_mtk(1), Some(0));
        assert_eq!(hdr_tone_mapping_mtk_to_ui(0), Some(1));

        // UI 2 (动态) -> MTK 2
        assert_eq!(hdr_tone_mapping_ui_to_mtk(2), Some(2));
        assert_eq!(hdr_tone_mapping_mtk_to_ui(2), Some(2));

        // UI 3 (明亮) -> MTK 1
        assert_eq!(hdr_tone_mapping_ui_to_mtk(3), Some(1));
        assert_eq!(hdr_tone_mapping_mtk_to_ui(1), Some(3));
    }

    #[test]
    fn test_is_hdr_tone_mapping_picture_mode() {
        assert!(is_hdr_tone_mapping_picture_mode(15)); // HDR 游戏
        assert!(is_hdr_tone_mapping_picture_mode(11)); // Dolby Vision 明亮
        assert!(!is_hdr_tone_mapping_picture_mode(14)); // 标准 SDR
        assert!(!is_hdr_tone_mapping_picture_mode(10)); // 游戏 SDR
    }
}
