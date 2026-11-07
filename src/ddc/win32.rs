use std::ffi::c_void;

pub const VCP_CODE_LUMINANCE: u8 = 0x10;

// Win32 I2C / DDC/CI 瞬态错误代码
pub const ERROR_GRAPHICS_I2C_TRANSMIT: u32 = 0xC0262582;
pub const ERROR_GRAPHICS_I2C_RECEIVE: u32 = 0xC0262583;
pub const ERROR_GRAPHICS_DDCCI_TIMING: u32 = 0xC0262584;
pub const ERROR_GRAPHICS_DDCCI_COMMAND: u32 = 0xC0262585;
pub const ERROR_GRAPHICS_DDCCI_LENGTH: u32 = 0xC0262586;
pub const ERROR_GRAPHICS_DDCCI_CHECKSUM: u32 = 0xC0262587;

pub const DISPLAY_DEVICE_ATTACHED_TO_DESKTOP: u32 = 0x00000001;
pub const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 0x00000001;

pub type BOOL = i32;
pub type DWORD = u32;
pub type BYTE = u8;
pub type HANDLE = *mut c_void;
pub type HMONITOR = *mut c_void;
pub type HDC = *mut c_void;
pub type LPARAM = isize;
pub type WCHAR = u16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MONITORINFOEXW {
    pub cb_size: DWORD,
    pub rc_monitor: RECT,
    pub rc_work: RECT,
    pub dw_flags: DWORD,
    pub sz_device: [WCHAR; 32],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DISPLAY_DEVICEW {
    pub cb: DWORD,
    pub device_name: [WCHAR; 32],
    pub device_string: [WCHAR; 128],
    pub state_flags: DWORD,
    pub device_id: [WCHAR; 128],
    pub device_key: [WCHAR; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PHYSICAL_MONITOR {
    pub h_physical_monitor: HANDLE,
    pub sz_physical_monitor_description: [WCHAR; 128],
}

pub type MONITORENUMPROC = unsafe extern "system" fn(
    hmonitor: HMONITOR,
    hdc: HDC,
    lprect: *mut RECT,
    lparam: LPARAM,
) -> BOOL;

#[link(name = "user32")]
extern "system" {
    pub fn EnumDisplayMonitors(
        hdc: HDC,
        lprc_clip: *const RECT,
        lpfn_enum: Option<MONITORENUMPROC>,
        dw_data: LPARAM,
    ) -> BOOL;

    pub fn GetMonitorInfoW(h_monitor: HMONITOR, lpmi: *mut MONITORINFOEXW) -> BOOL;

    pub fn EnumDisplayDevicesW(
        lp_device: *const WCHAR,
        i_dev_num: DWORD,
        lp_display_device: *mut DISPLAY_DEVICEW,
        dw_flags: DWORD,
    ) -> BOOL;
}

#[link(name = "dxva2")]
extern "system" {
    pub fn GetNumberOfPhysicalMonitorsFromHMONITOR(
        h_monitor: HMONITOR,
        pdw_number_of_physical_monitors: *mut DWORD,
    ) -> BOOL;

    pub fn GetPhysicalMonitorsFromHMONITOR(
        h_monitor: HMONITOR,
        dw_physical_monitor_array_size: DWORD,
        p_physical_monitor_array: *mut PHYSICAL_MONITOR,
    ) -> BOOL;

    pub fn DestroyPhysicalMonitors(
        dw_physical_monitor_array_size: DWORD,
        p_physical_monitor_array: *mut PHYSICAL_MONITOR,
    ) -> BOOL;

    pub fn DestroyPhysicalMonitor(h_monitor: HANDLE) -> BOOL;

    pub fn GetMonitorBrightness(
        h_monitor: HANDLE,
        pdw_minimum_brightness: *mut DWORD,
        pdw_current_brightness: *mut DWORD,
        pdw_maximum_brightness: *mut DWORD,
    ) -> BOOL;

    pub fn SetMonitorBrightness(h_monitor: HANDLE, dw_new_brightness: DWORD) -> BOOL;

    pub fn GetVCPFeatureAndVCPFeatureReply(
        h_monitor: HANDLE,
        b_vcp_code: BYTE,
        pvct: *mut DWORD,
        pdw_current_value: *mut DWORD,
        pdw_maximum_value: *mut DWORD,
    ) -> BOOL;

    pub fn SetVCPFeature(h_monitor: HANDLE, b_vcp_code: BYTE, dw_new_value: DWORD) -> BOOL;
}

/// 判断是否为可重试的 I2C/DDC 瞬态错误代码
pub fn is_transient_ddc_error(error_code: u32) -> bool {
    matches!(
        error_code,
        ERROR_GRAPHICS_I2C_TRANSMIT
            | ERROR_GRAPHICS_I2C_RECEIVE
            | ERROR_GRAPHICS_DDCCI_TIMING
            | ERROR_GRAPHICS_DDCCI_COMMAND
            | ERROR_GRAPHICS_DDCCI_LENGTH
            | ERROR_GRAPHICS_DDCCI_CHECKSUM
    )
}
