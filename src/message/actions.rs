use crate::adb::AdbDevice;
use crate::device::{guardian as g, mode_4k as m, picture as p};

#[derive(Debug, Clone)]
pub enum ToolAction {
    Shell(String),
    InstallApk(String),
    ReadStatus,
    CheckGuardian,
    StartGuardian,
    DeployGuardian,
    Resolution4k,
    Resolution1080p,
    Reboot,
    Advanced { parameter: String, value: i32 },
    Gains { red: u32, green: u32, blue: u32 },
    ResetPicture,
}
pub fn run(
    dev: &AdbDevice,
    action: &ToolAction,
) -> Result<(String, Option<g::GuardianStatus>), String> {
    let mut guardian = None;
    let report = dev.transaction(|| match action {
        ToolAction::Shell(command)=>dev.shell(command).map_err(|e|e.to_string()),
        ToolAction::InstallApk(path)=>{
            let path=std::path::Path::new(path);
            if !path.is_file()||!path.extension().is_some_and(|ext|ext.eq_ignore_ascii_case("apk")){return Err("请选择现有的 APK 文件".into());}
            let path=path.to_string_lossy();let output=dev.run_device_command(&["install","-r",&path],std::time::Duration::from_secs(90)).map_err(|e|e.to_string())?;
            if !output.lines().any(|line|line.trim()=="Success"){return Err(format!("APK 安装未成功：{output}"));}Ok(format!("APK 安装成功：{}",path))
        }
        ToolAction::CheckGuardian => Ok("已检测守护状态".into()),
        ToolAction::StartGuardian => {
            g::start_guardian(dev).map_err(|e| e.to_string())?;
            Ok("已启动 ADB 保活守护".into())
        }
        ToolAction::ReadStatus=>{
            let r=m::query_ui_resolution(dev).map_err(|e|e.to_string())?;
            let g=g::check_guardian_status(dev).map_err(|e|e.to_string())?;
            guardian = Some(g.clone());
            Ok(format!("实际 UI：{} × {}，{} DPI\nAdbGuardian：{}\n安装：{}　进程：{}　无障碍：{}\n保护掩码：{}\nADB：{} / Wi-Fi ADB：{}\n服务端口：{} / 持久端口：{}\nadbd：{}　允许后台运行：{}\n{}",
                r.active_width,r.active_height,r.active_density,if g.ok{"已就绪"}else{"未就绪"},g.installed,g.running,g.accessibility,g.mask,g.adb_enabled,g.adb_wifi_enabled,g.service_port,g.persist_port,g.adbd,g.stopped_is_false,g.missing_reasons().join("、")))
        }
        ToolAction::DeployGuardian=>g::deploy_guardian(dev).map(|r|{guardian=Some(r.status.clone());format!("Guardian 安装：{}\n无障碍权限：{}\n守护状态：{}\n已完成 {} 个部署步骤\n{}",if r.install_success{"成功"}else{"未完成"},if r.accessibility_enabled{"已启用"}else{"未启用"},if r.ready{"已就绪"}else{"尚未就绪"},r.completed_stages.len(),r.status.missing_reasons().join("、"))}).map_err(|e|e.to_string()),
        ToolAction::Resolution4k=>{m::enable_4k_ui(dev).map_err(|e|e.to_string())?;m::reboot_device(dev).map_err(|e|e.to_string())?;Ok("已设置 4K UI 并发送重启命令".into())},
        ToolAction::Resolution1080p=>{m::set_1080p_ui(dev).map_err(|e|e.to_string())?;m::reboot_device(dev).map_err(|e|e.to_string())?;Ok("已设置 1080p UI 并发送重启命令".into())},
        ToolAction::Reboot=>m::reboot_device(dev).map(|_|"已发送设备重启命令，请稍后重新连接".into()).map_err(|e|e.to_string()),
        ToolAction::ResetPicture=>p::reset_current_mode(dev).map(|_|"已发送当前画面模式复位，请刷新参数确认".into()).map_err(|e|e.to_string()),
        ToolAction::Gains{red,green,blue}=>{
            if ![*red,*green,*blue].iter().all(|v|(524..=1524).contains(v)){return Err("色增益范围为 524..1524".into());}
            p::set_color_gains(dev,*red,*green,*blue).map_err(|e|e.to_string())?;Ok("已设置 RGB 色增益，请读取确认".into())
        }
        ToolAction::Advanced{parameter,value}=>{
            let result=match parameter.as_str(){
                "color_temp" if [0,1,2,8,3].contains(value)=>p::set_color_temp(dev,*value),
                "color_space" if [0,3,6,4,5,7].contains(value)=>p::set_color_space(dev,*value as u32),
                "hdr_tone_mapping" if (0..=3).contains(value)=>{
                    p::set_hdr_tone_mapping(dev,*value as u32)
                }
                "dynamic_definition" if (0..=3).contains(value)=>p::set_dynamic_definition(dev,*value as u32),
                "response_time" if (1..=3).contains(value)=>p::set_response_time(dev,*value as u32),
                _=>return Err("不支持的参数或参数超出范围".into()),
            };result.map_err(|e|e.to_string())?;Ok("指令已完成，正在重新读取参数".into())
        }
    })?;
    Ok((report, guardian))
}
