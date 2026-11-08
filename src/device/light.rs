use crate::adb::commands::build_tvservice_app_process_command;
use crate::adb::{AdbDevice, AdbError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const COLORFUL_LED_TOOL_JAR: &str = "ColorfulLedTool.jar";
pub const REMOTE_CACHE_DIR: &str = "/data/data/mitv.service/cache";
pub const REMOTE_JAR_CACHE_PATH: &str = "/data/data/mitv.service/cache/ColorfulLedTool.jar";

/// 屏幕灯 5 种工作模式 (0=照明, 1=屏幕同色, 2=纯色, 3=七彩循环, 4=关)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenLightMode {
    Lighting = 0, // 照明模式 (带亮度与色温)
    Ambient = 1,  // 屏幕同色 (氛围模式)
    Solid = 2,    // 纯色静态 (带亮度与色值)
    Cycle = 3,    // 七彩循环 (炫彩梦境)
    Off = 4,      // 关闭
}

impl ScreenLightMode {
    pub fn name(&self) -> &'static str {
        match self {
            ScreenLightMode::Lighting => "照明",
            ScreenLightMode::Ambient => "屏幕同色",
            ScreenLightMode::Solid => "纯色",
            ScreenLightMode::Cycle => "七彩循环",
            ScreenLightMode::Off => "关闭",
        }
    }

    pub fn code(&self) -> u32 {
        *self as u32
    }

    pub fn try_from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(ScreenLightMode::Lighting),
            1 => Some(ScreenLightMode::Ambient),
            2 => Some(ScreenLightMode::Solid),
            3 => Some(ScreenLightMode::Cycle),
            4 => Some(ScreenLightMode::Off),
            _ => None,
        }
    }

    pub fn from_code(code: u32) -> Self {
        Self::try_from_code(code).unwrap_or(ScreenLightMode::Off)
    }
}

/// 屏幕灯完整参数配置 (必须基于设备真实读数构造，禁止盲目使用默认值覆盖其他设置)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenLightParameters {
    pub mode: ScreenLightMode,
    pub illumination: u32, // 0 - 14 (UI 展现为 1 - 15)
    pub color_temp: u32,   // 0=2700K, 1=4000K, 2=6500K
    pub color_value: u32,  // 0=冰蓝, 1=流金, 2=天青, 3=草地, 4=日落
}

impl Default for ScreenLightParameters {
    fn default() -> Self {
        Self {
            mode: ScreenLightMode::Off,
            illumination: 8,
            color_temp: 1,
            color_value: 0,
        }
    }
}

/// 屏幕灯真实状态 (支持部分字段未知与错误呈现，保障未完全读取前禁用不安全写入)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScreenLightState {
    pub mode: Option<ScreenLightMode>,
    pub illumination: Option<u32>, // 硬件 0..14 (UI 1..15)
    pub color_temp: Option<u32>,   // 0, 1, 2
    pub color_value: Option<u32>,  // 0, 1, 2, 3, 4
    pub is_reading: bool,
    pub is_writing: bool,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_revision: u64,
}

impl ScreenLightState {
    /// 校验四项真实硬件字段是否均已完整读取
    pub fn is_fully_known(&self) -> bool {
        self.mode.is_some()
            && self.illumination.is_some()
            && self.color_temp.is_some()
            && self.color_value.is_some()
    }

    /// 转换为可提交的参数，如存在未知字段则返回 None 阻止不安全覆盖
    pub fn to_parameters(&self) -> Option<ScreenLightParameters> {
        Some(ScreenLightParameters {
            mode: self.mode?,
            illumination: self.illumination?,
            color_temp: self.color_temp?,
            color_value: self.color_value?,
        })
    }
}

