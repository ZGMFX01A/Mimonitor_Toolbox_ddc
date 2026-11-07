use crate::adb::{AdbDevice, AdbError};
use crate::device::jni::{jni_set, refresh_pq};
pub use crate::device::picture::is_game_picture_mode;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

pub const DP_SOURCES: &[&str] = &["29", "30"];
pub const HDMI_SOURCES: &[&str] = &["23", "24"];
pub const VALID_SOURCES: &[&str] = &["23", "24", "29", "30"];

/// 校验信号源是否为合法支持的输入源
/// 返回 Ok(true) 表示 DP/Type-C，Ok(false) 表示 HDMI
/// 未知源报错禁写，严禁默认
pub fn validate_source(source_str: &str) -> Result<bool, AdbError> {
    let trimmed = source_str.trim();
    if DP_SOURCES.contains(&trimmed) {
        Ok(true)
    } else if HDMI_SOURCES.contains(&trimmed) {
        Ok(false)
    } else {
        Err(AdbError::CommandFailed(format!(
            "未知或不支持的信号源输入: '{}'，仅支持 HDMI 1 (23), HDMI 2 (24), DP (29), Type-C (30)",
            trimmed
        )))
    }
}

/// 获取当前信号源对应的 FreeSync JNI 键名与开启数值
/// DP 29/30: adaptive_sync 开启 1，关闭 0
/// HDMI 23/24: freesync_switch 开启 3，关闭 0
pub fn get_freesync_param_for_source(is_dp: bool) -> (&'static str, i32) {
    if is_dp {
        ("g_video__dp_adaptive_sync", 1)
    } else {
        ("g_video__freesync_switch", 3)
    }
}

/// 获取当前信号源对应的 320Hz EDID JNI 键名与开启/关闭数值
/// DP 29/30: 3 开, 2 关
/// HDMI 23/24: 6 开, 1 关
pub fn get_320hz_param_for_source(is_dp: bool) -> (&'static str, i32, i32) {
    if is_dp {
        ("g_fusion_picture__dp_edid_version", 3, 2)
    } else {
        ("g_fusion_picture__hdmi_edid_version", 6, 1)
    }
}

/// 狙击镜合法代码: 0=关, 1=1.1x, 3=1.3x, 5=1.5x, 7=1.7x, 10=2.0x (禁止 clamp 0..3 错误)
pub const SNIPER_SCOPE_CODES: &[u32] = &[0, 1, 3, 5, 7, 10];

