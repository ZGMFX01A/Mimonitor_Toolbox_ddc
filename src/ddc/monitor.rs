use crate::ddc::safe_filter::{format_display_label, is_xiaomi_device_id};
use crate::ddc::win32::*;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// 归一化物理亮度为 0-100% 整数百分比（处理 cur<min, cur>max, max<=min 及 u32 边界，杜绝下溢与除零）
pub fn normalize_brightness(cur_val: u32, min_val: u32, max_val: u32) -> u32 {
    if max_val <= min_val {
        return 0;
    }
    if cur_val <= min_val {
        return 0;
    }
    if cur_val >= max_val {
        return 100;
    }
    let range = (max_val - min_val) as u64;
    let delta = (cur_val - min_val) as u64;
    let pct = (delta * 100 + range / 2) / range;
    (pct as u32).min(100)
}

/// 将 0-100% 百分比反归一化为硬件底层寄存器数值（处理 u32 边界并四舍五入）
pub fn denormalize_brightness(pct: u32, min_val: u32, max_val: u32) -> u32 {
    let clamped_pct = pct.min(100) as u64;
    if max_val <= min_val {
        return min_val;
    }
    let range = (max_val - min_val) as u64;
    let raw = min_val as u64 + (clamped_pct * range + 50) / 100;
    (raw as u32).min(max_val)
}

/// 普通外接显示器元数据与亮度状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DdcMonitorInfo {
    pub device_id: String,
    pub device_name: String,
    pub label: String,
    pub min_brightness: u32,
    pub current_brightness: u32,
    pub max_brightness: u32,
    pub current_percent: u32,
    pub is_vcp_fallback: bool,
    pub is_supported: bool,
    pub write_revision: u64,
}

/// 原始 Windows 显示器枚举项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDisplayItem {
    pub device_name: String,
    pub device_id: String,
    pub device_string: String,
    pub label: String,
}

/// 句柄释放钩子类型 (用于测试中验证 RAII 自动释放)
type DropHook = Box<dyn Fn(u32) + Send + Sync>;

/// RAII 保护的物理监视器句柄包装，确保生命周期结束时严格调用 DestroyPhysicalMonitors 释放
pub struct PhysicalMonitorsGuard {
    pub count: u32,
    pub monitors: Vec<PHYSICAL_MONITOR>,
    drop_hook: Option<DropHook>,
}

impl PhysicalMonitorsGuard {
    pub fn new(count: u32, monitors: Vec<PHYSICAL_MONITOR>) -> Self {
        Self {
            count,
            monitors,
            drop_hook: None,
        }
    }

    pub fn with_hook(count: u32, monitors: Vec<PHYSICAL_MONITOR>, hook: DropHook) -> Self {
        Self {
            count,
            monitors,
            drop_hook: Some(hook),
        }
    }

    pub fn handle(&self, index: usize) -> Option<HANDLE> {
        self.monitors.get(index).map(|m| m.h_physical_monitor)
    }
}

impl Drop for PhysicalMonitorsGuard {
    fn drop(&mut self) {
        if let Some(hook) = &self.drop_hook {
            hook(self.count);
        }
        if self.count > 0 && !self.monitors.is_empty() {
            unsafe {
                DestroyPhysicalMonitors(self.count, self.monitors.as_mut_ptr());
            }
        }
    }
}

/// DDC 硬件访问适配器 Trait（用于支持真实 Win32 与测试仿真注入）
pub trait DdcHardwareAdapter: Send + Sync {
    fn list_displays(&self) -> Vec<RawDisplayItem>;
    fn get_gdi_monitor(&self, device_name: &str) -> Option<HMONITOR>;
    fn get_physical_monitors_count(&self, h_mon: HMONITOR) -> Option<u32>;
    fn open_physical_monitors(
        &self,
        h_mon: HMONITOR,
        count: u32,
    ) -> Result<PhysicalMonitorsGuard, String>;
    fn read_brightness(&self, handle: HANDLE) -> Option<(u32, u32, u32, bool)>;
    fn write_brightness(&self, handle: HANDLE, raw_val: u32, use_vcp: bool) -> bool;
}

fn wide_null_terminated(slice: &[u16]) -> String {
    let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..len]).trim().to_string()
}

/// 真实 Win32 生产环境适配器
pub struct Win32DdcAdapter;

