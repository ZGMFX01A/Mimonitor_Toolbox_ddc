use crate::adb::commands::{build_tvservice_app_process_command, build_tvservice_shell_command};
use crate::adb::{AdbDevice, AdbError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MTK_DIRECT_TOOL_JAR: &str = "MtkDirectTool.jar";
pub const REMOTE_CACHE_DIR: &str = "/data/data/mitv.service/cache";
pub const REMOTE_JAR_CACHE_PATH: &str = "/data/data/mitv.service/cache/MtkDirectTool.jar";
pub const MTK_BATCH_RESULT_FILE: &str = "/sdcard/Download/Mimonitor_Toolbox/.mtk_batch_result.txt";

/// 解析 MtkDirectTool batchGet 写入文件的键值对输出
pub fn parse_jni_batch_output(raw_output: &str) -> HashMap<String, i32> {
    let mut map = HashMap::new();
    for line in raw_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("__") || !trimmed.contains('=') {
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
        if parts.len() == 2 {
            let key = parts[0].trim().to_string();
            let val_str = parts[1].trim();
            if !val_str.starts_with("ERROR") {
                if let Ok(v) = val_str.parse::<i32>() {
                    map.insert(key, v);
                }
            }
        }
    }
    map
}

/// 定位本地分发的 MtkDirectTool.jar 文件路径
pub fn get_local_mtk_tool_path() -> Option<PathBuf> {
    // 1. 尝试当前 exe 相对路径
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent
                .join("assets")
                .join("runtime")
                .join(MTK_DIRECT_TOOL_JAR);
            if p.exists() {
                return Some(p);
            }
        }
    }
    // 2. 尝试工作区相对路径
    let p = Path::new("assets")
        .join("runtime")
        .join(MTK_DIRECT_TOOL_JAR);
    if p.exists() {
        return Some(p);
    }
    // 3. 尝试持久化 runtime 目录
    let persistent_p = crate::config::get_app_data_dir()
        .join("runtime")
        .join(MTK_DIRECT_TOOL_JAR);
    if persistent_p.exists() {
        return Some(persistent_p);
    }
    None
}

/// 检查并修复设备端远程 JAR（比对 stat -c %s 大小，缺失或不符则自动推送并部署到 cache）
pub fn check_and_heal_remote_jar(
    adb: &AdbDevice,
    filename: &str,
    local_path: Option<&Path>,
) -> Result<(), AdbError> {
    adb.transaction(|| -> Result<(), AdbError> {
        let sdcard_jar = format!("/sdcard/{}", filename);
        let cache_jar = format!("{}/{}", REMOTE_CACHE_DIR, filename);

        let local_size = local_path
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);

        // 检查 /sdcard/{filename} 远程文件大小 (使用 stat -c %s 杜绝遍历 ls 字段偏差)
        let check_cmd = format!("stat -c %s {} 2>/dev/null || echo 0", sdcard_jar);
        let sd_out = adb.shell(&check_cmd).unwrap_or_default();
        let sd_size: u64 = sd_out
            .lines()
            .last()
            .unwrap_or("0")
            .trim()
            .parse()
            .unwrap_or(0);

        if sd_size < 1000 || (local_size > 0 && sd_size != local_size) {
            crate::logger::log_adb(&format!(
                "{} 状态需修复 (本地 {} 字节, 远程 sdcard {} 字节)，开始自动推送...",
                filename, local_size, sd_size
            ));
            let local_path = local_path.ok_or_else(|| {
                AdbError::CommandFailed(format!("本地未找到 {filename}，无法推送到设备"))
            })?;
            adb.push_file(local_path, &sdcard_jar)?;
        }
        let cp_cmd = build_tvservice_shell_command(&["cp", &sdcard_jar, &cache_jar]);
        adb.shell(&cp_cmd)?;

        Ok(())
    })
}

/// 检查并修复设备端 MtkDirectTool.jar
pub fn check_and_heal_jar(adb: &AdbDevice) -> Result<(), AdbError> {
    let local_path = get_local_mtk_tool_path();
    check_and_heal_remote_jar(adb, MTK_DIRECT_TOOL_JAR, local_path.as_deref())
}

