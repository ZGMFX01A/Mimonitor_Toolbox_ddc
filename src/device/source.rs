use crate::adb::{AdbDevice, AdbError};
use crate::device::jni::refresh_pq;
use serde::{Deserialize, Serialize};

/// 视频输入信号源枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoSource {
    Hdmi1 = 23,
    Hdmi2 = 24,
    Dp = 29,
    TypeC = 30,
}

impl VideoSource {
    pub fn all() -> &'static [VideoSource] {
        &[
            VideoSource::Hdmi1,
            VideoSource::Hdmi2,
            VideoSource::Dp,
            VideoSource::TypeC,
        ]
    }

    pub fn id(&self) -> u32 {
        *self as u32
    }

    pub fn name(&self) -> &'static str {
        match self {
            VideoSource::Hdmi1 => "HDMI 1",
            VideoSource::Hdmi2 => "HDMI 2",
            VideoSource::Dp => "DP",
            VideoSource::TypeC => "Type-C (USBC)",
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            23 => Some(VideoSource::Hdmi1),
            24 => Some(VideoSource::Hdmi2),
            29 => Some(VideoSource::Dp),
            30 => Some(VideoSource::TypeC),
            _ => None,
        }
    }
}

/// 查询当前设备处于活动的输入信号源
pub fn get_current_source(adb: &AdbDevice) -> Result<Option<VideoSource>, AdbError> {
    let out = adb.get_setting("mitv.tvplayer.hdmi.last.source")?;
    if let Ok(id) = out.trim().parse::<u32>() {
        Ok(VideoSource::from_id(id))
    } else {
        Ok(None)
    }
}

/// 读取当前信号源及其原始字符串 (未知不能默认)
pub fn get_source_with_raw(adb: &AdbDevice) -> Result<(Option<VideoSource>, String), AdbError> {
    let out = adb.get_setting("mitv.tvplayer.hdmi.last.source")?;
    let trimmed = out.trim().to_string();
    let src = trimmed.parse::<u32>().ok().and_then(VideoSource::from_id);
    Ok((src, trimmed))
}

/// 校验 am start 调用的输出，识别常见的静默错误（识别 Error / Exception / Permission denial）
pub fn verify_am_start_output(output: &str) -> Result<(), AdbError> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("error") || lower.contains("exception") || lower.contains("permission denial")
    {
        return Err(AdbError::CommandFailed(format!(
            "am start 启动失败: {}",
            output.trim()
        )));
    }
    Ok(())
}

/// 切换显示器信号源（严格遵循 force-stop -> am start -> refresh_pq，步骤整体置于 adb.transaction 排他保护）
pub fn switch_source(adb: &AdbDevice, target: VideoSource) -> Result<(), AdbError> {
    let source_id = target.id();
    crate::logger::log_device(&format!(
        "开始切换输入信号源至: {} ({})",
        target.name(),
        source_id
    ));

    // 整个切源过程置于 adb.transaction 保护，防止与背光调节或其他并发 ADB 命令交错
    adb.transaction(|| {
        // 1. 强行终止当前的 tvplayer 播放器实例（错误必须如实传播）
        adb.shell("am force-stop com.xiaomi.mitv.tvplayer")?;

        // 2. 启动 ExternalSourceActivity 唤起新源管道（严禁先 put last.source 伪造成功，并对输出进行错误识别）
        let start_cmd = format!(
            "am start -a com.xiaomi.mitv.tvplayer.EXTSRC_PLAY -n com.xiaomi.mitv.tvplayer/.ExternalSourceActivity --ei input {} -f 0x10000000",
            source_id
        );
        let out = adb.shell(&start_cmd)?;
        verify_am_start_output(&out)?;

        // 3. 触发画质模式重载广播刷新硬件显示链路
        refresh_pq(adb)?;

        Ok(())
    })?;

    crate::logger::log_device(&format!("信号源切换指令已发送: {}", target.name()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_am_start_output() {
        // 正常输出
        assert!(verify_am_start_output(
            "Starting: Intent { act=com.xiaomi.mitv.tvplayer.EXTSRC_PLAY }"
        )
        .is_ok());
        assert!(verify_am_start_output("Warning: Activity not started, intent has been delivered to currently running top-most instance.").is_ok());

        // 包含 Error 的异常输出
        assert!(verify_am_start_output(
            "Error: Activity not started, unable to resolve Intent { ... }"
        )
        .is_err());
        assert!(verify_am_start_output("Error type 3: Activity class does not exist.").is_err());

        // 包含 Exception 的异常输出
        assert!(verify_am_start_output(
            "java.lang.SecurityException: Permission Denial: starting Intent"
        )
        .is_err());
        assert!(verify_am_start_output(
            "Exception occurred while executing 'start':\njava.lang.IllegalArgumentException"
        )
        .is_err());

        // 包含 Permission denial 的异常输出
        assert!(
            verify_am_start_output("Permission denial: check calling permission failed").is_err()
        );
    }

    #[test]
    fn test_video_source_mapping_and_names() {
        assert_eq!(VideoSource::Hdmi1.id(), 23);
        assert_eq!(VideoSource::Hdmi2.id(), 24);
        assert_eq!(VideoSource::Dp.id(), 29);
        assert_eq!(VideoSource::TypeC.id(), 30);

        assert_eq!(VideoSource::from_id(23), Some(VideoSource::Hdmi1));
        assert_eq!(VideoSource::from_id(24), Some(VideoSource::Hdmi2));
        assert_eq!(VideoSource::from_id(29), Some(VideoSource::Dp));
        assert_eq!(VideoSource::from_id(30), Some(VideoSource::TypeC));

        assert_eq!(VideoSource::all().len(), 4);
    }

    #[test]
    fn test_video_source_unknown_does_not_default() {
        // 关键约束：未知源绝不能默认到任何端口
        assert_eq!(VideoSource::from_id(0), None);
        assert_eq!(VideoSource::from_id(1), None);
        assert_eq!(VideoSource::from_id(22), None);
        assert_eq!(VideoSource::from_id(25), None);
        assert_eq!(VideoSource::from_id(28), None);
        assert_eq!(VideoSource::from_id(31), None);
        assert_eq!(VideoSource::from_id(999), None);
    }
}
