pub mod client;
pub mod commands;
pub mod scan;

pub use client::{
    adb_device_state, adb_device_state_with_cancel, adb_run, adb_run_with_cancel,
    adb_run_with_timeout, block_adb_spawns, cleanup_adb_processes, decode_adb_output,
    ensure_adb_server_ready, ensure_adb_server_ready_with_cancel, format_adb_serial,
    get_adb_server_port, get_bundled_adb_path, is_adb_server_alive, is_adb_spawn_blocked,
    unblock_adb_spawns, AdbDeviceState, ADB_DEVICE_PORT, DEFAULT_ADB_SERVER_PORT,
};
pub use scan::{scan_local_devices, DiscoveredDevice};

use parking_lot::{Mutex, ReentrantMutex};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

/// 全局按设备 Serial 共享的可重入互斥锁映射表
/// 确保同设备（即便由不同 AdbDevice 句柄发起）的全部操作与 transaction 共享同一把排他锁
static SERIAL_LOCKS: LazyLock<Mutex<HashMap<String, Arc<ReentrantMutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn get_serial_lock(serial: &str) -> Arc<ReentrantMutex<()>> {
    let mut map = SERIAL_LOCKS.lock();
    map.entry(serial.to_string())
        .or_insert_with(|| Arc::new(ReentrantMutex::new(())))
        .clone()
}

/// ADB 交互错误分类
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbError {
    NetworkUnreachable(String),
    PortClosed(String),
    HandshakeFailed(String),
    Unauthorized,
    DeviceOffline,
    NotFound,
    CommandFailed(String),
    ServerNotReady(String),
    Cancelled,
}

impl fmt::Display for AdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdbError::NetworkUnreachable(msg) => write!(f, "网络不可达: {}", msg),
            AdbError::PortClosed(msg) => write!(f, "ADB 端口未开放: {}", msg),
            AdbError::HandshakeFailed(msg) => write!(f, "握手失败: {}", msg),
            AdbError::Unauthorized => write!(f, "设备未授权 (请在显示器上勾选允许调试)"),
            AdbError::DeviceOffline => write!(f, "设备已离线"),
            AdbError::NotFound => write!(f, "未发现目标设备"),
            AdbError::CommandFailed(msg) => write!(f, "命令执行失败: {}", msg),
            AdbError::ServerNotReady(msg) => write!(f, "ADB 服务就绪失败: {}", msg),
            AdbError::Cancelled => write!(f, "操作已取消"),
        }
    }
}

impl std::error::Error for AdbError {}

/// 设备信息实体
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub ip: String,
    pub port: u16,
    pub serial: String,
    pub model: String,
    pub state: AdbDeviceState,
}

/// 判断型号是否属于小米/红米系列电视或显示器
pub fn is_mitv_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.contains("mitv")
        || lower.contains("redmi")
        || lower.contains("xiaomi")
        || lower.contains("g pro")
        || lower.contains("p27")
}

/// 与单一 ADB 设备交互的排他控制句柄 (自动绑定全局同 serial 共享锁)
#[derive(Debug, Clone)]
pub struct AdbDevice {
    pub ip: String,
    pub port: u16,
    session_cancel: Option<Arc<AtomicBool>>,
}

impl AdbDevice {
    pub fn new(ip: impl Into<String>) -> Self {
        Self {
            ip: ip.into(),
            port: ADB_DEVICE_PORT,
            session_cancel: None,
        }
    }

    pub fn with_port(ip: impl Into<String>, port: u16) -> Self {
        Self {
            ip: ip.into(),
            port,
            session_cancel: None,
        }
    }

    pub fn serial(&self) -> String {
        format_adb_serial(&self.ip, self.port)
    }

    pub fn run_device_command(&self, args: &[&str], timeout: Duration) -> Result<String, AdbError> {
        let serial = self.serial();
        let mut command = vec!["-s", serial.as_str()];
        command.extend_from_slice(args);
        self.transaction(|| {
            client::adb_run_with_cancel(&command, timeout, self.session_cancel.as_deref())
        })
        .map_err(AdbError::CommandFailed)
    }

    pub fn with_session_cancel(mut self, token: Arc<AtomicBool>) -> Self {
        self.session_cancel = Some(token);
        self
    }

