use crate::adb::{AdbDevice, AdbError};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// AdbGuardian 包名
pub const GUARDIAN_PACKAGE: &str = "com.example.adbguardian";
/// AdbGuardian 主界面组件名
pub const GUARDIAN_MAIN_ACTIVITY: &str = "com.example.adbguardian/.MainActivity";
/// AdbGuardian 无障碍辅助功能服务名
pub const GUARDIAN_ACCESSIBILITY: &str =
    "com.example.adbguardian/com.example.adbguardian.AdbGuardianAccessibilityService";
/// 本地分发 APK 文件名
pub const GUARDIAN_APK_NAME: &str = "adbguardian-signed.apk";
/// 远端 SDCARD 暂存路径 (作为备用)
pub const REMOTE_SDCARD_APK_PATH: &str =
    "/sdcard/Download/Mimonitor_Toolbox/adbguardian-signed.apk";
/// 保活广播 Action
pub const GUARDIAN_ACTION_KEEP_ALIVE: &str = "com.example.adbguardian.ACTION_KEEP_ALIVE";
/// 期望的休眠保护掩码值 (persist.appcontrol_w_mask)
pub const EXPECTED_APPCONTROL_MASK: &str = "-134250497";
/// 期望的 ADB 监听端口
pub const EXPECTED_ADB_PORT: &str = "5555";

/// AdbGuardian 完整状态，对齐原 Python mimonitor_toolbox 10 项细粒度指标
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianStatus {
    /// 是否已安装该 APK (pm path com.example.adbguardian)
    pub installed: bool,
    /// 守护进程 PID (pidof com.example.adbguardian)
    pub pid: Option<String>,
    /// 是否正在运行 (pid 是否非空)
    pub running: bool,
    /// persist.appcontrol_w_mask 属性值 (期望 "-134250497")
    pub mask: String,
    /// global adb_enabled (期望 "1")
    pub adb_enabled: String,
    /// global adb_wifi_enabled (期望 "1")
    pub adb_wifi_enabled: String,
    /// service.adb.tcp.port (期望 "5555")
    pub service_port: String,
    /// persist.adb.tcp.port (期望 "5555")
    pub persist_port: String,
    /// init.svc.adbd 状态 (期望 "running")
    pub adbd: String,
    /// 辅助功能服务是否已启用 (enabled_accessibility_services 是否包含)
    pub accessibility: bool,
    /// 应用是否未处于强制停止状态 (dumpsys package 中 User 0: 是否包含 stopped=false)
    pub stopped_is_false: bool,
    /// 全部 10 项指标是否均满足就绪要求
    pub ok: bool,

    // 保持对旧接口字段的向后兼容
    pub is_installed: bool,
    pub is_accessibility_enabled: bool,
}

impl GuardianStatus {
    /// 检查是否所有指标均达到保活就绪状态
    pub fn is_ok(&self) -> bool {
        self.ok
    }

    /// 获取未就绪的具体原因列表（对齐 Python 界面展示逻辑）
    pub fn missing_reasons(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.installed {
            missing.push("未安装 AdbGuardian");
            return missing;
        }
        if !self.running || self.pid.is_none() {
            missing.push("进程未运行");
        }
        if !self.accessibility {
            missing.push("辅助功能未启用");
        }
        if self.mask != EXPECTED_APPCONTROL_MASK {
            missing.push("休眠保护未写入");
        }
        if self.adb_enabled != "1" || self.adb_wifi_enabled != "1" {
            missing.push("ADB 开关异常");
        }
        if self.service_port != EXPECTED_ADB_PORT || self.persist_port != EXPECTED_ADB_PORT {
            missing.push("端口异常");
        }
        if self.adbd != "running" {
            missing.push("adbd 未运行");
        }
        if !self.stopped_is_false {
            missing.push("应用处于停止状态");
        }
        missing
    }
}

