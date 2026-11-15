//! UI-only settings catalog and remembered section selection. Never sends device commands.
use super::theme::ThemeColors;
use gpui::{div, prelude::*, px, rgb, App, Div, Global};
use std::collections::HashMap;

pub const PICTURE: &str = "画面设置";
pub const GAME: &str = "游戏模式";
pub const TOOLS: &str = "工具与设置";
pub const LIGHT: &str = "屏幕灯";
pub const TASKS: &str = "自动任务";
pub const PRESETS: &str = "预设模式";

pub struct Page {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub sections: &'static [(&'static str, &'static str)],
}
pub const PAGES: &[Page] = &[
    Page {
        id: "主页 & 连接",
        label: "设备连接",
        group: "显示器控制",
        sections: &[],
    },
    Page {
        id: PICTURE,
        label: PICTURE,
        group: "显示器控制",
        sections: &[
            ("mode", "模式与背光"),
            ("parameters", "画面参数"),
            ("color", "色彩"),
            ("motion", "动态与响应"),
        ],
    },
    Page {
        id: GAME,
        label: GAME,
        group: "显示器控制",
        sections: &[
            ("sync", "同步与刷新率"),
            ("aim", "瞄准辅助"),
            ("panel", "面板工具"),
        ],
    },
    Page {
        id: "信号源切换",
        label: "信号源",
        group: "显示器控制",
        sections: &[],
    },
    Page {
        id: LIGHT,
        label: LIGHT,
        group: "显示器控制",
        sections: &[
            ("mode", "模式与亮度"),
            ("temperature", "照明色温"),
            ("color", "纯色颜色"),
        ],
    },
    Page {
        id: "遥控器",
        label: "遥控器",
        group: "显示器控制",
        sections: &[],
    },
    Page {
        id: PRESETS,
        label: PRESETS,
        group: "场景与自动化",
        sections: &[("list", "预设列表"), ("create", "新建预设")],
    },
    Page {
        id: TASKS,
        label: TASKS,
        group: "场景与自动化",
        sections: &[
            ("list", "任务列表"),
            ("editor", "添加或编辑"),
            ("recovery", "恢复记录"),
        ],
    },
    Page {
        id: "托盘菜单",
        label: "托盘菜单",
        group: "应用",
        sections: &[],
    },
    Page {
        id: TOOLS,
        label: "工具与设置",
        group: "应用",
        sections: &[
            ("preferences", "应用偏好"),
            ("scan", "设备扫描"),
            ("keys", "快捷键"),
            ("device", "设备工具"),
            ("hdr", "HDR 记忆"),
            ("debug", "调试与日志"),
        ],
    },
];

