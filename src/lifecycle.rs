//! The resident process owns devices/configuration; GPUI exists only in an owned
//! child while an actual panel or OSD is open. Private inherited pipes carry the
//! protocol: there is no listening port, public pipe name, or desktop anchor.
use crate::{
    config::AppConfig,
    message::{AppCommand, AppEvent, MessageBus},
    tray::{SystemTrayManager, TrayEvent},
};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, LazyLock, OnceLock,
    },
    time::{Duration, Instant},
};

const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const QUEUE_LIMIT: usize = 256;
static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static UI_TASKS: LazyLock<(Mutex<usize>, Condvar)> =
    LazyLock::new(|| (Mutex::new(0), Condvar::new()));

/// Register before spawning, so closing the window cannot outrun a queued save.
pub(crate) fn spawn_ui_task(task: impl FnOnce() + Send + 'static) {
    struct Completion;
    impl Drop for Completion {
        fn drop(&mut self) {
            *UI_TASKS.0.lock() -= 1;
            UI_TASKS.1.notify_all();
        }
    }
    *UI_TASKS.0.lock() += 1;
    std::thread::spawn(move || {
        let _completion = Completion;
        task();
    });
}

pub(crate) fn request_exit() {
    EXIT_REQUESTED.store(true, Ordering::Release);
}