/// 纯解析函数：将底层各个 shell 命令的原始输出解析为强类型 `GuardianStatus`
pub fn parse_guardian_status_raw(
    pm_path_out: &str,
    pid_out: &str,
    mask_out: &str,
    adb_enabled_out: &str,
    adb_wifi_enabled_out: &str,
    service_port_out: &str,
    persist_port_out: &str,
    adbd_out: &str,
    services_out: &str,
    user_state_out: &str,
) -> GuardianStatus {
    let pm_trimmed = pm_path_out.trim();
    let installed = !pm_trimmed.is_empty()
        && (pm_trimmed.starts_with("package:") || pm_trimmed.contains(GUARDIAN_PACKAGE));

    let pid_trimmed = pid_out.trim();
    let pid = if !pid_trimmed.is_empty() && pid_trimmed.chars().any(|c| c.is_ascii_digit()) {
        Some(pid_trimmed.to_string())
    } else {
        None
    };
    let running = pid.is_some();

    let mask = mask_out.trim().to_string();
    let adb_enabled = adb_enabled_out.trim().to_string();
    let adb_wifi_enabled = adb_wifi_enabled_out.trim().to_string();
    let service_port = service_port_out.trim().to_string();
    let persist_port = persist_port_out.trim().to_string();
    let adbd = adbd_out.trim().to_string();

    let services_trimmed = services_out.trim();
    let accessibility = services_trimmed
        .split(':')
        .any(|s| s.trim() == GUARDIAN_ACCESSIBILITY);

    let stopped_is_false = user_state_out.contains("stopped=false");

    let ok = installed
        && running
        && adb_enabled == "1"
        && adb_wifi_enabled == "1"
        && service_port == EXPECTED_ADB_PORT
        && persist_port == EXPECTED_ADB_PORT
        && adbd == "running"
        && accessibility
        && mask == EXPECTED_APPCONTROL_MASK
        && stopped_is_false;

    GuardianStatus {
        installed,
        pid,
        running,
        mask,
        adb_enabled,
        adb_wifi_enabled,
        service_port,
        persist_port,
        adbd,
        accessibility,
        stopped_is_false,
        ok,
        is_installed: installed,
        is_accessibility_enabled: accessibility,
    }
}

/// 计算追加后的无障碍辅助服务配置字符串，确保已有服务安全保留不被覆盖
pub fn compute_updated_accessibility_services(current_services: &str) -> String {
    let trimmed = current_services.trim();
    if trimmed.is_empty() || trimmed == "null" {
        GUARDIAN_ACCESSIBILITY.to_string()
    } else {
        let already_present = trimmed
            .split(':')
            .any(|s| s.trim() == GUARDIAN_ACCESSIBILITY);
        if already_present {
            trimmed.to_string()
        } else {
            format!("{}:{}", trimmed, GUARDIAN_ACCESSIBILITY)
        }
    }
}

/// 获取本地分发的 adbguardian-signed.apk 路径
pub fn get_local_guardian_apk_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent
                .join("assets")
                .join("adb_guardian")
                .join(GUARDIAN_APK_NAME);
            if p.exists() {
                return Some(p);
            }
        }
    }
    let p = Path::new("assets")
        .join("adb_guardian")
        .join(GUARDIAN_APK_NAME);
    if p.exists() {
        return Some(p);
    }
    None
}

/// 检测设备上 AdbGuardian 的完整运行与保活状态
/// 错误向上透明传播（若底层通信失败，保留具体的 AdbError，绝不伪装未安装或全就绪）
pub fn check_guardian_status(adb: &AdbDevice) -> Result<GuardianStatus, AdbError> {
    adb.transaction(|| {
        let services =
            adb.shell("settings get secure enabled_accessibility_services 2>/dev/null")?;
        let user_state = adb.shell(&format!(
            "dumpsys package {} 2>/dev/null | grep \"User 0:\" | head -n 1 || true",
            GUARDIAN_PACKAGE
        ))?;
        let pm_path = adb.shell(&format!("pm path {} 2>/dev/null || true", GUARDIAN_PACKAGE))?;
        let pid = adb.shell(&format!("pidof {} 2>/dev/null || true", GUARDIAN_PACKAGE))?;
        let mask = adb.shell("getprop persist.appcontrol_w_mask")?;
        let adb_enabled = adb.shell("settings get global adb_enabled")?;
        let adb_wifi_enabled = adb.shell("settings get global adb_wifi_enabled")?;
        let service_port = adb.shell("getprop service.adb.tcp.port")?;
        let persist_port = adb.shell("getprop persist.adb.tcp.port")?;
        let adbd = adb.shell("getprop init.svc.adbd")?;

        Ok(parse_guardian_status_raw(
            &pm_path,
            &pid,
            &mask,
            &adb_enabled,
            &adb_wifi_enabled,
            &service_port,
            &persist_port,
            &adbd,
            &services,
            &user_state,
        ))
    })
}

