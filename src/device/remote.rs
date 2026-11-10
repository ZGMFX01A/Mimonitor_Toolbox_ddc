use crate::adb::{AdbDevice, AdbError};

/// Android 虚拟遥控器 Keycode 常量
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RemoteKey {
    DpadUp = 19,
    DpadDown = 20,
    DpadLeft = 21,
    DpadRight = 22,
    DpadCenter = 23, // 确认 / OK
    Back = 4,
    Home = 3,
    Menu = 82,
    VolumeUp = 24,
    VolumeDown = 25,
    VolumeMute = 164,
    Power = 26,
}

impl RemoteKey {
    pub fn all() -> &'static [RemoteKey] {
        &[
            RemoteKey::Power,
            RemoteKey::Home,
            RemoteKey::Menu,
            RemoteKey::Back,
            RemoteKey::DpadUp,
            RemoteKey::DpadDown,
            RemoteKey::DpadLeft,
            RemoteKey::DpadRight,
            RemoteKey::DpadCenter,
            RemoteKey::VolumeDown,
            RemoteKey::VolumeMute,
            RemoteKey::VolumeUp,
        ]
    }

    pub fn code(&self) -> u32 {
        *self as u32
    }

    pub fn label(&self) -> &'static str {
        match self {
            RemoteKey::DpadUp => "上",
            RemoteKey::DpadDown => "下",
            RemoteKey::DpadLeft => "左",
            RemoteKey::DpadRight => "右",
            RemoteKey::DpadCenter => "OK / 确定",
            RemoteKey::Back => "返回",
            RemoteKey::Home => "主页",
            RemoteKey::Menu => "菜单",
            RemoteKey::VolumeUp => "音量 +",
            RemoteKey::VolumeDown => "音量 -",
            RemoteKey::VolumeMute => "静音",
            RemoteKey::Power => "电源",
        }
    }
}

/// 校验 input keyevent 调用的输出，识别静默错误（识别 Error / Exception / Permission denial）
pub fn verify_input_cmd_output(output: &str) -> Result<(), AdbError> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("error") || lower.contains("exception") || lower.contains("permission denial")
    {
        return Err(AdbError::CommandFailed(format!(
            "遥控按键发送失败: {}",
            output.trim()
        )));
    }
    Ok(())
}

/// 发送虚拟遥控器按键事件
pub fn send_keyevent(adb: &AdbDevice, key: RemoteKey) -> Result<(), AdbError> {
    let cmd = format!("input keyevent {}", key.code());
    let out = adb.shell(&cmd)?;
    verify_input_cmd_output(&out)?;
    Ok(())
}

/// 发送自定义 Keycode
pub fn send_raw_keycode(adb: &AdbDevice, keycode: u32) -> Result<(), AdbError> {
    let cmd = format!("input keyevent {}", keycode);
    let out = adb.shell(&cmd)?;
    verify_input_cmd_output(&out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_input_cmd_output() {
        assert!(verify_input_cmd_output("").is_ok());
        assert!(verify_input_cmd_output("KbdEvent sent").is_ok());

        assert!(verify_input_cmd_output("Error: Unknown command: keyevent").is_err());
        assert!(verify_input_cmd_output(
            "java.lang.SecurityException: Permission denial: inject event"
        )
        .is_err());
        assert!(verify_input_cmd_output("Exception occurred while injecting").is_err());
    }

    #[test]
    fn test_remote_keys_count_and_chinese_labels() {
        let all_keys = RemoteKey::all();
        // 必须严格覆盖全部 12 键
        assert_eq!(all_keys.len(), 12);

        // 验证各键码与中文标签映射
        assert_eq!(RemoteKey::Power.code(), 26);
        assert_eq!(RemoteKey::Power.label(), "电源");

        assert_eq!(RemoteKey::Home.code(), 3);
        assert_eq!(RemoteKey::Home.label(), "主页");

        assert_eq!(RemoteKey::Menu.code(), 82);
        assert_eq!(RemoteKey::Menu.label(), "菜单");

        assert_eq!(RemoteKey::Back.code(), 4);
        assert_eq!(RemoteKey::Back.label(), "返回");

        assert_eq!(RemoteKey::DpadUp.code(), 19);
        assert_eq!(RemoteKey::DpadUp.label(), "上");

        assert_eq!(RemoteKey::DpadDown.code(), 20);
        assert_eq!(RemoteKey::DpadDown.label(), "下");

        assert_eq!(RemoteKey::DpadLeft.code(), 21);
        assert_eq!(RemoteKey::DpadLeft.label(), "左");

        assert_eq!(RemoteKey::DpadRight.code(), 22);
        assert_eq!(RemoteKey::DpadRight.label(), "右");

        assert_eq!(RemoteKey::DpadCenter.code(), 23);
        assert!(RemoteKey::DpadCenter.label().contains("确定"));

        assert_eq!(RemoteKey::VolumeDown.code(), 25);
        assert!(RemoteKey::VolumeDown.label().contains("音量"));

        assert_eq!(RemoteKey::VolumeMute.code(), 164);
        assert_eq!(RemoteKey::VolumeMute.label(), "静音");

        assert_eq!(RemoteKey::VolumeUp.code(), 24);
        assert!(RemoteKey::VolumeUp.label().contains("音量"));
    }
}
