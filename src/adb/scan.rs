use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::core::PWSTR;
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

const IF_TYPE_ETHERNET_CSMACD: u32 = 6;
const IF_TYPE_IEEE80211: u32 = 71;
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub ip: String,
    pub model: String,
    pub is_mitv: bool,
}

#[derive(Debug, Clone)]
pub struct PhysicalAdapterInfo {
    pub name: String,
    pub description: String,
    pub friendly_name: String,
    pub ip_addresses: Vec<Ipv4Addr>,
    pub subnets: Vec<(Ipv4Addr, u8)>,
}

/// 检查型号是否属于 MiTV 系列设备
pub fn is_mitv_model(model: &str) -> bool {
    model.to_lowercase().contains("mitv")
}

/// 将 PWSTR 转换为标准 Rust String
unsafe fn pwstr_to_string(pwstr: PWSTR) -> String {
    if pwstr.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *pwstr.0.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(pwstr.0, len);
    String::from_utf16_lossy(slice)
}

/// 枚举本机真实物理网络适配器与 IPv4 网段
pub fn enumerate_windows_physical_adapters() -> Vec<PhysicalAdapterInfo> {
    let mut adapters = Vec::new();
    let mut buf_len = 16384u32;
    let mut buffer: Vec<u8> = vec![0; buf_len as usize];

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut ret = unsafe {
        GetAdaptersAddresses(
            AF_INET.0 as u32,
            flags,
            None,
            Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
            &mut buf_len,
        )
    };

    if ret == 111 {
        // ERROR_BUFFER_OVERFLOW
        buffer.resize(buf_len as usize, 0);
        ret = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut buf_len,
            )
        };
    }

    if ret != 0 {
        crate::logger::log_info(&format!("GetAdaptersAddresses 调用返回错误码: {}", ret));
        return adapters;
    }

    let mut curr_ptr = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !curr_ptr.is_null() {
        let adapter = unsafe { &*curr_ptr };

        let if_type = adapter.IfType;
        let is_loopback = if_type == IF_TYPE_SOFTWARE_LOOPBACK;
        let is_physical = if_type == IF_TYPE_ETHERNET_CSMACD || if_type == IF_TYPE_IEEE80211;

        if is_physical && !is_loopback {
            let friendly_name = unsafe { pwstr_to_string(adapter.FriendlyName) };
            let description = unsafe { pwstr_to_string(adapter.Description) };
            let adapter_name = if !adapter.AdapterName.is_null() {
                unsafe { std::ffi::CStr::from_ptr(adapter.AdapterName.0 as *const i8) }
                    .to_string_lossy()
                    .to_string()
            } else {
                String::new()
            };

            let mut ips = Vec::new();
            let mut subnets = Vec::new();

            let mut unicast_ptr = adapter.FirstUnicastAddress;
            while !unicast_ptr.is_null() {
                let unicast = unsafe { &*unicast_ptr };
                let sockaddr_ptr = unicast.Address.lpSockaddr;
                if !sockaddr_ptr.is_null() && unsafe { (*sockaddr_ptr).sa_family == AF_INET } {
                    let sin = unsafe { &*(sockaddr_ptr as *const SOCKADDR_IN) };
                    let ip_bytes = unsafe { sin.sin_addr.S_un.S_addr.to_ne_bytes() };
                    let ip = Ipv4Addr::from(ip_bytes);
                    if !ip.is_loopback() && !ip.is_link_local() {
                        let prefix = unicast.OnLinkPrefixLength;
                        ips.push(ip);
                        subnets.push((ip, prefix));
                    }
                }
                unicast_ptr = unicast.Next;
            }

            if !ips.is_empty() {
                adapters.push(PhysicalAdapterInfo {
                    name: adapter_name,
                    description,
                    friendly_name,
                    ip_addresses: ips,
                    subnets,
                });
            }
        }

        curr_ptr = adapter.Next;
    }

    adapters
}

/// 快速探测指定 IP 的 TCP 5555 端口
pub fn probe_tcp_port(ip: &str, port: u16, timeout: Duration) -> bool {
    let addr_str = format!("{}:{}", ip, port);
    if let Ok(sa) = addr_str.parse::<SocketAddr>() {
        TcpStream::connect_timeout(&sa, timeout).is_ok()
    } else {
        false
    }
}