/// 部署流程各个明确阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployStage {
    /// 步骤1: 定位本地 assets/adb_guardian/adbguardian-signed.apk
    LocateApk,
    /// 步骤2: 本地执行 adb install -r -d 并验证输出包含 Success
    InstallApk,
    /// 步骤3: 授予 WRITE_SECURE_SETTINGS 权限
    GrantSecureSettings,
    /// 步骤4: 配置 deviceidle 电池优化白名单
    DeviceIdleWhitelist,
    /// 步骤5: 安全追加并启用无障碍辅助服务
    EnableAccessibility,
    /// 步骤6: 唤起主界面以完成自初始化 (am start)
    StartMainActivity,
    /// 步骤7: 发送保活广播 (am broadcast ACTION_KEEP_ALIVE)
    BroadcastKeepAlive,
    /// 步骤8: 延迟等待并可靠重连设备
    DelayAndReconnect,
    /// 步骤9: 复查保活守护全量状态
    VerifyStatus,
}

impl DeployStage {
    pub fn description(&self) -> &'static str {
        match self {
            DeployStage::LocateApk => "定位本地保活 APK",
            DeployStage::InstallApk => "本地执行 adb install -r -d",
            DeployStage::GrantSecureSettings => "授予 WRITE_SECURE_SETTINGS 权限",
            DeployStage::DeviceIdleWhitelist => "配置 deviceidle 白名单",
            DeployStage::EnableAccessibility => "安全追加并启用无障碍辅助服务",
            DeployStage::StartMainActivity => "启动守护主界面 (am start)",
            DeployStage::BroadcastKeepAlive => "发送保活广播 (am broadcast ACTION_KEEP_ALIVE)",
            DeployStage::DelayAndReconnect => "延迟重连设备",
            DeployStage::VerifyStatus => "复查守护全量状态",
        }
    }
}

/// 部署过程错误结构体，精准记录失败阶段及是否已完成 APK 安装
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianDeployError {
    /// 出现失败的部署阶段
    pub stage: DeployStage,
    /// 失败详细错误说明
    pub message: String,
    /// 在此失败发生前，APK 是否已经成功完成安装
    pub install_completed: bool,
}

impl std::fmt::Display for GuardianDeployError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "部署阶段 [{:?}] 失败: {} (APK 安装是否已成功: {})",
            self.stage, self.message, self.install_completed
        )
    }
}

impl std::error::Error for GuardianDeployError {}

/// 部署执行收据，明确区分安装成功与守护就绪
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianDeployReceipt {
    /// APK 安装是否成功 (通过 adb install Success 判定)
    pub install_success: bool,
    /// 辅助功能是否已在配置中开启
    pub accessibility_enabled: bool,
    /// 守护程序是否 10 项指标全部就绪
    pub ready: bool,
    /// 复查获取的完整守护状态
    pub status: GuardianStatus,
    /// 已成功完成的部署阶段列表
    pub completed_stages: Vec<DeployStage>,
}

