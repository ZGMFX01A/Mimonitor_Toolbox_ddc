use crate::adb::{AdbDevice, AdbError};
use serde::{Deserialize, Serialize};

/// 桌面场景，与 picture_mode 画面预设分别控制。
/// 编号核实自设备 TvHome 4.8.413 的 UserModeUtils/UserModeMgr。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DesktopMode {
    Standard = 0,
    Kids = 1,
    Elder = 2,
    Office = 3,
    Classic = 4,
}

impl DesktopMode {
    pub const SELECTABLE: [Self; 3] = [Self::Standard, Self::Elder, Self::Office];

    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Standard),
            1 => Some(Self::Kids),
            2 => Some(Self::Elder),
            3 => Some(Self::Office),
            4 => Some(Self::Classic),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "标准模式",
            Self::Kids => "儿童模式",
            Self::Elder => "长辈模式",
            Self::Office => "办公 / 会议模式",
            Self::Classic => "经典模式",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopModeState {
    pub mode: Option<DesktopMode>,
    pub raw_value: String,
}

impl DesktopModeState {
    fn parse(raw: &str) -> Self {
        let raw_value = raw.trim().to_string();
        let mode = raw_value.parse().ok().and_then(DesktopMode::from_id);
        Self { mode, raw_value }
    }
}

pub fn query_desktop_mode(adb: &AdbDevice) -> Result<DesktopModeState, AdbError> {
    adb.shell("settings get system user_mode")
        .map(|raw| DesktopModeState::parse(&raw))
}

fn switch_command(mode: DesktopMode) -> String {
    // 导出的 MainActivity alias 会初始化桌面；仅广播在标准模式销毁接收器后无效。
    // voicecontrol 路径保留系统的儿童模式密码检查。
    format!("am start -n com.mitv.tvhome/.MainActivity -a com.xiaomi.ACTION_MODE_CHANGE --ei user_mode {} --es from voicecontrol", mode as i32)
}

fn verify_switch_output(output: &str) -> Result<(), AdbError> {
    crate::device::source::verify_am_start_output(output)?;
    let lower = output.to_ascii_lowercase();
    if !lower.contains("starting: intent") && !lower.contains("warning: activity not started") {
        return Err(AdbError::CommandFailed(format!(
            "桌面模式切换未收到启动回执：{}",
            output.trim()
        )));
    }
    Ok(())
}

trait DesktopControl {
    fn read(&mut self) -> Result<DesktopModeState, AdbError>;
    fn send(&mut self, mode: DesktopMode) -> Result<(), AdbError>;
    fn wait(&mut self);
}

struct RealDesktopControl<'a>(&'a AdbDevice);
impl DesktopControl for RealDesktopControl<'_> {
    fn read(&mut self) -> Result<DesktopModeState, AdbError> {
        query_desktop_mode(self.0)
    }
    fn send(&mut self, mode: DesktopMode) -> Result<(), AdbError> {
        let output = self.0.shell(&switch_command(mode))?;
        verify_switch_output(&output)
    }
    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn switch_and_verify(
    control: &mut impl DesktopControl,
    target: DesktopMode,
) -> Result<DesktopModeState, AdbError> {
    if !DesktopMode::SELECTABLE.contains(&target) {
        return Err(AdbError::CommandFailed(
            "此模式请在显示器自带的模式菜单中选择".into(),
        ));
    }
    let current = control.read()?;
    if current.mode.is_none() {
        return Err(AdbError::CommandFailed(format!(
            "无法确认当前桌面模式（回读：{}），未发送切换指令",
            current.raw_value
        )));
    }
    if current.mode == Some(DesktopMode::Kids) {
        return Err(AdbError::CommandFailed(
            "请先在显示器上退出儿童模式，再切换桌面模式".into(),
        ));
    }
    if current.mode == Some(target) {
        return Ok(current);
    }
    control.send(target)?;
    for attempt in 0..10 {
        if attempt > 0 {
            control.wait();
        }
        let actual = control.read()?;
        if actual.mode == Some(target) {
            return Ok(actual);
        }
    }
    Err(AdbError::CommandFailed(
        "已发送桌面切换请求，但回读尚未确认；请检查显示器上的提示后刷新".into(),
    ))
}

