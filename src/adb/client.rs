use parking_lot::{Mutex, ReentrantMutex};
use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[allow(non_snake_case, non_upper_case_globals, dead_code)]
pub(crate) mod win32_job {
    use std::ffi::c_void;
    use std::ptr::null;

    pub const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    pub const JobObjectExtendedLimitInformation: u32 = 9;
    pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;
    pub const CREATE_SUSPENDED: u32 = 0x00000004;

    #[repr(C)]
    #[derive(Default)]
    pub struct IO_COUNTERS {
        pub ReadOperationCount: u64,
        pub WriteOperationCount: u64,
        pub OtherOperationCount: u64,
        pub ReadTransferCount: u64,
        pub WriteTransferCount: u64,
        pub OtherTransferCount: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
        pub PerProcessUserTimeLimit: i64,
        pub PerJobUserTimeLimit: i64,
        pub LimitFlags: u32,
        pub MinimumWorkingSetSize: usize,
        pub MaximumWorkingSetSize: usize,
        pub ActiveProcessLimit: u32,
        pub Affinity: usize,
        pub PriorityClass: u32,
        pub SchedulingClass: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        pub BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION,
        pub IoInfo: IO_COUNTERS,
        pub ProcessMemoryLimit: usize,
        pub JobMemoryLimit: usize,
        pub PeakProcessMemoryLimit: usize,
        pub PeakJobMemoryLimit: usize,
    }

    extern "system" {
        pub fn CreateJobObjectW(lpJobAttributes: *const c_void, lpName: *const u16) -> *mut c_void;
        pub fn AssignProcessToJobObject(hJob: *mut c_void, hProcess: *mut c_void) -> i32;
        pub fn SetInformationJobObject(
            hJob: *mut c_void,
            JobObjectInformationClass: u32,
            lpJobInformation: *const c_void,
            cbJobInformationLength: u32,
        ) -> i32;
        pub fn TerminateJobObject(hJob: *mut c_void, uExitCode: u32) -> i32;
        pub fn CloseHandle(hObject: *mut c_void) -> i32;
        pub fn PeekNamedPipe(
            hNamedPipe: *mut c_void,
            lpBuffer: *mut c_void,
            nBufferSize: u32,
            lpBytesRead: *mut u32,
            lpTotalBytesAvail: *mut u32,
            lpBytesLeftThisMessage: *mut u32,
        ) -> i32;
        pub fn GetModuleHandleA(lpModuleName: *const i8) -> *mut c_void;
        pub fn GetProcAddress(hModule: *mut c_void, lpProcName: *const i8) -> *mut c_void;
    }

    pub struct JobGuard {
        handle: *mut c_void,
    }

    // Owned, non-inheritable kernel handle. Moving it does not access thread-local
    // state; operations on the server guard are serialized by OWNED_ADB_SERVER.
    unsafe impl Send for JobGuard {}

    impl JobGuard {
        pub fn new() -> Option<Self> {
            let h = unsafe { CreateJobObjectW(null(), null()) };
            if h.is_null() {
                return None;
            }
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // 严格无 breakaway，确保所有通过客户端衍生（如 mock cmd/powershell）的子孙进程一律被捕获，并在关闭/超时时彻底被杀
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &mut info as *mut _ as *mut _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                unsafe { CloseHandle(h) };
                return None;
            }
            Some(Self { handle: h })
        }

        pub fn assign(&self, process_handle: *mut c_void) -> bool {
            if self.handle.is_null() || process_handle.is_null() {
                return false;
            }
            unsafe { AssignProcessToJobObject(self.handle, process_handle) != 0 }
        }

