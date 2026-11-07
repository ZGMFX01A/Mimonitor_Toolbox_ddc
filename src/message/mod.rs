use crate::adb::{AdbDeviceState, DiscoveredDevice};
use crate::ddc::DdcMonitorInfo;
use crate::device::light::ScreenLightParameters;
use crate::device::picture::{PictureParametersState, PictureSliderParam};
use crate::device::remote::RemoteKey;
use crate::device::source::VideoSource;
use crate::presets::ApplyResult;

/// 界面向后台工作管道提交的强类型业务指令（均携带会话或代数标识）
#[derive(Debug, Clone)]
pub enum AppCommand {
    Hotkey {
        event: crate::hotkey::HotkeyEvent,
        generation: u64,
    },
    CancelHotkey,
    HotkeyCommit {
        param: crate::hotkey::AdjustParam,
        value: i32,
        generation: u64,
    },
    SaveAutoTask(crate::config::AutoTaskItem),
    DeleteAutoTask(String),
    ConfirmTaskRestore(String),
    ConnectAdb {
        ip: String,
        generation: u64,
    },
    CancelConnection {
        generation: u64,
    },
    DisconnectAdb,
    RefreshMonitors,
    FetchBacklight {
        generation: u64,
    },
    SetPictureMode {
        mode: i32,
        generation: u64,
        revision: u64,
    },
    FetchPictureParameters {
        generation: u64,
    },
    SetPictureSlider {
        param: PictureSliderParam,
        value: u32,
        generation: u64,
        revision: u64,
    },
    SetLightSensor {
        enabled: bool,
        generation: u64,
        revision: u64,
    },
    SetLocalDimming {
        value: u32,
        generation: u64,
        revision: u64,
    },
    SetBacklight {
        value: u32,
        generation: u64,
    },
    SetDdcBrightness {
        device_id: String,
        percent: u32,
        revision: u64,
    },
    CapturePresetSnapshot {
        name: Option<String>,
        update_preset_id: Option<String>,
        generation: u64,
    },
    ApplyPreset {
        preset_id: String,
        values: std::collections::HashMap<String, serde_json::Value>,
        need_baseline: bool,
        generation: u64,
    },
    RestoreBaseline {
        allow_unknown_origin: bool,
        values: std::collections::HashMap<String, serde_json::Value>,
        generation: u64,
    },
    ScanNetworkDevices {
        generation: u64,
    },
    CancelScan,
    SwitchSource {
        source: VideoSource,
        generation: u64,
    },
    RefreshSource {
        generation: u64,
    },
    FetchScreenLight {
        generation: u64,
        revision: u64,
    },
    SetScreenLight {
        params: ScreenLightParameters,
        generation: u64,
        revision: u64,
    },
    SendRemoteKey {
        key: RemoteKey,
        generation: u64,
    },
    FetchGameParameters {
        generation: u64,
    },
    SetGameFreeSync {
        enabled: bool,
        restore_mode: Option<i32>,
        generation: u64,
    },
    SetGame320Hz {
        enabled: bool,
        generation: u64,
    },
    SetGameFrontSight {
        index: u32,
        is_game_mode: bool,
        generation: u64,
    },
    SetGameDynamicFrontSight {
        enabled: bool,
        generation: u64,
    },
    SetGameSniperScope {
        code: u32,
        generation: u64,
    },
    SetGameSniperNightVision {
        enabled: bool,
        generation: u64,
    },
    SetGameFpsCounter {
        value: u32,
        generation: u64,
    },
    SetGameStopwatch {
        value: u32,
        generation: u64,
    },
    SetGameTimer {
        seconds: u32,
        generation: u64,
    },
    SwitchGameModeAndApply {
        switch_to_game: bool,
        front_sight: Option<u32>,
        dynamic_front_sight: Option<bool>,
        sniper_scope: Option<u32>,
        sniper_night_vision: Option<bool>,
        generation: u64,
    },
    ToolAction {
        action: actions::ToolAction,
        generation: u64,
    },
    Shutdown,
}