/// 部署 AdbGuardian 并配置无障碍保活守护服务
/// 严格按阶段推进并产出 `GuardianDeployReceipt`；若任意阶段失败，返回携带具体阶段与安装状态的 `GuardianDeployError`
pub fn deploy_guardian(adb: &AdbDevice) -> Result<GuardianDeployReceipt, GuardianDeployError> {
    let mut completed_stages = Vec::new();

    // 阶段 1: 定位本地 APK
    let local_apk = match get_local_guardian_apk_path() {
        Some(p) => {
            completed_stages.push(DeployStage::LocateApk);
            p
        }
        None => {
            return Err(GuardianDeployError {
                stage: DeployStage::LocateApk,
                message: "未在 assets/adb_guardian 找到 adbguardian-signed.apk".to_string(),
                install_completed: false,
            });
        }
    };

    // 阶段 2: 本地执行 adb install -r -d 并验证 Success
    let apk_path_str = local_apk.to_string_lossy().to_string();
    crate::logger::log_device(&format!("正在执行本地 adb install -r -d: {}", apk_path_str));

    let install_out = adb
        .transaction(|| {
            adb.run_device_command(
                &["install", "-r", "-d", &apk_path_str],
                Duration::from_secs(90),
            )
        })
        .map_err(|e| GuardianDeployError {
            stage: DeployStage::InstallApk,
            message: format!("执行 adb install 失败: {}", e),
            install_completed: false,
        })?;

    if !install_out.contains("Success") {
        return Err(GuardianDeployError {
            stage: DeployStage::InstallApk,
            message: format!("adb install 未返回 Success: {}", install_out.trim()),
            install_completed: false,
        });
    }
    let install_completed = true;
    completed_stages.push(DeployStage::InstallApk);

    // 阶段 3: 授予 WRITE_SECURE_SETTINGS 权限
    crate::logger::log_device("正在授予 WRITE_SECURE_SETTINGS 权限...");
    let grant_out = adb
        .transaction(|| {
            adb.shell(&format!(
                "pm grant {} android.permission.WRITE_SECURE_SETTINGS 2>/dev/null || true",
                GUARDIAN_PACKAGE
            ))
        })
        .map_err(|e| GuardianDeployError {
            stage: DeployStage::GrantSecureSettings,
            message: format!("执行 pm grant 失败: {}", e),
            install_completed,
        })?;
    let lower_grant = grant_out.to_lowercase();
    if lower_grant.contains("security exception") || lower_grant.contains("error:") {
        return Err(GuardianDeployError {
            stage: DeployStage::GrantSecureSettings,
            message: format!("pm grant 输出异常: {}", grant_out.trim()),
            install_completed,
        });
    }
    completed_stages.push(DeployStage::GrantSecureSettings);

    // 阶段 4: 配置 deviceidle 电池白名单
    crate::logger::log_device("正在配置 deviceidle 白名单...");
    let whitelist_out = adb
        .transaction(|| {
            adb.shell(&format!(
                "cmd deviceidle whitelist +{} 2>/dev/null || true",
                GUARDIAN_PACKAGE
            ))
        })
        .map_err(|e| GuardianDeployError {
            stage: DeployStage::DeviceIdleWhitelist,
            message: format!("执行 deviceidle whitelist 失败: {}", e),
            install_completed,
        })?;
    let lower_wl = whitelist_out.to_lowercase();
    if lower_wl.contains("error:") || lower_wl.contains("exception") {
        return Err(GuardianDeployError {
            stage: DeployStage::DeviceIdleWhitelist,
            message: format!("deviceidle whitelist 输出异常: {}", whitelist_out.trim()),
            install_completed,
        });
    }
    completed_stages.push(DeployStage::DeviceIdleWhitelist);

    // 阶段 5: 安全追加并启用无障碍辅助服务 (不覆盖已有其他服务)
    crate::logger::log_device("正在安全追加并启用无障碍辅助服务...");
    adb.transaction(|| -> Result<(), AdbError> {
        let current =
            adb.shell("settings get secure enabled_accessibility_services 2>/dev/null")?;
        let new_services = compute_updated_accessibility_services(&current);
        adb.shell(&format!(
            "settings put secure enabled_accessibility_services '{}'",
            new_services
        ))?;
        adb.shell("settings put secure accessibility_enabled 1")?;
        Ok(())
    })
    .map_err(|e| GuardianDeployError {
        stage: DeployStage::EnableAccessibility,
        message: format!("配置无障碍服务失败: {}", e),
        install_completed,
    })?;
    completed_stages.push(DeployStage::EnableAccessibility);

    // 阶段 6: 唤起主界面以完成自初始化 (am start)
    crate::logger::log_device("正在启动 Guardian 主界面以初始化...");
    adb.transaction(|| {
        adb.shell(&format!(
            "am start -n {} >/dev/null",
            GUARDIAN_MAIN_ACTIVITY
        ))
    })
    .map_err(|e| GuardianDeployError {
        stage: DeployStage::StartMainActivity,
        message: format!("启动主界面失败: {}", e),
        install_completed,
    })?;
    completed_stages.push(DeployStage::StartMainActivity);

    // 阶段 7: 发送保活广播 (am broadcast ACTION_KEEP_ALIVE)
    crate::logger::log_device("正在发送 ACTION_KEEP_ALIVE 保活广播...");
    adb.transaction(|| {
        adb.shell(&format!(
            "am broadcast -a {} -p {} >/dev/null",
            GUARDIAN_ACTION_KEEP_ALIVE, GUARDIAN_PACKAGE
        ))
    })
    .map_err(|e| GuardianDeployError {
        stage: DeployStage::BroadcastKeepAlive,
        message: format!("发送保活广播失败: {}", e),
        install_completed,
    })?;
    completed_stages.push(DeployStage::BroadcastKeepAlive);

    // 阶段 8: 延迟等待并可靠重连设备
    crate::logger::log_device("等待守护自初始化并重新连接设备 (3秒延时)...");
    std::thread::sleep(Duration::from_secs(3));
    adb.reconnect().map_err(|e| GuardianDeployError {
        stage: DeployStage::DelayAndReconnect,
        message: format!("重连设备失败: {}", e),
        install_completed,
    })?;
    completed_stages.push(DeployStage::DelayAndReconnect);

    // 阶段 9: 复查守护全量状态
    crate::logger::log_device("复查 AdbGuardian 运行及保活状态...");
    let status = check_guardian_status(adb).map_err(|e| GuardianDeployError {
        stage: DeployStage::VerifyStatus,
        message: format!("复查守护状态失败: {}", e),
        install_completed,
    })?;
    completed_stages.push(DeployStage::VerifyStatus);

    let is_ready = status.ok;
    if is_ready {
        crate::logger::log_device("AdbGuardian 已成功安装并激活守护，全部 10 项指标就绪");
    } else {
        crate::logger::log_device(&format!(
            "AdbGuardian 已安装，但部分状态待就绪: {:?}",
            status.missing_reasons()
        ));
    }

    Ok(GuardianDeployReceipt {
        install_success: true,
        accessibility_enabled: status.accessibility,
        ready: is_ready,
        status,
        completed_stages,
    })
}