        pub fn terminate(&self, exit_code: u32) {
            if !self.handle.is_null() {
                unsafe {
                    TerminateJobObject(self.handle, exit_code);
                }
            }
        }
    }

    impl Drop for JobGuard {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    CloseHandle(self.handle);
                }
            }
        }
    }

    /// 恢复由 CREATE_SUSPENDED 挂起的进程执行
    pub fn resume_process(process_handle: *mut c_void) -> Result<(), String> {
        if process_handle.is_null() {
            return Err("进程句柄无效".to_string());
        }
        unsafe {
            let ntdll = GetModuleHandleA(b"ntdll.dll\0".as_ptr() as *const i8);
            if ntdll.is_null() {
                return Err("获取 ntdll.dll 模块句柄失败".to_string());
            }
            let func = GetProcAddress(ntdll, b"NtResumeProcess\0".as_ptr() as *const i8);
            if func.is_null() {
                return Err("找不到 NtResumeProcess 导出函数".to_string());
            }
            let nt_resume: extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(func);
            let status = nt_resume(process_handle);
            if status != 0 {
                return Err(format!(
                    "NtResumeProcess 调用失败 (NTSTATUS: 0x{:08X})",
                    status as u32
                ));
            }
            Ok(())
        }
    }

    #[cfg(test)]
    pub fn is_process_running(pid: u32) -> bool {
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;
        extern "system" {
            fn OpenProcess(
                dwDesiredAccess: u32,
                bInheritHandle: i32,
                dwProcessId: u32,
            ) -> *mut c_void;
            fn GetExitCodeProcess(hProcess: *mut c_void, lpExitCode: *mut u32) -> i32;
        }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            if ok != 0 {
                code == STILL_ACTIVE
            } else {
                false
            }
        }
    }

    #[cfg(test)]
    pub fn kill_process_by_pid_fallback(pid: u32) {
        const PROCESS_TERMINATE: u32 = 0x0001;
        extern "system" {
            fn OpenProcess(
                dwDesiredAccess: u32,
                bInheritHandle: i32,
                dwProcessId: u32,
            ) -> *mut c_void;
            fn TerminateProcess(hProcess: *mut c_void, uExitCode: u32) -> i32;
        }
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !h.is_null() {
                TerminateProcess(h, 1);
                CloseHandle(h);
            }
        }
    }
}

pub const MAX_PIPE_BUFFER_SIZE: usize = 2 * 1024 * 1024; // 2MB 单管道合理读取上限

