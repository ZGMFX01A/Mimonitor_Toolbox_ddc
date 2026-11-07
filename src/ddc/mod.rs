pub mod edid;
pub mod monitor;
pub mod safe_filter;
pub mod win32;

pub use monitor::{
    denormalize_brightness, enumerate_ddc_monitors, enumerate_gdi_monitors,
    get_ddc_device_coalescer, list_raw_windows_displays, next_ddc_revision, normalize_brightness,
    read_physical_monitor_brightness, set_ddc_brightness_safe, write_physical_monitor_brightness,
    DdcDeviceWriteCoalescer, DdcMonitorInfo, PhysicalMonitorsGuard, RawDisplayItem,
    DDC_WRITE_REVISION, DEFAULT_DDC_ADAPTER,
};
pub use safe_filter::{
    display_product_code, format_display_label, is_ambiguous_monitor_layout, is_xiaomi_device_id,
};
pub use win32::*;

/// 向指定普通外接显示器发送硬件亮度调节指令
pub fn set_ddc_brightness(monitor_id: &str, brightness_pct: u32) -> Result<(), String> {
    set_ddc_brightness_safe(monitor_id, brightness_pct)
}