/// 根据网卡信息与配置构建所有待探测的目标 IP 列表
pub fn build_scan_ip_targets(
    adapters: &[PhysicalAdapterInfo],
    force_subnets: &[String],
    force_devices: &[String],
    block_devices: &[String],
) -> Vec<Ipv4Addr> {
    let mut targets = std::collections::BTreeSet::new();

    // 1. 遍历通过物理网卡识别出的子网
    for adapter in adapters {
        let is_blocked = block_devices.iter().any(|b| {
            !b.is_empty()
                && (adapter.friendly_name.contains(b)
                    || adapter.description.contains(b)
                    || adapter.name.contains(b))
        });
        if is_blocked {
            continue;
        }

        let _is_forced = force_devices.iter().any(|f| {
            !f.is_empty()
                && (adapter.friendly_name.contains(f)
                    || adapter.description.contains(f)
                    || adapter.name.contains(f))
        });

        // 默认扫描 /24 网段
        for &(ip, _prefix) in &adapter.subnets {
            let octets = ip.octets();
            for last in 1..=254 {
                targets.insert(Ipv4Addr::new(octets[0], octets[1], octets[2], last));
            }
        }
    }

    // 2. 加入用户配置强制扫描的自定义网段 (例如 "192.168.31.0/24" 或 "192.168.31")
    for subnet_str in force_subnets {
        let clean = subnet_str
            .trim()
            .trim_end_matches(".0/24")
            .trim_end_matches("/24");
        let parts: Vec<&str> = clean.split('.').collect();
        if parts.len() >= 3 {
            if let (Ok(o0), Ok(o1), Ok(o2)) = (
                parts[0].parse::<u8>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
            ) {
                for last in 1..=254 {
                    targets.insert(Ipv4Addr::new(o0, o1, o2, last));
                }
            }
        }
    }

    targets.into_iter().collect()
}

/// 扫描内网 ADB 设备，采用受控有限并发与即时取消机制
pub fn scan_local_devices(
    cancel_token: Arc<AtomicBool>,
    on_found: impl Fn(DiscoveredDevice) + Send + Sync + 'static,
) {
    // Caller owns the scan thread and its cancellation lifetime.
    crate::logger::log_info("开始内网 ADB 设备并发扫描...");

    let config = crate::config::load_settings();
    let adapters = enumerate_windows_physical_adapters();
    let ip_targets = build_scan_ip_targets(
        &adapters,
        &config.scan_force_subnets,
        &config.scan_force_devices,
        &config.scan_block_devices,
    );

    crate::logger::log_info(&format!(
        "扫描目标已生成: {} 个物理网卡，共 {} 个 IP 待探测",
        adapters.len(),
        ip_targets.len()
    ));

    // 按批次分片执行有限并发探测 (每批 64 个并发连接，避免瞬时连接耗尽 socket 资源)
    let chunk_size = 64;
    let mut open_ips = Vec::new();

    for chunk in ip_targets.chunks(chunk_size) {
        if cancel_token.load(Ordering::Relaxed) {
            crate::logger::log_info("扫描任务收到取消信号，已中止端口探测");
            return;
        }

        let mut handles = Vec::new();
        for &ip in chunk {
            let cancel = Arc::clone(&cancel_token);
            handles.push(std::thread::spawn(move || {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                if probe_tcp_port(&ip.to_string(), 5555, Duration::from_millis(150)) {
                    Some(ip)
                } else {
                    None
                }
            }));
        }

        for h in handles {
            if let Ok(Some(open_ip)) = h.join() {
                open_ips.push(open_ip);
            }
        }
    }

    crate::logger::log_info(&format!(
        "端口探测完成，发现 {} 个开放 5555 端口的候选端点，开始 ADB 握手验证...",
        open_ips.len()
    ));

    // 对开放 5555 的 IP 执行完整的连接和型号读取验证
    for ip in open_ips {
        if cancel_token.load(Ordering::Relaxed) {
            crate::logger::log_info("扫描任务收到取消信号，已中止握手验证");
            return;
        }

        let target_dev = crate::adb::AdbDevice::new(ip.to_string());
        match target_dev
            .verify_and_get_model_with_cancel(Duration::from_secs(4), Some(&cancel_token))
        {
            Ok(model) => {
                let is_mitv = is_mitv_model(&model);
                crate::logger::log_info(&format!(
                    "发现 ADB 设备: {} (型号: {}, MiTV: {})",
                    ip, model, is_mitv
                ));
                on_found(DiscoveredDevice {
                    ip: ip.to_string(),
                    model,
                    is_mitv,
                });
            }
            Err(err) => {
                crate::logger::log_info(&format!(
                    "端点 {} 开放但 ADB 握手或型号读取失败: {}",
                    ip, err
                ));
            }
        }
    }

    crate::logger::log_info("内网 ADB 设备扫描流程圆满结束");
}
