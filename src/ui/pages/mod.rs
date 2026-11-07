pub mod advanced_picture;
pub mod auto_task;
pub mod game;
pub mod home;
pub mod light;
pub mod picture;
pub mod preset;
pub mod remote;
pub mod source;
pub mod tools_panel;
pub mod tray;

pub use auto_task::render_auto_task_page;
pub use game::render_game_page;
pub use home::render_home_page;
pub use light::render_light_page;
pub use picture::render_picture_page;
pub use preset::render_preset_page;
pub use remote::render_remote_page;
pub use source::render_source_page;
pub use tools_panel::render_tools_page;
pub use tray::render_tray_page;

pub mod hotkeys;