pub fn set_desktop_mode(
    adb: &AdbDevice,
    target: DesktopMode,
) -> Result<DesktopModeState, AdbError> {
    adb.transaction(|| switch_and_verify(&mut RealDesktopControl(adb), target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct MockControl {
        readings: VecDeque<&'static str>,
        sent: Vec<DesktopMode>,
        waits: usize,
    }
    impl DesktopControl for MockControl {
        fn read(&mut self) -> Result<DesktopModeState, AdbError> {
            Ok(DesktopModeState::parse(
                self.readings.pop_front().unwrap_or("3"),
            ))
        }
        fn send(&mut self, mode: DesktopMode) -> Result<(), AdbError> {
            self.sent.push(mode);
            Ok(())
        }
        fn wait(&mut self) {
            self.waits += 1;
        }
    }
    fn mock(readings: &[&'static str]) -> MockControl {
        MockControl {
            readings: readings.iter().copied().collect(),
            sent: Vec::new(),
            waits: 0,
        }
    }

    #[test]
    fn desktop_ids_match_device_and_unknown_never_defaults() {
        assert_eq!(
            DesktopModeState::parse("3\r\n").mode,
            Some(DesktopMode::Office)
        );
        assert_eq!(DesktopModeState::parse("2").mode, Some(DesktopMode::Elder));
        for raw in ["", "null", "N/A", "99", "-1", "permission denied"] {
            assert_eq!(DesktopModeState::parse(raw).mode, None);
        }
        assert!(
            switch_command(DesktopMode::Office).contains("--ei user_mode 3 --es from voicecontrol")
        );
        let command = switch_command(DesktopMode::Elder);
        assert!(command.starts_with(
            "am start -n com.mitv.tvhome/.MainActivity -a com.xiaomi.ACTION_MODE_CHANGE"
        ));
        assert!(!command.contains("settings put"));
    }
    #[test]
    fn switch_succeeds_only_after_matching_readback() {
        let mut control = mock(&["3", "3", "2"]);
        assert_eq!(
            switch_and_verify(&mut control, DesktopMode::Elder)
                .unwrap()
                .mode,
            Some(DesktopMode::Elder)
        );
        assert_eq!(control.sent, [DesktopMode::Elder]);
        assert_eq!(control.waits, 1);
    }
    #[test]
    fn request_acceptance_without_mode_change_is_a_failure() {
        let mut control = mock(&["3"]);
        assert!(switch_and_verify(&mut control, DesktopMode::Elder).is_err());
        assert_eq!(control.sent, [DesktopMode::Elder]);
        assert_eq!(control.waits, 9);
    }
    #[test]
    fn unknown_or_locked_mode_does_not_send_a_switch() {
        for raw in ["null", "99", "1"] {
            let mut control = mock(&[raw]);
            assert!(switch_and_verify(&mut control, DesktopMode::Office).is_err());
            assert!(control.sent.is_empty());
        }
    }
    #[test]
    fn already_selected_mode_does_not_send_another_broadcast() {
        let mut control = mock(&["3"]);
        assert_eq!(
            switch_and_verify(&mut control, DesktopMode::Office)
                .unwrap()
                .mode,
            Some(DesktopMode::Office)
        );
        assert!(control.sent.is_empty());
    }

    #[test]
    fn launcher_acknowledgement_distinguishes_success_warning_and_failure() {
        assert!(
            verify_switch_output("Starting: Intent { act=com.xiaomi.ACTION_MODE_CHANGE }").is_ok()
        );
        assert!(verify_switch_output("Warning: Activity not started, intent has been delivered to currently running top-most instance.").is_ok());
        for output in [
            "",
            "Error type 3: Activity class does not exist.",
            "java.lang.SecurityException: Permission Denial",
        ] {
            assert!(verify_switch_output(output).is_err());
        }
    }
}