impl DdcHardwareAdapter for Win32DdcAdapter {
    fn list_displays(&self) -> Vec<RawDisplayItem> {
        let mut displays = Vec::new();
        let mut adapter_index: u32 = 0;
        let mut slot_occurrence: HashMap<String, u32> = HashMap::new();

        unsafe {
            loop {
                let mut adapter: DISPLAY_DEVICEW = std::mem::zeroed();
                adapter.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;

                if EnumDisplayDevicesW(std::ptr::null(), adapter_index, &mut adapter, 0) == 0 {
                    break;
                }
                adapter_index += 1;

                if (adapter.state_flags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP) == 0 {
                    continue;
                }

                let adapter_name_str = wide_null_terminated(&adapter.device_name);
                let mut monitor_index: u32 = 0;

                loop {
                    let mut monitor: DISPLAY_DEVICEW = std::mem::zeroed();
                    monitor.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
                    let adapter_name_w: Vec<u16> = adapter.device_name.to_vec();

                    if EnumDisplayDevicesW(
                        adapter_name_w.as_ptr(),
                        monitor_index,
                        &mut monitor,
                        EDD_GET_DEVICE_INTERFACE_NAME,
                    ) == 0
                    {
                        break;
                    }
                    monitor_index += 1;

                    let dev_id = wide_null_terminated(&monitor.device_id);
                    if dev_id.is_empty() {
                        continue;
                    }

                    let dev_string = wide_null_terminated(&monitor.device_string);
                    let display_slot = adapter_name_str
                        .rsplit('\\')
                        .next()
                        .unwrap_or(&adapter_name_str);
                    let display_num = display_slot.strip_prefix("DISPLAY").unwrap_or(display_slot);
                    let mut slot_label = if display_num.chars().all(|c| c.is_ascii_digit()) {
                        format!("屏幕 {}", display_num)
                    } else {
                        display_slot.to_string()
                    };

                    let occ = slot_occurrence.entry(slot_label.clone()).or_insert(0);
                    *occ += 1;
                    if *occ > 1 {
                        slot_label = format!("{}-{}", slot_label, *occ);
                    }

                    let mut friendly_label =
                        format_display_label(&dev_string, &dev_id, &slot_label);
                    if let Some(size) = super::edid::read_display_size(&dev_id) {
                        friendly_label.push_str(&format!(" · {}", size.label()));
                    }

                    displays.push(RawDisplayItem {
                        device_name: adapter_name_str.clone(),
                        device_id: dev_id,
                        device_string: dev_string,
                        label: friendly_label,
                    });
                }
            }
        }

        displays
    }

    fn get_gdi_monitor(&self, device_name: &str) -> Option<HMONITOR> {
        let gdi_map = enumerate_gdi_monitors();
        gdi_map.get(device_name).copied()
    }

    fn get_physical_monitors_count(&self, h_mon: HMONITOR) -> Option<u32> {
        let mut count: u32 = 0;
        let ok = unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(h_mon, &mut count) };
        if ok != 0 {
            Some(count)
        } else {
            None
        }
    }

    fn open_physical_monitors(
        &self,
        h_mon: HMONITOR,
        count: u32,
    ) -> Result<PhysicalMonitorsGuard, String> {
        let mut monitors = vec![
            PHYSICAL_MONITOR {
                h_physical_monitor: std::ptr::null_mut(),
                sz_physical_monitor_description: [0; 128],
            };
            count as usize
        ];
        let ok = unsafe { GetPhysicalMonitorsFromHMONITOR(h_mon, count, monitors.as_mut_ptr()) };
        if ok != 0 {
            Ok(PhysicalMonitorsGuard::new(count, monitors))
        } else {
            Err("打开物理监视器句柄失败".into())
        }
    }

    fn read_brightness(&self, handle: HANDLE) -> Option<(u32, u32, u32, bool)> {
        // 1. 高层 API 尝试 3 次
        let mut min_b: u32 = 0;
        let mut cur_b: u32 = 0;
        let mut max_b: u32 = 0;

        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(40));
            }
            let ok = unsafe { GetMonitorBrightness(handle, &mut min_b, &mut cur_b, &mut max_b) };
            if ok != 0 {
                if max_b > min_b {
                    return Some((min_b, cur_b, max_b, false));
                }
            }
        }

        // 2. 回退底层 VCP 0x10 尝试 3 次
        let mut cur_vcp: u32 = 0;
        let mut max_vcp: u32 = 0;

        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(40));
            }
            let ok = unsafe {
                GetVCPFeatureAndVCPFeatureReply(
                    handle,
                    VCP_CODE_LUMINANCE,
                    std::ptr::null_mut(),
                    &mut cur_vcp,
                    &mut max_vcp,
                )
            };
            if ok != 0 {
                if max_vcp > 0 {
                    return Some((0, cur_vcp, max_vcp, true));
                }
            }
        }

        None
    }

    fn write_brightness(&self, handle: HANDLE, raw_val: u32, use_vcp: bool) -> bool {
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(50));
            }
            let ok = if use_vcp {
                unsafe { SetVCPFeature(handle, VCP_CODE_LUMINANCE, raw_val) }
            } else {
                unsafe { SetMonitorBrightness(handle, raw_val) }
            };
            if ok != 0 {
                return true;
            }
        }
        false
    }
}

pub static DEFAULT_DDC_ADAPTER: LazyLock<Win32DdcAdapter> = LazyLock::new(|| Win32DdcAdapter);

unsafe extern "system" fn enum_monitor_callback(
    h_monitor: HMONITOR,
    _hdc: HDC,
    _lprc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let map = &mut *(lparam as *mut HashMap<String, HMONITOR>);
    let mut info: MONITORINFOEXW = std::mem::zeroed();
    info.cb_size = std::mem::size_of::<MONITORINFOEXW>() as u32;

    if GetMonitorInfoW(h_monitor, &mut info) != 0 {
        let dev_name = wide_null_terminated(&info.sz_device);
        if !dev_name.is_empty() {
            map.insert(dev_name, h_monitor);
        }
    }
    1
}

