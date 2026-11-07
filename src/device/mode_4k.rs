use crate::adb::{AdbDevice, AdbError};

/// 4K UI 显示模式信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiResolutionInfo {
    /// 当前生效的 UI 分辨率是否达到 4K (3840x2160 或以上)
    pub is_4k: bool,
    /// 当前生效的水平宽度 (像素)
    pub active_width: u32,
    /// 当前生效的垂直高度 (像素)
    pub active_height: u32,
    /// 当前生效的像素密度 DPI
    pub active_density: u32,
    /// 物理原生分辨率 (宽, 高)
    pub physical_size: Option<(u32, u32)>,
    /// 用户覆盖分辨率 (宽, 高)
    pub override_size: Option<(u32, u32)>,
    /// 物理原生密度 DPI
    pub physical_density: Option<u32>,
    /// 用户覆盖密度 DPI
    pub override_density: Option<u32>,
    /// 底层 wm size 原始输出
    pub raw_size: String,
    /// 底层 wm density 原始输出
    pub raw_density: String,
}

/// 解析 `wm size` 输出：严格遵循 Override 优先于 Physical 的原则
/// 若两项均未获取或解析失败，返回 AdbError::CommandFailed，绝不默认或静默伪装
pub fn parse_wm_size(
    raw: &str,
) -> Result<(u32, u32, Option<(u32, u32)>, Option<(u32, u32)>), AdbError> {
    let mut physical = None;
    let mut override_val = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Override size:") {
            if let Some((w, h)) = parse_wh(rest.trim()) {
                override_val = Some((w, h));
            }
        } else if let Some(rest) = trimmed.strip_prefix("Physical size:") {
            if let Some((w, h)) = parse_wh(rest.trim()) {
                physical = Some((w, h));
            }
        }
    }

    // 优先判定 Override，其次回退至 Physical；若都不存在则解析失败
    let (active_w, active_h) = if let Some(o) = override_val {
        o
    } else if let Some(p) = physical {
        p
    } else {
        return Err(AdbError::CommandFailed(format!(
            "无法解析 wm size 输出，未找到 Physical 或 Override 分辨率: {:?}",
            raw.trim()
        )));
    };

    Ok((active_w, active_h, physical, override_val))
}

/// 辅助解析形如 "3840x2160" 的分辨率文本
fn parse_wh(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() == 2 {
        if let (Ok(w), Ok(h)) = (
            parts[0].trim().parse::<u32>(),
            parts[1].trim().parse::<u32>(),
        ) {
            return Some((w, h));
        }
    }
    None
}

/// 解析 `wm density` 输出：严格遵循 Override 优先于 Physical 的原则
/// 若两项均未获取或解析失败，返回 AdbError::CommandFailed
pub fn parse_wm_density(raw: &str) -> Result<(u32, Option<u32>, Option<u32>), AdbError> {
    let mut physical = None;
    let mut override_val = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Override density:") {
            if let Ok(d) = rest.trim().parse::<u32>() {
                override_val = Some(d);
            }
        } else if let Some(rest) = trimmed.strip_prefix("Physical density:") {
            if let Ok(d) = rest.trim().parse::<u32>() {
                physical = Some(d);
            }
        }
    }

    let active_d = if let Some(o) = override_val {
        o
    } else if let Some(p) = physical {
        p
    } else {
        return Err(AdbError::CommandFailed(format!(
            "无法解析 wm density 输出，未找到 Physical 或 Override 密度: {:?}",
            raw.trim()
        )));
    };

    Ok((active_d, physical, override_val))
}

/// 校验 wm 子命令输出：防止 shell 返回 0 但实际上输出包含错误或异常信息
fn validate_wm_output(cmd: &str, output: &str) -> Result<(), AdbError> {
    let lower = output.to_lowercase();
    if lower.contains("error")
        || lower.contains("exception")
        || lower.contains("bad")
        || lower.contains("unknown")
        || lower.contains("denied")
        || lower.contains("killed")
        || lower.contains("usage:")
    {
        return Err(AdbError::CommandFailed(format!(
            "命令 '{}' 返回失败输出: {}",
            cmd,
            output.trim()
        )));
    }
    Ok(())
}

/// 查询设备当前 UI 渲染分辨率与密度 (Override 优先于 Physical)
pub fn query_ui_resolution(adb: &AdbDevice) -> Result<UiResolutionInfo, AdbError> {
    adb.transaction(|| {
        let size_out = adb.shell("wm size")?;
        let density_out = adb.shell("wm density")?;

        let (active_w, active_h, physical_size, override_size) = parse_wm_size(&size_out)?;
        let (active_density, physical_density, override_density) = parse_wm_density(&density_out)?;

        // 生效分辨率达到 3840x2160 时判定为 4K UI 模式
        let is_4k = active_w >= 3840 && active_h >= 2160;

        Ok(UiResolutionInfo {
            is_4k,
            active_width: active_w,
            active_height: active_h,
            active_density,
            physical_size,
            override_size,
            physical_density,
            override_density,
            raw_size: size_out.trim().to_string(),
            raw_density: density_out.trim().to_string(),
        })
    })
}

/// 启用 4K 原生渲染 UI (3840x2160, density 640)
/// 多步事务执行，若任一步输出异常则立即中断并回传错误。
/// 注意：本函数仅设置分辨率与密度，不执行重启操作；重启需由 UI 交互确认后调用 `reboot_device`
pub fn enable_4k_ui(adb: &AdbDevice) -> Result<(), AdbError> {
    crate::logger::log_device("正在开启 4K UI (wm size 3840x2160, density 640)...");
    adb.transaction(|| {
        let out_size = adb.shell("wm size 3840x2160")?;
        validate_wm_output("wm size 3840x2160", &out_size)?;

        let out_density = adb.shell("wm density 640")?;
        validate_wm_output("wm density 640", &out_density)?;

        Ok(())
    })
}