/// 发送画质广播以应用最新 PQ 参数 (精确对齐原 Python 命令)
pub fn refresh_pq(adb: &AdbDevice) -> Result<(), AdbError> {
    crate::logger::log_adb("refresh_pq: 发送 PIC_MODE_CHANGED 广播");
    adb.shell("am broadcast -a com.xiaomi.mitv.action.PIC_MODE_CHANGED --ei picmode 7")?;
    Ok(())
}

/// 调用 JNI 单键读取，严格按原 Python logcat GET 回执顺序。
pub fn jni_get(adb: &AdbDevice, key: &str) -> Result<i32, AdbError> {
    adb.transaction(|| -> Result<i32, AdbError> {
        adb.shell("logcat -c")?;
        let command = build_tvservice_app_process_command(
            REMOTE_JAR_CACHE_PATH,
            &["MtkDirectTool", "get", key],
        );
        let _ = adb.shell(&command)?;

        std::thread::sleep(Duration::from_millis(800));

        let log_cmd = format!("logcat -d | grep 'GET {}' | tail -1", key);
        let log_out = adb.shell(&log_cmd)?;

        if let Some(pos) = log_out.find("= ") {
            let val_part = log_out[pos + 2..].trim();
            if let Ok(v) = val_part.parse::<i32>() {
                return Ok(v);
            }
        }

        Err(AdbError::CommandFailed(format!(
            "未读取到 JNI 键值: {}",
            key
        )))
    })
}

/// 调用 JNI 单键设置 (upd 默认为 3，光感为 1)
pub fn jni_set(adb: &AdbDevice, key: &str, val: i32, upd: i32) -> Result<(), AdbError> {
    let val_str = val.to_string();
    let upd_str = upd.to_string();
    let cmd = build_tvservice_app_process_command(
        REMOTE_JAR_CACHE_PATH,
        &["MtkDirectTool", "set", key, &val_str, &upd_str],
    );
    adb.shell(&cmd)?;
    Ok(())
}

/// 调用 JNI 批量获取键值 (各 safe key 独立作为命令行参数，带清除旧文件与 3 秒轮询等待)
pub fn jni_batch_get(adb: &AdbDevice, keys: &[&str]) -> Result<HashMap<String, i32>, AdbError> {
    let safe_keys: Vec<&str> = keys
        .iter()
        .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .copied()
        .collect();

    if safe_keys.is_empty() {
        return Ok(HashMap::new());
    }

    adb.transaction(|| -> Result<HashMap<String, i32>, AdbError> {
        let mut app_args = vec!["MtkDirectTool", "batchGet"];
        app_args.extend_from_slice(&safe_keys);

        let batch_command = build_tvservice_app_process_command(REMOTE_JAR_CACHE_PATH, &app_args);

        // 清除旧结果文件 -> 执行 batch 命令 -> 轮询等待新文件产生 (最多 30 次 x 100ms = 3 秒) -> 读取内容
        let cmd = format!(
            "mkdir -p /sdcard/Download/Mimonitor_Toolbox; \
             rm -f {}; \
             {} >/dev/null; \
             i=0; while [ $i -lt 30 ] && [ ! -f {} ]; do sleep 0.1; i=$((i+1)); done; \
             cat {} 2>/dev/null",
            MTK_BATCH_RESULT_FILE, batch_command, MTK_BATCH_RESULT_FILE, MTK_BATCH_RESULT_FILE
        );

        let out = adb.shell(&cmd)?;
        let result = parse_jni_batch_output(&out);
        Ok(result)
    })
}

/// 批量获取并对未命中的键逐个单键 fallback 回退
pub fn jni_batch_get_or_single(
    adb: &AdbDevice,
    keys: &[&str],
) -> Result<HashMap<String, i32>, AdbError> {
    let mut vals = jni_batch_get(adb, keys).unwrap_or_else(|error| {
        crate::logger::log_adb(&format!("批量读取失败，逐项重试：{error}"));
        HashMap::new()
    });
    for &key in keys {
        if !vals.contains_key(key) {
            if let Ok(v) = jni_get(adb, key) {
                vals.insert(key.to_string(), v);
            }
        }
    }
    Ok(vals)
}