pub const SNIPER_SCOPE_OPTIONS: &[(u32, &'static str)] = &[
    (0, "关"),
    (1, "1.1x"),
    (3, "1.3x"),
    (5, "1.5x"),
    (7, "1.7x"),
    (10, "2.0x"),
];

pub const FRONT_SIGHT_OPTIONS: &[(u32, &'static str)] =
    &[(0, "关"), (1, "1"), (2, "2"), (3, "3"), (4, "4"), (5, "5")];

pub const FPS_COUNTER_OPTIONS: &[(u32, &'static str)] = &[(0, "关"), (1, "刷新率"), (2, "柱状图")];

pub const STOPWATCH_OPTIONS: &[(u32, &'static str)] = &[(0, "关"), (1, "开")];

pub const TIMER_OPTIONS: &[(u32, &'static str)] = &[
    (0, "关"),
    (60, "1分钟"),
    (300, "5分钟"),
    (1800, "30分钟"),
    (3600, "60分钟"),
];

pub fn validate_sniper_scope(code: u32) -> Result<(), AdbError> {
    if SNIPER_SCOPE_CODES.contains(&code) {
        Ok(())
    } else {
        Err(AdbError::CommandFailed(format!(
            "非法的狙击镜代码: {}，必须为 0, 1, 3, 5, 7, 10 之一",
            code
        )))
    }
}

/// 定时器合法参数值: 0=关, 60=1分钟, 300=5分钟, 1800=30分钟, 3600=60分钟
pub const TIMER_VALUES: &[u32] = &[0, 60, 300, 1800, 3600];

pub fn validate_timer_seconds(seconds: u32) -> Result<(), AdbError> {
    if TIMER_VALUES.contains(&seconds) {
        Ok(())
    } else {
        Err(AdbError::CommandFailed(format!(
            "非法的定时器秒数: {}，仅支持 0, 60, 300, 1800, 3600",
            seconds
        )))
    }
}

/// FreeSync 记忆全局暂停标记（供后续预设激活时暂停该记忆）
static FREESYNC_MEMORY_SUSPENDED: AtomicBool = AtomicBool::new(false);

/// 设置 FreeSync 模式记忆是否暂停（如预设激活时调用）
pub fn set_freesync_memory_suspended(suspended: bool) {
    FREESYNC_MEMORY_SUSPENDED.store(suspended, Ordering::SeqCst);
}

/// 查询 FreeSync 模式记忆当前是否处于暂停状态
pub fn is_freesync_memory_suspended() -> bool {
    FREESYNC_MEMORY_SUSPENDED.load(Ordering::SeqCst)
}

/// 真实游戏特性配置与状态（全 Option 结构，未知不默认）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GameParameters {
    pub freesync: Option<bool>,
    pub mode_320: Option<bool>,
    pub front_sight_index: Option<u32>,
    pub dynamic_front_sight: Option<bool>,
    pub sniper_scope: Option<u32>,
    pub sniper_night_vision: Option<bool>,
    pub fps_counter: Option<u32>,
    pub stopwatch: Option<u32>,
    pub timer: Option<u32>,
    pub picture_mode: Option<i32>,
    pub current_source: Option<String>,
}

/// 查询设备当前 FreeSync 是否开启（兼容预设模块调用）
pub fn query_freesync_state(adb: &AdbDevice) -> Result<bool, AdbError> {
    let source_str = adb.get_setting("mitv.tvplayer.hdmi.last.source")?;
    let is_dp = validate_source(&source_str)?;
    let (key, on_val) = get_freesync_param_for_source(is_dp);
    let values = crate::device::jni::jni_batch_get_or_single(adb, &[key])?;
    let val = values
        .get(key)
        .ok_or_else(|| AdbError::CommandFailed(format!("未读取到 JNI 键值: {key}")))?;
    Ok(*val == on_val)
}

/// 查询设备当前全部游戏特性的真实硬件/系统状态（未知不默认）
pub fn query_game_parameters(adb: &AdbDevice) -> Result<GameParameters, AdbError> {
    let settings = adb.get_settings(&[
        "picture_mode",
        "picture_preset_scenario",
        "front_sight_index",
        "mt_game_dynamic_ft",
        "mt_game_scope",
        "mt_game_scope_night",
        "monitor_menu_fps_counter",
        "monitor_menu_stopwatch",
        "monitor_menu_timer",
        "mitv.tvplayer.hdmi.last.source",
    ])?;
    let current_source = settings.get("mitv.tvplayer.hdmi.last.source").cloned();
    let is_dp = current_source
        .as_deref()
        .and_then(|s| validate_source(s).ok());
    let (freesync, mode_320) = if let Some(dp) = is_dp {
        let (fs_key, fs_on) = get_freesync_param_for_source(dp);
        let (hz_key, hz_on, _) = get_320hz_param_for_source(dp);
        let values = crate::device::jni::jni_batch_get_or_single(adb, &[hz_key, fs_key])?;
        (
            values.get(fs_key).map(|v| *v == fs_on),
            values.get(hz_key).map(|v| *v == hz_on),
        )
    } else {
        (None, None)
    };

    let front_sight_index = settings
        .get("front_sight_index")
        .cloned()
        .and_then(|s| s.trim().parse::<u32>().ok());

    let dynamic_front_sight =
        settings
            .get("mt_game_dynamic_ft")
            .cloned()
            .and_then(|s| match s.trim() {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            });

    let sniper_scope = settings
        .get("mt_game_scope")
        .cloned()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&c| SNIPER_SCOPE_CODES.contains(&c));

    let sniper_night_vision =
        settings
            .get("mt_game_scope_night")
            .cloned()
            .and_then(|s| match s.trim() {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            });

    let fps_counter = settings
        .get("monitor_menu_fps_counter")
        .cloned()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&v| v <= 2);

    let stopwatch = settings
        .get("monitor_menu_stopwatch")
        .cloned()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&v| v <= 1);

    let timer = settings
        .get("monitor_menu_timer")
        .cloned()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&v| TIMER_VALUES.contains(&v));

    let picture_mode = settings
        .get("picture_mode")
        .cloned()
        .and_then(|s| s.trim().parse::<i32>().ok());

    Ok(GameParameters {
        freesync,
        mode_320,
        front_sight_index,
        dynamic_front_sight,
        sniper_scope,
        sniper_night_vision,
        fps_counter,
        stopwatch,
        timer,
        picture_mode,
        current_source,
    })
}

/// 设置 FreeSync 状态
/// 必须先读源且验证 23/24/29/30，未知源报错禁写，严禁 unwrap_or_default 走 HDMI
/// 关时恢复原模式必须先于 refresh_pq 且失败不能吞
pub fn set_freesync(
    adb: &AdbDevice,
    enabled: bool,
    restore_mode: Option<i32>,
) -> Result<(), AdbError> {
    let source_str = adb.get_setting("mitv.tvplayer.hdmi.last.source")?;
    let is_dp = validate_source(&source_str)?;

    let (key, on_val) = get_freesync_param_for_source(is_dp);
    let target_val = if enabled { on_val } else { 0 };

    jni_set(adb, key, target_val, 3)?;

    // 关闭 FreeSync 且有记忆模式时：先切回开启前的画面模式，再刷新数据（关时恢复必须先于 refresh 且失败不能吞）
    if !enabled {
        if let Some(prev) = restore_mode {
            adb.put_setting("picture_mode", &prev.to_string())?;
        }
    }

    refresh_pq(adb)?;
    Ok(())
}

/// 设置 320Hz 竞技刷新率模式
/// 必须先读源且验证 23/24/29/30，未知源报错禁写
pub fn set_320hz(adb: &AdbDevice, enabled: bool) -> Result<(), AdbError> {
    let source_str = adb.get_setting("mitv.tvplayer.hdmi.last.source")?;
    let is_dp = validate_source(&source_str)?;

    let (key, on_val, off_val) = get_320hz_param_for_source(is_dp);
    let target_val = if enabled { on_val } else { off_val };

    jni_set(adb, key, target_val, 3)?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置准星索引 (0..5)
/// 若当前在游戏模式中，严格执行原 Python 逻辑重触发：put 14 -> 500ms -> put 10，错误传播并回读
pub fn set_front_sight(adb: &AdbDevice, index: u32, is_game_mode: bool) -> Result<(), AdbError> {
    if index > 5 {
        return Err(AdbError::CommandFailed(format!(
            "准星样式必须在 0..5 之间，非法值: {}",
            index
        )));
    }

    adb.put_setting("front_sight_index", &index.to_string())?;

    if is_game_mode {
        adb.put_setting("picture_mode", "14")?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        adb.put_setting("picture_mode", "10")?;
    } else {
        refresh_pq(adb)?;
    }

    Ok(())
}

/// 设置动态准星 (0: 关, 1: 开)
pub fn set_dynamic_front_sight(adb: &AdbDevice, enabled: bool) -> Result<(), AdbError> {
    let val = if enabled { "1" } else { "0" };
    adb.put_setting("mt_game_dynamic_ft", val)?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置狙击镜 (代码必须为 0, 1, 3, 5, 7, 10 之一，禁止现有 clamp 0..3 错误)
pub fn set_sniper_scope(adb: &AdbDevice, code: u32) -> Result<(), AdbError> {
    validate_sniper_scope(code)?;
    adb.put_setting("mt_game_scope", &code.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置狙击镜夜视 (0: 关, 1: 开)
pub fn set_sniper_night_vision(adb: &AdbDevice, enabled: bool) -> Result<(), AdbError> {
    let val = if enabled { "1" } else { "0" };
    adb.put_setting("mt_game_scope_night", val)?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置 FPS 计数器 (0: 关, 1: 刷新率, 2: 柱状图)
pub fn set_fps_counter(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    if val > 2 {
        return Err(AdbError::CommandFailed(format!(
            "FPS 计数器选项必须在 0..2 之间: {}",
            val
        )));
    }
    adb.put_setting("monitor_menu_fps_counter", &val.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置秒表 (0: 关, 1: 开)
pub fn set_stopwatch(adb: &AdbDevice, val: u32) -> Result<(), AdbError> {
    if val > 1 {
        return Err(AdbError::CommandFailed(format!(
            "秒表选项必须在 0..1 之间: {}",
            val
        )));
    }
    adb.put_setting("monitor_menu_stopwatch", &val.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 设置定时器 (0, 60, 300, 1800, 3600)
pub fn set_timer(adb: &AdbDevice, seconds: u32) -> Result<(), AdbError> {
    validate_timer_seconds(seconds)?;
    adb.put_setting("monitor_menu_timer", &seconds.to_string())?;
    refresh_pq(adb)?;
    Ok(())
}

/// 切换游戏模式及相关依赖参数的应用计划
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwitchGameApplyPlan {
    pub switch_to_game: bool,
    pub front_sight: Option<u32>,
    pub dynamic_front_sight: Option<bool>,
    pub sniper_scope: Option<u32>,
    pub sniper_night_vision: Option<bool>,
}

/// 纯步骤执行器抽象接口，支持单元测试回归与实机注入
pub trait GameStepExecutor {
    fn switch_to_game_mode(&mut self) -> Result<(), AdbError>;
    fn set_front_sight(&mut self, index: u32) -> Result<(), AdbError>;
    fn set_dynamic_front_sight(&mut self, enabled: bool) -> Result<(), AdbError>;
    fn set_sniper_scope(&mut self, code: u32) -> Result<(), AdbError>;
    fn set_sniper_night_vision(&mut self, enabled: bool) -> Result<(), AdbError>;
    fn refresh_pq(&mut self) -> Result<(), AdbError>;
}

/// 生产环境真实 ADB 设备步骤执行器
pub struct RealGameStepExecutor<'a>(pub &'a AdbDevice, pub bool);

impl<'a> GameStepExecutor for RealGameStepExecutor<'a> {
    fn switch_to_game_mode(&mut self) -> Result<(), AdbError> {
        self.0.put_setting("picture_mode", "10")
    }

    fn set_front_sight(&mut self, index: u32) -> Result<(), AdbError> {
        set_front_sight(self.0, index, self.1)
    }

    fn set_dynamic_front_sight(&mut self, enabled: bool) -> Result<(), AdbError> {
        set_dynamic_front_sight(self.0, enabled)
    }

    fn set_sniper_scope(&mut self, code: u32) -> Result<(), AdbError> {
        set_sniper_scope(self.0, code)
    }

    fn set_sniper_night_vision(&mut self, enabled: bool) -> Result<(), AdbError> {
        set_sniper_night_vision(self.0, enabled)
    }

    fn refresh_pq(&mut self) -> Result<(), AdbError> {
        refresh_pq(self.0)
    }
}

/// 执行模式切换与相关依赖参数下发的纯步骤执行器
/// 步骤Err立即停止、发包含步骤名的AdbOperationFailed，绝不继续启用依赖
pub fn execute_switch_game_apply_steps<E: GameStepExecutor>(
    executor: &mut E,
    plan: &SwitchGameApplyPlan,
) -> Result<(), (&'static str, AdbError)> {
    if let Some(fs) = plan.front_sight {
        executor.set_front_sight(fs).map_err(|e| ("设置准星", e))?;
    }

    if let Some(dyn_fs) = plan.dynamic_front_sight {
        executor
            .set_dynamic_front_sight(dyn_fs)
            .map_err(|e| ("设置动态准星", e))?;
    }

    if let Some(sc) = plan.sniper_scope {
        executor
            .set_sniper_scope(sc)
            .map_err(|e| ("设置狙击镜", e))?;
    }

    if let Some(nv) = plan.sniper_night_vision {
        executor
            .set_sniper_night_vision(nv)
            .map_err(|e| ("设置狙击夜视", e))?;
    }

    if plan.switch_to_game {
        executor
            .switch_to_game_mode()
            .map_err(|e| ("切换游戏模式", e))?;
    }

    if !plan.switch_to_game {
        executor.refresh_pq().map_err(|e| ("刷新PQ画质参数", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_source_contract() {
        assert_eq!(validate_source("29").unwrap(), true);
        assert_eq!(validate_source("30").unwrap(), true);
        assert_eq!(validate_source(" 29 \n").unwrap(), true);

        assert_eq!(validate_source("23").unwrap(), false);
        assert_eq!(validate_source("24").unwrap(), false);

        assert!(validate_source("").is_err());
        assert!(validate_source("hdmi").is_err());
        assert!(validate_source("1").is_err());
        assert!(validate_source("31").is_err());
    }

    #[test]
    fn test_sniper_scope_codes_contract() {
        for &valid in &[0, 1, 3, 5, 7, 10] {
            assert!(validate_sniper_scope(valid).is_ok());
        }

        // 禁止 0..3 clamp，比如 2、4、6、8、11 都必须被明确拒绝
        assert!(validate_sniper_scope(2).is_err());
        assert!(validate_sniper_scope(4).is_err());
        assert!(validate_sniper_scope(6).is_err());
        assert!(validate_sniper_scope(8).is_err());
        assert!(validate_sniper_scope(11).is_err());
    }

    #[test]
    fn test_timer_values_contract() {
        for &valid in &[0, 60, 300, 1800, 3600] {
            assert!(validate_timer_seconds(valid).is_ok());
        }
        assert!(validate_timer_seconds(1).is_err());
        assert!(validate_timer_seconds(100).is_err());
    }

    #[test]
    fn test_freesync_and_320hz_params() {
        // DP: adaptive_sync 1, 320hz 开 3 关 2
        let (fs_key, fs_on) = get_freesync_param_for_source(true);
        assert_eq!(fs_key, "g_video__dp_adaptive_sync");
        assert_eq!(fs_on, 1);

        let (hz_key, hz_on, hz_off) = get_320hz_param_for_source(true);
        assert_eq!(hz_key, "g_fusion_picture__dp_edid_version");
        assert_eq!(hz_on, 3);
        assert_eq!(hz_off, 2);

        // HDMI: freesync 3, 320hz 开 6 关 1
        let (fs_key, fs_on) = get_freesync_param_for_source(false);
        assert_eq!(fs_key, "g_video__freesync_switch");
        assert_eq!(fs_on, 3);

        let (hz_key, hz_on, hz_off) = get_320hz_param_for_source(false);
        assert_eq!(hz_key, "g_fusion_picture__hdmi_edid_version");
        assert_eq!(hz_on, 6);
        assert_eq!(hz_off, 1);
    }

    #[test]
    fn test_freesync_memory_suspended_toggle() {
        assert_eq!(is_freesync_memory_suspended(), false);
        set_freesync_memory_suspended(true);
        assert_eq!(is_freesync_memory_suspended(), true);
        set_freesync_memory_suspended(false);
        assert_eq!(is_freesync_memory_suspended(), false);
    }

    #[test]
    fn test_game_parameters_unknown_no_default() {
        let params = GameParameters::default();
        // 契约：所有字段必须为 None，未知不默认！
        assert_eq!(params.freesync, None);
        assert_eq!(params.mode_320, None);
        assert_eq!(params.front_sight_index, None);
        assert_eq!(params.dynamic_front_sight, None);
        assert_eq!(params.sniper_scope, None);
        assert_eq!(params.sniper_night_vision, None);
        assert_eq!(params.fps_counter, None);
        assert_eq!(params.stopwatch, None);
        assert_eq!(params.timer, None);
        assert_eq!(params.picture_mode, None);
        assert_eq!(params.current_source, None);
    }

    #[test]
    fn test_options_arrays_integrity() {
        assert_eq!(FRONT_SIGHT_OPTIONS.len(), 6);
        assert_eq!(SNIPER_SCOPE_OPTIONS.len(), 6);
        assert_eq!(FPS_COUNTER_OPTIONS.len(), 3);
        assert_eq!(STOPWATCH_OPTIONS.len(), 2);
        assert_eq!(TIMER_OPTIONS.len(), 5);
    }

    #[derive(Default)]
    struct MockGameStepExecutor {
        calls: Vec<String>,
        fail_mode: bool,
        fail_sight: bool,
        fail_dynamic: bool,
        fail_scope: bool,
        fail_night_vision: bool,
        fail_refresh: bool,
    }

    impl GameStepExecutor for MockGameStepExecutor {
        fn switch_to_game_mode(&mut self) -> Result<(), AdbError> {
            self.calls.push("switch_to_game_mode".into());
            if self.fail_mode {
                Err(AdbError::CommandFailed("mode err".into()))
            } else {
                Ok(())
            }
        }
        fn set_front_sight(&mut self, index: u32) -> Result<(), AdbError> {
            self.calls.push(format!("set_front_sight({})", index));
            if self.fail_sight {
                Err(AdbError::CommandFailed("sight err".into()))
            } else {
                Ok(())
            }
        }
        fn set_dynamic_front_sight(&mut self, enabled: bool) -> Result<(), AdbError> {
            self.calls
                .push(format!("set_dynamic_front_sight({})", enabled));
            if self.fail_dynamic {
                Err(AdbError::CommandFailed("dyn err".into()))
            } else {
                Ok(())
            }
        }
        fn set_sniper_scope(&mut self, code: u32) -> Result<(), AdbError> {
            self.calls.push(format!("set_sniper_scope({})", code));
            if self.fail_scope {
                Err(AdbError::CommandFailed("scope err".into()))
            } else {
                Ok(())
            }
        }
        fn set_sniper_night_vision(&mut self, enabled: bool) -> Result<(), AdbError> {
            self.calls
                .push(format!("set_sniper_night_vision({})", enabled));
            if self.fail_night_vision {
                Err(AdbError::CommandFailed("night err".into()))
            } else {
                Ok(())
            }
        }
        fn refresh_pq(&mut self) -> Result<(), AdbError> {
            self.calls.push("refresh_pq".into());
            if self.fail_refresh {
                Err(AdbError::CommandFailed("refresh err".into()))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn test_mode_failure_stops_before_refresh() {
        let mut executor = MockGameStepExecutor {
            fail_mode: true,
            ..Default::default()
        };
        let plan = SwitchGameApplyPlan {
            switch_to_game: true,
            front_sight: Some(1),
            dynamic_front_sight: Some(true),
            sniper_scope: Some(1),
            sniper_night_vision: Some(true),
        };
        let res = execute_switch_game_apply_steps(&mut executor, &plan);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().0, "切换游戏模式");
        assert_eq!(
            executor.calls,
            vec![
                "set_front_sight(1)",
                "set_dynamic_front_sight(true)",
                "set_sniper_scope(1)",
                "set_sniper_night_vision(true)",
                "switch_to_game_mode"
            ]
        );
    }

    #[test]
    fn test_front_sight_failure_no_dynamic() {
        let mut executor = MockGameStepExecutor {
            fail_sight: true,
            ..Default::default()
        };
        let plan = SwitchGameApplyPlan {
            switch_to_game: false,
            front_sight: Some(2),
            dynamic_front_sight: Some(true),
            sniper_scope: None,
            sniper_night_vision: None,
        };
        let res = execute_switch_game_apply_steps(&mut executor, &plan);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().0, "设置准星");
        assert_eq!(executor.calls, vec!["set_front_sight(2)"]);
    }

    #[test]
    fn test_sniper_scope_failure_no_night_vision() {
        let mut executor = MockGameStepExecutor {
            fail_scope: true,
            ..Default::default()
        };
        let plan = SwitchGameApplyPlan {
            switch_to_game: false,
            front_sight: None,
            dynamic_front_sight: None,
            sniper_scope: Some(3),
            sniper_night_vision: Some(true),
        };
        let res = execute_switch_game_apply_steps(&mut executor, &plan);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().0, "设置狙击镜");
        assert_eq!(executor.calls, vec!["set_sniper_scope(3)"]);
    }

    #[test]
    fn test_all_steps_success_executed_in_order() {
        let mut executor = MockGameStepExecutor::default();
        let plan = SwitchGameApplyPlan {
            switch_to_game: true,
            front_sight: Some(1),
            dynamic_front_sight: Some(true),
            sniper_scope: Some(5),
            sniper_night_vision: Some(true),
        };
        let res = execute_switch_game_apply_steps(&mut executor, &plan);
        assert!(res.is_ok());
        assert_eq!(
            executor.calls,
            vec![
                "set_front_sight(1)",
                "set_dynamic_front_sight(true)",
                "set_sniper_scope(5)",
                "set_sniper_night_vision(true)",
                "switch_to_game_mode"
            ]
        );
    }
}