/// 后台向界面通知的异步业务事件
#[derive(Debug, Clone)]
pub enum AppEvent {
    GuardianStatusUpdated {
        status: crate::device::guardian::GuardianStatus,
        generation: u64,
    },
    DeviceActivity {
        generation: u64,
        active: bool,
        message: String,
        is_error: bool,
    },
    OpenBrightness,
    HotkeyOsd {
        message: Option<String>,
    },
    ToolResult {
        report: String,
        is_error: bool,
        generation: u64,
    },
    ConfigUpdated(crate::config::AppConfig),
    AdbStateChanged {
        ip: String,
        state: AdbDeviceState,
        model: String,
        generation: u64,
    },
    BacklightUpdated {
        value: Option<u32>,
        generation: u64,
        revision: u64,
    },
    AdbOperationFailed {
        message: String,
        generation: u64,
    },
    WindowsDisplaysDiscovered(Vec<crate::ddc::RawDisplayItem>),
    DdcMonitorsDiscovered(Vec<DdcMonitorInfo>),
    DdcBrightnessChanged {
        device_id: String,
        percent: u32,
        revision: u64,
    },
    DdcOperationFailed {
        device_id: String,
        error: String,
    },
    ScanDeviceDiscovered {
        device: DiscoveredDevice,
        generation: u64,
    },
    ScanCompleted {
        found_count: usize,
        generation: u64,
    },
    PresetSnapshotCaptured {
        name: Option<String>,
        update_preset_id: Option<String>,
        values: std::collections::HashMap<String, serde_json::Value>,
        generation: u64,
    },
    PresetApplied {
        preset_id: String,
        result: ApplyResult,
        captured_baseline: Option<std::collections::HashMap<String, serde_json::Value>>,
        generation: u64,
    },
    BaselineRestored {
        result: ApplyResult,
        generation: u64,
    },
    PresetOperationFailed {
        message: String,
        generation: u64,
    },
    SourceUpdated {
        source: Option<VideoSource>,
        raw_value: String,
        generation: u64,
    },
    SourceSwitchSuccess {
        source: VideoSource,
        generation: u64,
    },
    SourceSwitchFailed {
        source: VideoSource,
        error: String,
        generation: u64,
    },
    SourceReadFailed {
        error: String,
        generation: u64,
    },
    RemoteKeySuccess {
        key: RemoteKey,
        generation: u64,
    },
    RemoteKeyFailed {
        key: RemoteKey,
        error: String,
        generation: u64,
    },
    ToastNotification {
        message: String,
        is_error: bool,
    },
    ScreenLightUpdated {
        state: crate::device::light::ScreenLightState,
        generation: u64,
        revision: u64,
    },
    ScreenLightCommitted {
        params: ScreenLightParameters,
        generation: u64,
        revision: u64,
    },
    ScreenLightOperationFailed {
        message: String,
        generation: u64,
        revision: u64,
    },
    PictureParametersFetched {
        state: Box<PictureParametersState>,
        generation: u64,
    },
    PictureFetchFailed {
        message: String,
        generation: u64,
    },
    PictureSliderUpdated {
        param: PictureSliderParam,
        value: u32,
        generation: u64,
        revision: u64,
    },
    PictureModeUpdated {
        mode: i32,
        generation: u64,
        revision: u64,
    },
    LightSensorUpdated {
        enabled: bool,
        generation: u64,
        revision: u64,
    },
    LocalDimmingUpdated {
        value: u32,
        generation: u64,
        revision: u64,
    },
    GameParametersUpdated {
        revision: u64,
        params: crate::device::game::GameParameters,
        generation: u64,
    },
}

pub mod actions;
mod runtime;
mod scheduler;
mod shortcuts;
pub use runtime::{CommandSender, MessageBus, GAME_REVISION};