/// 枚举 GDI 设备名称到 HMONITOR 句柄的映射表
pub fn enumerate_gdi_monitors() -> HashMap<String, HMONITOR> {
    let mut map: HashMap<String, HMONITOR> = HashMap::new();
    unsafe {
        let lparam = &mut map as *mut HashMap<String, HMONITOR> as LPARAM;
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(enum_monitor_callback),
            lparam,
        );
    }
    map
}

pub fn list_raw_windows_displays() -> Vec<RawDisplayItem> {
    DEFAULT_DDC_ADAPTER.list_displays()
}

pub fn read_physical_monitor_brightness(handle: HANDLE) -> Option<(u32, u32, u32, bool)> {
    DEFAULT_DDC_ADAPTER.read_brightness(handle)
}

pub fn write_physical_monitor_brightness(handle: HANDLE, raw_val: u32, use_vcp: bool) -> bool {
    DEFAULT_DDC_ADAPTER.write_brightness(handle, raw_val, use_vcp)
}

/// 基于抽象适配器枚举所有支持 DDC/CI 的外接显示器（排他过滤小米，单物理屏约束）
pub fn enumerate_ddc_monitors_with_adapter(
    adapter: &dyn DdcHardwareAdapter,
) -> Vec<DdcMonitorInfo> {
    let displays = adapter.list_displays();
    let mut result = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for disp in &displays {
        if disp.device_id.is_empty() || seen_ids.contains(&disp.device_id) {
            continue;
        }
        seen_ids.insert(disp.device_id.clone());

        // 严格排除小米/Redmi 设备
        if is_xiaomi_device_id(&disp.device_id) || is_xiaomi_device_id(&disp.device_string) {
            continue;
        }

        // 检查是否有多台显示器共享同一 GDI 端口 (MST 或复合镜像歧义)
        let same_gdi_count = displays
            .iter()
            .filter(|d| d.device_name == disp.device_name)
            .count();
        if same_gdi_count != 1 {
            result.push(DdcMonitorInfo {
                device_id: disp.device_id.clone(),
                device_name: disp.device_name.clone(),
                label: disp.label.clone(),
                min_brightness: 0,
                current_brightness: 0,
                max_brightness: 100,
                current_percent: 0,
                is_vcp_fallback: false,
                is_supported: false,
                write_revision: 0,
            });
            continue;
        }

        let h_mon = match adapter.get_gdi_monitor(&disp.device_name) {
            Some(h) => h,
            None => continue,
        };

        let num_phys = adapter.get_physical_monitors_count(h_mon).unwrap_or(0);
        if num_phys != 1 {
            result.push(DdcMonitorInfo {
                device_id: disp.device_id.clone(),
                device_name: disp.device_name.clone(),
                label: disp.label.clone(),
                min_brightness: 0,
                current_brightness: 0,
                max_brightness: 100,
                current_percent: 0,
                is_vcp_fallback: false,
                is_supported: false,
                write_revision: 0,
            });
            continue;
        }

        if let Ok(guard) = adapter.open_physical_monitors(h_mon, 1) {
            if let Some(h_p) = guard.handle(0) {
                if let Some((min_b, cur_b, max_b, is_vcp)) = adapter.read_brightness(h_p) {
                    if max_b > min_b {
                        let pct = normalize_brightness(cur_b, min_b, max_b);
                        let coalescer = get_ddc_device_coalescer(&disp.device_id);
                        let cur_rev = coalescer.last_revision.load(Ordering::SeqCst);
                        result.push(DdcMonitorInfo {
                            device_id: disp.device_id.clone(),
                            device_name: disp.device_name.clone(),
                            label: disp.label.clone(),
                            min_brightness: min_b,
                            current_brightness: cur_b,
                            max_brightness: max_b,
                            current_percent: pct,
                            is_vcp_fallback: is_vcp,
                            is_supported: true,
                            write_revision: cur_rev,
                        });
                    } else {
                        result.push(DdcMonitorInfo {
                            device_id: disp.device_id.clone(),
                            device_name: disp.device_name.clone(),
                            label: disp.label.clone(),
                            min_brightness: min_b,
                            current_brightness: cur_b,
                            max_brightness: max_b,
                            current_percent: 0,
                            is_vcp_fallback: is_vcp,
                            is_supported: false,
                            write_revision: 0,
                        });
                    }
                } else {
                    result.push(DdcMonitorInfo {
                        device_id: disp.device_id.clone(),
                        device_name: disp.device_name.clone(),
                        label: disp.label.clone(),
                        min_brightness: 0,
                        current_brightness: 0,
                        max_brightness: 100,
                        current_percent: 0,
                        is_vcp_fallback: false,
                        is_supported: false,
                        write_revision: 0,
                    });
                }
            }
        }
    }

    result
}

pub fn enumerate_ddc_monitors() -> Vec<DdcMonitorInfo> {
    enumerate_ddc_monitors_with_adapter(&*DEFAULT_DDC_ADAPTER)
}