/// 成组提交 RGB 三色增益 (524 - 1524，调用专用 setColorGains 一次性提交底层 setWbGainOffsetEx)
pub fn jni_set_color_gains(adb: &AdbDevice, r: u32, g: u32, b: u32) -> Result<(), AdbError> {
    let r_str = r.to_string();
    let g_str = g.to_string();
    let b_str = b.to_string();
    let cmd = build_tvservice_app_process_command(
        REMOTE_JAR_CACHE_PATH,
        &["MtkDirectTool", "setColorGains", &r_str, &g_str, &b_str],
    );
    adb.shell(&cmd)?;
    Ok(())
}

/// 设置 HDR Tone Mapping (MTK 映射值与 update 标志，精确对齐 setHdrToneMapping val upd)
pub fn jni_set_hdr_tone_mapping(adb: &AdbDevice, mtk_val: i32, upd: i32) -> Result<(), AdbError> {
    let val_str = mtk_val.to_string();
    let upd_str = upd.to_string();
    let cmd = build_tvservice_app_process_command(
        REMOTE_JAR_CACHE_PATH,
        &["MtkDirectTool", "setHdrToneMapping", &val_str, &upd_str],
    );
    adb.shell(&cmd)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jni_batch_output_robustness() {
        let raw = "\
g_disp__disp_back_light=75\r\n\
g_video__clr_temp=1\n\
invalid_line_without_equal\n\
g_video__vid_local_dimming=ERROR:InvocationTargetException\n\
__elapsed_ms=18\n\
g_video__clr_gain_r=1024\n\
";
        let parsed = parse_jni_batch_output(raw);
        assert_eq!(parsed.get("g_disp__disp_back_light"), Some(&75));
        assert_eq!(parsed.get("g_video__clr_temp"), Some(&1));
        assert_eq!(parsed.get("g_video__clr_gain_r"), Some(&1024));
        assert!(!parsed.contains_key("invalid_line_without_equal"));
        assert!(!parsed.contains_key("g_video__vid_local_dimming"));
        assert!(!parsed.contains_key("__elapsed_ms"));
    }

    #[test]
    fn test_command_construction_aligns_with_protocol() {
        let batch_cmd = build_tvservice_app_process_command(
            REMOTE_JAR_CACHE_PATH,
            &["MtkDirectTool", "batchGet", "k1", "k2"],
        );
        // 关键断言：无 package，类名为 MtkDirectTool，且各 key 必须为独立 argv
        assert!(batch_cmd.contains("\\${IFS}MtkDirectTool\\${IFS}batchGet\\${IFS}k1\\${IFS}k2"));
        assert!(!batch_cmd.contains("com.example"));
        assert!(!batch_cmd.contains("k1,k2"));

        let rgb_cmd = build_tvservice_app_process_command(
            REMOTE_JAR_CACHE_PATH,
            &["MtkDirectTool", "setColorGains", "1024", "1024", "1024"],
        );
        assert!(rgb_cmd.contains(
            "\\${IFS}MtkDirectTool\\${IFS}setColorGains\\${IFS}1024\\${IFS}1024\\${IFS}1024"
        ));

        let hdr_cmd = build_tvservice_app_process_command(
            REMOTE_JAR_CACHE_PATH,
            &["MtkDirectTool", "setHdrToneMapping", "5", "3"],
        );
        assert!(
            hdr_cmd.contains("\\${IFS}MtkDirectTool\\${IFS}setHdrToneMapping\\${IFS}5\\${IFS}3")
        );
    }

    #[test]
    fn test_stat_size_parsing_from_shell_output() {
        let sample_output =
            "stat: cannot stat '/sdcard/ColorfulLedTool.jar': No such file or directory\n0\n";
        let lines: Vec<&str> = sample_output.lines().collect();
        let last_sz: u64 = lines.last().unwrap_or(&"0").trim().parse().unwrap_or(0);
        assert_eq!(last_sz, 0);

        let valid_output = "5958\r\n";
        let valid_lines: Vec<&str> = valid_output.lines().collect();
        let valid_sz: u64 = valid_lines
            .last()
            .unwrap_or(&"0")
            .trim()
            .parse()
            .unwrap_or(0);
        assert_eq!(valid_sz, 5958);
    }
}
