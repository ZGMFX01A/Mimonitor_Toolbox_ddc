use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

static LOG_ENABLED: AtomicBool = AtomicBool::new(true);
static LOG_LOCK: Mutex<()> = Mutex::new(());

/// 结构化日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub category: String,
    pub message: String,
}

const MAX_RING_BUFFER_ENTRIES: usize = 1000;
static LOG_RING_BUFFER: LazyLock<Mutex<VecDeque<LogEntry>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(MAX_RING_BUFFER_ENTRIES)));

/// 获取日志存储目录 (`%LOCALAPPDATA%/.gpro_controller/logs`)
pub fn get_log_dir() -> PathBuf {
    let dir = crate::config::get_app_data_dir().join("logs");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn append_entry(category: &str, msg: &str) {
    if !LOG_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let entry = LogEntry {
        timestamp: timestamp.clone(),
        category: category.to_string(),
        message: msg.to_string(),
    };

    {
        let mut buf = LOG_RING_BUFFER.lock();
        if buf.len() >= MAX_RING_BUFFER_ENTRIES {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    let _guard = LOG_LOCK.lock();
    let line = format!("[{}] [{}] {}\n", timestamp, category, msg);

    // 控制台输出
    // stdout is reserved for the private parent/UI protocol in UI processes.
    eprint!("{}", line);

    // 写入 app.log
    let app_log_path = get_log_dir().join("app.log");
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(app_log_path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// 记录系统或业务通用日志
pub fn log_info(msg: &str) {
    append_entry("INFO", msg);
}

/// 记录设备命令执行日志
pub fn log_device(msg: &str) {
    append_entry("DEVICE", msg);
}

/// 记录 DDC/CI 硬件调用日志
pub fn log_ddc(msg: &str) {
    append_entry("DDC", msg);
}

/// 记录预设与调度日志
pub fn log_preset(msg: &str) {
    append_entry("PRESET", msg);
}

/// 记录专用 ADB 操作审计日志（同时独立追加至 adb.log）
pub fn log_adb(cmd_and_result: &str) {
    append_entry("ADB", cmd_and_result);

    let _guard = LOG_LOCK.lock();
    let timestamp = chrono::Local::now().format("%H:%M:%S");
    let line = format!("[{}] {}\n", timestamp, cmd_and_result);

    let path = get_log_dir().join("adb.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// 获取最近保存在内存环形缓冲区中的结构化日志
pub fn get_recent_logs() -> Vec<LogEntry> {
    let buf = LOG_RING_BUFFER.lock();
    buf.iter().cloned().collect()
}

/// 导出完整日志文件到指定文件路径
pub fn export_logs_to_file(target_path: &Path) -> Result<(), std::io::Error> {
    let app_log = get_log_dir().join("app.log");
    if app_log.exists() {
        fs::copy(&app_log, target_path)?;
    } else {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(target_path)?;
        let buf = LOG_RING_BUFFER.lock();
        for entry in buf.iter() {
            writeln!(
                file,
                "[{}] [{}] {}",
                entry.timestamp, entry.category, entry.message
            )?;
        }
    }
    Ok(())
}