/// 后台读取屏幕灯四字段真实硬件/系统参数
pub fn read_screen_light_state(adb: &AdbDevice) -> ScreenLightState {
    let mut state = ScreenLightState::default();
    let mut errs = Vec::new();

    let settings = match adb.get_settings(&[
        "atmosphere_light_switcher_pm2",
        "atmosphere_light_illumination",
        "atmosphere_light_color_temp",
        "atmosphere_light_color_value",
    ]) {
        Ok(v) => v,
        Err(e) => {
            state.last_error = Some(e.to_string());
            return state;
        }
    };
    match settings
        .get("atmosphere_light_switcher_pm2")
        .ok_or("未读取到字段")
    {
        Ok(s) => match s.trim().parse::<u32>() {
            Ok(code) => match ScreenLightMode::try_from_code(code) {
                Some(m) => state.mode = Some(m),
                None => errs.push(format!("未知模式值: {}", code)),
            },
            Err(_) => errs.push(format!("模式格式非法: {:?}", s)),
        },
        Err(e) => errs.push(format!("模式读取失败: {}", e)),
    }

    match settings
        .get("atmosphere_light_illumination")
        .ok_or("未读取到字段")
    {
        Ok(s) => match s.trim().parse::<u32>() {
            Ok(val) if val <= 14 => state.illumination = Some(val),
            Ok(val) => errs.push(format!("亮度越界 ({} > 14)", val)),
            Err(_) => errs.push(format!("亮度格式非法: {:?}", s)),
        },
        Err(e) => errs.push(format!("亮度读取失败: {}", e)),
    }

    match settings
        .get("atmosphere_light_color_temp")
        .ok_or("未读取到字段")
    {
        Ok(s) => match s.trim().parse::<u32>() {
            Ok(val) if val <= 2 => state.color_temp = Some(val),
            Ok(val) => errs.push(format!("色温越界 ({} > 2)", val)),
            Err(_) => errs.push(format!("色温格式非法: {:?}", s)),
        },
        Err(e) => errs.push(format!("色温读取失败: {}", e)),
    }

    match settings
        .get("atmosphere_light_color_value")
        .ok_or("未读取到字段")
    {
        Ok(s) => match s.trim().parse::<u32>() {
            Ok(val) if val <= 4 => state.color_value = Some(val),
            Ok(val) => errs.push(format!("颜色越界 ({} > 4)", val)),
            Err(_) => errs.push(format!("颜色格式非法: {:?}", s)),
        },
        Err(e) => errs.push(format!("颜色读取失败: {}", e)),
    }

    if !errs.is_empty() {
        state.last_error = Some(errs.join("; "));
    }
    state
}

/// 定位本地 ColorfulLedTool.jar
pub fn get_local_colorful_led_tool_path() -> Option<PathBuf> {
    crate::resources::helper_path(COLORFUL_LED_TOOL_JAR)
}

/// 检查并修复设备端的 ColorfulLedTool.jar
pub fn check_and_heal_colorful_led_tool(adb: &AdbDevice) -> Result<(), AdbError> {
    let local_path = get_local_colorful_led_tool_path();
    crate::device::jni::check_and_heal_remote_jar(adb, COLORFUL_LED_TOOL_JAR, local_path.as_deref())
}

/// 校验 ColorfulLedTool 执行输出，杜绝将 Parcel 假象或仅 shell 退出码 0 当作硬件生效
pub fn verify_colorful_led_output(out: &str) -> Result<(), AdbError> {
    let trimmed = out.trim();
    let lower = trimmed.to_lowercase();
    if lower.contains("exception") || lower.contains("error") || lower.contains("fatal") {
        return Err(AdbError::CommandFailed(format!(
            "ColorfulLedTool 异常: {}",
            trimmed
        )));
    }
    // TvService returns a Binder envelope, not the child's stdout (same as Python).
    // This confirms dispatch only; the UI must not claim LED hardware readback.
    if let Some(parcel) = trimmed.strip_prefix("Result: Parcel(") {
        if parcel.split_whitespace().next() == Some("00000000") && trimmed.ends_with(')') {
            return Ok(());
        }
    }
    if !trimmed.contains("SetColorfulLed") {
        return Err(AdbError::CommandFailed(format!(
            "ColorfulLedTool 未产生有效输出: {}",
            trimmed
        )));
    }
    if !trimmed.contains("ret=0") {
        return Err(AdbError::CommandFailed(format!(
            "ColorfulLedTool 返回非零状态: {}",
            trimmed
        )));
    }
    Ok(())
}

/// 执行 ColorfulLedTool 子命令
pub fn execute_colorful_led(
    adb: &AdbDevice,
    action: &str,
    args: &[&str],
) -> Result<String, AdbError> {
    check_and_heal_colorful_led_tool(adb)?;
    let mut cmd_parts = vec!["ColorfulLedTool", action];
    cmd_parts.extend_from_slice(args);

    let cmd = build_tvservice_app_process_command(REMOTE_JAR_CACHE_PATH, &cmd_parts);
    adb.shell(&cmd)
}

