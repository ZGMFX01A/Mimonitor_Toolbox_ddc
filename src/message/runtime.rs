use super::{AppCommand, AppEvent};
use crate::adb::{AdbDevice, AdbDeviceState};
use crate::ddc::{get_ddc_device_coalescer, DEFAULT_DDC_ADAPTER};
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{channel, Receiver, SendError, Sender},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub static GAME_REVISION: AtomicU64 = AtomicU64::new(0);

/// Serializes a session transition with a configuration commit from that session.
pub struct SessionGeneration {
    value: AtomicU64,
    commit_gate: Mutex<()>,
}
impl SessionGeneration {
    fn new(value: u64) -> Self {
        Self { value: AtomicU64::new(value), commit_gate: Mutex::new(()) }
    }
    pub fn load(&self, order: Ordering) -> u64 { self.value.load(order) }
    pub fn store(&self, value: u64, order: Ordering) {
        let _guard = self.commit_gate.lock();
        self.value.store(value, order);
    }
    pub fn fetch_add(&self, value: u64, order: Ordering) -> u64 {
        let _guard = self.commit_gate.lock();
        self.value.fetch_add(value, order)
    }
    pub(crate) fn advance_to(&self, value: u64) {
        let _guard = self.commit_gate.lock();
        self.value.fetch_max(value, Ordering::AcqRel);
    }
    pub fn with_current<T>(&self, generation: u64, commit: impl FnOnce() -> T) -> Option<T> {
        let _guard = self.commit_gate.lock();
        (self.value.load(Ordering::Acquire) == generation).then(commit)
    }
}
fn is_game(command: &AppCommand) -> bool {
    use AppCommand::*;
    matches!(
        command,
        FetchGameParameters { .. }
            | SetGameFreeSync { .. }
            | SetGame320Hz { .. }
            | SetGameFrontSight { .. }
            | SetGameDynamicFrontSight { .. }
            | SetGameSniperScope { .. }
            | SetGameSniperNightVision { .. }
            | SetGameFpsCounter { .. }
            | SetGameStopwatch { .. }
            | SetGameTimer { .. }
            | SwitchGameModeAndApply { .. }
    )
}
#[derive(Clone)]
struct Envelope {
    command: AppCommand,
    token: Option<Arc<AtomicBool>>,
    revision: u64,
    dispatch_generation: u64,
}
#[derive(Default)]
struct Inbox {
    events: Option<Sender<AppEvent>>,
    queue: Mutex<VecDeque<Envelope>>,
    changed: Condvar,
    closed: AtomicBool,
    last_push: Mutex<Option<std::time::Instant>>,
}
impl Inbox {
    fn push(&self, message: Envelope) -> Result<(), SendError<AppCommand>> {
        let mut q = self.queue.lock();
        if self.closed.load(Ordering::Acquire) {
            return Err(SendError(message.command));
        }
        if let Some(key) = coalesce_key(&message.command) {
            if let Some(index) = q
                .iter()
                .position(|old| coalesce_key(&old.command).as_ref() == Some(&key))
            {
                if let Some(Envelope {
                    command: AppCommand::TrayValueCommit { request_id, generation, .. }, ..
                }) = q.remove(index) {
                    if let Some(events) = &self.events {
                        let _ = events.send(AppEvent::TrayValueResult {
                            request_id, generation,
                            error: Some("此操作已被更新的设置替代".into()),
                        });
                    }
                }
            }
        }
        if q.len() >= 128 {
            return Err(SendError(message.command));
        }
        q.push_back(message);
        *self.last_push.lock() = Some(std::time::Instant::now());
        self.changed.notify_one();
        Ok(())
    }
    fn receive(&self) -> Option<Envelope> {
        let mut q = self.queue.lock();
        while q.is_empty() && !self.closed.load(Ordering::Acquire) {
            self.changed.wait(&mut q);
        }
        if self.closed.load(Ordering::Acquire) {
            return None;
        }
        q.pop_front()
    }
    fn close(&self) {
        let mut q = self.queue.lock();
        self.closed.store(true, Ordering::Release);
        q.clear();
        self.changed.notify_all();
    }
    fn receive_timed(&self) -> Option<Envelope> {
        let mut q = self.queue.lock();
        let deadline = Instant::now() + Duration::from_millis(500);
        if q.is_empty() && !self.closed.load(Ordering::Acquire) {
            self.changed.wait_for(&mut q, Duration::from_millis(100));
        }
        // A short quiet window collapses bursts before the first hardware write.
        while !self.closed.load(Ordering::Acquire)
            && q.front()
                .is_some_and(|e| coalesce_key(&e.command).is_some())
        {
            let remaining = self
                .last_push
                .lock()
                .map(|t| Duration::from_millis(180).saturating_sub(t.elapsed()))
                .unwrap_or_default();
            let remaining = remaining.min(deadline.saturating_duration_since(Instant::now()));
            if remaining.is_zero() || q.iter().any(|e| matches!(e.command,
                AppCommand::ConnectAdb { .. } | AppCommand::DisconnectAdb | AppCommand::CancelConnection { .. })) {
                break;
            }
            self.changed.wait_for(&mut q, remaining);
        }
        if self.closed.load(Ordering::Acquire) {
            None
        } else {
            q.pop_front()
        }
    }
}
fn coalesce_key(command: &AppCommand) -> Option<String> {
    use AppCommand::*;
    Some(match command {
        SetBacklight { generation, .. } => format!("backlight:{generation}"),
        HotkeyCommit {
            param, generation, ..
        } | TrayValueCommit {
            param, generation, ..
        } => format!("hotkey_commit:{generation}:{param:?}"),
        SetPictureSlider {
            param, generation, ..
        } => format!("picture:{generation}:{param:?}"),
        SetScreenLight { generation, .. } => format!("light:{generation}"),
        SetDdcBrightness { device_id, .. } => format!("ddc:{device_id}"),
        RefreshMonitors => "monitors".into(),
        FetchPictureParameters { generation } => format!("read-picture:{generation}"),
        FetchGameParameters { generation } => format!("read-game:{generation}"),
        SetPictureMode { generation, .. } => format!("mode:{generation}"),
        SetLightSensor { generation, .. } => format!("sensor:{generation}"),
        SetLocalDimming { generation, .. } => format!("dimming:{generation}"),
        cmd if is_game(cmd) => format!(
            "game:{:?}:{}",
            std::mem::discriminant(cmd),
            generation(cmd).unwrap_or(0)
        ),
        _ => return None,
    })
}
fn generation(command: &AppCommand) -> Option<u64> {
    use AppCommand::*;
    match command {
        TrayValueCommit { generation, .. }
        | HotkeyCommit { generation, .. }
        | Hotkey { generation, .. }
        | ConnectAdb { generation, .. }
        | CancelConnection { generation }
        | FetchBacklight { generation }
        | SetPictureMode { generation, .. }
        | FetchPictureParameters { generation }
        | SetPictureSlider { generation, .. }
        | SetLightSensor { generation, .. }
        | SetLocalDimming { generation, .. }
        | SetBacklight { generation, .. }
        | CapturePresetSnapshot { generation, .. }
        | ApplyPreset { generation, .. }
        | RestoreBaseline { generation, .. }
        | SwitchSource { generation, .. }
        | RefreshSource { generation }
        | FetchScreenLight { generation, .. }
        | SetScreenLight { generation, .. }
        | SendRemoteKey { generation, .. }
        | FetchGameParameters { generation }
        | SetGameFreeSync { generation, .. }
        | SetGame320Hz { generation, .. }
        | SetGameFrontSight { generation, .. }
        | SetGameDynamicFrontSight { generation, .. }
        | SetGameSniperScope { generation, .. }
        | SetGameSniperNightVision { generation, .. }
        | SetGameFpsCounter { generation, .. }
        | SetGameStopwatch { generation, .. }
        | SetGameTimer { generation, .. }
        | SwitchGameModeAndApply { generation, .. }
        | ToolAction { generation, .. } => Some(*generation),
        _ => None,
    }
}

