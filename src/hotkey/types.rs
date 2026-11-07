use serde::{Deserialize, Serialize};

/// 8 种固定动作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CycleAction {
    PictureModeCycle,
    LocalDimmingCycle,
    LocalDimmingToggleOff,
    ColorSpaceCycle,
    ColorTempCycle,
    ResponseTimeCycle,
    FreeSyncToggle,
    InputSourceCycle,
}

impl CycleAction {
    pub const ALL: [CycleAction; 8] = [
        CycleAction::PictureModeCycle,
        CycleAction::LocalDimmingCycle,
        CycleAction::LocalDimmingToggleOff,
        CycleAction::ColorSpaceCycle,
        CycleAction::ColorTempCycle,
        CycleAction::ResponseTimeCycle,
        CycleAction::FreeSyncToggle,
        CycleAction::InputSourceCycle,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            CycleAction::PictureModeCycle => "picture_mode_cycle",
            CycleAction::LocalDimmingCycle => "local_dimming_cycle",
            CycleAction::LocalDimmingToggleOff => "local_dimming_toggle_off",
            CycleAction::ColorSpaceCycle => "color_space_cycle",
            CycleAction::ColorTempCycle => "color_temp_cycle",
            CycleAction::ResponseTimeCycle => "response_time_cycle",
            CycleAction::FreeSyncToggle => "freesync_toggle",
            CycleAction::InputSourceCycle => "input_source_cycle",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CycleAction::PictureModeCycle => "画面模式 循环切换",
            CycleAction::LocalDimmingCycle => "精密控光 循环切换",
            CycleAction::LocalDimmingToggleOff => "精密控光 开关切换",
            CycleAction::ColorSpaceCycle => "色域 循环切换",
            CycleAction::ColorTempCycle => "色温 循环切换",
            CycleAction::ResponseTimeCycle => "响应时间 循环切换",
            CycleAction::FreeSyncToggle => "FreeSync 开关切换",
            CycleAction::InputSourceCycle => "信号源 循环切换",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "picture_mode_cycle" => Some(CycleAction::PictureModeCycle),
            "local_dimming_cycle" => Some(CycleAction::LocalDimmingCycle),
            "local_dimming_toggle_off" => Some(CycleAction::LocalDimmingToggleOff),
            "color_space_cycle" => Some(CycleAction::ColorSpaceCycle),
            "color_temp_cycle" => Some(CycleAction::ColorTempCycle),
            "response_time_cycle" => Some(CycleAction::ResponseTimeCycle),
            "freesync_toggle" => Some(CycleAction::FreeSyncToggle),
            "input_source_cycle" => Some(CycleAction::InputSourceCycle),
            _ => None,
        }
    }
}

/// 10 种可微调参数类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdjustParam {
    Backlight,
    BlackLevel,
    Contrast,
    Saturation,
    Hue,
    Sharpness,
    RedGain,
    GreenGain,
    BlueGain,
    AtmosphereIllumination,
}