#[derive(Clone, Copy, Debug)]
pub struct Setting {
    pub id: &'static str,
    pub label: &'static str,
    pub page: &'static str,
    pub section: &'static str,
    pub aliases: &'static str,
}
macro_rules! setting {
    ($id:literal, $label:literal, $page:expr, $section:literal, $aliases:literal) => {
        Setting {
            id: $id,
            label: $label,
            page: $page,
            section: $section,
            aliases: $aliases,
        }
    };
}
pub const SETTINGS: &[Setting] = &[
    setting!(
        "connection",
        "显示器 IP 与局域网连接",
        "主页 & 连接",
        "",
        "adb connect ip 地址 无线 扫描 配对"
    ),
    setting!(
        "picture-mode",
        "画面模式",
        PICTURE,
        "mode",
        "picture mode 标准 游戏 电影 场景"
    ),
    setting!(
        "sensor",
        "自动光感",
        PICTURE,
        "mode",
        "light sensor 环境光 自适应"
    ),
    setting!(
        "dimming",
        "精密控光",
        PICTURE,
        "mode",
        "local dimming miniled 分区 背光"
    ),
    setting!(
        "backlight",
        "背光",
        PICTURE,
        "mode",
        "backlight brightness 亮度"
    ),
    setting!(
        "black-level",
        "黑色级别",
        PICTURE,
        "parameters",
        "black level 黑位"
    ),
    setting!("contrast", "对比度", PICTURE, "parameters", "contrast"),
    setting!("saturation", "饱和度", PICTURE, "parameters", "saturation"),
    setting!("hue", "色调", PICTURE, "parameters", "hue"),
    setting!(
        "sharpness",
        "锐度",
        PICTURE,
        "parameters",
        "sharpness 清晰度"
    ),
    setting!(
        "color_temp",
        "色温",
        PICTURE,
        "color",
        "color temperature color_temp 冷 暖 原色 自定义"
    ),
    setting!(
        "color_space",
        "色域",
        PICTURE,
        "color",
        "color space srgb dci-p3 adobe rgb bt.2020 bt.709"
    ),
    setting!(
        "red-gain",
        "红色增益",
        PICTURE,
        "color",
        "rgb red gain r 红色增益 色温"
    ),
    setting!(
        "green-gain",
        "绿色增益",
        PICTURE,
        "color",
        "rgb green gain g 绿色增益 色温"
    ),
    setting!(
        "blue-gain",
        "蓝色增益",
        PICTURE,
        "color",
        "rgb blue gain b 蓝色增益 色温"
    ),
    setting!(
        "hdr_tone_mapping",
        "HDR 色调映射",
        PICTURE,
        "motion",
        "hdr tone mapping hgig 层次 动态 明亮"
    ),
    setting!(
        "dynamic_definition",
        "动态清晰度",
        PICTURE,
        "motion",
        "dynamic definition 清晰度"
    ),
    setting!(
        "response_time",
        "响应时间",
        PICTURE,
        "motion",
        "response time overdrive 响应速度 拖影"
    ),
    setting!(
        "reset-picture",
        "恢复当前模式默认值",
        PICTURE,
        "motion",
        "reset 恢复 默认 重置"
    ),
    setting!(
        "freesync",
        "FreeSync Premium Pro",
        GAME,
        "sync",
        "freesync vrr 同步 防撕裂"
    ),
    setting!(
        "freesync-memory",
        "FreeSync 模式记忆",
        GAME,
        "sync",
        "freesync memory 记忆 恢复"
    ),
    setting!(
        "320hz",
        "320Hz 高刷模式",
        GAME,
        "sync",
        "320 hz 刷新率 高刷"
    ),
    setting!(
        "crosshair",
        "准星样式",
        GAME,
        "aim",
        "crosshair front sight 瞄准"
    ),
    setting!(
        "crosshair-memory",
        "准星游戏模式联动",
        GAME,
        "aim",
        "crosshair game mode 记忆 仅游戏显示"
    ),
    setting!(
        "dynamic-crosshair",
        "动态准星",
        GAME,
        "aim",
        "dynamic front sight 动态准星"
    ),
    setting!("sniper", "狙击镜放大倍率", GAME, "aim", "sniper scope 放大"),
    setting!(
        "night-vision",
        "狙击镜夜视",
        GAME,
        "aim",
        "sniper night vision 夜视"
    ),
    setting!("fps", "FPS 计数器", GAME, "panel", "fps counter 帧率"),
    setting!("stopwatch", "秒表", GAME, "panel", "stopwatch"),
    setting!("timer", "定时器", GAME, "panel", "timer 计时器"),
    setting!(
        "source",
        "视频输入端口",
        "信号源切换",
        "",
        "input source hdmi dp type-c usb-c 信号源 切换"
    ),
    setting!(
        "light-mode",
        "炫彩灯模式",
        LIGHT,
        "mode",
        "screen light mode 屏幕灯 照明 呼吸 流光 纯色"
    ),
    setting!(
        "light-brightness",
        "屏幕灯亮度挡位",
        LIGHT,
        "mode",
        "screen light brightness illumination 亮度 档位"
    ),
    setting!(
        "light-temperature",
        "照明色温",
        LIGHT,
        "temperature",
        "screen light temperature 屏幕灯 冷 暖"
    ),
    setting!(
        "light-color",
        "纯色颜色",
        LIGHT,
        "color",
        "screen light color 屏幕灯 颜色"
    ),
    setting!(
        "remote",
        "遥控器按键",
        "遥控器",
        "",
        "remote 电源 音量 方向 主页 菜单 返回"
    ),
    setting!(
        "presets",
        "预设切换与恢复基线",
        PRESETS,
        "list",
        "preset 场景 快照 恢复 基线"
    ),
    setting!(
        "create-preset",
        "新建预设",
        PRESETS,
        "create",
        "preset capture 抓取 创建 名称"
    ),
    setting!(
        "tasks",
        "自动任务列表",
        TASKS,
        "list",
        "schedule task 自动化 定时"
    ),
    setting!(
        "task-editor",
        "添加或编辑时间段",
        TASKS,
        "editor",
        "schedule start end 开始 结束 预设 时间"
    ),
    setting!(
        "task-recovery",
        "自动任务恢复记录",
        TASKS,
        "recovery",
        "recovery restore 还原 基线 失败"
    ),
    setting!(
        "tray",
        "托盘快捷控制",
        "托盘菜单",
        "",
        "tray 菜单 添加 上移 下移 调序 快捷"
    ),
    setting!(
        "close",
        "关闭窗口行为",
        TOOLS,
        "preferences",
        "close 关闭 最小化 退出 托盘"
    ),
    setting!(
        "autostart",
        "开机启动",
        TOOLS,
        "preferences",
        "autostart startup 登录 启动"
    ),
    setting!(
        "theme",
        "界面主题",
        TOOLS,
        "preferences",
        "theme 深色 浅色 跟随系统 外观"
    ),
    setting!(
        "scan-force",
        "强制扫描 IP",
        TOOLS,
        "scan",
        "scan force ip 设备扫描 范围"
    ),
    setting!(
        "scan-block",
        "排除 IP",
        TOOLS,
        "scan",
        "scan block ignore ip 忽略 设备扫描"
    ),
    setting!(
        "scan-subnet",
        "额外 CIDR 网段",
        TOOLS,
        "scan",
        "scan subnet cidr 网段 设备扫描"
    ),
    setting!(
        "hotkeys",
        "全局快捷键",
        TOOLS,
        "keys",
        "hotkey shortcut 组合键 绑定 增大 减小 步长"
    ),
    setting!(
        "countdown",
        "倒计时提交与等待时间",
        TOOLS,
        "keys",
        "countdown delay 松手 等待 秒 提交"
    ),
    setting!(
        "desktop",
        "显示器桌面模式",
        TOOLS,
        "device",
        "desktop launcher 办公 会议 投屏"
    ),
    setting!(
        "guardian",
        "ADB 保活守护",
        TOOLS,
        "device",
        "adb guardian 保活 待机 唤醒 5555"
    ),
    setting!(
        "deploy",
        "部署 AdbGuardian",
        TOOLS,
        "device",
        "adb guardian 部署 安装 保活"
    ),
    setting!(
        "resolution",
        "显示器 UI 分辨率",
        TOOLS,
        "device",
        "4k 1080p resolution 分辨率"
    ),
    setting!(
        "reboot",
        "重启显示器",
        TOOLS,
        "device",
        "reboot restart 重启"
    ),
    setting!(
        "hdr-memory",
        "HDR / SDR 精密控光记忆",
        TOOLS,
        "hdr",
        "hdr sdr local dimming memory 自动恢复 目标 显示器"
    ),
    setting!(
        "shell",
        "ADB 终端与 Shell 命令",
        TOOLS,
        "debug",
        "adb shell terminal 命令行 控制台 终端"
    ),
    setting!(
        "apk",
        "APK 软件包安装",
        TOOLS,
        "debug",
        "apk install 旁加载 软件包 安装"
    ),
    setting!(
        "logs",
        "运行日志",
        TOOLS,
        "debug",
        "log export 日志 导出 排错"
    ),
    setting!(
        "licenses",
        "第三方许可",
        TOOLS,
        "debug",
        "license 第三方 许可 开源"
    ),
];