/// 写入前核验唯一定位物理监视器（双重枚举验证，防热插拔在枚举间隙发生造成错屏）
pub fn verify_and_get_unique_monitor(
    adapter: &dyn DdcHardwareAdapter,
    device_id: &str,
) -> Result<(PhysicalMonitorsGuard, u32, u32, bool, String), String> {
    if is_xiaomi_device_id(device_id) {
        return Err("小米/Redmi 显示器严禁使用 DDC/CI 调节".into());
    }

    let displays = adapter.list_displays();
    let matches: Vec<&RawDisplayItem> = displays
        .iter()
        .filter(|d| d.device_id == device_id)
        .collect();

    if matches.is_empty() {
        return Err(format!(
            "写前核验失败: 目标设备已断开或不存在 ({})",
            device_id
        ));
    }
    if matches.len() > 1 {
        return Err(format!(
            "写前核验失败: 检测到重复设备标识符 ({})",
            device_id
        ));
    }

    let target = matches[0];
    let same_gdi = displays
        .iter()
        .filter(|d| d.device_name == target.device_name)
        .count();
    if same_gdi != 1 {
        return Err(format!(
            "写前核验失败: GDI 端口 {} 存在多设备共享/MST 输出歧义，安全拒绝写入",
            target.device_name
        ));
    }

    let h_mon = adapter
        .get_gdi_monitor(&target.device_name)
        .ok_or_else(|| {
            format!(
                "未找到端口 {} 对应的活动 GDI 监视器句柄",
                target.device_name
            )
        })?;

    let count = adapter
        .get_physical_monitors_count(h_mon)
        .ok_or_else(|| "无法获取物理监视器数量".to_string())?;

    if count != 1 {
        return Err(format!(
            "写前核验失败: 物理监视器数量不为 1 (实际为 {})，拒绝写入",
            count
        ));
    }

    let guard = adapter.open_physical_monitors(h_mon, 1)?;
    let h_phys = guard
        .handle(0)
        .ok_or_else(|| "获取物理监视器句柄失败".to_string())?;

    // 写前二次核验：在获取物理句柄后重新核验设备路径、GDI 映射和句柄未发生热插拔变更
    let displays_after = adapter.list_displays();
    let matches_after: Vec<&RawDisplayItem> = displays_after
        .iter()
        .filter(|d| d.device_id == device_id)
        .collect();
    if matches_after.len() != 1 {
        return Err(format!(
            "写前核验二次复核失败: 设备在获取物理句柄后已断开或变更 ({})",
            device_id
        ));
    }
    let target_after = matches_after[0];
    if target_after.device_name != target.device_name {
        return Err(format!(
            "写前核验二次复核失败: 设备 GDI 端口由 {} 变为 {}",
            target.device_name, target_after.device_name
        ));
    }
    let same_gdi_after = displays_after
        .iter()
        .filter(|d| d.device_name == target_after.device_name)
        .count();
    if same_gdi_after != 1 {
        return Err(format!(
            "写前核验二次复核失败: GDI 端口 {} 存在多设备共享/MST 输出歧义，安全拒绝写入",
            target_after.device_name
        ));
    }
    let h_mon_after = adapter.get_gdi_monitor(&target_after.device_name);
    if h_mon_after != Some(h_mon) {
        return Err("写前核验二次复核失败: GDI 监视器句柄发生变更，可能发生了热插拔".into());
    }
    let count_after = adapter.get_physical_monitors_count(h_mon).unwrap_or(0);
    if count_after != 1 {
        return Err("写前核验二次复核失败: 物理监视器数量已不再为 1".into());
    }

    let (min_b, _, max_b, is_vcp) = adapter
        .read_brightness(h_phys)
        .ok_or_else(|| format!("无法读取显示器 [{}] 亮度极值范围", target.label))?;

    if max_b <= min_b {
        return Err(format!(
            "写前核验二次复核失败: 显示器 [{}] 亮度极值异常 (min={}, max={})，标记为不可控",
            target.label, min_b, max_b
        ));
    }

    Ok((guard, min_b, max_b, is_vcp, target.label.clone()))
}

/// 安全地向普通显示器写入硬件亮度百分比 (写前重新枚举双重核验)
pub fn set_ddc_brightness_safe(device_id: &str, target_pct: u32) -> Result<(), String> {
    set_ddc_brightness_with_adapter(&*DEFAULT_DDC_ADAPTER, device_id, target_pct)
}