fn finish_ui_tasks() {
    let deadline = Instant::now() + Duration::from_secs(11);
    let mut remaining = UI_TASKS.0.lock();
    while *remaining > 0 && Instant::now() < deadline {
        UI_TASKS.1.wait_for(
            &mut remaining,
            deadline.saturating_duration_since(Instant::now()),
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum UiAction {
    Main,
    Brightness,
    Value(crate::hotkey::AdjustParam),
    Osd(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Activation {
    pub id: u64,
    pub action: UiAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Counters {
    generation: u64,
    scan: u64,
    light: u64,
    game: u64,
    ddc: u64,
}
impl Counters {
    fn capture(bus: &MessageBus) -> Self {
        Self {
            generation: bus.active_generation.load(Ordering::Acquire),
            scan: bus.current_scan_generation.load(Ordering::Acquire),
            light: bus.light_revision.load(Ordering::Acquire),
            game: crate::message::GAME_REVISION.load(Ordering::Acquire),
            ddc: crate::ddc::DDC_WRITE_REVISION.load(Ordering::Acquire),
        }
    }
    fn install(&self, bus: &MessageBus) {
        // Never roll back an optimistic UI command when an earlier hardware
        // response crosses it in flight.
        bus.active_generation.advance_to(self.generation);
        bus.current_scan_generation
            .fetch_max(self.scan, Ordering::AcqRel);
        bus.light_revision.fetch_max(self.light, Ordering::AcqRel);
        crate::message::GAME_REVISION.fetch_max(self.game, Ordering::AcqRel);
        crate::ddc::DDC_WRITE_REVISION.fetch_max(self.ddc, Ordering::AcqRel);
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum ServerMessage {
    Start {
        version: u32,
        config: AppConfig,
        migration_blocked: bool,
        counters: Counters,
        snapshot: Vec<AppEvent>,
        activation: Activation,
    },
    Event {
        event: AppEvent,
        counters: Counters,
    },
    Activate(Activation),
    ConfigResult {
        id: u64,
        result: Result<AppConfig, String>,
    },
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum UiRequest {
    Command(AppCommand),
    SaveConfig { id: u64, changes: Vec<ConfigChange> },
    Activated(u64),
    Closing,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConfigChange {
    path: Vec<String>,
    before: Option<Value>,
    after: Option<Value>,
}

fn diff_config(
    before: &Value,
    after: &Value,
    path: &mut Vec<String>,
    changes: &mut Vec<ConfigChange>,
) {
    if let (Some(before), Some(after)) = (before.as_object(), after.as_object()) {
        let keys: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).collect();
        for key in keys {
            path.push(key.clone());
            match (before.get(key), after.get(key)) {
                (Some(a), Some(b)) => diff_config(a, b, path, changes),
                (a, b) => changes.push(ConfigChange {
                    path: path.clone(),
                    before: a.cloned(),
                    after: b.cloned(),
                }),
            }
            path.pop();
        }
    } else if before != after {
        changes.push(ConfigChange {
            path: path.clone(),
            before: Some(before.clone()),
            after: Some(after.clone()),
        });
    }
}

fn apply_changes(config: &mut AppConfig, changes: &[ConfigChange]) -> Result<(), String> {
    let mut value = serde_json::to_value(&*config).map_err(|e| e.to_string())?;
    for change in changes {
        let (key, parents) = change.path.split_last().ok_or("配置字段路径为空")?;
        let mut object = &mut value;
        for parent in parents {
            object = object
                .get_mut(parent)
                .ok_or("配置已变化，请重新打开面板后重试")?;
        }
        let object = object.as_object_mut().ok_or("配置字段类型已变化")?;
        if object.get(key) != change.before.as_ref() && object.get(key) != change.after.as_ref() {
            return Err(format!(
                "配置 {} 已被后台更新，请刷新后重试",
                change.path.join(".")
            ));
        }
        if let Some(after) = &change.after {
            object.insert(key.clone(), after.clone());
        } else {
            object.remove(key);
        }
    }
    // Work on a temporary value so a conflict never partially applies a patch.
    *config = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(())
}

fn write_frame(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("IPC frame too large"));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}
fn read_frame<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(io::Error::other("Invalid IPC frame length"));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

type ConfigReply = mpsc::Sender<Result<AppConfig, String>>;
pub(crate) struct UiClient {
    sender: SyncSender<UiRequest>,
    next_id: AtomicU64,
    replies: Mutex<HashMap<u64, ConfigReply>>,
    flushed: (Mutex<bool>, Condvar),
}
static UI_CLIENT: OnceLock<Arc<UiClient>> = OnceLock::new();
pub(crate) fn ui_client() -> Option<&'static Arc<UiClient>> {
    UI_CLIENT.get()
}
impl UiClient {
    fn finish(&self) {
        if self.send(UiRequest::Closing).is_ok() {
            let mut done = self.flushed.0.lock();
            if !*done {
                self.flushed.1.wait_for(&mut done, Duration::from_secs(1));
            }
        }
    }
    pub fn send(&self, request: UiRequest) -> Result<(), String> {
        self.sender
            .try_send(request)
            .map_err(|_| "后台通信已断开或队列已满，请重新打开界面".into())
    }
    pub fn save_config(&self, before: &AppConfig, after: &AppConfig) -> Result<AppConfig, String> {
        let mut changes = Vec::new();
        diff_config(
            &serde_json::to_value(before).map_err(|e| e.to_string())?,
            &serde_json::to_value(after).map_err(|e| e.to_string())?,
            &mut Vec::new(),
            &mut changes,
        );
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.replies.lock().insert(id, tx);
        if let Err(error) = self.send(UiRequest::SaveConfig { id, changes }) {
            self.replies.lock().remove(&id);
            return Err(error);
        }
        let result = rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "配置保存确认超时，请重新打开面板确认结果".to_string());
        self.replies.lock().remove(&id);
        result?
    }
}

pub(crate) fn run_ui_process() -> Result<(), String> {
    let (mut input, output) = crate::process_io::take_ui_pipes().map_err(|e| e.to_string())?;
    // A manually launched --ui-process without an inherited pipe fails closed.
    let start: ServerMessage = read_frame(&mut input).map_err(|e| e.to_string())?;
    let ServerMessage::Start {
        version,
        config,
        migration_blocked,
        counters,
        snapshot,
        activation,
    } = start
    else {
        return Err("Missing UI startup handshake".into());
    };
    if version != PROTOCOL_VERSION {
        return Err("UI protocol version mismatch".into());
    }
    crate::config::install_snapshot(config.clone(), migration_blocked);
    let (tx, rx) = mpsc::sync_channel(QUEUE_LIMIT);
    let client = Arc::new(UiClient {
        sender: tx.clone(),
        next_id: AtomicU64::new(1),
        replies: Mutex::new(HashMap::new()),
        flushed: (Mutex::new(false), Condvar::new()),
    });
    UI_CLIENT
        .set(client.clone())
        .map_err(|_| "UI transport already initialized")?;
    let bus = Arc::new(MessageBus::remote(tx));
    counters.install(&bus);
    let tray = Arc::new(SystemTrayManager::new());
    for event in snapshot {
        let _ = bus.event_sender.send(event);
    }
    let (actions_tx, actions_rx) = mpsc::sync_channel(QUEUE_LIMIT);
    let stopped = bus.is_shutdown.clone();
    let writer_tray = tray.clone();
    let writer_client = client.clone();
    std::thread::spawn(move || {
        let mut stdout = output;
        while let Ok(request) = rx.recv() {
            if write_frame(&mut stdout, &request).is_err() {
                break;
            }
            if matches!(request, UiRequest::Closing) {
                break;
            }
        }
        *writer_client.flushed.0.lock() = true;
        writer_client.flushed.1.notify_all();
        stopped.store(true, Ordering::Release);
        writer_client.replies.lock().clear();
        writer_tray.dispatch(TrayEvent::Quit);
    });
    let reader_bus = bus.clone();
    let reader_tray = tray.clone();
    std::thread::spawn(move || {
        let mut stdin = input;
        while let Ok(message) = read_frame::<ServerMessage>(&mut stdin) {
            match message {
                ServerMessage::Event { event, counters } => {
                    counters.install(&reader_bus);
                    if let AppEvent::ConfigUpdated(config) = &event {
                        crate::config::install_snapshot(config.clone(), migration_blocked);
                    }
                    if reader_bus.event_sender.send(event).is_err() {
                        break;
                    }
                }
                ServerMessage::Activate(action) => {
                    if actions_tx.try_send(action).is_err() {
                        break;
                    }
                }
                ServerMessage::ConfigResult { id, result } => {
                    if let Ok(config) = &result {
                        crate::config::install_snapshot(config.clone(), migration_blocked);
                    }
                    if let Some(reply) = client.replies.lock().remove(&id) {
                        let _ = reply.send(result);
                    }
                }
                // Keep the pipe reader alive until pending saves have replies.
                ServerMessage::Shutdown => reader_tray.dispatch(TrayEvent::Quit),
                ServerMessage::Start { .. } => break,
            }
        }
        reader_bus.is_shutdown.store(true, Ordering::Release);
        client.replies.lock().clear();
        reader_tray.dispatch(TrayEvent::Quit);
    });
    crate::run_ui_application(bus, tray, Arc::new(config), activation, actions_rx);
    finish_ui_tasks();
    if EXIT_REQUESTED.load(Ordering::Acquire) {
        if let Some(client) = ui_client() {
            let _ = client.send(UiRequest::Quit);
        }
    }
    if let Some(client) = ui_client() {
        client.finish();
    }
    Ok(())
}

struct UiChild {
    child: Child,
    sender: SyncSender<ServerMessage>,
    receiver: Receiver<UiRequest>,
    closing: bool,
    requests_done: bool,
    _job: crate::adb::client::win32_job::JobGuard,
}
impl UiChild {
    fn spawn(start: ServerMessage) -> io::Result<Self> {
        use crate::adb::client::win32_job::{self, JobGuard};
        let job = JobGuard::ui_process()
            .ok_or_else(|| io::Error::other("Cannot create UI process job"))?;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--ui-process")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(crate::adb::client::CREATE_NO_WINDOW | win32_job::CREATE_SUSPENDED)
            .spawn()?;
        if !job.assign(child.as_raw_handle()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Cannot assign UI process to job"));
        }
        if let Err(error) = win32_job::resume_process(child.as_raw_handle()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(error));
        }
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Missing UI stdin"))?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Missing UI stdout"))?;
        let (sender, messages) = mpsc::sync_channel(QUEUE_LIMIT);
        let (requests, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        // Startup is first in the pipe, before any events or activations.
        sender
            .try_send(start)
            .map_err(|_| io::Error::other("UI startup queue closed"))?;
        std::thread::spawn(move || {
            while let Ok(message) = messages.recv() {
                if write_frame(&mut input, &message).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            while let Ok(request) = read_frame::<UiRequest>(&mut output) {
                if requests.send(request).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            sender,
            receiver,
            closing: false,
            requests_done: false,
            _job: job,
        })
    }
    fn send(&mut self, message: ServerMessage) -> bool {
        if self.closing {
            return false;
        }
        if self.sender.try_send(message).is_err() {
            // A stuck UI may be discarded without stopping hardware workers.
            self.closing = true;
            let _ = self.child.kill();
            return false;
        }
        true
    }
    fn process_request(&mut self, request: UiRequest, bus: &MessageBus) -> Option<UiRequest> {
        match request {
            UiRequest::Command(AppCommand::Shutdown) => return Some(UiRequest::Quit),
            UiRequest::Command(command) => {
                match &command {
                    AppCommand::SetDdcBrightness { revision, .. } => {
                        crate::ddc::DDC_WRITE_REVISION.fetch_max(*revision, Ordering::AcqRel);
                    }
                    AppCommand::TrayValueCommit { request_id, .. }
                    | AppCommand::EditPresetConfig { request_id, .. } => {
                        crate::ddc::DDC_WRITE_REVISION.fetch_max(*request_id, Ordering::AcqRel);
                    }
                    _ => {}
                }
                if let Err(error) = bus.cmd_sender.send(command) {
                    let _ = bus.event_sender.send(AppEvent::ToastNotification {
                        message: format!("后台队列繁忙，请重试：{}", error),
                        is_error: true,
                    });
                }
            }
            UiRequest::SaveConfig { id, changes } => {
                let result = crate::config::update_checked(|config| {
                    apply_changes(config, &changes).map_err(crate::config::ConfigError::Conflict)
                })
                .map(|cfg| (*cfg).clone())
                .map_err(|e| e.to_string());
                if let Ok(config) = &result {
                    let _ = bus
                        .event_sender
                        .send(AppEvent::ConfigUpdated(config.clone()));
                }
                self.send(ServerMessage::ConfigResult { id, result });
            }
            request => return Some(request),
        }
        None
    }
    fn stop(&mut self, bus: &MessageBus) {
        let _ = self.sender.try_send(ServerMessage::Shutdown);
        let deadline = Instant::now() + Duration::from_secs(12);
        while Instant::now() < deadline {
            while let Ok(request) = self.receiver.try_recv() {
                let _ = self.process_request(request, bus);
            }
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct Snapshot {
    connection: Option<AppEvent>,
    backlight: Option<AppEvent>,
    displays: Option<AppEvent>,
    monitors: Option<AppEvent>,
    activity: Option<AppEvent>,
}
impl Snapshot {
    fn record(&mut self, event: &AppEvent) {
        match event {
            AppEvent::AdbStateChanged { state, generation, .. } => {
                let same_session = matches!(self.connection.as_ref(),
                    Some(AppEvent::AdbStateChanged { generation: previous, .. })
                        if previous == generation);
                if !state.is_connected() || !same_session {
                    self.backlight = None;
                }
                self.connection = Some(event.clone());
            }
            AppEvent::BacklightUpdated { generation, revision, .. } => {
                let connected = matches!(self.connection.as_ref(),
                    Some(AppEvent::AdbStateChanged { state, generation: current, .. })
                        if state.is_connected() && current == generation);
                let outdated = matches!(self.backlight.as_ref(),
                    Some(AppEvent::BacklightUpdated { revision: previous, .. })
                        if revision < previous);
                if connected && !outdated {
                    self.backlight = Some(event.clone());
                }
            }
            AppEvent::WindowsDisplaysDiscovered(_) => self.displays = Some(event.clone()),
            AppEvent::DdcMonitorsDiscovered(_) => self.monitors = Some(event.clone()),
            AppEvent::DeviceActivity { active, .. } => {
                self.activity = active.then(|| event.clone());
            }
            AppEvent::DdcBrightnessChanged {
                device_id,
                percent,
                revision,
            } => {
                if let Some(AppEvent::DdcMonitorsDiscovered(monitors)) = &mut self.monitors {
                    if let Some(monitor) = monitors.iter_mut().find(|m| &m.device_id == device_id) {
                        monitor.current_percent = *percent;
                        monitor.write_revision = *revision;
                    }
                }
            }
            _ => {}
        }
    }
    fn events(&self) -> Vec<AppEvent> {
        self.connection
            .iter()
            .chain(self.backlight.iter())
            .chain(self.displays.iter())
            .chain(self.monitors.iter())
            .chain(self.activity.iter())
            .cloned()
            .collect()
    }
}

pub(crate) fn run_background(
    instance: Arc<crate::instance::Instance>,
    bus: Arc<MessageBus>,
    tray: Arc<SystemTrayManager>,
    initial: Option<UiAction>,
) {
    let mut ui: Option<UiChild> = None;
    let mut snapshot = Snapshot::default();
    let mut pending = VecDeque::<Activation>::new();
    let mut next_activation = 1;
    let mut activate = |action, pending: &mut VecDeque<Activation>| {
        // Countdown OSD updates replace one another; repeated main-window
        // activation is idempotent. Keep toggle requests in arrival order.
        if matches!(action, UiAction::Osd(_)) {
            pending.retain(|a| !matches!(a.action, UiAction::Osd(_)));
        }
        if matches!(action, UiAction::Main)
            && pending.iter().any(|a| matches!(a.action, UiAction::Main))
        {
            return;
        }
        if pending.len() >= QUEUE_LIMIT {
            crate::logger::log_info("界面请求队列已满，请稍后重试");
            return;
        }
        pending.push_back(Activation {
            id: next_activation,
            action,
        });
        next_activation += 1;
    };
    if let Some(action) = initial {
        activate(action, &mut pending);
    }
    let cfg = crate::config::load_settings();
    if !cfg.saved_ip.trim().is_empty() {
        let _ = bus.cmd_sender.send(AppCommand::ConnectAdb {
            ip: cfg.saved_ip.clone(),
            generation: bus.active_generation.load(Ordering::Acquire),
        });
    }
    let mut sent_through = 0;
    let mut spawn_attempts = (0, 0u8);
    let mut quitting = false;
    while !quitting {
        if instance.take_main() {
            activate(UiAction::Main, &mut pending);
        }
        if instance.take_brightness() {
            activate(UiAction::Brightness, &mut pending);
        }

        if let Some(child) = &mut ui {
            // Drain already received commands even if the UI exited immediately
            // after submitting them. Acknowledged activations aren't replayed.
            for _ in 0..QUEUE_LIMIT {
                let request = match child.receiver.try_recv() {
                    Ok(request) => request,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        child.requests_done = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                };
                let Some(request) = child.process_request(request, &bus) else {
                    continue;
                };
                match request {
                    UiRequest::Activated(id) => pending.retain(|a| a.id != id),
                    UiRequest::Closing => child.closing = true,
                    UiRequest::Quit | UiRequest::Command(AppCommand::Shutdown) => quitting = true,
                    _ => unreachable!(),
                }
            }
        }

        for _ in 0..64 {
            let Some(mut event) = bus.try_recv_event() else {
                break;
            };
            match &event {
                AppEvent::OpenBrightness => {
                    activate(UiAction::Brightness, &mut pending);
                    continue;
                }
                AppEvent::HotkeyOsd {
                    message: Some(message),
                } => {
                    activate(UiAction::Osd(message.clone()), &mut pending);
                    continue;
                }
                AppEvent::HotkeyOsd { message: None } => {
                    pending.retain(|a| !matches!(a.action, UiAction::Osd(_)))
                }
                AppEvent::ConfigUpdated(_) => {
                    event = AppEvent::ConfigUpdated((*crate::config::load_settings()).clone())
                }
                _ => {}
            }
            snapshot.record(&event);
            if let Some(child) = &mut ui {
                child.send(ServerMessage::Event {
                    event,
                    counters: Counters::capture(&bus),
                });
            }
        }
        for _ in 0..64 {
            let Some(event) = tray.try_recv_event() else {
                break;
            };
            match event {
                TrayEvent::OpenMainWindow => activate(UiAction::Main, &mut pending),
                TrayEvent::ToggleBrightnessPanel => activate(UiAction::Brightness, &mut pending),
                TrayEvent::OpenValueSlider(param) => activate(UiAction::Value(param), &mut pending),
                TrayEvent::RefreshMonitors => {
                    let _ = bus.cmd_sender.send(AppCommand::RefreshMonitors);
                }
                TrayEvent::Custom(choice) => {
                    let _ = bus
                        .cmd_sender
                        .send(choice.command(bus.active_generation.load(Ordering::Acquire)));
                }
                TrayEvent::Quit => quitting = true,
            }
        }
        if quitting {
            break;
        }
        if let Some(child) = &mut ui {
            if child.requests_done && matches!(child.child.try_wait(), Ok(Some(_))) {
                ui = None;
                sent_through = 0;
            }
        }
        if ui.is_none() {
            if let Some(activation) = pending.front().cloned() {
                let id = activation.id;
                spawn_attempts = if spawn_attempts.0 == id {
                    (id, spawn_attempts.1 + 1)
                } else {
                    (id, 1)
                };
                if spawn_attempts.1 > 3 {
                    crate::logger::log_info(
                        "界面连续启动失败，已停止自动重试；后台继续运行，可再次手动打开",
                    );
                    pending.pop_front();
                    continue;
                }
                match UiChild::spawn(ServerMessage::Start {
                    version: PROTOCOL_VERSION,
                    config: (*crate::config::load_settings()).clone(),
                    migration_blocked: crate::config::migration_blocked(),
                    counters: Counters::capture(&bus),
                    snapshot: snapshot.events(),
                    activation,
                }) {
                    Ok(child) => {
                        ui = Some(child);
                        sent_through = id;
                    }
                    Err(error) => {
                        crate::logger::log_info(&format!("无法打开界面：{error}"));
                        pending.clear();
                    }
                }
            }
        }
        if let Some(child) = &mut ui {
            for activation in &pending {
                if activation.id > sent_through
                    && child.send(ServerMessage::Activate(activation.clone()))
                {
                    sent_through = activation.id;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    if let Some(child) = &mut ui {
        child.stop(&bus);
    }
    bus.shutdown();
    tray.shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_replays_backlight_after_connection_when_ui_reopens() {
        let mut snapshot = Snapshot::default();
        snapshot.record(&AppEvent::AdbStateChanged {
            ip: "192.168.5.19".into(),
            state: crate::adb::AdbDeviceState::Device,
            model: "MiTV-MFFU1".into(),
            generation: 4,
        });
        snapshot.record(&AppEvent::BacklightUpdated {
            value: Some(15), generation: 4, revision: 9,
        });
        for _ in 0..2 {
            let events = snapshot.events();
            assert!(matches!(events.first(), Some(AppEvent::AdbStateChanged { generation: 4, .. })));
            assert!(matches!(events.get(1), Some(AppEvent::BacklightUpdated {
                value: Some(15), generation: 4, revision: 9,
            })), "reopened UI must receive the backlight already read by the backend");
        }
    }

    #[test]
    fn snapshot_keeps_latest_backlight_and_rejects_other_sessions() {
        let mut snapshot = Snapshot::default();
        snapshot.record(&AppEvent::AdbStateChanged {
            ip: "192.168.5.19".into(),
            state: crate::adb::AdbDeviceState::Device,
            model: "MiTV-MFFU1".into(),
            generation: 4,
        });
        for (value, generation, revision) in [
            (Some(15), 4, 9), (Some(20), 4, 10), (Some(15), 4, 9),
            (Some(80), 3, 11), (Some(90), 5, 12),
        ] {
            snapshot.record(&AppEvent::BacklightUpdated { value, generation, revision });
        }
        assert!(matches!(snapshot.events().get(1), Some(AppEvent::BacklightUpdated {
            value: Some(20), generation: 4, revision: 10,
        })));
        snapshot.record(&AppEvent::BacklightUpdated { value: None, generation: 4, revision: 11 });
        assert!(matches!(snapshot.events().get(1), Some(AppEvent::BacklightUpdated {
            value: None, generation: 4, revision: 11,
        })));
    }

    #[test]
    fn snapshot_clears_backlight_on_disconnect_or_new_session() {
        let connection = |generation, state| AppEvent::AdbStateChanged {
            ip: "192.168.5.19".into(), state, model: "MiTV-MFFU1".into(), generation,
        };
        let reading = |generation| AppEvent::BacklightUpdated {
            value: Some(15), generation, revision: 9,
        };
        let mut snapshot = Snapshot::default();
        snapshot.record(&reading(4));
        assert!(snapshot.events().is_empty());
        snapshot.record(&connection(4, crate::adb::AdbDeviceState::Device));
        snapshot.record(&reading(4));
        snapshot.record(&connection(4, crate::adb::AdbDeviceState::Device));
        assert_eq!(snapshot.events().len(), 2);
        snapshot.record(&connection(4, crate::adb::AdbDeviceState::Offline));
        snapshot.record(&reading(4));
        assert_eq!(snapshot.events().len(), 1);
        snapshot.record(&connection(4, crate::adb::AdbDeviceState::Device));
        assert_eq!(snapshot.events().len(), 1);
        snapshot.record(&reading(4));
        snapshot.record(&connection(5, crate::adb::AdbDeviceState::Device));
        snapshot.record(&reading(4));
        assert_eq!(snapshot.events().len(), 1);
        snapshot.record(&reading(5));
        assert_eq!(snapshot.events().len(), 2);
    }

    #[test]
    fn protocol_roundtrips_and_rejects_oversized_or_truncated_frames() {
        let command = UiRequest::Command(AppCommand::SetPictureSlider {
            param: crate::device::picture::PictureSliderParam::Backlight,
            value: 37,
            generation: 4,
            revision: 9,
        });
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &command).unwrap();
        assert!(matches!(
            read_frame::<UiRequest>(&mut bytes.as_slice()).unwrap(),
            UiRequest::Command(AppCommand::SetPictureSlider {
                value: 37,
                generation: 4,
                revision: 9,
                ..
            })
        ));
        assert!(
            read_frame::<UiRequest>(&mut ((MAX_FRAME + 1) as u32).to_le_bytes().as_slice())
                .is_err()
        );
        assert!(read_frame::<UiRequest>(&mut &bytes[..bytes.len() - 1]).is_err());
    }
    #[test]
    fn config_delta_preserves_background_fields_and_rejects_conflicts_atomically() {
        let original = AppConfig::default();
        let mut edited = original.clone();
        edited.crosshair_game_mode_only = false;
        edited
            .extra
            .insert("theme".into(), Value::String("light".into()));
        let mut changes = Vec::new();
        diff_config(
            &serde_json::to_value(&original).unwrap(),
            &serde_json::to_value(&edited).unwrap(),
            &mut Vec::new(),
            &mut changes,
        );
        let mut live = original.clone();
        live.saved_ip = "192.168.1.12".into();
        live.extra.insert(
            "scheduler_recovery".into(),
            serde_json::json!({"active": true}),
        );
        apply_changes(&mut live, &changes).unwrap();
        assert_eq!(live.saved_ip, "192.168.1.12");
        assert_eq!(
            live.extra["scheduler_recovery"],
            serde_json::json!({"active": true})
        );
        assert_eq!(live.extra["theme"], "light");
        live.extra
            .insert("theme".into(), Value::String("dark".into()));
        let before = serde_json::to_value(&live).unwrap();
        assert!(apply_changes(&mut live, &changes).is_err());
        assert_eq!(serde_json::to_value(&live).unwrap(), before);
    }
    #[test]
    fn ui_proxy_has_no_workers_and_cannot_stop_backend() {
        let (tx, rx) = mpsc::sync_channel(8);
        let proxy = MessageBus::remote(tx);
        proxy.cmd_sender.send(AppCommand::RefreshMonitors).unwrap();
        assert!(matches!(
            rx.recv().unwrap(),
            UiRequest::Command(AppCommand::RefreshMonitors)
        ));
        assert!(proxy.try_recv_event().is_none());
        proxy.shutdown();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn ui_shutdown_waits_for_already_requested_background_saves() {
        let completed = Arc::new(AtomicBool::new(false));
        let saved = completed.clone();
        spawn_ui_task(move || {
            std::thread::sleep(Duration::from_millis(20));
            saved.store(true, Ordering::Release);
        });
        finish_ui_tasks();
        assert!(completed.load(Ordering::Acquire));
    }

    #[test]
    fn crossing_old_events_cannot_undo_optimistic_ui_counters() {
        let (tx, _rx) = mpsc::sync_channel(8);
        let proxy = MessageBus::remote(tx);
        let mut earlier = Counters::capture(&proxy);
        earlier.generation = 5;
        earlier.scan = 7;
        earlier.light = 9;
        earlier.install(&proxy);
        proxy.active_generation.fetch_add(1, Ordering::AcqRel);
        proxy.current_scan_generation.fetch_add(1, Ordering::AcqRel);
        proxy.light_revision.fetch_add(1, Ordering::AcqRel);
        earlier.install(&proxy);
        assert_eq!(proxy.active_generation.load(Ordering::Acquire), 6);
        assert_eq!(proxy.current_scan_generation.load(Ordering::Acquire), 8);
        assert_eq!(proxy.light_revision.load(Ordering::Acquire), 10);
    }
}
