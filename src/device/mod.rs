pub mod game;
pub mod guardian;
pub mod jni;
pub mod light;
pub mod mode_4k;
pub mod picture;
pub mod remote;
pub mod source;

pub use game::GameParameters;
pub use guardian::{check_guardian_status, install_and_enable_guardian, GuardianStatus};
pub use light::{commit_screen_light, ScreenLightMode, ScreenLightParameters};
pub use mode_4k::{
    enable_4k_ui, query_ui_resolution, reset_ui_resolution, set_1080p_ui, UiResolutionInfo,
};
pub use picture::{
    get_picture_scene_name, is_game_picture_mode, is_hdr_tone_mapping_picture_mode, set_backlight,
    set_color_gains, set_color_space, set_color_temp, set_hdr_tone_mapping, set_light_sensor,
    set_local_dimming, set_picture_mode, set_plain_setting, PictureParameters,
};
pub use remote::{send_keyevent, send_raw_keycode, RemoteKey};
pub use source::{get_current_source, switch_source, VideoSource};

use std::sync::atomic::{AtomicU64, Ordering};

/// 连接状态与代数管理（杜绝陈旧异步操作覆盖当前会话）
#[derive(Debug, Default)]
pub struct DeviceStateManager {
    generation: AtomicU64,
}

impl DeviceStateManager {
    pub fn new() -> Self {
        Self {
            generation: AtomicU64::new(1),
        }
    }

    /// 递增并生成新的连接代数
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// 获取当前有效代数
    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// 校验指定代数是否仍然有效
    pub fn is_current(&self, gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == gen
    }
}

pub mod hdr;