    /// 在该设备的全局排他锁保护下执行多步事务
    pub fn transaction<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let lock = get_serial_lock(&self.serial());
        let _guard = lock.lock();
        f()
    }

    /// 查询设备当前连接状态
    pub fn get_state(&self) -> AdbDeviceState {
        self.get_state_with_cancel(Duration::from_secs(2), self.session_cancel.as_deref())
    }

    /// 查询设备当前连接状态（支持自定义超时与取消令牌）
    pub fn get_state_with_cancel(
        &self,
        timeout: Duration,
        cancel_token: Option<&AtomicBool>,
    ) -> AdbDeviceState {
        let serial = self.serial();
        client::adb_device_state_with_cancel(&serial, timeout, cancel_token)
    }

    /// 统一的可取消、带重试与严格排他锁的底层连接与验证事务
    ///
    /// - 贯穿同一个总 deadline 和 cancel_token（锁竞争、ensure_server、connect、get_state、get_model 共享同一时间预算）
    /// - 使用 try_lock_for 循环竞争设备排他锁，扫描和手动连接共用锁，禁止并发连接/命令错序
    /// - 瞬时握手失败可重试，不永久排除；unauthorized/offline/network/port/handshake 分明确错误
    /// - 离线有界重连，未授权立即停止重试并报错
    /// - fetch_model 为 true 时完成型号验证读取并返回型号，为 false 时完成连接即返回 Ok(None)
    pub fn connect_transaction(
        &self,
        total_timeout: Duration,
        max_retries: usize,
        fetch_model: bool,
        cancel_token: Option<&AtomicBool>,
    ) -> Result<Option<String>, AdbError> {
        let serial = self.serial();
        let deadline = std::time::Instant::now() + total_timeout;

        // 1. 在同一个总 deadline 和 cancel_token 下，循环 try_lock 抢占设备排他锁
        let lock = get_serial_lock(&serial);
        let _guard = loop {
            if client::is_adb_spawn_blocked() {
                return Err(AdbError::Cancelled);
            }
            if let Some(token) = cancel_token {
                if token.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(AdbError::Cancelled);
                }
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(AdbError::HandshakeFailed("等待设备排他锁超时".to_string()));
            }
            let step = (deadline - now).min(Duration::from_millis(20));
            if let Some(guard) = lock.try_lock_for(step) {
                break guard;
            }
        };

        // 2. 确保 ADB Server 就绪（在同一总 deadline 与 cancel 约束下）
        let remaining_server = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining_server.is_zero() {
            return Err(AdbError::HandshakeFailed(
                "等待 ADB Server 超时".to_string(),
            ));
        }
        client::ensure_adb_server_ready_with_cancel(remaining_server, cancel_token)
            .map_err(AdbError::ServerNotReady)?;

        let mut last_err = AdbError::HandshakeFailed("连接尝试失败".into());

        // 3. 循环重试连接与状态确认
        let retries = max_retries.max(1);
        for attempt in 1..=retries {
            if client::is_adb_spawn_blocked() {
                return Err(AdbError::Cancelled);
            }
            if let Some(token) = cancel_token {
                if token.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(AdbError::Cancelled);
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(last_err);
            }

            // 先快速复核当前状态：若已处于连接状态，直接进入后续逻辑（对齐快速路径）
            let check_timeout = deadline
                .saturating_duration_since(std::time::Instant::now())
                .min(Duration::from_secs(2));
            let state = self.get_state_with_cancel(check_timeout, cancel_token);
            match state {
                AdbDeviceState::Device => {
                    if fetch_model {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        let model_timeout = remaining.min(Duration::from_secs(4));
                        match self.get_model_with_cancel(model_timeout, cancel_token) {
                            Ok(model) => {
                                let trimmed = model.trim().to_string();
                                if !trimmed.is_empty() {
                                    return Ok(Some(trimmed));
                                } else {
                                    last_err = AdbError::HandshakeFailed("读取到空型号".into());
                                }
                            }
                            Err(AdbError::Unauthorized) => return Err(AdbError::Unauthorized),
                            Err(AdbError::Cancelled) => return Err(AdbError::Cancelled),
                            Err(e) => {
                                // 瞬时型号读取失败纳入同一总 deadline 重试，不使用 ? 提前中断
                                last_err = e;
                            }
                        }
                    } else {
                        return Ok(None);
                    }
                }
                AdbDeviceState::Unauthorized => {
                    // 关键质量门：未授权立即终止重试，绝不无意义轮询
                    return Err(AdbError::Unauthorized);
                }
                _ => {}
            }

            // 发起 connect 命令
            let step_timeout = deadline
                .saturating_duration_since(std::time::Instant::now())
                .min(Duration::from_secs(4));
            if step_timeout.is_zero() {
                return Err(last_err);
            }

            let out =
                client::adb_run_with_cancel(&["connect", &serial], step_timeout, cancel_token);
            match out {
                Ok(msg) => {
                    let lower = msg.to_lowercase();
                    if lower.contains("unauthorized") {
                        return Err(AdbError::Unauthorized);
                    }
                    if lower.contains("refused") || lower.contains("cannot connect") {
                        last_err = AdbError::PortClosed(msg);
                    } else if lower.contains("connected to") || lower.contains("already connected")
                    {
                        let verify_timeout = deadline
                            .saturating_duration_since(std::time::Instant::now())
                            .min(Duration::from_secs(2));
                        let verify_state = self.get_state_with_cancel(verify_timeout, cancel_token);
                        match verify_state {
                            AdbDeviceState::Device => {
                                if fetch_model {
                                    let remaining = deadline
                                        .saturating_duration_since(std::time::Instant::now());
                                    let model_timeout = remaining.min(Duration::from_secs(4));
                                    match self.get_model_with_cancel(model_timeout, cancel_token) {
                                        Ok(model) => {
                                            let trimmed = model.trim().to_string();
                                            if !trimmed.is_empty() {
                                                return Ok(Some(trimmed));
                                            } else {
                                                last_err = AdbError::HandshakeFailed(
                                                    "读取到空型号".into(),
                                                );
                                            }
                                        }
                                        Err(AdbError::Unauthorized) => {
                                            return Err(AdbError::Unauthorized)
                                        }
                                        Err(AdbError::Cancelled) => {
                                            return Err(AdbError::Cancelled)
                                        }
                                        Err(e) => {
                                            last_err = e;
                                        }
                                    }
                                } else {
                                    return Ok(None);
                                }
                            }
                            AdbDeviceState::Unauthorized => {
                                return Err(AdbError::Unauthorized);
                            }
                            AdbDeviceState::Offline => {
                                last_err = AdbError::DeviceOffline;
                            }
                            _ => {
                                last_err = AdbError::HandshakeFailed(format!(
                                    "状态未就绪: {:?}",
                                    verify_state
                                ));
                            }
                        }
                    } else {
                        last_err = AdbError::HandshakeFailed(msg);
                    }
                }
                Err(e) => {
                    if e.contains("取消") {
                        return Err(AdbError::Cancelled);
                    }
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(last_err);
                    }
                    // connect 客户端超时或返回错误时，在剩余时间内复核状态以防服务端实际已接入
                    let verify_timeout = remaining.min(Duration::from_millis(300));
                    let verify_state = self.get_state_with_cancel(verify_timeout, cancel_token);
                    if verify_state == AdbDeviceState::Device {
                        if fetch_model {
                            let rem_model =
                                deadline.saturating_duration_since(std::time::Instant::now());
                            let model_timeout = rem_model.min(Duration::from_secs(4));
                            match self.get_model_with_cancel(model_timeout, cancel_token) {
                                Ok(model) => {
                                    let trimmed = model.trim().to_string();
                                    if !trimmed.is_empty() {
                                        return Ok(Some(trimmed));
                                    } else {
                                        last_err = AdbError::HandshakeFailed("读取到空型号".into());
                                    }
                                }
                                Err(AdbError::Unauthorized) => return Err(AdbError::Unauthorized),
                                Err(AdbError::Cancelled) => return Err(AdbError::Cancelled),
                                Err(e_model) => {
                                    last_err = e_model;
                                }
                            }
                        } else {
                            return Ok(None);
                        }
                    } else if verify_state == AdbDeviceState::Unauthorized {
                        return Err(AdbError::Unauthorized);
                    } else {
                        let lower = e.to_lowercase();
                        if lower.contains("refused") || lower.contains("cannot connect") {
                            last_err = AdbError::PortClosed(e);
                        } else if lower.contains("offline") {
                            last_err = AdbError::DeviceOffline;
                        } else {
                            last_err = AdbError::NetworkUnreachable(e);
                        }
                    }
                }
            }

            // 重试间歇，带取消感知的微休眠
            if attempt < retries && std::time::Instant::now() < deadline {
                let sleep_ms = 150 * attempt as u64;
                let sleep_end = std::time::Instant::now() + Duration::from_millis(sleep_ms);
                while std::time::Instant::now() < sleep_end && std::time::Instant::now() < deadline
                {
                    if let Some(token) = cancel_token {
                        if token.load(std::sync::atomic::Ordering::Relaxed) {
                            return Err(AdbError::Cancelled);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }

        Err(last_err)
    }

    /// 尝试连接设备并进行有限次瞬态失败重试
    pub fn connect(&self) -> Result<(), AdbError> {
        self.connect_with_retry(3, None)
    }

    /// 显式带重试与取消令牌的连接逻辑（统一底层事务，贯穿总 deadline 与 cancel）
    pub fn connect_with_retry(
        &self,
        max_retries: usize,
        cancel_token: Option<&AtomicBool>,
    ) -> Result<(), AdbError> {
        self.connect_transaction(
            Duration::from_secs(10),
            max_retries,
            false,
            cancel_token.or(self.session_cancel.as_deref()),
        )
        .map(|_| ())
    }

    /// 确保设备处于已连接可用状态
    pub fn ensure_connected(&self) -> Result<(), AdbError> {
        let state = self.get_state();
        if state.is_connected() {
            return Ok(());
        }
        self.connect()
    }

    /// 断开并重新连接
    pub fn reconnect(&self) -> Result<(), AdbError> {
        let serial = self.serial();
        let _ = adb_run(&["disconnect", &serial]);
        std::thread::sleep(Duration::from_millis(150));
        self.connect()
    }

    /// 断开连接
    pub fn disconnect(&self) -> Result<(), AdbError> {
        let serial = self.serial();
        // Cleanup must survive cancellation of the session being disconnected.
        adb_run_with_timeout(&["disconnect", &serial], Duration::from_secs(2))
            .map_err(AdbError::CommandFailed)?;
        Ok(())
    }

    /// 校验连接并读取型号 (用于设备扫描发现，向前兼容封装)
    pub fn verify_and_get_model(&self, timeout: Duration) -> Result<String, AdbError> {
        self.verify_and_get_model_with_cancel(timeout, None)
    }

    /// 支持显式取消令牌的连接与型号验证逻辑（共用事务入口）
    pub fn verify_and_get_model_with_cancel(
        &self,
        timeout: Duration,
        cancel_token: Option<&AtomicBool>,
    ) -> Result<String, AdbError> {
        self.connect_transaction(timeout, 2, true, cancel_token)
            .and_then(|opt| {
                opt.ok_or_else(|| AdbError::HandshakeFailed("未能读取到有效设备型号".into()))
            })
    }

    /// 读取设备型号 (ro.product.model 或 ro.product.name)
    pub fn get_model(&self) -> Result<String, AdbError> {
        self.get_model_with_cancel(Duration::from_secs(10), self.session_cancel.as_deref())
    }

    /// 读取设备型号（支持自定义超时与取消令牌）
    pub fn get_model_with_cancel(
        &self,
        timeout: Duration,
        cancel_token: Option<&AtomicBool>,
    ) -> Result<String, AdbError> {
        let serial = self.serial();
        let lock = get_serial_lock(&serial);
        let _guard = lock.lock();

        let model = client::adb_run_with_cancel(
            &["-s", &serial, "shell", "getprop ro.product.model"],
            timeout,
            cancel_token,
        )
        .map_err(AdbError::CommandFailed)?;

        let trimmed = model.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }

        let name = client::adb_run_with_cancel(
            &["-s", &serial, "shell", "getprop ro.product.name"],
            timeout,
            cancel_token,
        )
        .map_err(AdbError::CommandFailed)?;

        Ok(name.trim().to_string())
    }

    /// 向设备推送文件
    pub fn push_file(&self, local_path: &Path, remote_path: &str) -> Result<(), AdbError> {
        if !local_path.exists() {
            return Err(AdbError::CommandFailed(format!(
                "本地文件不存在: {}",
                local_path.display()
            )));
        }
        let local_str = local_path.to_string_lossy().to_string();
        let serial = self.serial();
        let lock = get_serial_lock(&serial);
        let _guard = lock.lock();

        let out = adb_run_with_cancel(
            &["-s", &serial, "push", &local_str, remote_path],
            Duration::from_secs(15),
            self.session_cancel.as_deref(),
        )
        .map_err(AdbError::CommandFailed)?;

        if out.to_lowercase().contains("error") || out.to_lowercase().contains("failed") {
            return Err(AdbError::CommandFailed(out));
        }

        Ok(())
    }

    /// 执行远程 shell 命令 (受设备排他锁保护)
    pub fn shell(&self, cmd: &str) -> Result<String, AdbError> {
        let serial = self.serial();
        let lock = get_serial_lock(&serial);
        let _guard = lock.lock();
        adb_run_with_cancel(
            &["-s", &serial, "shell", cmd],
            Duration::from_secs(10),
            self.session_cancel.as_deref(),
        )
        .map_err(AdbError::CommandFailed)
    }

    /// 读取 Android Global Setting
    /// Python _refresh_page_data uses one settings snapshot command.
    pub fn get_settings(
        &self,
        keys: &[&str],
    ) -> Result<std::collections::HashMap<String, String>, AdbError> {
        let out = self.shell(&format!(
            "for k in {}; do echo $k=$(settings get global $k); done",
            keys.join(" ")
        ))?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let (key, value) = line.trim().split_once('=')?;
                if matches!(value, "" | "null" | "N/A") {
                    None
                } else {
                    Some((key.to_string(), value.to_string()))
                }
            })
            .collect())
    }
    pub fn get_setting(&self, key: &str) -> Result<String, AdbError> {
        let cmd = format!("settings get global {}", key);
        let out = self.shell(&cmd)?;
        Ok(out.trim().to_string())
    }

    /// 写入 Android Global Setting
    pub fn put_setting(&self, key: &str, val: &str) -> Result<(), AdbError> {
        let cmd = format!("settings put global {} {}", key, val);
        self.shell(&cmd)?;
        Ok(())
    }

    /// 发送广播
    pub fn broadcast(&self, action: &str, extras: &[(&str, &str)]) -> Result<(), AdbError> {
        let mut cmd = format!("am broadcast -a {}", action);
        for (k, v) in extras {
            cmd.push_str(&format!(" {} {}", k, v));
        }
        self.shell(&cmd)?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) static TEST_PROCESS_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn test_per_device_mutex_locking_between_different_instances() {
        let serial = "192.168.31.250:5555";
        let dev1 = AdbDevice::new("192.168.31.250");
        let dev2 = AdbDevice::new("192.168.31.250");

        assert_eq!(dev1.serial(), serial);
        assert_eq!(dev2.serial(), serial);

        let execution_order = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let order_clone = execution_order.clone();

        let handle = std::thread::spawn(move || {
            dev1.transaction(|| {
                order_clone.lock().push("t1_start");
                std::thread::sleep(Duration::from_millis(100));
                order_clone.lock().push("t1_end");
            });
        });

        // 稍作延时确保 t1 已经进入锁
        std::thread::sleep(Duration::from_millis(20));

        dev2.transaction(|| {
            execution_order.lock().push("t2_inside");
        });

        handle.join().unwrap();

        let order = execution_order.lock().clone();
        // 关键验证：即使由不同 AdbDevice 句柄调用，同 serial 事务也必须严格排他串行！
        assert_eq!(order, vec!["t1_start", "t1_end", "t2_inside"]);
    }

    #[test]
    fn bundled_adb_is_private_and_matches_embedded_bytes() {
        let _lock = TEST_PROCESS_LOCK.lock();
        std::env::remove_var("MIMONITOR_FAKE_ADB_PATH");
        let path = client::get_bundled_adb_path();
        assert!(path.is_absolute());
        assert!(path.starts_with(crate::config::get_app_data_dir().join("runtime")));
        assert_eq!(
            std::fs::read(&path).unwrap().as_slice(),
            include_bytes!("../../assets/runtime/adb.exe")
        );
        assert_eq!(
            std::fs::read(path.with_file_name("AdbWinApi.dll"))
                .unwrap()
                .as_slice(),
            include_bytes!("../../assets/runtime/AdbWinApi.dll")
        );
    }
    #[test]
    fn test_port_configuration_and_rejection_of_zero() {
        let _lock = TEST_PROCESS_LOCK.lock();
        // 默认端口
        std::env::remove_var("MIMONITOR_ADB_SERVER_PORT");
        std::env::remove_var("ADB_SERVER_PORT");
        assert_eq!(get_adb_server_port(), 15038);

        // 覆盖为合法端口
        std::env::set_var("MIMONITOR_ADB_SERVER_PORT", "5040");
        assert_eq!(get_adb_server_port(), 5040);

        // 尝试设置为 0 端口：必须严厉拒绝并退回有效端口
        std::env::set_var("MIMONITOR_ADB_SERVER_PORT", "0");
        assert_eq!(get_adb_server_port(), 15038);

        std::env::remove_var("MIMONITOR_ADB_SERVER_PORT");
    }

    fn create_test_fake_adb_script(name: &str, body: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "test_adb_{}_{}",
            name,
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let script = dir.join("fake_adb.cmd");
        let content = format!("@echo off\r\n{}", body);
        std::fs::write(&script, content).unwrap();
        (dir, script)
    }

    #[test]
    fn test_fake_adb_timeout_forced_kill() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let (dir, script) = create_test_fake_adb_script(
            "timeout",
            r#"powershell -NoProfile -Command "Start-Sleep -Seconds 30""#,
        );

        let start = Instant::now();
        // 设置 200ms 超时，子进程尝试 sleep 30s
        let res = client::adb_run_with_custom_path(
            Some(&script),
            &["devices"],
            Duration::from_millis(200),
            None,
        );
        let elapsed = start.elapsed();

        let _ = std::fs::remove_dir_all(&dir);

        // 关键质量门：必须超时强制 kill 退出，绝不挂起！
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("超时"));
        assert!(elapsed < Duration::from_secs(3));
    }

    #[test]
    fn test_fake_adb_large_output_pipe_drain_no_deadlock() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let (dir, script) = create_test_fake_adb_script(
            "large",
            r#"powershell -NoProfile -Command "[Console]::Out.Write('X' * 131072)""#,
        );

        // 子进程输出 128KB 文本，如果父进程不异步排空双管道，会产生管道写满死锁
        let res = client::adb_run_with_custom_path(
            Some(&script),
            &["dump"],
            Duration::from_secs(5),
            None,
        );

        let _ = std::fs::remove_dir_all(&dir);

        assert!(res.is_ok());
        let output = res.unwrap();
        assert_eq!(output.len(), 131072);
    }

    #[test]
    fn test_fake_adb_nonzero_exit_status_and_stderr() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let (dir, script) = create_test_fake_adb_script(
            "nonzero",
            r#"powershell -NoProfile -Command "[Console]::Error.WriteLine('error: device unauthorized. Check prompt.'); exit 1""#,
        );

        let res = client::adb_run_with_custom_path(
            Some(&script),
            &["shell", "ls"],
            Duration::from_secs(5),
            None,
        );

        let _ = std::fs::remove_dir_all(&dir);

        // 关键质量门：非零退出码结合 stderr 必须被判定为错误，绝不能把设备命令失败当成功！
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("退出码 1"));
        assert!(err.contains("device unauthorized"));
    }

    #[test]
    fn test_fake_adb_device_state_classification() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        // 1. 模拟 offline 状态
        let (dir1, script_offline) =
            create_test_fake_adb_script("offline", "echo offline\r\nexit /b 0");
        let state1 = client::adb_device_state_with_custom_path(
            Some(&script_offline),
            "192.168.1.50:5555",
            Duration::from_secs(2),
        );
        let _ = std::fs::remove_dir_all(&dir1);
        assert_eq!(state1, AdbDeviceState::Offline);

        // 2. 模拟 unauthorized 状态
        let (dir2, script_unauth) =
            create_test_fake_adb_script("unauth", "echo unauthorized\r\nexit /b 0");
        let state2 = client::adb_device_state_with_custom_path(
            Some(&script_unauth),
            "192.168.1.50:5555",
            Duration::from_secs(2),
        );
        let _ = std::fs::remove_dir_all(&dir2);
        assert_eq!(state2, AdbDeviceState::Unauthorized);

        // 3. 模拟正常 device 状态
        let (dir3, script_dev) = create_test_fake_adb_script("device", "echo device\r\nexit /b 0");
        let state3 = client::adb_device_state_with_custom_path(
            Some(&script_dev),
            "192.168.1.50:5555",
            Duration::from_secs(2),
        );
        let _ = std::fs::remove_dir_all(&dir3);
        assert_eq!(state3, AdbDeviceState::Device);
    }

    #[test]
    fn test_fake_adb_cancel_token_interruption() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let (dir, script) = create_test_fake_adb_script(
            "cancel",
            r#"powershell -NoProfile -Command "Start-Sleep -Seconds 30""#,
        );

        let cancel_token = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_token.clone();

        // 50ms 后触发取消
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancel_clone.store(true, Ordering::SeqCst);
        });

        let start = Instant::now();
        let res = client::adb_run_with_custom_path(
            Some(&script),
            &["shell", "job"],
            Duration::from_secs(10),
            Some(&cancel_token),
        );
        let elapsed = start.elapsed();

        let _ = std::fs::remove_dir_all(&dir);

        // 必须立即响应取消退出，耗时远小于 10 秒
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("取消"));
        assert!(elapsed < Duration::from_secs(2));
    }

    #[test]
    fn test_fake_adb_lock_contention_queue_cancel_and_block() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let (dir, script) = create_test_fake_adb_script(
            "lock_holder",
            r#"powershell -NoProfile -Command "Start-Sleep -Milliseconds 800""#,
        );

        let script_clone = script.clone();
        // 线程 1 率先持有 ADB 命令锁执行 800ms
        let t1 = std::thread::spawn(move || {
            let _ = client::adb_run_with_custom_path(
                Some(&script_clone),
                &["shell", "long_job"],
                Duration::from_secs(3),
                None,
            );
        });

        // 确保线程 1 已经进入并持锁
        std::thread::sleep(Duration::from_millis(50));

        // 线程 2 排队等待锁，但携带了取消令牌，在 60ms 时触发取消
        let cancel_token = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            cancel_clone.store(true, Ordering::SeqCst);
        });

        let start = Instant::now();
        let res2 = client::adb_run_with_custom_path(
            Some(&script),
            &["devices"],
            Duration::from_secs(5),
            Some(&cancel_token),
        );
        let elapsed = start.elapsed();

        t1.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        // 关键断言：排队等待拿锁时，必须响应取消，耗时远小于持有锁的 800ms！
        assert!(res2.is_err());
        assert!(res2.unwrap_err().contains("取消"));
        assert!(elapsed < Duration::from_millis(500));
    }

    #[test]
    fn test_fake_adb_transient_handshake_retry_success() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        // 模拟前一次握手失败、第二次成功的瞬态网络抖动重试
        let counter_id = uuid::Uuid::new_v4().simple().to_string();
        let counter_file = std::env::temp_dir().join(format!("fake_adb_retry_{}.txt", counter_id));
        let counter_path_str = counter_file.to_string_lossy().to_string();

        let (dir, script) = create_test_fake_adb_script(
            "retry",
            &format!(
                r#"setlocal enabledelayedexpansion
set "CFILE={}"
set COUNT=0
if exist "!CFILE!" (
    set /p COUNT=<"!CFILE!"
)
set /a COUNT+=1
echo !COUNT!>"!CFILE!"
echo %* | findstr /i "connect" >nul && (
    if !COUNT! equ 1 (
        echo cannot connect to target: connection refused
        exit /b 0
    ) else (
        echo connected to 192.168.1.88:5555
        exit /b 0
    )
)
echo %* | findstr /i "get-state" >nul && (
    if !COUNT! leq 2 (
        echo offline
        exit /b 0
    ) else (
        echo device
        exit /b 0
    )
)
echo OK
exit /b 0
"#,
                counter_path_str
            ),
        );

        // 注入临时假 adb 执行
        std::env::set_var("MIMONITOR_FAKE_ADB_PATH", script.to_str().unwrap());

        let dev = AdbDevice::new("192.168.1.88");
        let connect_res = dev.connect_with_retry(3, None);

        std::env::remove_var("MIMONITOR_FAKE_ADB_PATH");
        let _ = std::fs::remove_file(&counter_file);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            connect_res.is_ok(),
            "重试机制应在第 2~3 次成功建立连接: {:?}",
            connect_res
        );
    }

    #[test]
    fn test_fake_adb_dual_128kb_output_pipe_drain_no_deadlock() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let (dir, script) = create_test_fake_adb_script(
            "dual_large",
            r#"powershell -NoProfile -Command "[Console]::Out.Write('X' * 131072); [Console]::Error.Write('Y' * 131072)""#,
        );

        // 子进程同时输出 128KB stdout 和 128KB stderr，双管道并发排空防止死锁
        let res = client::adb_run_with_custom_path(
            Some(&script),
            &["dual_dump"],
            Duration::from_secs(8),
            None,
        );

        let _ = std::fs::remove_dir_all(&dir);

        assert!(res.is_ok(), "双管道 128KB 输出应成功排空返回: {:?}", res);
        let output = res.unwrap();
        assert_eq!(
            output.len(),
            131072,
            "stdout 尾部数据不得提前丢失且完整读取"
        );
    }

    #[test]
    fn test_fake_adb_continuous_output_and_grandchild_pipe_cancel_and_timeout() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();

        let test_uuid = uuid::Uuid::new_v4().simple().to_string();
        let pid_file =
            std::env::temp_dir().join(format!("fake_adb_grandchild_pid_{}.txt", test_uuid));
        let pid_file_str = pid_file.to_string_lossy().to_string().replace('\\', "/");

        // mock 衍生孙子 powershell 进程持续输出数据并持有管道，同时将孙子进程真实 PID 写入唯一临时文件
        let script_body = format!(
            r#"powershell -NoProfile -Command "$p = Start-Process powershell -ArgumentList '-NoProfile','-Command','while($true){{[Console]::Out.WriteLine(\"spam_stream\");Start-Sleep -Milliseconds 10}}' -PassThru; [System.IO.File]::WriteAllText('{}', $p.Id.ToString()); $p.WaitForExit()""#,
            pid_file_str
        );

        let (dir, script) = create_test_fake_adb_script("continuous_stream", &script_body);

        let cancel_token = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_token.clone();
        let pid_file_clone = pid_file.clone();

        let recorded_pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let recorded_pid_clone = recorded_pid.clone();

        // 异步等待并确认子孙进程确实已被创建、读出其真实 PID 并确认其正在运行，然后再触发取消
        let trigger_thread = std::thread::spawn(move || {
            let wait_start = Instant::now();
            let mut detected_pid = 0u32;
            while wait_start.elapsed() < Duration::from_secs(5) {
                if let Ok(content) = std::fs::read_to_string(&pid_file_clone) {
                    let trimmed = content.trim();
                    if let Ok(pid) = trimmed.parse::<u32>() {
                        if client::win32_job::is_process_running(pid) {
                            detected_pid = pid;
                            break;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if detected_pid > 0 {
                recorded_pid_clone.store(detected_pid, Ordering::SeqCst);
                cancel_clone.store(true, Ordering::SeqCst);
            }
        });

        let start = Instant::now();
        let res_cancel = client::adb_run_with_custom_path(
            Some(&script),
            &["stream"],
            Duration::from_secs(6),
            Some(&cancel_token),
        );
        let elapsed = start.elapsed();

        trigger_thread.join().unwrap();
        let grandchild_pid = recorded_pid.load(Ordering::SeqCst);

        // 关键断言 1：验证子孙进程已被真实创建并获取到有效 PID
        assert!(grandchild_pid > 0, "必须确认子孙进程已成功创建并记录了 PID");

        // 关键断言 2：操作按时退出
        assert!(res_cancel.is_err(), "操作应返回取消错误");
        assert!(res_cancel.unwrap_err().contains("取消"));
        assert!(
            elapsed < Duration::from_secs(3),
            "取消必须按时返回，耗时 {:?}",
            elapsed
        );

        // 关键断言 3：真实断言子孙进程已被彻底终止，绝对无子孙进程残留！
        let wait_exit_start = Instant::now();
        let mut exited = false;
        while wait_exit_start.elapsed() < Duration::from_secs(2) {
            if !client::win32_job::is_process_running(grandchild_pid) {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }

        // 兜底清理（防止断言失败导致孤儿进程）
        client::win32_job::kill_process_by_pid_fallback(grandchild_pid);
        let _ = std::fs::remove_file(&pid_file);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            exited,
            "断言失败：子孙进程 PID {} 仍在运行，未被完整杀灭！",
            grandchild_pid
        );
    }

    #[test]
    fn test_fake_adb_pipe_output_overflow_truncated_error() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        // 尝试输出超过 2MB (2100000 字节)
        let (dir, script) = create_test_fake_adb_script(
            "overflow",
            r#"powershell -NoProfile -Command "[Console]::Out.Write('Z' * 2100000)""#,
        );

        let res = client::adb_run_with_custom_path(
            Some(&script),
            &["dump_huge"],
            Duration::from_secs(6),
            None,
        );
        let _ = std::fs::remove_dir_all(&dir);

        // 关键质量门：输出超限必须明确返回截断错误，杜绝静默截断当成功
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("截断"), "错误信息应明确提示截断: {}", err);
    }

    #[test]
    fn test_fake_adb_verify_and_get_model_transient_retry_success() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let counter_id = uuid::Uuid::new_v4().simple().to_string();
        let counter_file = std::env::temp_dir().join(format!("fake_adb_verify_{}.txt", counter_id));
        let counter_path_str = counter_file.to_string_lossy().to_string();

        let (dir, script) = create_test_fake_adb_script(
            "verify_retry",
            &format!(
                r#"setlocal enabledelayedexpansion
set "CFILE={}"
set COUNT=0
if exist "!CFILE!" (
    set /p COUNT=<"!CFILE!"
)
set /a COUNT+=1
echo !COUNT!>"!CFILE!"
echo %* | findstr /i "connect" >nul && (
    if !COUNT! equ 1 (
        echo cannot connect to 192.168.1.99:5555: connection refused
        exit /b 0
    ) else (
        echo connected to 192.168.1.99:5555
        exit /b 0
    )
)
echo %* | findstr /i "get-state" >nul && (
    if !COUNT! leq 2 (
        echo offline
        exit /b 0
    ) else (
        echo device
        exit /b 0
    )
)
echo %* | findstr /i "getprop ro.product.model" >nul && (
    if !COUNT! leq 3 (
        echo.
        exit /b 1
    ) else (
        echo MiTV-Test
        exit /b 0
    )
)
echo OK
exit /b 0
"#,
                counter_path_str
            ),
        );

        std::env::set_var("MIMONITOR_FAKE_ADB_PATH", script.to_str().unwrap());

        let dev = AdbDevice::new("192.168.1.99");
        let model_res = dev.verify_and_get_model(Duration::from_secs(6));

        std::env::remove_var("MIMONITOR_FAKE_ADB_PATH");
        let _ = std::fs::remove_file(&counter_file);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            model_res.is_ok(),
            "扫描验证应支持瞬时型号读取失败重试并最终成功获取型号: {:?}",
            model_res
        );
        assert_eq!(model_res.unwrap(), "MiTV-Test");
    }

    #[test]
    fn test_fake_adb_unauthorized_stops_retry_immediately() {
        let _lock = TEST_PROCESS_LOCK.lock();
        client::unblock_adb_spawns();
        let (dir, script) = create_test_fake_adb_script(
            "unauth_stop",
            r#"echo %* | findstr /i "connect" >nul && (
    echo connected to 192.168.1.77:5555
    exit /b 0
)
echo %* | findstr /i "get-state" >nul && (
    echo unauthorized
    exit /b 0
)
echo OK
exit /b 0
"#,
        );

        std::env::set_var("MIMONITOR_FAKE_ADB_PATH", script.to_str().unwrap());

        let dev = AdbDevice::new("192.168.1.77");
        let connect_res = dev.connect_with_retry(3, None);

        std::env::remove_var("MIMONITOR_FAKE_ADB_PATH");
        let _ = std::fs::remove_dir_all(&dir);

        // 关键质量门：遇到设备未授权 (Unauthorized)，应立即中止重试并向用户报错提示授权
        assert_eq!(connect_res, Err(AdbError::Unauthorized));
    }
}