impl AdjustParam {
    pub const ALL: [AdjustParam; 10] = [
        AdjustParam::Backlight,
        AdjustParam::BlackLevel,
        AdjustParam::Contrast,
        AdjustParam::Saturation,
        AdjustParam::Hue,
        AdjustParam::Sharpness,
        AdjustParam::RedGain,
        AdjustParam::GreenGain,
        AdjustParam::BlueGain,
        AdjustParam::AtmosphereIllumination,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            AdjustParam::Backlight => "backlight",
            AdjustParam::BlackLevel => "black_level",
            AdjustParam::Contrast => "contrast",
            AdjustParam::Saturation => "saturation",
            AdjustParam::Hue => "hue",
            AdjustParam::Sharpness => "sharpness",
            AdjustParam::RedGain => "red_gain",
            AdjustParam::GreenGain => "green_gain",
            AdjustParam::BlueGain => "blue_gain",
            AdjustParam::AtmosphereIllumination => "atmosphere_illumination",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AdjustParam::Backlight => "背光",
            AdjustParam::BlackLevel => "黑色级别",
            AdjustParam::Contrast => "对比度",
            AdjustParam::Saturation => "饱和度",
            AdjustParam::Hue => "色调",
            AdjustParam::Sharpness => "锐度",
            AdjustParam::RedGain => "红色增益",
            AdjustParam::GreenGain => "绿色增益",
            AdjustParam::BlueGain => "蓝色增益",
            AdjustParam::AtmosphereIllumination => "屏幕灯亮度",
        }
    }

    pub fn min(&self) -> i32 {
        match self {
            AdjustParam::Backlight => 1,
            AdjustParam::BlackLevel => 0,
            AdjustParam::Contrast => 0,
            AdjustParam::Saturation => 0,
            AdjustParam::Hue => 0,
            AdjustParam::Sharpness => 0,
            AdjustParam::RedGain => 524,
            AdjustParam::GreenGain => 524,
            AdjustParam::BlueGain => 524,
            AdjustParam::AtmosphereIllumination => 1,
        }
    }

    pub fn max(&self) -> i32 {
        match self {
            AdjustParam::Backlight => 100,
            AdjustParam::BlackLevel => 100,
            AdjustParam::Contrast => 100,
            AdjustParam::Saturation => 100,
            AdjustParam::Hue => 100,
            AdjustParam::Sharpness => 100,
            AdjustParam::RedGain => 1524,
            AdjustParam::GreenGain => 1524,
            AdjustParam::BlueGain => 1524,
            AdjustParam::AtmosphereIllumination => 15,
        }
    }

    pub fn default_step(&self) -> u32 {
        match self {
            AdjustParam::Backlight => 5,
            AdjustParam::BlackLevel => 5,
            AdjustParam::Contrast => 5,
            AdjustParam::Saturation => 5,
            AdjustParam::Hue => 5,
            AdjustParam::Sharpness => 1,
            AdjustParam::RedGain => 10,
            AdjustParam::GreenGain => 10,
            AdjustParam::BlueGain => 10,
            AdjustParam::AtmosphereIllumination => 1,
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "backlight" | "picture_backlight" | "xiaomi_picture_backlight" => {
                Some(AdjustParam::Backlight)
            }
            "black_level" | "picture_brightness" => Some(AdjustParam::BlackLevel),
            "contrast" | "picture_contrast" => Some(AdjustParam::Contrast),
            "saturation" | "picture_saturation" => Some(AdjustParam::Saturation),
            "hue" | "picture_hue" => Some(AdjustParam::Hue),
            "sharpness" | "picture_sharpness" => Some(AdjustParam::Sharpness),
            "red_gain" | "picture_red_gain" | "red" => Some(AdjustParam::RedGain),
            "green_gain" | "picture_green_gain" | "green" => Some(AdjustParam::GreenGain),
            "blue_gain" | "picture_blue_gain" | "blue" => Some(AdjustParam::BlueGain),
            "atmosphere_illumination" | "atmosphere_light_illumination" => {
                Some(AdjustParam::AtmosphereIllumination)
            }
            _ => None,
        }
    }
}

/// 调节方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdjustDirection {
    Increase,
    Decrease,
}

impl AdjustDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdjustDirection::Increase => "increase",
            AdjustDirection::Decrease => "decrease",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "increase" | "增加" | "up" | "+" => Some(AdjustDirection::Increase),
            "decrease" | "减少" | "down" | "-" => Some(AdjustDirection::Decrease),
            _ => None,
        }
    }
}

/// 触发的快捷键事件（交由宿主消费，不在此做 ADB）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    Cycle(CycleAction),
    Adjust {
        param: AdjustParam,
        direction: AdjustDirection,
        step: u32,
    },
}

/// 修饰键解析与映射
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyModifier {
    None,
    CtrlAlt,
    CtrlShift,
    AltShift,
    WinShift,
}

impl HotkeyModifier {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed == "无" || trimmed.eq_ignore_ascii_case("none") {
            Some(HotkeyModifier::None)
        } else if trimmed.eq_ignore_ascii_case("Ctrl + Alt")
            || trimmed.eq_ignore_ascii_case("Alt + Ctrl")
        {
            Some(HotkeyModifier::CtrlAlt)
        } else if trimmed.eq_ignore_ascii_case("Ctrl + Shift")
            || trimmed.eq_ignore_ascii_case("Shift + Ctrl")
        {
            Some(HotkeyModifier::CtrlShift)
        } else if trimmed.eq_ignore_ascii_case("Alt + Shift")
            || trimmed.eq_ignore_ascii_case("Shift + Alt")
        {
            Some(HotkeyModifier::AltShift)
        } else if trimmed.eq_ignore_ascii_case("Win + Shift")
            || trimmed.eq_ignore_ascii_case("Shift + Win")
        {
            Some(HotkeyModifier::WinShift)
        } else {
            None
        }
    }

    pub fn win32_flags(&self) -> u32 {
        // MOD_ALT = 0x0001, MOD_CONTROL = 0x0002, MOD_SHIFT = 0x0004, MOD_WIN = 0x0008
        match self {
            HotkeyModifier::None => 0,
            HotkeyModifier::CtrlAlt => 0x0002 | 0x0001,
            HotkeyModifier::CtrlShift => 0x0002 | 0x0004,
            HotkeyModifier::AltShift => 0x0001 | 0x0004,
            HotkeyModifier::WinShift => 0x0008 | 0x0004,
        }
    }
}

/// 按键白名单与虚拟键码 (VK)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyKey {
    None,
    F(u8),
    Digit(u8),
    Alpha(char),
    Plus,
    Minus,
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

impl HotkeyKey {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed == "无" || trimmed.eq_ignore_ascii_case("none") {
            return Some(HotkeyKey::None);
        }

        // F1..F12
        if let Some(rest) = trimmed
            .strip_prefix('F')
            .or_else(|| trimmed.strip_prefix('f'))
        {
            if let Ok(n) = rest.parse::<u8>() {
                if (1..=12).contains(&n) {
                    return Some(HotkeyKey::F(n));
                }
            }
        }