#[derive(Default)]
pub struct SettingsNavigation {
    sections: HashMap<&'static str, &'static str>,
    pub target: Option<&'static Setting>,
    pub target_anchor: Option<gpui::ScrollAnchor>,
}
impl Global for SettingsNavigation {}

pub fn reveal(anchor: &gpui::ScrollAnchor, window: &mut gpui::Window, cx: &mut App) {
    anchor.scroll_to(window, cx);
    // The anchor records its position during prepaint. Repaint after the deferred scroll.
    window.on_next_frame(|window, _| window.refresh());
}

pub fn page(id: &str) -> &'static Page {
    PAGES.iter().find(|p| p.id == id).unwrap_or(&PAGES[0])
}
pub fn section(cx: &App, page_id: &str) -> &'static str {
    cx.try_global::<SettingsNavigation>()
        .and_then(|s| s.sections.get(page_id).copied())
        .unwrap_or_else(|| page(page_id).sections.first().map(|s| s.0).unwrap_or(""))
}
pub fn section_label(cx: &App, page_id: &str) -> &'static str {
    page(page_id)
        .sections
        .iter()
        .find(|s| s.0 == section(cx, page_id))
        .map(|s| s.1)
        .unwrap_or(page(page_id).label)
}
pub fn select(
    cx: &mut App,
    page_id: &'static str,
    section_id: &'static str,
    target: Option<&'static Setting>,
) {
    let nav = cx.default_global::<SettingsNavigation>();
    if page(page_id).sections.iter().any(|s| s.0 == section_id) {
        nav.sections.insert(page_id, section_id);
    }
    nav.target = target;
    nav.target_anchor = None;
    cx.refresh_windows();
}
pub fn is_target(cx: &App, id: &str) -> bool {
    cx.try_global::<SettingsNavigation>()
        .and_then(|s| s.target)
        .is_some_and(|s| s.id == id || id.starts_with(s.label))
}
pub fn emphasis(element: Div, cx: &App, id: &str) -> gpui::Stateful<Div> {
    element
        .id(gpui::SharedString::from(format!("setting-{id}")))
        .when(is_target(cx, id), |d| {
            d.p_2()
                .rounded_md()
                .bg(rgb(ThemeColors::ACCENT_BG_SUBTLE()))
                .border_l_2()
                .border_color(rgb(ThemeColors::ACCENT_CYAN()))
                .anchor_scroll(
                    cx.try_global::<SettingsNavigation>()
                        .and_then(|s| s.target_anchor.clone()),
                )
        })
}
pub fn emphasis_any(element: Div, cx: &App, ids: &[&str]) -> gpui::Stateful<Div> {
    if let Some(target) = cx
        .try_global::<SettingsNavigation>()
        .and_then(|s| s.target)
        .filter(|s| ids.contains(&s.id))
    {
        emphasis(element, cx, target.id)
    } else {
        element.id(gpui::SharedString::from(format!(
            "setting-group-{}",
            ids.first().unwrap_or(&"default")
        )))
    }
}
pub fn search(query: &str) -> Vec<&'static Setting> {
    let query = query.trim().to_lowercase();
    let words = query.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return Vec::new();
    }
    let mut matches = SETTINGS
        .iter()
        .filter_map(|s| {
            let p = page(s.page);
            let group = p
                .sections
                .iter()
                .find(|t| t.0 == s.section)
                .map(|t| t.1)
                .unwrap_or("");
            let text = format!("{} {} {} {} {}", s.label, s.aliases, s.page, group, p.group)
                .to_lowercase();
            if !words.iter().all(|word| text.contains(word)) {
                return None;
            }
            let label = s.label.to_lowercase();
            let score = if label == query {
                0
            } else if label.starts_with(&query) {
                1
            } else if label.contains(&query) {
                2
            } else {
                3
            };
            Some((score, s))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(score, _)| *score);
    matches.into_iter().map(|(_, s)| s).collect()
}