#[derive(Clone)]
pub struct CommandSender {
    remote: Option<std::sync::mpsc::SyncSender<crate::lifecycle::UiRequest>>,
    adb: Arc<Inbox>,
    ddc: Arc<Inbox>,
    scan: Arc<Inbox>,
    active_connect_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    active_scan_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    active_generation: Arc<SessionGeneration>,
    current_scan_generation: Arc<AtomicU64>,
    connect_cancel_token: Arc<AtomicBool>,
    scan_cancel_token: Arc<AtomicBool>,
    light_revision: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
}
impl CommandSender {
    pub fn send(&self, command: AppCommand) -> Result<(), SendError<AppCommand>> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(SendError(command));
        }
        let mut token = None;
        let mut revision = if is_game(&command) {
            if matches!(command, AppCommand::FetchGameParameters { .. }) {
                GAME_REVISION.load(Ordering::Acquire)
            } else {
                GAME_REVISION.fetch_add(1, Ordering::AcqRel) + 1
            }
        } else {
            self.light_revision.load(Ordering::Acquire)
        };
        match &command {
            AppCommand::ConnectAdb { generation, .. } => {
                let mut slot = self.active_connect_cancel.lock();
                if let Some(old) = slot.take() {
                    old.store(true, Ordering::Release);
                }
                let fresh = Arc::new(AtomicBool::new(false));
                *slot = Some(fresh.clone());
                token = Some(fresh);
                self.active_generation.store(*generation, Ordering::Release);
                self.connect_cancel_token.store(false, Ordering::Release);
            }
            AppCommand::DisconnectAdb | AppCommand::CancelConnection { .. } => {
                if let AppCommand::CancelConnection { generation } = &command {
                    if *generation != self.active_generation.load(Ordering::Acquire) {
                        return Ok(());
                    }
                }
                self.active_generation.fetch_add(1, Ordering::AcqRel);
                self.connect_cancel_token.store(true, Ordering::Release);
                if let Some(old) = self.active_connect_cancel.lock().as_ref() {
                    old.store(true, Ordering::Release);
                }
            }
            AppCommand::ScanNetworkDevices { generation } => {
                let mut slot = self.active_scan_cancel.lock();
                if let Some(old) = slot.take() {
                    old.store(true, Ordering::Release);
                }
                let fresh = Arc::new(AtomicBool::new(false));
                *slot = Some(fresh.clone());
                token = Some(fresh);
                self.current_scan_generation
                    .store(*generation, Ordering::Release);
                self.scan_cancel_token.store(false, Ordering::Release);
            }
            AppCommand::CancelScan => {
                self.scan_cancel_token.store(true, Ordering::Release);
                if let Some(old) = self.active_scan_cancel.lock().as_ref() {
                    old.store(true, Ordering::Release);
                }
            }
            AppCommand::HotkeyCommit {
                param: crate::hotkey::AdjustParam::Backlight,
                ..
            }
            | AppCommand::TrayValueCommit {
                param: crate::hotkey::AdjustParam::Backlight,
                ..
            }
            | AppCommand::SetBacklight { .. }
            | AppCommand::SetPictureSlider {
                param: crate::device::picture::PictureSliderParam::Backlight,
                ..
            } => {
                revision = self.light_revision.fetch_add(1, Ordering::AcqRel) + 1;
            }
            AppCommand::SetDdcBrightness {
                device_id,
                percent,
                revision,
            } => {
                get_ddc_device_coalescer(device_id).push_target_with_revision(*percent, *revision);
            }
            _ => {}
        }
        if let Some(remote) = &self.remote {
            return remote.try_send(crate::lifecycle::UiRequest::Command(command))
                .map_err(|error| match error {
                    std::sync::mpsc::TrySendError::Full(crate::lifecycle::UiRequest::Command(command))
                    | std::sync::mpsc::TrySendError::Disconnected(crate::lifecycle::UiRequest::Command(command)) => SendError(command),
                    _ => unreachable!(),
                });
        }
        let queue = match &command {
            AppCommand::RefreshMonitors | AppCommand::SetDdcBrightness { .. } => &self.ddc,
            AppCommand::ScanNetworkDevices { .. } | AppCommand::CancelScan => &self.scan,
            _ => &self.adb,
        };
        queue.push(Envelope {
            command,
            token,
            revision,
            dispatch_generation: self.active_generation.load(Ordering::Acquire),
        })
    }
}

#[derive(Clone)]
pub struct MessageBus {
    pub cmd_sender: CommandSender,
    pub event_sender: Sender<AppEvent>,
    event_receiver: Arc<Mutex<Receiver<AppEvent>>>,
    pub active_generation: Arc<SessionGeneration>,
    pub current_scan_generation: Arc<AtomicU64>,
    pub light_revision: Arc<AtomicU64>,
    pub connect_cancel_token: Arc<AtomicBool>,
    pub scan_cancel_token: Arc<AtomicBool>,
    pub active_connect_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    pub active_scan_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    pub is_shutdown: Arc<AtomicBool>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
}
impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}
impl MessageBus {
    pub fn new() -> Self {
        Self::create(None)
    }

    /// A UI owns only a command proxy and event inbox, never hardware workers.
    pub(crate) fn remote(sender: std::sync::mpsc::SyncSender<crate::lifecycle::UiRequest>) -> Self {
        Self::create(Some(sender))
    }