        // 0..9
        if trimmed.len() == 1 {
            let ch = trimmed.chars().next().unwrap();
            if ch.is_ascii_digit() {
                return Some(HotkeyKey::Digit(ch as u8 - b'0'));
            }
            if ch.is_ascii_alphabetic() {
                return Some(HotkeyKey::Alpha(ch.to_ascii_uppercase()));
            }
            if ch == '+' {
                return Some(HotkeyKey::Plus);
            }
            if ch == '-' {
                return Some(HotkeyKey::Minus);
            }
            if ch == '↑' {
                return Some(HotkeyKey::ArrowUp);
            }
            if ch == '↓' {
                return Some(HotkeyKey::ArrowDown);
            }
            if ch == '←' {
                return Some(HotkeyKey::ArrowLeft);
            }
            if ch == '→' {
                return Some(HotkeyKey::ArrowRight);
            }
        }

        if trimmed.eq_ignore_ascii_case("PageUp") {
            return Some(HotkeyKey::PageUp);
        }
        if trimmed.eq_ignore_ascii_case("PageDown") {
            return Some(HotkeyKey::PageDown);
        }
        if trimmed.eq_ignore_ascii_case("Up") {
            return Some(HotkeyKey::ArrowUp);
        }
        if trimmed.eq_ignore_ascii_case("Down") {
            return Some(HotkeyKey::ArrowDown);
        }
        if trimmed.eq_ignore_ascii_case("Left") {
            return Some(HotkeyKey::ArrowLeft);
        }
        if trimmed.eq_ignore_ascii_case("Right") {
            return Some(HotkeyKey::ArrowRight);
        }

        None
    }

    pub fn vk_code(&self) -> u32 {
        match self {
            HotkeyKey::None => 0,
            HotkeyKey::F(n) => 0x6F + (*n as u32), // F1 = 0x70, F12 = 0x7B
            HotkeyKey::Digit(d) => 0x30 + (*d as u32),
            HotkeyKey::Alpha(c) => *c as u32,
            HotkeyKey::Plus => 0xBB,
            HotkeyKey::Minus => 0xBD,
            HotkeyKey::PageUp => 0x21,
            HotkeyKey::PageDown => 0x22,
            HotkeyKey::ArrowLeft => 0x25,
            HotkeyKey::ArrowUp => 0x26,
            HotkeyKey::ArrowRight => 0x27,
            HotkeyKey::ArrowDown => 0x28,
        }
    }
}

/// 纯按键组合结构体
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCombination {
    pub modifier: HotkeyModifier,
    pub key: HotkeyKey,
}

impl KeyCombination {
    pub fn is_unbound(&self) -> bool {
        self.modifier == HotkeyModifier::None && self.key == HotkeyKey::None
    }

    pub fn vk_code(&self) -> u32 {
        self.key.vk_code()
    }

    pub fn modifier_flags(&self) -> u32 {
        self.modifier.win32_flags()
    }
}

/// 单个按键绑定配置（对应 config.extra["hotkeys"][action]）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HotkeyBindingConfig {
    #[serde(default)]
    pub modifier: String,
    #[serde(default)]
    pub key: String,
}

/// 可调参数快捷键配置（对应 config.extra["adjust_hotkeys"][i]）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AdjustHotkeyConfig {
    #[serde(default)]
    pub param: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub step: Option<u32>,
    #[serde(default)]
    pub modifier: String,
    #[serde(default)]
    pub key: String,
}

/// 注册结果状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationStatus {
    /// 注册成功，返回分配的 Win32 热键 ID
    Success { id: i32 },
    /// 未绑定（如修饰键和键位均为"无"）
    SkippedUnbound,
    /// 配置非法（如非法按键、非法参数、step 不是正整数或超出有效范围）
    InvalidConfig(String),
    /// 重复热键冲突（在同一批配置中已有其他动作注册了相同组合）
    DuplicateBinding,
    /// 系统或第三方应用占用冲突 (Win32 RegisterHotKey 失败)
    SystemOccupied {
        error_code: u32,
        error_message: String,
    },
}

/// 单个快捷键的注册结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingleRegistrationResult {
    pub target_name: String,
    pub modifier: String,
    pub key: String,
    pub status: RegistrationStatus,
}

/// 整体注册报告
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyRegistrationReport {
    pub results: Vec<SingleRegistrationResult>,
}

impl HotkeyRegistrationReport {
    pub fn success_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.status, RegistrationStatus::Success { .. }))
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    RegistrationStatus::InvalidConfig(_)
                        | RegistrationStatus::DuplicateBinding
                        | RegistrationStatus::SystemOccupied { .. }
                )
            })
            .count()
    }

    pub fn skipped_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.status, RegistrationStatus::SkippedUnbound))
            .count()
    }

    pub fn is_all_ok(&self) -> bool {
        self.failed_count() == 0
    }
}