pub fn set_ddc_brightness_with_adapter(
    adapter: &dyn DdcHardwareAdapter,
    device_id: &str,
    target_pct: u32,
) -> Result<(), String> {
    set_ddc_brightness_verified(adapter, device_id, target_pct).map(|_| ())
}
fn set_ddc_brightness_verified(
    adapter: &dyn DdcHardwareAdapter,
    device_id: &str,
    target_pct: u32,
) -> Result<u32, String> {
    let (guard, min_b, max_b, is_vcp, label) = verify_and_get_unique_monitor(adapter, device_id)?;
    let h_phys = guard.handle(0).expect("Handle guaranteed");

    let raw_val = denormalize_brightness(target_pct, min_b, max_b);

    if adapter.write_brightness(h_phys, raw_val, is_vcp) {
        let mut confirmed = false;
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(80));
            }
            if let Some((lo, actual, hi, _)) = adapter.read_brightness(h_phys) {
                if lo == min_b && hi == max_b && actual == raw_val {
                    confirmed = true;
                    break;
                }
            }
        }
        if !confirmed {
            return Err(format!(
                "DDC 显示器 [{label}] 写入后未能回读确认亮度，请刷新重试"
            ));
        }
        crate::logger::log_ddc(&format!(
            "DDC 显示器 [{}] 亮度已成功调整为 {}% (硬件值 {})",
            label, target_pct, raw_val
        ));
        Ok(normalize_brightness(raw_val, min_b, max_b))
    } else {
        Err(format!("写入 DDC 显示器 [{}] 硬件亮度失败", label))
    }
}

/// 全局 DDC 写入代数计数器 (防止陈旧异步读覆盖最新写入)
pub static DDC_WRITE_REVISION: AtomicU64 = AtomicU64::new(1);

/// 获取最新 DDC 写入代数
pub fn next_ddc_revision() -> u64 {
    DDC_WRITE_REVISION.fetch_add(1, Ordering::SeqCst) + 1
}

/// 单设备写队列与合并去抖控制器 (Last-Value Coalescing 与同设备串行写入锁)
#[derive(Debug, Default)]
pub struct DdcDeviceWriteCoalescer {
    pub pending_target: Mutex<Option<(u32, u64)>>,
    pub last_written_pct: Mutex<Option<u32>>,
    pub last_written_revision: AtomicU64,
    pub last_write_time: Mutex<Option<Instant>>,
    pub last_revision: AtomicU64,
    pub write_mutex: Mutex<()>,
}