/// 切换至 1080P UI (1920x1080, density 320)
/// 多步事务执行，若任一步输出异常则立即中断并回传错误。
/// 注意：本函数仅设置分辨率与密度，不执行重启操作；重启需由 UI 交互确认后调用 `reboot_device`
pub fn set_1080p_ui(adb: &AdbDevice) -> Result<(), AdbError> {
    crate::logger::log_device("正在切换至 1080P UI (wm size 1920x1080, density 320)...");
    adb.transaction(|| {
        let out_size = adb.shell("wm size 1920x1080")?;
        validate_wm_output("wm size 1920x1080", &out_size)?;

        let out_density = adb.shell("wm density 320")?;
        validate_wm_output("wm density 320", &out_density)?;

        Ok(())
    })
}

/// 重置 UI 分辨率与密度为设备出厂默认值
pub fn reset_ui_resolution(adb: &AdbDevice) -> Result<(), AdbError> {
    crate::logger::log_device("正在重置 UI 分辨率与密度为系统默认值...");
    adb.transaction(|| {
        let out_size = adb.shell("wm size reset")?;
        validate_wm_output("wm size reset", &out_size)?;

        let out_density = adb.shell("wm density reset")?;
        validate_wm_output("wm density reset", &out_density)?;

        Ok(())
    })
}

/// 向设备发送重启命令
/// 独立 API，由上层 UI 弹窗确认后再明确触发
pub fn reboot_device(adb: &AdbDevice) -> Result<(), AdbError> {
    crate::logger::log_device("正在向设备发送 reboot 重启命令...");
    let out = adb.shell("reboot")?;
    let lower = out.to_lowercase();
    if lower.contains("error") || lower.contains("denied") || lower.contains("exception") {
        return Err(AdbError::CommandFailed(format!(
            "reboot 命令执行失败: {}",
            out.trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wm_size_override_priority_over_physical() {
        // Physical 虽为 3840x2160，但 Override 为 1920x1080，必须以 Override 1920x1080 为准，且判定不是 4K
        let raw = "Physical size: 3840x2160\nOverride size: 1920x1080\n";
        let (w, h, phys, ovr) = parse_wm_size(raw).expect("解析成功");
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
        assert_eq!(phys, Some((3840, 2160)));
        assert_eq!(ovr, Some((1920, 1080)));
        assert!(!(w >= 3840 && h >= 2160));
    }

    #[test]
    fn test_parse_wm_size_override_4k_over_physical_1080p() {
        // Physical 为 1920x1080，Override 为 3840x2160，必须以 Override 为准，且判定为 4K
        let raw = "Physical size: 1920x1080\r\nOverride size: 3840x2160\r\n";
        let (w, h, phys, ovr) = parse_wm_size(raw).expect("解析成功");
        assert_eq!(w, 3840);
        assert_eq!(h, 2160);
        assert_eq!(phys, Some((1920, 1080)));
        assert_eq!(ovr, Some((3840, 2160)));
        assert!(w >= 3840 && h >= 2160);
    }

    #[test]
    fn test_parse_wm_size_physical_only() {
        let raw_1080p = "Physical size: 1920x1080";
        let (w, h, phys, ovr) = parse_wm_size(raw_1080p).expect("解析成功");
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
        assert_eq!(phys, Some((1920, 1080)));
        assert_eq!(ovr, None);

        let raw_4k = "Physical size: 3840x2160";
        let (w4, h4, phys4, ovr4) = parse_wm_size(raw_4k).expect("解析成功");
        assert_eq!(w4, 3840);
        assert_eq!(h4, 2160);
        assert_eq!(phys4, Some((3840, 2160)));
        assert_eq!(ovr4, None);
    }

    #[test]
    fn test_parse_wm_size_invalid_returns_err() {
        let raw_empty = "";
        assert!(parse_wm_size(raw_empty).is_err());

        let raw_garbage = "Error: unable to get display size\nUsage: wm [subcommand]";
        assert!(parse_wm_size(raw_garbage).is_err());
    }

    #[test]
    fn test_parse_wm_density_override_priority() {
        let raw = "Physical density: 320\nOverride density: 640";
        let (d, phys, ovr) = parse_wm_density(raw).expect("解析成功");
        assert_eq!(d, 640);
        assert_eq!(phys, Some(320));
        assert_eq!(ovr, Some(640));
    }

    #[test]
    fn test_parse_wm_density_physical_only() {
        let raw = "Physical density: 320";
        let (d, phys, ovr) = parse_wm_density(raw).expect("解析成功");
        assert_eq!(d, 320);
        assert_eq!(phys, Some(320));
        assert_eq!(ovr, None);
    }

    #[test]
    fn test_parse_wm_density_invalid_returns_err() {
        assert!(parse_wm_density("density broken").is_err());
    }

    #[test]
    fn test_validate_wm_output_catches_errors() {
        assert!(validate_wm_output("wm size", "").is_ok());
        assert!(validate_wm_output("wm size", "Override size: 3840x2160").is_ok());
        assert!(validate_wm_output("wm size", "Error: Bad size").is_err());
        assert!(validate_wm_output("wm size", "java.lang.IllegalArgumentException").is_err());
        assert!(validate_wm_output("wm size", "Permission denied").is_err());
    }
}