/// 启动可取消、有界且确保尾部排空的非挂起管道异步读取线程（支持输出截断精准感知）
fn spawn_bounded_pipe_reader<R>(
    mut pipe: R,
    stop_immediate: Arc<AtomicBool>,
    child_exited: Arc<AtomicBool>,
    truncated: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<u8>>
where
    R: Read + AsRawHandle + Send + 'static,
{
    std::thread::spawn(move || {
        let raw_handle = pipe.as_raw_handle() as *mut std::ffi::c_void;
        let mut result = Vec::new();
        let mut buffer = [0u8; 8192];

        loop {
            // 1. 超时或外部显式取消信号：立即终止读取并退出线程
            if stop_immediate.load(Ordering::Relaxed) {
                break;
            }

            let mut bytes_avail = 0u32;
            let peek_ok = unsafe {
                win32_job::PeekNamedPipe(
                    raw_handle,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut bytes_avail,
                    std::ptr::null_mut(),
                )
            };

            if peek_ok == 0 {
                // 管道对端写句柄已全部关闭或破裂
                break;
            }

            if bytes_avail > 0 {
                let remaining_capacity = MAX_PIPE_BUFFER_SIZE.saturating_sub(result.len());
                if remaining_capacity == 0 {
                    // 已达 2MB 上限且管道中仍有数据到达，标记截断并终止读取
                    truncated.store(true, Ordering::SeqCst);
                    break;
                }

                let to_read = (bytes_avail as usize)
                    .min(buffer.len())
                    .min(remaining_capacity);
                if to_read == 0 {
                    truncated.store(true, Ordering::SeqCst);
                    break;
                }
                match pipe.read(&mut buffer[..to_read]) {
                    Ok(0) => break,
                    Ok(n) => {
                        result.extend_from_slice(&buffer[..n]);
                        if (bytes_avail as usize) > remaining_capacity {
                            truncated.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                    Err(_) => break,
                }
            } else {
                // 当前管道暂无数据
                // 若子进程已确认退出，说明所有尾部输出已经完整排空，正常安全结束
                if child_exited.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        result
    })
}

pub const CREATE_NO_WINDOW: u32 = 0x08000000;
pub const DEFAULT_ADB_SERVER_PORT: u16 = 15038;
pub const ADB_DEVICE_PORT: u16 = 5555;

pub const ADB_DISCONNECTED_MARKERS: &[&str] = &[
    "offline",
    "unauthorized",
    "no devices",
    "not found",
    "cannot",
    "failed",
    "error:",
];

static ADB_COMMAND_LOCK: ReentrantMutex<()> = ReentrantMutex::new(());
static ADB_SPAWN_GATE: Mutex<()> = Mutex::new(());
static ADB_SPAWN_BLOCKED: AtomicBool = AtomicBool::new(false);
static OWNED_ADB_SERVER: Mutex<Option<OwnedAdbServer>> = Mutex::new(None);

struct OwnedAdbServer {
    child: Child,
    job: win32_job::JobGuard,
}

impl Drop for OwnedAdbServer {
    fn drop(&mut self) {
        // Only this exact process tree belongs to us. Never send kill-server,
        // enumerate processes by name, or terminate an already-running server.
        self.job.terminate(0);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Also handles CLI returns. On an abnormal exit Windows closes the job handle
/// and JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE provides the same ownership boundary.
pub struct AdbLifetime;
impl Drop for AdbLifetime {
    fn drop(&mut self) {
        cleanup_adb_processes(false);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbDeviceState {
    Device,
    Offline,
    Unauthorized,
    NotFound,
    Error(String),
    Unknown,
}

impl AdbDeviceState {
    pub fn is_connected(&self) -> bool {
        matches!(self, AdbDeviceState::Device)
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            AdbDeviceState::Device => "已连接",
            AdbDeviceState::Offline => "设备离线",
            AdbDeviceState::Unauthorized => "设备未授权",
            AdbDeviceState::NotFound => "未找到设备",
            AdbDeviceState::Error(_) => "连接异常",
            AdbDeviceState::Unknown => "状态未知",
        }
    }
}

/// 检查文本中是否包含表示断开的标记
pub fn adb_text_has_disconnected_marker(text: &str) -> bool {
    let lower = text.to_lowercase();
    ADB_DISCONNECTED_MARKERS.iter().any(|&m| lower.contains(m))
}

/// 格式化设备序列号 (ip:5555)
pub fn format_adb_serial(ip: &str, port: u16) -> String {
    let clean_ip = ip.trim();
    if clean_ip.is_empty() {
        String::new()
    } else {
        format!("{}:{}", clean_ip, port)
    }
}

/// 获取应用专用的 ADB 运行端口（隔离宿主默认 5037 端口，兼容环境变量覆盖，拒绝 0 端口）
pub fn get_adb_server_port() -> u16 {
    if let Ok(port_str) = env::var("MIMONITOR_ADB_SERVER_PORT") {
        if let Ok(p) = port_str.trim().parse::<u16>() {
            if p > 0 {
                return p;
            }
        }
    }
    DEFAULT_ADB_SERVER_PORT
}

/// 确保将随程序分发的 adb 资源部署并获取其有效路径
pub fn get_bundled_adb_path() -> PathBuf {
    // 0. 优先允许测试环境下注入受控 Fake ADB 二进制 (生产环境不读取)
    #[cfg(test)]
    if let Ok(override_path) = env::var("MIMONITOR_FAKE_ADB_PATH") {
        return PathBuf::from(override_path);
    }

    // Private runtime is embedded in the application. Never consult CWD or PATH.
    let dir = crate::config::get_app_data_dir()
        .join("runtime")
        .join(env!("CARGO_PKG_VERSION"));
    let resources: &[(&str, &[u8])] = &[
        ("adb.exe", include_bytes!("../../assets/runtime/adb.exe")),
        (
            "AdbWinApi.dll",
            include_bytes!("../../assets/runtime/AdbWinApi.dll"),
        ),
        (
            "AdbWinUsbApi.dll",
            include_bytes!("../../assets/runtime/AdbWinUsbApi.dll"),
        ),
    ];
    static DEPLOY: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    let deployed = DEPLOY.get_or_init(|| {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (name, bytes) in resources {
            let path = dir.join(name);
            if std::fs::read(&path).ok().as_deref() != Some(*bytes) {
                let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
                std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
            }
        }
        // The portable package supplies the official redistributable beside the EXE.
        if let Some(parent) = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            let crt = parent.join("vcruntime140.dll");
            if crt.is_file() {
                let bytes = std::fs::read(crt).map_err(|e| e.to_string())?;
                let target = dir.join("vcruntime140.dll");
                if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
                    std::fs::write(target, bytes).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    });
    if let Err(error) = deployed {
        crate::logger::log_adb(&format!("内置 ADB 解包失败：{error}"));
        // A missing private path produces an explicit launch error; no external fallback.
        return dir.join("unavailable").join("adb.exe");
    }
    dir.join("adb.exe")
}

/// 检查特定端口的 ADB Server 是否处于监听就绪状态
pub fn is_adb_server_alive(timeout: Duration) -> bool {
    #[cfg(test)]
    if env::var("MIMONITOR_FAKE_ADB_PATH").is_ok() {
        return true;
    }
    let port = get_adb_server_port();
    let addr = format!("127.0.0.1:{}", port);
    let probe = || -> std::io::Result<bool> {
        let address = addr.parse().map_err(|_| std::io::ErrorKind::InvalidInput)?;
        let mut stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(b"000chost:version")?;
        let mut reply = [0u8; 12];
        stream.read_exact(&mut reply)?;
        // Do not let an adb client automatically kill an incompatible server.
        Ok(&reply[..8] == b"OKAY0004" && &reply[8..] == b"0029")
    };
    probe().unwrap_or(false)
}

/// 阻止后续 ADB 子进程衍生（在应用开始退出时调用）
pub fn block_adb_spawns() {
    let _gate = ADB_SPAWN_GATE.lock();
    ADB_SPAWN_BLOCKED.store(true, Ordering::SeqCst);
}

/// 恢复允许 ADB 子进程衍生
pub fn unblock_adb_spawns() {
    ADB_SPAWN_BLOCKED.store(false, Ordering::SeqCst);
}

pub fn is_adb_spawn_blocked() -> bool {
    ADB_SPAWN_BLOCKED.load(Ordering::Relaxed)
}

/// 确保独立端口的 ADB Server 启动并处于就绪监听状态（向前兼容封装）
pub fn ensure_adb_server_ready(timeout: Duration) -> Result<(), String> {
    ensure_adb_server_ready_with_cancel(timeout, None)
}

/// 确保独立端口的 ADB Server 启动并处于就绪监听状态（带确定性超时、取消感知与防破裂保护）
pub fn ensure_adb_server_ready_with_cancel(
    timeout: Duration,
    cancel_token: Option<&AtomicBool>,
) -> Result<(), String> {
    #[cfg(test)]
    if env::var("MIMONITOR_FAKE_ADB_PATH").is_ok() {
        return Ok(());
    }

    if is_adb_spawn_blocked() {
        return Err("应用正在退出，已取消启动 ADB Server".to_string());
    }
    if let Some(token) = cancel_token {
        if token.load(Ordering::Relaxed) {
            return Err("操作已取消".to_string());
        }
    }

    if is_adb_server_alive(Duration::from_millis(150)) {
        return if OWNED_ADB_SERVER
            .lock()
            .as_mut()
            .is_some_and(|server| matches!(server.child.try_wait(), Ok(None)))
        {
            Ok(())
        } else {
            Err(format!("ADB 专属端口 {} 已被其他服务占用；未复用外部 ADB，请关闭本应用的其他实例或设置 MIMONITOR_ADB_SERVER_PORT", get_adb_server_port()))
        };
    }

    let deadline = Instant::now() + timeout;

    // 锁排队阶段计入总超时，并持续响应取消令牌与退出状态
    let _lock = loop {
        if is_adb_spawn_blocked() {
            return Err("应用正在退出，已取消启动 ADB Server".to_string());
        }
        if let Some(token) = cancel_token {
            if token.load(Ordering::Relaxed) {
                return Err("操作已取消".to_string());
            }
        }
        if Instant::now() >= deadline {
            return Err("等待获取 ADB Server 启动锁超时".to_string());
        }
        if let Some(guard) = ADB_COMMAND_LOCK.try_lock() {
            break guard;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    if is_adb_spawn_blocked() {
        return Err("应用正在退出，已取消启动 ADB Server".to_string());
    }
    if let Some(token) = cancel_token {
        if token.load(Ordering::Relaxed) {
            return Err("操作已取消".to_string());
        }
    }

    if is_adb_server_alive(Duration::from_millis(150)) {
        return if OWNED_ADB_SERVER
            .lock()
            .as_mut()
            .is_some_and(|server| matches!(server.child.try_wait(), Ok(None)))
        {
            Ok(())
        } else {
            Err(format!("ADB 专属端口 {} 已被其他服务占用；未复用外部 ADB，请关闭本应用的其他实例或设置 MIMONITOR_ADB_SERVER_PORT", get_adb_server_port()))
        };
    }

    let port = get_adb_server_port();
    crate::logger::log_adb(&format!(
        "ADB Server 未运行，开始启动 (端口 {}, 超时 {:?})...",
        port, timeout
    ));

    let adb_path = get_bundled_adb_path();
    // The bundled Windows adb rejects host-qualified listen sockets. With no
    // -a, tcp:PORT binds loopback (verified in the native startup test).
    let socket = format!("tcp:{}", port);
    let server_log = crate::logger::get_log_dir().join("adb-server.log");
    let server_stderr = std::fs::File::create(&server_log)
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    let mut cmd = Command::new(&adb_path);
    cmd.args(["-L", &socket, "server", "nodaemon"])
        // This toolbox discovers fixed TCP ADB endpoints itself. Disabling mDNS,
        // USB and emulator discovery avoids unsolicited multicast/listening and
        // unrelated USB devices; these variables affect this child only.
        .env("ADB_MDNS", "0")
        .env("ADB_USB", "0")
        .env("ADB_EMU", "0")
        .env_remove("ADB_TRACE")
        .env_remove("ADB_SERVER_SOCKET")
        .creation_flags(CREATE_NO_WINDOW | win32_job::CREATE_SUSPENDED)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(server_stderr);
    let mut child = {
        let _gate = ADB_SPAWN_GATE.lock();
        if is_adb_spawn_blocked() || cancel_token.is_some_and(|t| t.load(Ordering::Relaxed)) {
            return Err("ADB Server 启动已取消".into());
        }
        cmd.spawn()
            .map_err(|e| format!("启动 ADB Server 失败: {}", e))?
    };
    let job = match win32_job::JobGuard::new() {
        Some(job) => job,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("无法创建 ADB Server 生命周期保护".into());
        }
    };
    if !job.assign(child.as_raw_handle() as *mut _) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("无法隔离本应用的 ADB Server，已取消启动".into());
    }
    let mut owned = OwnedAdbServer { child, job };
    win32_job::resume_process(owned.child.as_raw_handle() as *mut _)?;
    loop {
        if is_adb_spawn_blocked() {
            return Err("应用正在退出，已取消启动 ADB Server".into());
        }
        if cancel_token.is_some_and(|t| t.load(Ordering::Relaxed)) {
            return Err("操作已取消".into());
        }
        if Instant::now() >= deadline {
            return Err(format!("ADB Server 启动超时，端口 {} 未就绪", port));
        }
        if let Some(status) = owned.child.try_wait().map_err(|e| e.to_string())? {
            // A competing server is never adopted or terminated.
            if is_adb_server_alive(Duration::from_millis(100)) {
                return Err("ADB 专属端口被其他服务抢占，已停止连接；未复用外部 ADB".into());
            }
            let detail = std::fs::File::open(&server_log)
                .ok()
                .and_then(|file| {
                    let mut bytes = Vec::new();
                    file.take(2048).read_to_end(&mut bytes).ok()?;
                    Some(decode_adb_output(&bytes))
                })
                .unwrap_or_default();
            return Err(format!("ADB Server 启动失败 ({status}): {detail}"));
        }
        if is_adb_server_alive(Duration::from_millis(100)) {
            let mut slot = OWNED_ADB_SERVER.lock();
            // Publish and shutdown use the same lock. A cancellation or exit
            // between readiness and publication must drop this local job.
            if is_adb_spawn_blocked() || cancel_token.is_some_and(|t| t.load(Ordering::Relaxed)) {
                return Err("ADB Server 启动已取消".into());
            }
            *slot = Some(owned);
            crate::logger::log_adb(&format!(
                "本应用 ADB Server 已就绪：127.0.0.1:{}；退出时自动回收",
                port
            ));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 解码原始字节输出（优先 UTF-8，回退 GBK / Latin1，杜绝 UnicodeDecodeError）
pub fn decode_adb_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.trim().to_string(),
        Err(_) => {
            let (cow, _, had_errors) = encoding_rs::GBK.decode(bytes);
            if !had_errors {
                cow.trim().to_string()
            } else {
                String::from_utf8_lossy(bytes).trim().to_string()
            }
        }
    }
}

/// 执行指定参数的 ADB 命令
pub fn adb_run(args: &[&str]) -> Result<String, String> {
    adb_run_with_timeout(args, Duration::from_secs(10))
}

/// 执行带明确超时、双管道异步排空（防死锁）与退出码校验的 ADB 子进程调用
pub fn adb_run_with_timeout(args: &[&str], timeout: Duration) -> Result<String, String> {
    adb_run_with_cancel(args, timeout, None)
}

/// 带超时和显式取消令牌的底层 ADB 命令调用
pub fn adb_run_with_cancel(
    args: &[&str],
    timeout: Duration,
    cancel_token: Option<&AtomicBool>,
) -> Result<String, String> {
    adb_run_with_custom_path(None, args, timeout, cancel_token)
}

/// 支持指定自定义 ADB 可执行文件路径（专供仿真测试注入）的底层调用
pub fn adb_run_with_custom_path(
    custom_adb: Option<&Path>,
    args: &[&str],
    timeout: Duration,
    cancel_token: Option<&AtomicBool>,
) -> Result<String, String> {
    if is_adb_spawn_blocked() {
        return Err("应用正在退出，已取消执行 ADB 命令".to_string());
    }
    if let Some(token) = cancel_token {
        if token.load(Ordering::Relaxed) {
            return Err("操作已取消".to_string());
        }
    }

    let deadline = Instant::now() + timeout;

    // 锁排队阶段计入总超时，并持续响应取消令牌与退出状态
    let _lock = loop {
        if is_adb_spawn_blocked() {
            return Err("应用正在退出，已取消执行 ADB 命令".to_string());
        }
        if let Some(token) = cancel_token {
            if token.load(Ordering::Relaxed) {
                return Err("操作已取消".to_string());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!("等待 ADB 命令锁超时 ({:?})", timeout));
        }
        if let Some(guard) = ADB_COMMAND_LOCK.try_lock() {
            break guard;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    // spawn 前复核取消和退出状态
    if is_adb_spawn_blocked() {
        return Err("应用正在退出，已取消执行 ADB 命令".to_string());
    }
    if let Some(token) = cancel_token {
        if token.load(Ordering::Relaxed) {
            return Err("操作已取消".to_string());
        }
    }
    if Instant::now() >= deadline {
        return Err(format!("ADB 命令等待锁已耗尽超时 ({:?})", timeout));
    }

    let adb_path = custom_adb
        .map(PathBuf::from)
        .unwrap_or_else(get_bundled_adb_path);
    // All production entry points must use our owned server launcher. Otherwise
    // an ordinary adb command can silently daemonize a server after a crash.
    if custom_adb.is_none() {
        ensure_adb_server_ready_with_cancel(
            deadline.saturating_duration_since(Instant::now()),
            cancel_token,
        )?;
    }
    let port_str = get_adb_server_port().to_string();

    let mut full_args = vec!["-H", "127.0.0.1", "-P", &port_str];
    full_args.extend_from_slice(args);

    let mut cmd = Command::new(&adb_path);
    cmd.args(&full_args)
        .env_remove("ADB_SERVER_SOCKET")
        .creation_flags(CREATE_NO_WINDOW | win32_job::CREATE_SUSPENDED)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = {
        let _gate = ADB_SPAWN_GATE.lock();
        if is_adb_spawn_blocked() || cancel_token.is_some_and(|t| t.load(Ordering::Relaxed)) {
            return Err("ADB 命令已取消，未启动进程".into());
        }
        if Instant::now() >= deadline {
            return Err("等待 ADB 服务已耗尽命令超时".into());
        }
        cmd.spawn()
            .map_err(|e| format!("启动 ADB 进程失败: {}", e))?
    };

    // 创建只保护本次客户端进程树的 JobGuard（无 breakaway，确保所有后代进程一律锁入 Job）
    let job = match win32_job::JobGuard::new() {
        Some(j) => j,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("创建 Win32 Job 隔离对象失败".to_string());
        }
    };
    if !job.assign(child.as_raw_handle() as *mut _) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("将 ADB 子进程分配至 Win32 Job 隔离对象失败".to_string());
    }
    // 确保 assign 完成后再恢复子进程主线程执行，防止任何子孙进程在入 Job 前被衍生
    if let Err(e) = win32_job::resume_process(child.as_raw_handle() as *mut _) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("恢复 ADB 挂起子进程失败: {}", e));
    }

    let stdout_pipe = child.stdout.take().expect("Failed to open stdout");
    let stderr_pipe = child.stderr.take().expect("Failed to open stderr");

    let stop_immediate = Arc::new(AtomicBool::new(false));
    let child_exited_flag = Arc::new(AtomicBool::new(false));
    let stdout_truncated = Arc::new(AtomicBool::new(false));
    let stderr_truncated = Arc::new(AtomicBool::new(false));

    // 启动两条有界非挂起 reader 线程排空 stdout 与 stderr（支持截断检测）
    let stdout_reader = spawn_bounded_pipe_reader(
        stdout_pipe,
        stop_immediate.clone(),
        child_exited_flag.clone(),
        stdout_truncated.clone(),
    );
    let stderr_reader = spawn_bounded_pipe_reader(
        stderr_pipe,
        stop_immediate.clone(),
        child_exited_flag.clone(),
        stderr_truncated.clone(),
    );

    let mut child_exited = false;
    let mut exit_status = None;

    while Instant::now() < deadline {
        if is_adb_spawn_blocked() {
            stop_immediate.store(true, Ordering::SeqCst);
            job.terminate(1);
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("应用正在退出，已取消执行 ADB 命令".to_string());
        }

        if let Some(token) = cancel_token {
            if token.load(Ordering::Relaxed) {
                stop_immediate.store(true, Ordering::SeqCst);
                job.terminate(1);
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err("操作已取消".to_string());
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                child_exited = true;
                exit_status = Some(status);
                break;
            }
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                stop_immediate.store(true, Ordering::SeqCst);
                job.terminate(1);
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("等待 ADB 进程失败: {}", e));
            }
        }
    }

    if !child_exited {
        // 超时未退出，强制终止整棵客户端树并清理
        stop_immediate.store(true, Ordering::SeqCst);
        job.terminate(1);
        let _ = child.kill();
        let _ = child.wait();
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        return Err(format!("ADB 命令执行超时 ({:?})", timeout));
    }

    // 子进程已退出，通知 reader 排空尾部并正常结束
    child_exited_flag.store(true, Ordering::SeqCst);

    // 启动看门狗线程：确保 reader 在子孙进程持有管道时不会无限挂起
    let stop_watchdog = stop_immediate.clone();
    let watchdog = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        stop_watchdog.store(true, Ordering::Relaxed);
    });

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    stop_immediate.store(true, Ordering::Relaxed);
    let _ = watchdog;

    // 清理本次命令的所有衍生子孙进程（若有孤儿孙子进程持有管道）
    job.terminate(0);

    // 关键质量门：若输出超过 2MB 缓冲区导致截断，明确报错拒绝，杜绝将截断当完整成功
    if stdout_truncated.load(Ordering::SeqCst) || stderr_truncated.load(Ordering::SeqCst) {
        crate::logger::log_adb(&format!(
            "adb {} 失败: 输出超过单次缓冲区限制 ({} 字节)",
            full_args.join(" "),
            MAX_PIPE_BUFFER_SIZE
        ));
        return Err(format!(
            "ADB 输出超过单次缓冲区限制 ({} 字节)，结果已被截断",
            MAX_PIPE_BUFFER_SIZE
        ));
    }

    let stdout_str = decode_adb_output(&stdout_bytes);
    let stderr_str = decode_adb_output(&stderr_bytes);

    let status = exit_status.expect("Child exited must have status");
    let exit_code = status.code().unwrap_or(-1);

    // 检查退出码：非零退出码结合 stderr 判错，杜绝将错误当成功
    if exit_code != 0 {
        let err_detail = if !stderr_str.is_empty() {
            stderr_str
        } else if !stdout_str.is_empty() {
            stdout_str
        } else {
            "进程异常退出，无错误输出".to_string()
        };
        crate::logger::log_adb(&format!(
            "adb {} [退出码 {}] 失败: {}",
            full_args.join(" "),
            exit_code,
            err_detail
        ));
        return Err(format!("ADB 退出码 {}: {}", exit_code, err_detail));
    }

    let result = if !stdout_str.is_empty() {
        stdout_str
    } else {
        stderr_str
    };

    crate::logger::log_adb(&format!("adb {} => {}", full_args.join(" "), result));
    Ok(result)
}

/// 获取设备的连接状态
pub fn adb_device_state(serial: &str, timeout: Duration) -> AdbDeviceState {
    adb_device_state_with_cancel(serial, timeout, None)
}

/// 支持取消令牌的状态查询
pub fn adb_device_state_with_cancel(
    serial: &str,
    timeout: Duration,
    cancel_token: Option<&AtomicBool>,
) -> AdbDeviceState {
    adb_device_state_with_custom_path_and_cancel(None, serial, timeout, cancel_token)
}

/// 支持指定自定义路径进行状态查询（用于测试仿真注入）
pub fn adb_device_state_with_custom_path(
    custom_adb: Option<&Path>,
    serial: &str,
    timeout: Duration,
) -> AdbDeviceState {
    adb_device_state_with_custom_path_and_cancel(custom_adb, serial, timeout, None)
}

/// 支持指定自定义路径与取消令牌的状态查询
pub fn adb_device_state_with_custom_path_and_cancel(
    custom_adb: Option<&Path>,
    serial: &str,
    timeout: Duration,
    cancel_token: Option<&AtomicBool>,
) -> AdbDeviceState {
    if custom_adb.is_none() && !is_adb_server_alive(Duration::from_millis(150)) {
        return AdbDeviceState::Unknown;
    }
    match adb_run_with_custom_path(
        custom_adb,
        &["-s", serial, "get-state"],
        timeout,
        cancel_token,
    ) {
        Ok(out) => {
            let lower = out.to_lowercase();
            if lower == "device" {
                AdbDeviceState::Device
            } else if lower.contains("offline") {
                AdbDeviceState::Offline
            } else if lower.contains("unauthorized") {
                AdbDeviceState::Unauthorized
            } else if lower.contains("not found") || lower.contains("no devices") {
                AdbDeviceState::NotFound
            } else if lower.contains("cannot")
                || lower.contains("failed")
                || lower.contains("error:")
            {
                AdbDeviceState::Error(out)
            } else {
                AdbDeviceState::Unknown
            }
        }
        Err(e) => {
            let lower = e.to_lowercase();
            if lower.contains("offline") {
                AdbDeviceState::Offline
            } else if lower.contains("unauthorized") {
                AdbDeviceState::Unauthorized
            } else if lower.contains("not found") {
                AdbDeviceState::NotFound
            } else {
                AdbDeviceState::Error(e)
            }
        }
    }
}

/// 清理所有由本项目产生的 ADB 衍生子进程
/// 阻止新命令，关闭本应用创建的 Server Job；不接管或终止共享服务。
pub fn cleanup_adb_processes(_kill_server: bool) {
    block_adb_spawns();
    let owned = OWNED_ADB_SERVER.lock().take();
    drop(owned);
}