/// 安装 AdbGuardian 并配置无障碍守护服务 (兼容老签名的包装函数)
/// 返回 GuardianDeployReceipt，同时在失败时保留具体阶段信息
pub fn install_and_enable_guardian(adb: &AdbDevice) -> Result<GuardianDeployReceipt, AdbError> {
    deploy_guardian(adb).map_err(|e| AdbError::CommandFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_updated_accessibility_services() {
        // 空值或 null 处理
        assert_eq!(
            compute_updated_accessibility_services(""),
            GUARDIAN_ACCESSIBILITY
        );
        assert_eq!(
            compute_updated_accessibility_services("null"),
            GUARDIAN_ACCESSIBILITY
        );

        // 已有其他服务时安全追加，不覆盖
        let current = "com.google.android.marvin.talkback/.TalkBackService";
        let updated = compute_updated_accessibility_services(current);
        assert_eq!(updated, format!("{}:{}", current, GUARDIAN_ACCESSIBILITY));

        // 已存在时保持不变，避免重复追加
        let existing = format!("foo.bar/.Service:{}", GUARDIAN_ACCESSIBILITY);
        assert_eq!(compute_updated_accessibility_services(&existing), existing);
    }

    #[test]
    fn test_parse_guardian_status_all_ready() {
        let status = parse_guardian_status_raw(
            "package:/data/app/com.example.adbguardian-1/base.apk\n",
            "12345\n",
            "-134250497\n",
            "1\n",
            "1\n",
            "5555\n",
            "5555\n",
            "running\n",
            &format!(
                "com.android.talkback/.TalkBackService:{}",
                GUARDIAN_ACCESSIBILITY
            ),
            "User 0: ceDataInode=123 installed=true hidden=false stopped=false notLaunched=false",
        );

        assert!(status.installed);
        assert_eq!(status.pid.as_deref(), Some("12345"));
        assert!(status.running);
        assert_eq!(status.mask, "-134250497");
        assert_eq!(status.adb_enabled, "1");
        assert_eq!(status.adb_wifi_enabled, "1");
        assert_eq!(status.service_port, "5555");
        assert_eq!(status.persist_port, "5555");
        assert_eq!(status.adbd, "running");
        assert!(status.accessibility);
        assert!(status.stopped_is_false);
        assert!(status.ok);
        assert!(status.is_ok());
        assert!(status.is_installed);
        assert!(status.is_accessibility_enabled);
        assert!(status.missing_reasons().is_empty());
    }

    #[test]
    fn test_parse_guardian_status_partial_failures() {
        // 缺少 pid 且 mask 异常
        let status = parse_guardian_status_raw(
            "package:/data/app/com.example.adbguardian-1/base.apk",
            "",
            "0",
            "1",
            "1",
            "5555",
            "5555",
            "running",
            GUARDIAN_ACCESSIBILITY,
            "User 0: stopped=false",
        );

        assert!(status.installed);
        assert!(!status.running);
        assert!(!status.ok);
        let reasons = status.missing_reasons();
        assert!(reasons.contains(&"进程未运行"));
        assert!(reasons.contains(&"休眠保护未写入"));
    }

    #[test]
    fn test_parse_guardian_status_not_installed() {
        let status = parse_guardian_status_raw("", "", "", "", "", "", "", "", "", "");

        assert!(!status.installed);
        assert!(!status.ok);
        let reasons = status.missing_reasons();
        assert_eq!(reasons, vec!["未安装 AdbGuardian"]);
    }

    #[test]
    fn test_parse_guardian_status_stopped_true() {
        let status = parse_guardian_status_raw(
            "package:/data/app/com.example.adbguardian-1/base.apk",
            "12345",
            "-134250497",
            "1",
            "1",
            "5555",
            "5555",
            "running",
            GUARDIAN_ACCESSIBILITY,
            "User 0: ceDataInode=123 installed=true hidden=false stopped=true notLaunched=false",
        );

        assert!(status.installed);
        assert!(!status.stopped_is_false);
        assert!(!status.ok);
        assert!(status.missing_reasons().contains(&"应用处于停止状态"));
    }

    #[test]
    fn test_deploy_stage_plan() {
        let stages = vec![
            DeployStage::LocateApk,
            DeployStage::InstallApk,
            DeployStage::GrantSecureSettings,
            DeployStage::DeviceIdleWhitelist,
            DeployStage::EnableAccessibility,
            DeployStage::StartMainActivity,
            DeployStage::BroadcastKeepAlive,
            DeployStage::DelayAndReconnect,
            DeployStage::VerifyStatus,
        ];
        assert_eq!(stages.len(), 9);
        for stage in stages {
            assert!(!stage.description().is_empty());
        }
    }
}

/// Same accessibility merge and start sequence as Python _start_guardian.
pub fn start_guardian(adb: &AdbDevice) -> Result<(), AdbError> {
    adb.transaction(|| {
        let current =
            adb.shell("settings get secure enabled_accessibility_services 2>/dev/null")?;
        let services = compute_updated_accessibility_services(&current);
        adb.shell(&format!(
            "settings put secure enabled_accessibility_services '{}'",
            services
        ))?;
        adb.shell("settings put secure accessibility_enabled 1")?;
        adb.shell(&format!(
            "am start -n {} >/dev/null",
            GUARDIAN_MAIN_ACTIVITY
        ))?;
        adb.shell(&format!(
            "am broadcast -a {} -p {} >/dev/null",
            GUARDIAN_ACTION_KEEP_ALIVE, GUARDIAN_PACKAGE
        ))?;
        std::thread::sleep(Duration::from_secs(2));
        Ok(())
    })
}
