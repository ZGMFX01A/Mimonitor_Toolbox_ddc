/// 检测设备标识符是否属于小米 / Redmi 显示器（绝对排除在 DDC/CI 控制之外）
/// 小米显示器（如 XMI3009, XMI27B3, XMI300A）统一使用 ADB 协议控制背光
pub fn is_xiaomi_device_id(device_id_or_name: &str) -> bool {
    let upper = device_id_or_name.to_uppercase();
    upper.contains("XMI") || upper.contains("REDMI") || upper.contains("XIAOMI")
}

/// 检查物理监视器数量与多屏共享约束
/// 如果一个 HMONITOR 对应多个物理显示器，或者多个 GDI 端口共享同一设备，判定为歧义复合输出并安全拒绝
pub fn is_ambiguous_monitor_layout(physical_count: u32, is_shared_output: bool) -> bool {
    physical_count != 1 || is_shared_output
}

/// 从设备接口名中提取 EDID 产品码（例如 `\\?\DISPLAY#XMI3009#5&...` -> `XMI3009`）
pub fn display_product_code(device_id: &str) -> String {
    let parts: Vec<&str> = device_id.split('#').collect();
    if parts.len() > 1 {
        parts[1].trim().to_string()
    } else {
        String::new()
    }
}

/// 通用占位监视器名称集合（避免界面显示无意义的通用噪音文本）
pub const GENERIC_MONITOR_NAMES: &[&str] = &[
    "generic pnp monitor",
    "generic monitor",
    "generic non-pnp monitor",
    "default monitor",
    "通用即插即用监视器",
    "通用非即插即用监视器",
    "显示器",
];

/// 格式化供界面下拉或列表显示的友好名称
pub fn format_display_label(device_string: &str, device_id: &str, slot_label: &str) -> String {
    let name = device_string.trim();
    let lower_name = name.to_lowercase();
    let mut parts = Vec::new();

    if !name.is_empty() && !GENERIC_MONITOR_NAMES.iter().any(|&g| lower_name == g) {
        parts.push(name.to_string());
    }

    let code = display_product_code(device_id);
    if !code.is_empty() && (parts.is_empty() || parts[0] != code) {
        parts.push(code);
    }

    if !slot_label.is_empty() {
        parts.push(slot_label.to_string());
    }

    if parts.is_empty() {
        "外接显示器".to_string()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_xiaomi_device_detection() {
        assert!(is_xiaomi_device_id(
            r"\\?\DISPLAY#XMI3009#5&382902b&0&UID256#{e6f07b4f-ee26-4604-9882-02e0b647323b}"
        ));
        assert!(is_xiaomi_device_id(r"DISPLAY\XMI300A"));
        assert!(is_xiaomi_device_id("Redmi G Pro 27"));
        assert!(is_xiaomi_device_id("Xiaomi Monitor"));

        // 普通外接显示器
        assert!(!is_xiaomi_device_id(
            r"\\?\DISPLAY#YHY1600#5&382902b&0&UID256#{e6f07b4f-ee26-4604-9882-02e0b647323b}"
        ));
        assert!(!is_xiaomi_device_id("DELL U2720Q"));
        assert!(!is_xiaomi_device_id("LG 27GP850"));
    }

    #[test]
    fn test_is_ambiguous_layout() {
        // 单物理屏无共享：安全
        assert!(!is_ambiguous_monitor_layout(1, false));

        // 物理屏数量为 0 或 2：歧义拒绝
        assert!(is_ambiguous_monitor_layout(0, false));
        assert!(is_ambiguous_monitor_layout(2, false));

        // 端口多设备共享：歧义拒绝
        assert!(is_ambiguous_monitor_layout(1, true));
    }

    #[test]
    fn test_display_product_code_and_label() {
        let dev_id = r"\\?\DISPLAY#YHY1600#5&123456#{guid}";
        assert_eq!(display_product_code(dev_id), "YHY1600");

        let label = format_display_label("通用即插即用监视器", dev_id, "屏幕 2");
        assert_eq!(label, "YHY1600 · 屏幕 2");

        let brand_label = format_display_label("UltraSharp", dev_id, "屏幕 1");
        assert_eq!(brand_label, "UltraSharp · YHY1600 · 屏幕 1");
    }
}