    fn create(remote: Option<std::sync::mpsc::SyncSender<crate::lifecycle::UiRequest>>) -> Self {
        let is_remote = remote.is_some();
        let (tx, rx) = channel();
        let adb = Arc::new(Inbox { events: Some(tx.clone()), ..Inbox::default() });
        let ddc = Arc::new(Inbox::default());
        let scan = Arc::new(Inbox::default());
        let sender = CommandSender {
            remote,
            adb: adb.clone(),
            ddc: ddc.clone(),
            scan: scan.clone(),
            active_connect_cancel: Arc::new(Mutex::new(None)),
            active_scan_cancel: Arc::new(Mutex::new(None)),
            active_generation: Arc::new(SessionGeneration::new(1)),
            current_scan_generation: Arc::new(AtomicU64::new(1)),
            connect_cancel_token: Arc::new(AtomicBool::new(false)),
            scan_cancel_token: Arc::new(AtomicBool::new(false)),
            light_revision: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
        };
        let bus = Self {
            cmd_sender: sender.clone(),
            event_sender: tx.clone(),
            event_receiver: Arc::new(Mutex::new(rx)),
            active_generation: sender.active_generation.clone(),
            current_scan_generation: sender.current_scan_generation.clone(),
            light_revision: sender.light_revision.clone(),
            connect_cancel_token: sender.connect_cancel_token.clone(),
            scan_cancel_token: sender.scan_cancel_token.clone(),
            active_connect_cancel: sender.active_connect_cancel.clone(),
            active_scan_cancel: sender.active_scan_cancel.clone(),
            is_shutdown: sender.stopped.clone(),
            workers: Arc::new(Mutex::new(Vec::new())),
        };
        if is_remote {
            return bus;
        }
        let tx_ddc = tx.clone();
        let stop = bus.is_shutdown.clone();
        bus.workers.lock().push(thread::spawn(move || {
            while let Some(env) = ddc.receive() {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                match env.command {
                    AppCommand::RefreshMonitors => {
                        let _ = tx_ddc.send(AppEvent::WindowsDisplaysDiscovered(
                            crate::ddc::list_raw_windows_displays(),
                        ));
                        let monitors = crate::ddc::enumerate_ddc_monitors();
                        let _ = tx_ddc.send(AppEvent::DdcMonitorsDiscovered(monitors));
                    }
                    AppCommand::SetDdcBrightness { device_id, .. } => {
                        let c = get_ddc_device_coalescer(&device_id);
                        match c.drain_latest_and_write(&*DEFAULT_DDC_ADAPTER, &device_id) {
                            Ok(Some((percent, revision))) => {
                                let _ = tx_ddc.send(AppEvent::DdcBrightnessChanged {
                                    device_id,
                                    percent,
                                    revision,
                                });
                            }
                            Err(error) => {
                                *c.last_write_time.lock() = None;
                                let _ =
                                    tx_ddc.send(AppEvent::DdcOperationFailed { device_id, error });
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }));
        let tx_scan = tx.clone();
        let scan_gen = bus.current_scan_generation.clone();
        let stop = bus.is_shutdown.clone();
        bus.workers.lock().push(thread::spawn(move || {
            while let Some(env) = scan.receive() {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if let AppCommand::ScanNetworkDevices { generation } = env.command {
                    if generation != scan_gen.load(Ordering::Acquire) {
                        continue;
                    }
                    let token = env.token.unwrap();
                    let event_tx = tx_scan.clone();
                    let live = scan_gen.clone();
                    let count = Arc::new(AtomicU64::new(0));
                    let counted = count.clone();
                    crate::adb::scan_local_devices(token, move |device| {
                        if generation == live.load(Ordering::Acquire) {
                            counted.fetch_add(1, Ordering::Relaxed);
                            let _ = event_tx
                                .send(AppEvent::ScanDeviceDiscovered { device, generation });
                        }
                    });
                    let _ = tx_scan.send(AppEvent::ScanCompleted {
                        found_count: count.load(Ordering::Relaxed) as usize,
                        generation,
                    });
                }
            }
        }));
        let worker_bus = bus.clone();
        bus.workers
            .lock()
            .push(thread::spawn(move || adb_loop(adb, worker_bus)));
        let _ = bus.cmd_sender.send(AppCommand::RefreshMonitors);
        bus
    }
    pub fn cancel_connection(&self) {
        let _ = self.cmd_sender.send(AppCommand::CancelConnection {
            generation: self.active_generation.load(Ordering::Acquire),
        });
    }
    pub fn cancel_scan(&self) {
        let _ = self.cmd_sender.send(AppCommand::CancelScan);
    }
    pub fn shutdown(&self) {
        if self.cmd_sender.remote.is_some() {
            self.is_shutdown.store(true, Ordering::Release);
            return;
        }
        if self.is_shutdown.swap(true, Ordering::AcqRel) {
            return;
        }
        self.connect_cancel_token.store(true, Ordering::Release);
        self.scan_cancel_token.store(true, Ordering::Release);
        for slot in [&self.active_connect_cancel, &self.active_scan_cancel] {
            if let Some(t) = slot.lock().as_ref() {
                t.store(true, Ordering::Release);
            }
        }
        self.cmd_sender.adb.close();
        self.cmd_sender.ddc.close();
        self.cmd_sender.scan.close();
        crate::adb::block_adb_spawns();
        // Join and release owned server resources off the GUI thread. Shared servers are preserved.
        let workers = std::mem::take(&mut *self.workers.lock());
        thread::spawn(move || {
            for worker in workers {
                let _ = worker.join();
            }
            crate::adb::cleanup_adb_processes(false);
        });
    }
    pub fn try_recv_event(&self) -> Option<AppEvent> {
        self.event_receiver.lock().try_recv().ok()
    }
}

fn connection_retry_delay(error: &crate::adb::AdbError, retries: u8) -> Option<Duration> {
    use crate::adb::AdbError::*;
    if retries >= 6 || !matches!(error,
        NetworkUnreachable(_) | PortClosed(_) | HandshakeFailed(_) | DeviceOffline | NotFound) {
        return None;
    }
    Some(Duration::from_secs((2u64 << retries).min(30)))
}

fn adb_loop(inbox: Arc<Inbox>, bus: MessageBus) {
    let mut device: Option<AdbDevice> = None;
    let mut scheduler = super::scheduler::SchedulerEngine::new();
    let mut shortcuts = super::shortcuts::Shortcuts::new();
    let mut hdr = crate::device::hdr::HdrMemory::new();
    let mut preset_sync = super::preset_sync::PresetSyncDebouncer::new();
    let mut retry: Option<(Instant, Envelope, u8)> = None;
    let mut session: Option<Envelope> = None;
    let mut last_health_check = Instant::now();
    let mut last_background = Instant::now();
    loop {
        if bus.is_shutdown.load(Ordering::Acquire) {
            break;
        }
        shortcuts.tick(device.as_ref(), &bus);
        // Time-based task transitions must also run when commands keep arriving.
        scheduler.tick(device.as_ref(), &bus);
        if last_health_check.elapsed() >= Duration::from_secs(5) {
            last_health_check = Instant::now();
            if let (Some(dev), Some(current)) = (device.as_ref(), session.as_ref()) {
                let live = bus.active_generation.load(Ordering::Acquire);
                if current.dispatch_generation == live && !current.token.as_ref().unwrap().load(Ordering::Acquire) {
                    let state = dev.get_state_with_cancel(Duration::from_secs(1), current.token.as_deref());
                    if !state.is_connected() {
                        let _ = dev.disconnect();
                        let _ = bus.event_sender.send(AppEvent::AdbStateChanged {
                            ip: dev.ip.clone(), state: state.clone(), model: String::new(), generation: live,
                        });
                        if state != AdbDeviceState::Unauthorized {
                            retry = Some((Instant::now() + Duration::from_secs(2), current.clone(), 1));
                        }
                        device = None;
                        preset_sync.clear();
                    }
                }
            }
        }
        if retry.as_ref().is_some_and(|(_, env, _)|
            env.dispatch_generation != bus.active_generation.load(Ordering::Acquire)
            || env.token.as_ref().unwrap().load(Ordering::Acquire)) {
            retry = None;
        }
        let (next, retry_count) = if retry.as_ref().is_some_and(|(due, _, _)| Instant::now() >= *due) {
            let (_, env, count) = retry.take().unwrap();
            (Some(env), count)
        } else {
            (inbox.receive_timed(), 0)
        };
        if next.is_none() || last_background.elapsed() >= Duration::from_secs(2) {
            last_background = Instant::now();
            hdr.tick(device.as_ref(), &bus);
            if let Some(dev) = device.as_ref() {
                if !scheduler.is_active_or_pending_restore() {
                    let live_gen = bus.active_generation.load(Ordering::Acquire);
                    let cfg = crate::config::load_settings();
                    let active_id = cfg.active_preset_id.as_deref();
                    if let Some(target_id) = preset_sync.take_due(live_gen, active_id, Instant::now()) {
                        if let Some(preset_name) =
                            cfg.presets.iter().find(|p| p.id == target_id).map(|p| p.name.clone())
                        {
                            match crate::presets::capture_hardware_snapshot(dev) {
                                Ok(values) if !values.is_empty()
                                    && live_gen == bus.active_generation.load(Ordering::Acquire) => {
                                    let mut applied = false;
                                    let saved = bus.active_generation.with_current(live_gen, || crate::config::update_settings(|cfg| {
                                        applied = super::preset_sync::apply_preset_values_to_config(
                                            cfg,
                                            &target_id,
                                            values,
                                        );
                                    }));
                                    match saved {
                                        None => {},
                                        Some(Ok(updated)) => {
                                            if applied {
                                                let _ = bus
                                                    .event_sender
                                                    .send(AppEvent::ConfigUpdated((*updated).clone()));
                                                let _ = bus.event_sender.send(AppEvent::ToastNotification {
                                                    message: format!("预设「{}」已自动同步", preset_name),
                                                    is_error: false,
                                                });
                                            }
                                        }
                                        Some(Err(err)) => {
                                            crate::logger::log_preset(&format!(
                                                "自动同步预设「{preset_name}」配置保存失败：{err}"
                                            ));
                                            let _ = bus.event_sender.send(AppEvent::ToastNotification {
                                                message: format!("预设「{preset_name}」自动保存失败：{err}"),
                                                is_error: true,
                                            });
                                        }
                                    }
                                }
                                Ok(_) => {}
                                Err(err) => {
                                    crate::logger::log_preset(&format!(
                                        "自动同步预设「{preset_name}」硬件回读失败：{err}"
                                    ));
                                    if live_gen == bus.active_generation.load(Ordering::Acquire) {
                                        let _ = bus.event_sender.send(AppEvent::ToastNotification {
                                            message: format!("预设「{preset_name}」未自动保存，设备回读失败：{err}"),
                                            is_error: true,
                                        });
                                    }
                                }
                            }
                        }
                    }
                } else {
                    preset_sync.clear();
                }
            } else {
                preset_sync.clear();
            }
        }
        let Some(env) = next else { continue; };
        if matches!(env.command, AppCommand::ConfirmTaskRestore(_))
            && env.dispatch_generation != bus.active_generation.load(Ordering::Acquire)
        {
            continue;
        }
        if scheduler.edit(&env.command, &bus, device.as_ref()) {
            preset_sync.clear();
            continue;
        }
        let live = bus.active_generation.load(Ordering::Acquire);
        if matches!(
            env.command,
            AppCommand::DisconnectAdb | AppCommand::CancelConnection { .. }
        ) && env.dispatch_generation != live
        {
            continue;
        }
        if let Some(gen) = generation(&env.command) {
            if gen != live && !matches!(env.command, AppCommand::CancelConnection { .. }) {
                continue;
            }
        }
        match &env.command {
            AppCommand::EditPresetConfig { mutation, request_id } => {
                let save = || {
                    let mut result = Ok(());
                    let saved = crate::config::update_settings(|cfg| result = mutation.apply(cfg));
                    saved.map_err(|error| error.to_string()).and_then(|cfg| result.map(|()| cfg))
                };
                let saved = match mutation.generation() {
                    Some(generation) => bus.active_generation.with_current(generation, save),
                    None => Some(save()),
                };
                match saved {
                    Some(Ok(config)) => {
                        let _ = bus.event_sender.send(AppEvent::ConfigUpdated((*config).clone()));
                        let _ = bus.event_sender.send(AppEvent::PresetConfigResult {
                            request_id: *request_id, mutation: mutation.clone(), error: None,
                        });
                    }
                    Some(Err(error)) => {
                        let _ = bus.event_sender.send(AppEvent::ConfigUpdated((*crate::config::load_settings()).clone()));
                        let _ = bus.event_sender.send(AppEvent::ToastNotification {
                            message: format!("保存预设失败：{error}"), is_error: true,
                        });
                        let _ = bus.event_sender.send(AppEvent::PresetConfigResult {
                            request_id: *request_id, mutation: mutation.clone(), error: Some(error),
                        });
                    }
                    None => {
                        let _ = bus.event_sender.send(AppEvent::ConfigUpdated((*crate::config::load_settings()).clone()));
                        let _ = bus.event_sender.send(AppEvent::PresetConfigResult {
                            request_id: *request_id, mutation: mutation.clone(),
                            error: Some("连接已改变，预设保存已取消".into()),
                        });
                    }
                }
            }
            AppCommand::CancelHotkey => {
                shortcuts.cancel(&bus);
                preset_sync.clear();
            }
            AppCommand::Hotkey { event, .. } => {
                let result = device
                    .as_ref()
                    .ok_or_else(|| "设备未连接".to_string())
                    .and_then(|dev| shortcuts.stage(dev, event, &bus));
                if let Err(message) = result {
                    let _ = bus.event_sender.send(AppEvent::ToastNotification {
                        message,
                        is_error: true,
                    });
                }
            }
            AppCommand::ConnectAdb { ip, generation } => {
                preset_sync.clear();
                retry = None;
                if let Some(old) = device.take() {
                    let _ = old.disconnect();
                }
                let token = env.token.as_ref().unwrap().clone();
                if token.load(Ordering::Acquire) {
                    continue;
                }
                session = Some(env.clone());
                let dev = AdbDevice::new(ip.clone()).with_session_cancel(token.clone());
                let result =
                    dev.connect_transaction(Duration::from_secs(12), 3, true, Some(&token));
                if *generation != bus.active_generation.load(Ordering::Acquire)
                    || token.load(Ordering::Acquire)
                {
                    let _ = dev.disconnect();
                    continue;
                }
                match result {
                    Ok(model) => {
                        let heal = crate::device::jni::check_and_heal_jar(&dev);
                        if *generation != bus.active_generation.load(Ordering::Acquire)
                            || token.load(Ordering::Acquire)
                        {
                            let _ = dev.disconnect();
                            continue;
                        }
                        device = Some(dev.clone());
                        last_health_check = Instant::now();
                        if let Err(error) = heal {
                            let _ = bus.event_sender.send(AppEvent::ToastNotification {
                                message: format!("JNI 工具部署失败：{error}"),
                                is_error: true,
                            });
                        }
                        let _ = bus.event_sender.send(AppEvent::AdbStateChanged {
                            ip: ip.clone(),
                            state: AdbDeviceState::Device,
                            model: model.unwrap_or_else(|| "小米显示器".into()),
                            generation: *generation,
                        });
                        let _ = bus.cmd_sender.send(AppCommand::FetchBacklight {
                            generation: *generation,
                        });
                        let _ = bus.cmd_sender.send(AppCommand::RefreshSource {
                            generation: *generation,
                        });
                    }
                    Err(error) => {
                        let _ = dev.disconnect();
                        if let Some(delay) = connection_retry_delay(&error, retry_count) {
                            retry = Some((Instant::now() + delay, env.clone(), retry_count + 1));
                        }
                        crate::logger::log_adb(&format!("连接 {ip} 失败：{error}"));
                        let _ = bus.event_sender.send(AppEvent::AdbStateChanged {
                            ip: ip.clone(),
                            state: AdbDeviceState::Error(error.to_string()),
                            model: String::new(),
                            generation: *generation,
                        });
                    }
                }
            }
            AppCommand::DisconnectAdb | AppCommand::CancelConnection { .. } => {
                preset_sync.clear();
                retry = None;
                session = None;
                if let Some(old) = device.take() {
                    let _ = old.disconnect();
                }
                let _ = bus.event_sender.send(AppEvent::AdbStateChanged {
                    ip: String::new(),
                    state: AdbDeviceState::NotFound,
                    model: String::new(),
                    generation: live,
                });
            }
            AppCommand::Shutdown => break,
            _ => {
                let _ = bus.event_sender.send(AppEvent::DeviceActivity {
                    generation: live,
                    active: true,
                    message: String::new(),
                    is_error: false,
                });
                let result = if let Some(dev) = device.as_ref() {
                    execute(dev, &env.command, &bus, env.revision)
                } else {
                    Err("设备未连接，请先连接显示器".into())
                };
                if let AppCommand::TrayValueCommit { request_id, generation, .. } = &env.command {
                    let _ = bus.event_sender.send(AppEvent::TrayValueResult {
                        request_id: *request_id, generation: *generation,
                        error: result.as_ref().err().cloned(),
                    });
                }
                if matches!(
                    env.command,
                    AppCommand::ApplyPreset { .. } | AppCommand::RestoreBaseline { .. }
                ) {
                    preset_sync.clear();
                } else if result.is_ok()
                    && super::preset_sync::is_manual_picture_change(&env.command)
                    && !scheduler.is_active_or_pending_restore()
                    && generation(&env.command) == Some(bus.active_generation.load(Ordering::Acquire))
                {
                    let cfg = crate::config::load_settings();
                    let live_gen = live;
                    if let Some(active_id) = cfg.active_preset_id.as_deref() {
                        if cfg.presets.iter().any(|p| p.id == active_id) {
                            preset_sync.note(Some(active_id), live_gen, Instant::now());
                        } else {
                            preset_sync.clear();
                        }
                    } else {
                        preset_sync.clear();
                    }
                }
                // Keep a quiet interval between transactions so rapid clicks cannot flood firmware.
                std::thread::sleep(Duration::from_millis(250));
                let _ = bus.event_sender.send(AppEvent::DeviceActivity {
                    generation: live,
                    active: false,
                    message: match &result {
                        Ok(_)
                            if matches!(
                                env.command,
                                AppCommand::FetchPictureParameters { .. }
                                    | AppCommand::FetchBacklight { .. }
                                    | AppCommand::FetchGameParameters { .. }
                                    | AppCommand::FetchScreenLight { .. }
                                    | AppCommand::RefreshSource { .. }
                            ) =>
                        {
                            String::new()
                        }
                        Ok(_) => "设备操作已完成".into(),
                        Err(e) => format!("设备操作失败：{e}"),
                    },
                    is_error: result.is_err(),
                });
                if let Err(error) = result {
                    let _ = bus.event_sender.send(AppEvent::AdbOperationFailed {
                        message: error.clone(),
                        generation: live,
                    });
                    match env.command {
                        AppCommand::FetchPictureParameters { .. } => {
                            let _ = bus.event_sender.send(AppEvent::PictureFetchFailed {
                                message: error,
                                generation: live,
                            });
                        }
                        AppCommand::FetchScreenLight { revision, .. }
                        | AppCommand::SetScreenLight { revision, .. } => {
                            let _ = bus.event_sender.send(AppEvent::ScreenLightOperationFailed {
                                message: error,
                                generation: live,
                                revision,
                            });
                        }
                        AppCommand::CapturePresetSnapshot { .. }
                        | AppCommand::ApplyPreset { .. }
                        | AppCommand::RestoreBaseline { .. } => {
                            let _ = bus.event_sender.send(AppEvent::PresetOperationFailed {
                                message: error,
                                generation: live,
                            });
                        }
                        AppCommand::SendRemoteKey { key, .. } => {
                            let _ = bus.event_sender.send(AppEvent::RemoteKeyFailed {
                                key,
                                error,
                                generation: live,
                            });
                        }
                        AppCommand::SwitchSource { source, .. } => {
                            let _ = bus.event_sender.send(AppEvent::SourceSwitchFailed {
                                source,
                                error,
                                generation: live,
                            });
                        }
                        AppCommand::RefreshSource { .. } => {
                            let _ = bus.event_sender.send(AppEvent::SourceReadFailed {
                                error,
                                generation: live,
                            });
                        }
                        AppCommand::ToolAction { .. } => {
                            let _ = bus
                                .cmd_sender
                                .send(AppCommand::FetchPictureParameters { generation: live });
                            let _ = bus.event_sender.send(AppEvent::ToolResult {
                                report: error,
                                is_error: true,
                                generation: live,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn execute(
    dev: &AdbDevice,
    command: &AppCommand,
    bus: &MessageBus,
    read_revision: u64,
) -> Result<(), String> {
    use crate::device::{game as g, light as l, picture as p, remote as r, source as s};
    use AppCommand::*;
    let gen = generation(command).unwrap_or_else(|| bus.active_generation.load(Ordering::Acquire));
    let tx = &bus.event_sender;
    let fail = |e: crate::adb::AdbError| e.to_string();
    match command {
        TrayValueCommit { param, value, generation, .. } => {
            return execute(dev, &HotkeyCommit { param: *param, value: *value, generation: *generation }, bus, read_revision);
        }
        HotkeyCommit {
            param,
            value,
            generation,
        } => {
            use crate::hotkey::AdjustParam as A;
            if !(param.min()..=param.max()).contains(value) {
                return Err("快捷键目标超出范围".into());
            }
            let next = match param {
                A::AtmosphereIllumination => {
                    let mut params = l::read_screen_light_state(dev)
                        .to_parameters()
                        .ok_or("屏幕灯参数未完整读取")?;
                    params.illumination = (*value - 1) as u32;
                    SetScreenLight {
                        params,
                        generation: *generation,
                        revision: crate::ddc::next_ddc_revision(),
                    }
                }
                A::RedGain | A::GreenGain | A::BlueGain => {
                    let state = p::query_picture_parameters(dev).map_err(fail)?;
                    if state.color_temp != Some(3) {
                        return Err("当前不是自定义色温，RGB 调整已取消".into());
                    }
                    let (Some(mut red), Some(mut green), Some(mut blue)) =
                        (state.red_gain, state.green_gain, state.blue_gain)
                    else {
                        return Err("RGB 参数未完整读取".into());
                    };
                    match param {
                        A::RedGain => red = *value as u32,
                        A::GreenGain => green = *value as u32,
                        _ => blue = *value as u32,
                    };
                    ToolAction {
                        action: super::actions::ToolAction::Gains { red, green, blue },
                        generation: *generation,
                    }
                }
                _ => {
                    let param = match param {
                        A::Backlight => p::PictureSliderParam::Backlight,
                        A::BlackLevel => p::PictureSliderParam::BlackLevel,
                        A::Contrast => p::PictureSliderParam::Contrast,
                        A::Saturation => p::PictureSliderParam::Saturation,
                        A::Hue => p::PictureSliderParam::Hue,
                        _ => p::PictureSliderParam::Sharpness,
                    };
                    SetPictureSlider {
                        param,
                        value: *value as u32,
                        generation: *generation,
                        revision: crate::ddc::next_ddc_revision(),
                    }
                }
            };
            if let SetPictureSlider { param, value, .. } = &next {
                dev.transaction(|| p::set_picture_slider_param(dev, *param, *value)).map_err(fail)?;
                let _ = bus.cmd_sender.send(FetchPictureParameters { generation: gen });
                if *param == p::PictureSliderParam::Backlight {
                    let _ = bus.cmd_sender.send(FetchBacklight { generation: gen });
                }
                return Ok(());
            }
            return execute(dev, &next, bus, read_revision);
        }
        FetchBacklight { .. } => {
            let value = p::get_backlight(dev).map_err(fail)?;
            let _ = tx.send(AppEvent::BacklightUpdated {
                value: Some(value),
                generation: gen,
                revision: read_revision,
            });
        }
        SetBacklight { value, .. } => {
            dev.transaction(|| p::set_backlight(dev, *value))
                .map_err(fail)?;
            let value = p::get_backlight(dev).map_err(fail)?;
            let _ = tx.send(AppEvent::BacklightUpdated {
                value: Some(value),
                generation: gen,
                revision: read_revision,
            });
        }
        FetchPictureParameters { .. } => {
            let state = p::fetch_picture_parameters(dev).map_err(fail)?;
            let _ = tx.send(AppEvent::PictureParametersFetched {
                state: Box::new(state),
                generation: gen,
            });
        }
        SetPictureMode { mode, revision, .. } => {
            dev.transaction(|| p::set_picture_mode(dev, *mode))
                .map_err(fail)?;
            let _ = tx.send(AppEvent::PictureModeUpdated {
                mode: *mode,
                generation: gen,
                revision: *revision,
            });
        }
        SetPictureSlider {
            param,
            value,
            revision,
            ..
        } => {
            dev.transaction(|| p::set_picture_slider_param(dev, *param, *value))
                .map_err(fail)?;
            let _ = tx.send(AppEvent::PictureSliderUpdated {
                param: *param,
                value: *value,
                generation: gen,
                revision: *revision,
            });
            if *param == p::PictureSliderParam::Backlight {
                let value = p::get_backlight(dev).map_err(fail)?;
                let _ = tx.send(AppEvent::BacklightUpdated {
                    value: Some(value),
                    generation: gen,
                    revision: read_revision,
                });
            }
        }
        SetLightSensor {
            enabled, revision, ..
        } => {
            dev.transaction(|| p::set_light_sensor(dev, *enabled))
                .map_err(fail)?;
            let _ = tx.send(AppEvent::LightSensorUpdated {
                enabled: *enabled,
                generation: gen,
                revision: *revision,
            });
        }
        SetLocalDimming {
            value, revision, ..
        } => {
            dev.transaction(|| p::set_local_dimming(dev, *value))
                .map_err(fail)?;
            let _ = tx.send(AppEvent::LocalDimmingUpdated {
                value: *value,
                generation: gen,
                revision: *revision,
            });
        }
        RefreshSource { .. } => {
            let (source, raw_value) = s::get_source_with_raw(dev).map_err(fail)?;
            let _ = tx.send(AppEvent::SourceUpdated {
                source,
                raw_value,
                generation: gen,
            });
        }
        SwitchSource { source, .. } => {
            s::switch_source(dev, *source).map_err(fail)?;
            thread::sleep(Duration::from_millis(1500));
            let (actual, raw_value) = s::get_source_with_raw(dev).map_err(fail)?;
            let _ = tx.send(AppEvent::SourceUpdated {
                source: actual,
                raw_value,
                generation: gen,
            });
            if actual != Some(*source) {
                return Err("切换后读回信号源与目标不一致".into());
            }
            let _ = tx.send(AppEvent::SourceSwitchSuccess {
                source: *source,
                generation: gen,
            });
        }
        SendRemoteKey { key, .. } => {
            r::send_keyevent(dev, *key).map_err(fail)?;
            let _ = tx.send(AppEvent::RemoteKeySuccess {
                key: *key,
                generation: gen,
            });
        }
        FetchScreenLight { revision, .. } => {
            let state = dev.transaction(|| l::read_screen_light_state(dev));
            let _ = tx.send(AppEvent::ScreenLightUpdated {
                state,
                generation: gen,
                revision: *revision,
            });
        }
        SetScreenLight {
            params, revision, ..
        } => {
            l::commit_screen_light(dev, params).map_err(fail)?;
            let mut state = l::read_screen_light_state(dev);
            state.last_status = Some("设置记录已回读；灯光实际效果需目视确认".into());
            let _ = tx.send(AppEvent::ScreenLightUpdated {
                state,
                generation: gen,
                revision: *revision,
            });
        }
        CapturePresetSnapshot {
            name,
            update_preset_id,
            ..
        } => {
            let values = crate::presets::capture_hardware_snapshot(dev).map_err(fail)?;
            let _ = tx.send(AppEvent::PresetSnapshotCaptured {
                name: name.clone(),
                update_preset_id: update_preset_id.clone(),
                values,
                generation: gen,
            });
        }
        ApplyPreset {
            preset_id,
            values,
            need_baseline,
            ..
        } => {
            let config = crate::config::load_settings();
            let identity = crate::presets::recovery::DeviceIdentity::new(
                dev.serial(),
                crate::presets::query_hardware_serial(dev),
            );
            let source: Option<crate::presets::recovery::DeviceIdentity> = config
                .extra
                .get("preset_snapshot_device")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let same_origin = source
                .as_ref()
                .is_some_and(|s| s.is_definitely_same_device(&identity));
            if config.preset_snapshot.is_some() && !same_origin {
                return Err("已有来源设备不同或无法确认的初始基线。请先连接来源显示器并恢复基线，再应用新预设；已阻止未保存基线的硬件写入。".into());
            }
            let captured_baseline = if *need_baseline || config.preset_snapshot.is_none() {
                Some(crate::presets::capture_hardware_snapshot(dev).map_err(fail)?)
            } else {
                None
            };
            if let Some(baseline) = &captured_baseline {
                let identity = crate::presets::recovery::DeviceIdentity::new(
                    dev.serial(),
                    crate::presets::query_hardware_serial(dev),
                );
                let updated = crate::config::update_settings(|cfg| {
                    cfg.preset_snapshot = Some(baseline.clone());
                    cfg.extra.insert(
                        "preset_snapshot_device".into(),
                        serde_json::to_value(&identity).unwrap(),
                    );
                })
                .map_err(|e| format!("初始基线保存失败，未应用预设：{e}"))?;
                let _ = tx.send(AppEvent::ConfigUpdated((*updated).clone()));
            }
            let result = crate::presets::apply_preset_values(dev, values);
            let updated = crate::config::update_settings(|cfg| {
                if result.ok() && !result.applied.is_empty() {
                    cfg.active_preset_id = Some(preset_id.clone());
                }
            })
            .map_err(|e| format!("预设下发后保存状态失败，初始基线仍保留：{e}"))?;
            let _ = tx.send(AppEvent::ConfigUpdated((*updated).clone()));
            let _ = tx.send(AppEvent::PresetApplied {
                preset_id: preset_id.clone(),
                result,
                captured_baseline,
                generation: gen,
            });
        }
        RestoreBaseline {
            values,
            allow_unknown_origin,
            ..
        } => {
            let cfg = crate::config::load_settings();
            let source: Option<crate::presets::recovery::DeviceIdentity> = cfg
                .extra
                .get("preset_snapshot_device")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let current = crate::presets::recovery::DeviceIdentity::new(
                dev.serial(),
                crate::presets::query_hardware_serial(dev),
            );
            if !(source
                .as_ref()
                .is_some_and(|s| s.is_definitely_same_device(&current))
                || source.as_ref().is_none_or(|s| {
                    s.hardware_serial.is_none() || current.hardware_serial.is_none()
                }) && *allow_unknown_origin)
            {
                return Err("基线来源设备无法确认，已阻止恢复。旧版基线请先导出备份，并在来源显示器上重新捕获。".into());
            }
            let result = crate::presets::apply_preset_values(dev, values);
            if result.ok() && !result.applied.is_empty() {
                let updated = crate::config::update_settings(|cfg| {
                    cfg.preset_snapshot = None;
                    cfg.active_preset_id = None;
                    cfg.extra.remove("preset_snapshot_device");
                })
                .map_err(|e| format!("硬件还原后保存状态失败，基线快照仍保留：{e}"))?;
                let _ = tx.send(AppEvent::ConfigUpdated((*updated).clone()));
            }
            let _ = tx.send(AppEvent::BaselineRestored {
                result,
                generation: gen,
            });
        }
        ToolAction { action, .. } => {
            let (report, guardian) = super::actions::run(dev, action)?;
            let guardian = if let Some(status) = guardian {
                Some(status)
            } else if matches!(
                action,
                super::actions::ToolAction::CheckGuardian
                    | super::actions::ToolAction::StartGuardian
            ) {
                Some(
                    crate::device::guardian::check_guardian_status(dev)
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            if let Some(status) = guardian {
                let _ = tx.send(AppEvent::GuardianStatusUpdated {
                    status,
                    generation: gen,
                });
            }
            let _ = tx.send(AppEvent::ToolResult {
                report,
                is_error: false,
                generation: gen,
            });
            if matches!(
                action,
                super::actions::ToolAction::Advanced { .. }
                    | super::actions::ToolAction::Gains { .. }
                    | super::actions::ToolAction::ResetPicture
            ) {
                let _ = bus
                    .cmd_sender
                    .send(FetchPictureParameters { generation: gen });
            }
        }
        _ => {
            dev.transaction(|| -> Result<(), crate::adb::AdbError> {
                match command {
                    FetchGameParameters { .. } => {}
                    SetGameFreeSync {
                        enabled,
                        restore_mode,
                        ..
                    } => {
                        let cfg = crate::config::load_settings();
                        let mut restore = *restore_mode;
                        if cfg.freesync_mode_memory_enabled && !g::is_freesync_memory_suspended() {
                            let current = g::query_game_parameters(dev)?;
                            if *enabled && current.freesync == Some(false) {
                                let mode = current.picture_mode.ok_or_else(|| {
                                    crate::adb::AdbError::CommandFailed(
                                        "无法读取原画面模式，未切换 FreeSync".into(),
                                    )
                                })?;
                                if !g::is_game_picture_mode(mode) {
                                    let updated = crate::config::update_settings(|cfg| {
                                        cfg.freesync_previous_mode = Some(mode)
                                    })
                                    .map_err(|e| {
                                        crate::adb::AdbError::CommandFailed(e.to_string())
                                    })?;
                                    let _ = tx.send(AppEvent::ConfigUpdated((*updated).clone()));
                                }
                            } else if !*enabled && current.freesync == Some(true) {
                                restore = cfg.freesync_previous_mode;
                            }
                        }
                        g::set_freesync(dev, *enabled, restore)?;
                    }
                    SetGame320Hz { enabled, .. } => g::set_320hz(dev, *enabled)?,
                    SetGameFrontSight {
                        index,
                        is_game_mode,
                        ..
                    } => g::set_front_sight(dev, *index, *is_game_mode)?,
                    SetGameDynamicFrontSight { enabled, .. } => {
                        g::set_dynamic_front_sight(dev, *enabled)?
                    }
                    SetGameSniperScope { code, .. } => g::set_sniper_scope(dev, *code)?,
                    SetGameSniperNightVision { enabled, .. } => {
                        g::set_sniper_night_vision(dev, *enabled)?
                    }
                    SetGameFpsCounter { value, .. } => g::set_fps_counter(dev, *value)?,
                    SetGameStopwatch { value, .. } => g::set_stopwatch(dev, *value)?,
                    SetGameTimer { seconds, .. } => g::set_timer(dev, *seconds)?,
                    SwitchGameModeAndApply {
                        switch_to_game,
                        front_sight,
                        dynamic_front_sight,
                        sniper_scope,
                        sniper_night_vision,
                        ..
                    } => {
                        let plan = g::SwitchGameApplyPlan {
                            switch_to_game: *switch_to_game,
                            front_sight: *front_sight,
                            dynamic_front_sight: *dynamic_front_sight,
                            sniper_scope: *sniper_scope,
                            sniper_night_vision: *sniper_night_vision,
                        };
                        g::execute_switch_game_apply_steps(
                            &mut g::RealGameStepExecutor(dev, !plan.switch_to_game),
                            &plan,
                        )
                        .map_err(|(_, e)| e)?;
                    }
                    _ => {}
                }
                Ok(())
            })
            .map_err(fail)?;
            let params = g::query_game_parameters(dev).map_err(fail)?;
            let _ = tx.send(AppEvent::GameParametersUpdated {
                params,
                generation: gen,
                revision: read_revision,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connection_recovery_is_bounded_and_stops_for_authorization_or_cancel() {
        use crate::adb::AdbError::*;
        assert_eq!(connection_retry_delay(&NetworkUnreachable("wifi".into()), 0), Some(Duration::from_secs(2)));
        assert_eq!(connection_retry_delay(&DeviceOffline, 5), Some(Duration::from_secs(30)));
        assert_eq!(connection_retry_delay(&DeviceOffline, 6), None);
        assert_eq!(connection_retry_delay(&Unauthorized, 0), None);
        assert_eq!(connection_retry_delay(&Cancelled, 0), None);
        assert_eq!(connection_retry_delay(&ServerNotReady("foreign owner".into()), 0), None);
    }

    #[test]
    fn continuous_updates_cannot_starve_the_first_write() {
        let inbox = Arc::new(Inbox::default());
        let message = Envelope {
            command: AppCommand::SetBacklight { value: 20, generation: 1 },
            token: None, revision: 0, dispatch_generation: 1,
        };
        inbox.push(message.clone()).unwrap();
        let producer_inbox = inbox.clone();
        let producer = thread::spawn(move || {
            for _ in 0..15 {
                thread::sleep(Duration::from_millis(80));
                producer_inbox.push(message.clone()).unwrap();
            }
        });
        let start = Instant::now();
        assert!(inbox.receive_timed().is_some());
        let elapsed = start.elapsed();
        producer.join().unwrap();
        assert!(elapsed < Duration::from_millis(900), "debounce starved for {elapsed:?}");
    }

    #[test]
    fn session_transition_waits_for_current_commit() {
        let generation = Arc::new(SessionGeneration::new(1));
        let (requested_tx, requested_rx) = channel();
        let (finished_tx, finished_rx) = channel();
        let mut transition = None;
        assert_eq!(generation.with_current(1, || {
            let other = generation.clone();
            transition = Some(thread::spawn(move || {
                requested_tx.send(()).unwrap();
                other.fetch_add(1, Ordering::AcqRel);
                finished_tx.send(()).unwrap();
            }));
            requested_rx.recv().unwrap();
            assert!(finished_rx.try_recv().is_err());
            assert_eq!(generation.load(Ordering::Acquire), 1);
            "committed"
        }), Some("committed"));
        transition.unwrap().join().unwrap();
        finished_rx.recv().unwrap();
        assert_eq!(generation.load(Ordering::Acquire), 2);
        assert_eq!(generation.with_current(1, || "stale write"), None);
    }
    #[test]
    fn hotkey_replacing_tray_commit_completes_superseded_request() {
        use crate::hotkey::AdjustParam;
        let (tx, rx) = channel();
        let q = Inbox { events: Some(tx), ..Inbox::default() };
        q.push(Envelope {
            command: AppCommand::TrayValueCommit {
                param: AdjustParam::Contrast, value: 30, generation: 7, request_id: 41,
            }, token: None, dispatch_generation: 7, revision: 0,
        }).unwrap();
        q.push(Envelope {
            command: AppCommand::HotkeyCommit {
                param: AdjustParam::Contrast, value: 40, generation: 7,
            }, token: None, dispatch_generation: 7, revision: 0,
        }).unwrap();
        assert_eq!(q.queue.lock().len(), 1);
        assert!(matches!(q.receive().unwrap().command, AppCommand::HotkeyCommit { value: 40, .. }));
        assert!(matches!(rx.try_recv().unwrap(), AppEvent::TrayValueResult {
            request_id: 41, generation: 7, error: Some(_),
        }));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn slider_queue_keeps_last_and_orders_after_reads() {
        let q = Inbox::default();
        q.push(Envelope {
            command: AppCommand::SetBacklight {
                value: 10,
                generation: 1,
            },
            token: None,
            dispatch_generation: 1,
            revision: 1,
        })
        .unwrap();
        q.push(Envelope {
            command: AppCommand::FetchBacklight { generation: 1 },
            token: None,
            dispatch_generation: 1,
            revision: 1,
        })
        .unwrap();
        q.push(Envelope {
            command: AppCommand::SetBacklight {
                value: 20,
                generation: 1,
            },
            token: None,
            dispatch_generation: 1,
            revision: 2,
        })
        .unwrap();
        assert_eq!(q.queue.lock().len(), 2);
        assert!(matches!(
            q.receive().unwrap().command,
            AppCommand::FetchBacklight { .. }
        ));
        assert!(matches!(
            q.receive().unwrap().command,
            AppCommand::SetBacklight { value: 20, .. }
        ));
    }
    #[test]
    fn mode_click_burst_only_sends_latest() {
        let q = Inbox::default();
        for mode in [14, 10, 9] {
            q.push(Envelope {
                command: AppCommand::SetPictureMode {
                    mode,
                    generation: 1,
                    revision: mode as u64,
                },
                token: None,
                revision: 0,
                dispatch_generation: 1,
            })
            .unwrap();
        }
        assert_eq!(q.queue.lock().len(), 1);
        assert!(matches!(
            q.receive_timed().unwrap().command,
            AppCommand::SetPictureMode { mode: 9, .. }
        ));
    }
    #[test]
    fn close_discards_queued_hardware_writes() {
        let q = Inbox::default();
        q.push(Envelope {
            command: AppCommand::SetBacklight {
                value: 30,
                generation: 1,
            },
            token: None,
            dispatch_generation: 1,
            revision: 1,
        })
        .unwrap();
        q.close();
        assert!(q.receive().is_none());
        assert!(q
            .push(Envelope {
                command: AppCommand::FetchBacklight { generation: 1 },
                token: None,
                dispatch_generation: 1,
                revision: 1
            })
            .is_err());
    }
    #[test]
    fn hotkey_commit_coalesces_same_param_and_preserves_different_params() {
        use crate::hotkey::AdjustParam;
        let q = Inbox::default();
        q.push(Envelope {
            command: AppCommand::HotkeyCommit {
                param: AdjustParam::Backlight,
                value: 30,
                generation: 1,
            },
            token: None,
            dispatch_generation: 1,
            revision: 1,
        })
        .unwrap();
        q.push(Envelope {
            command: AppCommand::HotkeyCommit {
                param: AdjustParam::Backlight,
                value: 40,
                generation: 1,
            },
            token: None,
            dispatch_generation: 1,
            revision: 2,
        })
        .unwrap();
        q.push(Envelope {
            command: AppCommand::HotkeyCommit {
                param: AdjustParam::Contrast,
                value: 50,
                generation: 1,
            },
            token: None,
            dispatch_generation: 1,
            revision: 3,
        })
        .unwrap();
        assert_eq!(q.queue.lock().len(), 2);
        let first = q.receive().unwrap().command;
        assert!(matches!(
            first,
            AppCommand::HotkeyCommit {
                param: AdjustParam::Backlight,
                value: 40,
                ..
            }
        ));
        let second = q.receive().unwrap().command;
        assert!(matches!(
            second,
            AppCommand::HotkeyCommit {
                param: AdjustParam::Contrast,
                value: 50,
                ..
            }
        ));
    }
}