pub fn location_hint(cx: &App) -> Div {
    let target = cx.try_global::<SettingsNavigation>().and_then(|s| s.target);
    div().children(target.map(|s| {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(ThemeColors::ACCENT_BG_SUBTLE()))
            .border_l_2()
            .border_color(rgb(ThemeColors::ACCENT_CYAN()))
            .text_size(px(12.0))
            .text_color(rgb(ThemeColors::TEXT_PRIMARY()))
            .child(format!(
                "已定位：{} · {}",
                s.label,
                page(s.page)
                    .sections
                    .iter()
                    .find(|t| t.0 == s.section)
                    .map(|t| t.1)
                    .unwrap_or(page(s.page).label)
            ))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_routes_are_valid_and_ids_unique() {
        let mut ids = std::collections::HashSet::new();
        for s in SETTINGS {
            assert!(ids.insert(s.id), "duplicate {}", s.id);
            let p = PAGES.iter().find(|p| p.id == s.page).unwrap();
            assert!(
                p.sections.is_empty() && s.section.is_empty()
                    || p.sections.iter().any(|t| t.0 == s.section),
                "{}",
                s.id
            );
        }
    }
    #[test]
    fn search_handles_chinese_aliases_case_and_multiple_words() {
        for (query, id) in [
            ("控光", "dimming"),
            ("LOCAL dimming", "dimming"),
            ("游戏 320", "320hz"),
            ("画面 色温", "color_temp"),
            ("开机", "autostart"),
            ("FPS", "fps"),
            ("  ", ""),
            ("不存在的配置项xyz", ""),
        ] {
            let results = search(query);
            if id.is_empty() {
                assert!(results.is_empty());
            } else {
                assert!(results.iter().any(|s| s.id == id), "{query}");
            }
        }
        assert_eq!(search("色温")[0].id, "color_temp");
        assert_eq!(search("背光")[0].id, "backlight");
        assert!(!search("hdr").is_empty());
        assert!(search("色温 不存在xyz").is_empty());
    }
}