impl DdcDeviceWriteCoalescer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 提交新滑块目标值并获取当前写入代数（自动覆盖上一个未完成的目标，并严格更新 last_revision）
    pub fn push_target(&self, pct: u32) -> u64 {
        let rev = next_ddc_revision();
        self.push_target_with_revision(pct, rev);
        rev
    }

    /// 绑定特定代数提交目标值，确保请求与代数强一致绑定
    pub fn push_target_with_revision(&self, pct: u32, rev: u64) {
        let mut pending = self.pending_target.lock();
        if rev <= self.last_revision.load(Ordering::Acquire) {
            return;
        }
        let mut cur = self.last_revision.load(Ordering::Relaxed);
        while rev > cur {
            match self.last_revision.compare_exchange_weak(
                cur,
                rev,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
        *pending = Some((pct, rev));
    }

    /// 工作线程提取最新合并值执行硬件写入 (持有单设备排他锁，严格串行写入，返回实际写入的值与代数)
    pub fn drain_latest_and_write(
        &self,
        adapter: &dyn DdcHardwareAdapter,
        device_id: &str,
    ) -> Result<Option<(u32, u64)>, String> {
        let _write_guard = self.write_mutex.lock();

        let mut last_written = None;
        loop {
            let target = {
                let mut pending = self.pending_target.lock();
                pending.take()
            };

            match target {
                Some((pct, rev)) => {
                    let pct = set_ddc_brightness_verified(adapter, device_id, pct)?;
                    let mut last_p = self.last_written_pct.lock();
                    *last_p = Some(pct);
                    self.last_written_revision.store(rev, Ordering::SeqCst);
                    let mut last_t = self.last_write_time.lock();
                    *last_t = Some(Instant::now());
                    last_written = Some((pct, rev));
                }
                None => break,
            }
        }
        Ok(last_written)
    }

    /// 判断读取到的亮度是否应该被抑制（如果用户刚刚在 1.5 秒内写入过，抑制陈旧读）
    pub fn should_suppress_read_update(
        &self,
        read_revision: u64,
        suppress_duration: Duration,
    ) -> bool {
        if read_revision < self.last_revision.load(Ordering::Relaxed) {
            return true;
        }
        let last_t = self.last_write_time.lock();
        if let Some(t) = *last_t {
            if t.elapsed() < suppress_duration {
                return true;
            }
        }
        false
    }
}

static DDC_COALESCERS: LazyLock<Mutex<HashMap<String, Arc<DdcDeviceWriteCoalescer>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 获取指定设备的写合并与串行化协调器
pub fn get_ddc_device_coalescer(device_id: &str) -> Arc<DdcDeviceWriteCoalescer> {
    let mut map = DDC_COALESCERS.lock();
    map.entry(device_id.to_string())
        .or_insert_with(|| Arc::new(DdcDeviceWriteCoalescer::new()))
        .clone()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_normalize_brightness_boundary_and_limits() {
        assert_eq!(normalize_brightness(0, 0, 100), 0);
        assert_eq!(normalize_brightness(50, 0, 100), 50);
        assert_eq!(normalize_brightness(100, 0, 100), 100);

        // 硬件寄存器极值 20 - 220
        assert_eq!(normalize_brightness(20, 20, 220), 0);
        assert_eq!(normalize_brightness(120, 20, 220), 50);
        assert_eq!(normalize_brightness(220, 20, 220), 100);

        // 防下溢与极值：cur < min 时返回 0，cur > max 时返回 100
        assert_eq!(normalize_brightness(10, 20, 220), 0);
        assert_eq!(normalize_brightness(250, 20, 220), 100);

        // 异常硬件 limits (max <= min) 防 panic / 除以零
        assert_eq!(normalize_brightness(50, 100, 50), 0);
        assert_eq!(normalize_brightness(50, 100, 100), 0);
    }

    #[test]
    fn test_denormalize_brightness_boundary() {
        assert_eq!(denormalize_brightness(0, 0, 100), 0);
        assert_eq!(denormalize_brightness(50, 0, 100), 50);
        assert_eq!(denormalize_brightness(100, 0, 100), 100);

        assert_eq!(denormalize_brightness(0, 20, 220), 20);
        assert_eq!(denormalize_brightness(50, 20, 220), 120);
        assert_eq!(denormalize_brightness(100, 20, 220), 220);

        // 超过 100% clamp 到 max
        assert_eq!(denormalize_brightness(150, 20, 220), 220);
        // max <= min
        assert_eq!(denormalize_brightness(50, 100, 50), 100);
    }

    /// 模拟测试适配器
    pub struct MockDdcAdapter {
        pub displays: Vec<RawDisplayItem>,
        pub gdi_map: HashMap<String, HMONITOR>,
        pub monitor_counts: HashMap<HMONITOR, u32>,
        pub brightness_data: HashMap<HANDLE, (u32, u32, u32, bool)>,
        pub write_history: Arc<Mutex<Vec<(HANDLE, u32, bool)>>>,
        pub destroyed_count: Arc<Mutex<u32>>,
    }

    unsafe impl Send for MockDdcAdapter {}
    unsafe impl Sync for MockDdcAdapter {}

    impl DdcHardwareAdapter for MockDdcAdapter {
        fn list_displays(&self) -> Vec<RawDisplayItem> {
            self.displays.clone()
        }

        fn get_gdi_monitor(&self, device_name: &str) -> Option<HMONITOR> {
            self.gdi_map.get(device_name).copied()
        }

        fn get_physical_monitors_count(&self, h_mon: HMONITOR) -> Option<u32> {
            self.monitor_counts.get(&h_mon).copied()
        }

        fn open_physical_monitors(
            &self,
            _h_mon: HMONITOR,
            count: u32,
        ) -> Result<PhysicalMonitorsGuard, String> {
            let destroy_counter = self.destroyed_count.clone();
            let mut monitors = Vec::new();
            for i in 0..count {
                monitors.push(PHYSICAL_MONITOR {
                    h_physical_monitor: (1000 + i as usize) as HANDLE,
                    sz_physical_monitor_description: [0; 128],
                });
            }
            Ok(PhysicalMonitorsGuard::with_hook(
                count,
                monitors,
                Box::new(move |c| {
                    let mut lock = destroy_counter.lock();
                    *lock += c;
                }),
            ))
        }

        fn read_brightness(&self, handle: HANDLE) -> Option<(u32, u32, u32, bool)> {
            let mut value = self.brightness_data.get(&handle).copied()?;
            if let Some((_, raw, _)) = self
                .write_history
                .lock()
                .iter()
                .rev()
                .find(|(h, _, _)| *h == handle)
            {
                value.1 = *raw;
            }
            Some(value)
        }

        fn write_brightness(&self, handle: HANDLE, raw_val: u32, use_vcp: bool) -> bool {
            let mut h = self.write_history.lock();
            h.push((handle, raw_val, use_vcp));
            true
        }
    }

    #[test]
    fn test_mock_ddc_raii_and_hotplug_verification() {
        let destroyed = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(Vec::new()));

        let mock = MockDdcAdapter {
            displays: vec![RawDisplayItem {
                device_name: r"\\.\DISPLAY2".into(),
                device_id: r"\\?\DISPLAY#YHY1600#123".into(),
                device_string: "YHY1600".into(),
                label: "YHY1600 · 屏幕 2".into(),
            }],
            gdi_map: {
                let mut m = HashMap::new();
                m.insert(r"\\.\DISPLAY2".into(), 0x1234 as HMONITOR);
                m
            },
            monitor_counts: {
                let mut m = HashMap::new();
                m.insert(0x1234 as HMONITOR, 1);
                m
            },
            brightness_data: {
                let mut m = HashMap::new();
                m.insert(1000 as HANDLE, (0, 50, 100, false));
                m
            },
            write_history: writes.clone(),
            destroyed_count: destroyed.clone(),
        };

        // 1. 正常写入测试
        let res = set_ddc_brightness_with_adapter(&mock, r"\\?\DISPLAY#YHY1600#123", 75);
        assert!(res.is_ok());
        assert_eq!(writes.lock().len(), 1);
        assert_eq!(writes.lock()[0], (1000 as HANDLE, 75, false));
        // RAII 销毁验证：写入后物理监视器句柄必须立即被安全销毁释放
        assert_eq!(*destroyed.lock(), 1);

        // 2. 小米设备严厉拒绝测试
        let xiaomi_res = set_ddc_brightness_with_adapter(&mock, r"\\?\DISPLAY#XMI300A#999", 50);
        assert!(xiaomi_res.is_err());
        assert!(xiaomi_res.unwrap_err().contains("小米/Redmi"));

        // 3. 热插拔错屏测试：若设备已在两次枚举间断开
        let disconnected_res =
            set_ddc_brightness_with_adapter(&mock, r"\\?\DISPLAY#DISCONNECTED#000", 50);
        assert!(disconnected_res.is_err());
        assert!(disconnected_res.unwrap_err().contains("已断开或不存在"));
    }

    #[test]
    fn test_coalescer_high_frequency_merge_and_suppression() {
        let coalescer = DdcDeviceWriteCoalescer::new();

        // 快速连续滑动 3 次
        coalescer.push_target(60);
        coalescer.push_target(70);
        coalescer.push_target(80);

        let mock = MockDdcAdapter {
            displays: vec![RawDisplayItem {
                device_name: r"\\.\DISPLAY2".into(),
                device_id: r"\\?\DISPLAY#YHY1600#123".into(),
                device_string: "YHY1600".into(),
                label: "YHY1600".into(),
            }],
            gdi_map: {
                let mut m = HashMap::new();
                m.insert(r"\\.\DISPLAY2".into(), 0x1234 as HMONITOR);
                m
            },
            monitor_counts: {
                let mut m = HashMap::new();
                m.insert(0x1234 as HMONITOR, 1);
                m
            },
            brightness_data: {
                let mut m = HashMap::new();
                m.insert(1000 as HANDLE, (0, 50, 100, false));
                m
            },
            write_history: Arc::new(Mutex::new(Vec::new())),
            destroyed_count: Arc::new(Mutex::new(0)),
        };

        // 执行合并写入：必须只执行最新一次的值 80 及其对应代数
        let written = coalescer
            .drain_latest_and_write(&mock, r"\\?\DISPLAY#YHY1600#123")
            .unwrap();
        assert!(written.is_some());
        let (written_pct, written_rev) = written.unwrap();
        assert_eq!(written_pct, 80);
        assert_eq!(written_rev, coalescer.last_revision.load(Ordering::Relaxed));
        assert_eq!(*coalescer.last_written_pct.lock(), Some(80));

        // 验证读取抑制：1.5 秒内读回的值必须被抑制，防旧读反弹
        assert!(coalescer.should_suppress_read_update(0, Duration::from_secs(2)));
    }

    #[test]
    fn test_ddc_max_le_min_and_vcp_zero_marked_unsupported() {
        let mock_bad = MockDdcAdapter {
            displays: vec![RawDisplayItem {
                device_name: r"\\.\DISPLAY2".into(),
                device_id: r"\\?\DISPLAY#BAD_MON#123".into(),
                device_string: "BAD_MON".into(),
                label: "BAD_MON".into(),
            }],
            gdi_map: {
                let mut m = HashMap::new();
                m.insert(r"\\.\DISPLAY2".into(), 0x1234 as HMONITOR);
                m
            },
            monitor_counts: {
                let mut m = HashMap::new();
                m.insert(0x1234 as HMONITOR, 1);
                m
            },
            brightness_data: {
                let mut m = HashMap::new();
                // 模拟 max <= min 异常
                m.insert(1000 as HANDLE, (100, 50, 50, false));
                m
            },
            write_history: Arc::new(Mutex::new(Vec::new())),
            destroyed_count: Arc::new(Mutex::new(0)),
        };

        let list = enumerate_ddc_monitors_with_adapter(&mock_bad);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].is_supported, false, "max<=min 必须标记为不可控");

        // 尝试写入也必须被写前核验拒绝
        let write_res = set_ddc_brightness_with_adapter(&mock_bad, r"\\?\DISPLAY#BAD_MON#123", 50);
        assert!(write_res.is_err());
        assert!(write_res.unwrap_err().contains("不可控"));
    }

    #[test]
    fn test_ddc_second_pass_mst_shared_gdi_rejected() {
        struct SecondPassMstMock {
            list_calls: Mutex<u32>,
            base: MockDdcAdapter,
        }

        impl DdcHardwareAdapter for SecondPassMstMock {
            fn list_displays(&self) -> Vec<RawDisplayItem> {
                let mut count = self.list_calls.lock();
                *count += 1;
                if *count == 1 {
                    self.base.list_displays()
                } else {
                    // 第二次枚举发现同 GDI 端口接入了第二台显示器 (MST 复合歧义)
                    vec![
                        RawDisplayItem {
                            device_name: r"\\.\DISPLAY2".into(),
                            device_id: r"\\?\DISPLAY#TEST001#123".into(),
                            device_string: "TEST001".into(),
                            label: "TEST001".into(),
                        },
                        RawDisplayItem {
                            device_name: r"\\.\DISPLAY2".into(),
                            device_id: r"\\?\DISPLAY#TEST002#456".into(),
                            device_string: "TEST002".into(),
                            label: "TEST002".into(),
                        },
                    ]
                }
            }

            fn get_gdi_monitor(&self, device_name: &str) -> Option<HMONITOR> {
                self.base.get_gdi_monitor(device_name)
            }

            fn get_physical_monitors_count(&self, h_mon: HMONITOR) -> Option<u32> {
                self.base.get_physical_monitors_count(h_mon)
            }

            fn open_physical_monitors(
                &self,
                h_mon: HMONITOR,
                count: u32,
            ) -> Result<PhysicalMonitorsGuard, String> {
                self.base.open_physical_monitors(h_mon, count)
            }

            fn read_brightness(&self, handle: HANDLE) -> Option<(u32, u32, u32, bool)> {
                self.base.read_brightness(handle)
            }

            fn write_brightness(&self, handle: HANDLE, raw_val: u32, use_vcp: bool) -> bool {
                self.base.write_brightness(handle, raw_val, use_vcp)
            }
        }

        let base = MockDdcAdapter {
            displays: vec![RawDisplayItem {
                device_name: r"\\.\DISPLAY2".into(),
                device_id: r"\\?\DISPLAY#TEST001#123".into(),
                device_string: "TEST001".into(),
                label: "TEST001".into(),
            }],
            gdi_map: {
                let mut m = HashMap::new();
                m.insert(r"\\.\DISPLAY2".into(), 0x1234 as HMONITOR);
                m
            },
            monitor_counts: {
                let mut m = HashMap::new();
                m.insert(0x1234 as HMONITOR, 1);
                m
            },
            brightness_data: {
                let mut m = HashMap::new();
                m.insert(1000 as HANDLE, (0, 50, 100, false));
                m
            },
            write_history: Arc::new(Mutex::new(Vec::new())),
            destroyed_count: Arc::new(Mutex::new(0)),
        };

        let mst_mock = SecondPassMstMock {
            list_calls: Mutex::new(0),
            base,
        };

        let res = set_ddc_brightness_with_adapter(&mst_mock, r"\\?\DISPLAY#TEST001#123", 50);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("二次复核失败") && err.contains("歧义"),
            "实际错误: {}",
            err
        );
    }

    struct HotplugBetweenSnapshotsMock {
        list_calls: Mutex<u32>,
        base: MockDdcAdapter,
    }

    impl DdcHardwareAdapter for HotplugBetweenSnapshotsMock {
        fn list_displays(&self) -> Vec<RawDisplayItem> {
            let mut count = self.list_calls.lock();
            *count += 1;
            if *count == 1 {
                // 第一次枚举：设备正常存在
                self.base.list_displays()
            } else {
                // 第二次枚举（取到句柄后）：模拟设备热插拔拔出，返回空列表
                Vec::new()
            }
        }

        fn get_gdi_monitor(&self, device_name: &str) -> Option<HMONITOR> {
            self.base.get_gdi_monitor(device_name)
        }

        fn get_physical_monitors_count(&self, h_mon: HMONITOR) -> Option<u32> {
            self.base.get_physical_monitors_count(h_mon)
        }

        fn open_physical_monitors(
            &self,
            h_mon: HMONITOR,
            count: u32,
        ) -> Result<PhysicalMonitorsGuard, String> {
            self.base.open_physical_monitors(h_mon, count)
        }

        fn read_brightness(&self, handle: HANDLE) -> Option<(u32, u32, u32, bool)> {
            self.base.read_brightness(handle)
        }

        fn write_brightness(&self, handle: HANDLE, raw_val: u32, use_vcp: bool) -> bool {
            self.base.write_brightness(handle, raw_val, use_vcp)
        }
    }

    #[test]
    fn test_verify_and_get_unique_monitor_rejects_hotplug_between_snapshots() {
        let base = MockDdcAdapter {
            displays: vec![RawDisplayItem {
                device_name: r"\\.\DISPLAY2".into(),
                device_id: r"\\?\DISPLAY#TEST001#123".into(),
                device_string: "TEST001".into(),
                label: "TEST001".into(),
            }],
            gdi_map: {
                let mut m = HashMap::new();
                m.insert(r"\\.\DISPLAY2".into(), 0x1234 as HMONITOR);
                m
            },
            monitor_counts: {
                let mut m = HashMap::new();
                m.insert(0x1234 as HMONITOR, 1);
                m
            },
            brightness_data: {
                let mut m = HashMap::new();
                m.insert(1000 as HANDLE, (0, 50, 100, false));
                m
            },
            write_history: Arc::new(Mutex::new(Vec::new())),
            destroyed_count: Arc::new(Mutex::new(0)),
        };

        let hotplug_adapter = HotplugBetweenSnapshotsMock {
            list_calls: Mutex::new(0),
            base,
        };

        // 尝试写入：必须在二次复核时捕获热插拔并严厉拒绝写入，绝不盲目往旧句柄写！
        let res = set_ddc_brightness_with_adapter(&hotplug_adapter, r"\\?\DISPLAY#TEST001#123", 50);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("二次复核失败"), "实际错误: {}", err);
    }
}