/// 提交屏幕灯完整状态 (包含系统设置写入与 Binder 命令下发，且严格校验 Jar 回执)
pub fn commit_screen_light(
    adb: &AdbDevice,
    params: &ScreenLightParameters,
) -> Result<(), AdbError> {
    let mode_val = params.mode.code();
    let ill_val = params.illumination.clamp(0, 14);
    let temp_val = params.color_temp.clamp(0, 2);
    let col_val = params.color_value.clamp(0, 4);

    adb.transaction(|| -> Result<(), AdbError> {
        adb.put_setting("atmosphere_light_switcher_pm2", &mode_val.to_string())?;
        adb.put_setting("atmosphere_light_illumination", &ill_val.to_string())?;
        adb.put_setting("atmosphere_light_color_temp", &temp_val.to_string())?;
        adb.put_setting("atmosphere_light_color_value", &col_val.to_string())?;

        let ill_str = ill_val.to_string();
        let temp_str = temp_val.to_string();
        let col_str = col_val.to_string();

        let out = match params.mode {
            ScreenLightMode::Lighting => {
                execute_colorful_led(adb, "lighting", &[&ill_str, &temp_str])?
            }
            ScreenLightMode::Ambient => execute_colorful_led(adb, "ambient", &[])?,
            ScreenLightMode::Solid => execute_colorful_led(adb, "solid", &[&ill_str, &col_str])?,
            ScreenLightMode::Cycle => execute_colorful_led(adb, "cycle", &[])?,
            ScreenLightMode::Off => execute_colorful_led(adb, "off", &[])?,
        };

        verify_colorful_led_output(&out)?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_light_mode_mappings() {
        assert_eq!(
            ScreenLightMode::try_from_code(0),
            Some(ScreenLightMode::Lighting)
        );
        assert_eq!(
            ScreenLightMode::try_from_code(1),
            Some(ScreenLightMode::Ambient)
        );
        assert_eq!(
            ScreenLightMode::try_from_code(2),
            Some(ScreenLightMode::Solid)
        );
        assert_eq!(
            ScreenLightMode::try_from_code(3),
            Some(ScreenLightMode::Cycle)
        );
        assert_eq!(
            ScreenLightMode::try_from_code(4),
            Some(ScreenLightMode::Off)
        );
        assert_eq!(ScreenLightMode::try_from_code(5), None);
    }

    #[test]
    fn test_verify_colorful_led_output_handling() {
        assert!(
            verify_colorful_led_output("Result: Parcel(00000000 00000001   '........')").is_ok()
        );
        assert!(verify_colorful_led_output("Result: Parcel(ffffffff 00000001)").is_err());
        // 成功回执
        assert!(verify_colorful_led_output("SetColorfulLed mode=11 color=22 ret=0\n").is_ok());
        // 非零退出
        assert!(verify_colorful_led_output("SetColorfulLed mode=11 color=22 ret=-1").is_err());
        assert!(verify_colorful_led_output("SetColorfulLed mode=0 color=1 ret=1").is_err());
        // 异常堆栈
        assert!(verify_colorful_led_output(
            "java.lang.RuntimeException: CreateConfigInterface failed"
        )
        .is_err());
        // 空输出或缺失标记
        assert!(verify_colorful_led_output("").is_err());
        assert!(verify_colorful_led_output("Killed").is_err());
    }

    #[test]
    fn test_screen_light_state_safety_check() {
        let mut state = ScreenLightState::default();
        assert!(!state.is_fully_known());
        assert!(state.to_parameters().is_none());

        state.mode = Some(ScreenLightMode::Lighting);
        state.illumination = Some(8);
        state.color_temp = Some(1);
        assert!(!state.is_fully_known());

        state.color_value = Some(0);
        assert!(state.is_fully_known());
        let params = state.to_parameters().unwrap();
        assert_eq!(params.mode, ScreenLightMode::Lighting);
        assert_eq!(params.illumination, 8);
        assert_eq!(params.color_temp, 1);
        assert_eq!(params.color_value, 0);
    }
}
