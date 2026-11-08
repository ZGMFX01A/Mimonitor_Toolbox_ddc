use std::sync::atomic::{AtomicBool, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn set_preference(preference: &str) -> bool {
    let light =
        preference == "light" || (preference == "auto" && crate::config::system::system_is_light());
    LIGHT.swap(light, Ordering::Relaxed) != light
}

pub struct ThemeColors;

#[allow(non_snake_case)]
impl ThemeColors {
    fn pick(dark: u32, light: u32) -> u32 {
        if LIGHT.load(Ordering::Relaxed) {
            light
        } else {
            dark
        }
    }

    /// 根页面背景色：深色采用沉稳冷灰，浅色采用纯净微冷灰白
    pub fn BG_ROOT() -> u32 {
        Self::pick(0x14171d, 0xf8fafc)
    }

    /// 卡片背景色：深浅对比鲜明，层次分明
    pub fn BG_CARD() -> u32 {
        Self::pick(0x1f232b, 0xffffff)
    }

    /// 悬停背景色
    pub fn BG_HOVER() -> u32 {
        Self::pick(0x2b313c, 0xe2e8f0)
    }

    /// 次级/默认按钮背景色
    pub fn BG_BUTTON() -> u32 {
        Self::pick(0x262b35, 0xffffff)
    }

    /// 次级/默认按钮悬停背景色
    pub fn BG_BUTTON_HOVER() -> u32 {
        Self::pick(0x2f3642, 0xf1f5f9)
    }

    /// 禁用状态背景色
    pub fn BG_DISABLED() -> u32 {
        Self::pick(0x181b21, 0xf8fafc)
    }

    /// 进度条等滑轨背景色
    pub fn BG_TRACK() -> u32 {
        Self::pick(0x252a34, 0xe2e8f0)
    }

    /// 主边框线条颜色
    pub fn BORDER() -> u32 {
        Self::pick(0x2d3440, 0xe2e8f0)
    }

    /// 禁用状态边框色
    pub fn BORDER_DISABLED() -> u32 {
        Self::pick(0x222730, 0xedf2f7)
    }

    /// 现代克制蓝/青强调色（主操作/激活态）
    pub fn ACCENT_CYAN() -> u32 {
        Self::pick(0x0284c7, 0x0284c7)
    }

    /// 强调色悬停态
    pub fn ACCENT_HOVER() -> u32 {
        Self::pick(0x0369a1, 0x0369a1)
    }

    /// 强调色极淡背景（用于徽章、轻量高亮等）
    pub fn ACCENT_BG_SUBTLE() -> u32 {
        Self::pick(0x132637, 0xf0f7ff)
    }

    /// 一级主文本
    pub fn TEXT_PRIMARY() -> u32 {
        Self::pick(0xf8fafc, 0x0f172a)
    }

    /// 二级次要文本
    pub fn TEXT_SECONDARY() -> u32 {
        Self::pick(0x94a3b8, 0x475569)
    }

    /// 静音/辅助文本
    pub fn TEXT_MUTED() -> u32 {
        Self::pick(0x718096, 0x64748b)
    }

    /// 禁用文本颜色
    pub fn TEXT_DISABLED() -> u32 {
        Self::pick(0x59677b, 0x8b9cb0)
    }

    /// 强调色背景上的前景色
    pub fn TEXT_ON_ACCENT() -> u32 {
        Self::pick(0xffffff, 0xffffff)
    }

    /// 成功状态色
    pub fn STATUS_SUCCESS() -> u32 {
        Self::pick(0x10b981, 0x059669)
    }

    /// 警告状态色
    pub fn STATUS_WARNING() -> u32 {
        Self::pick(0xf59e0b, 0xd97706)
    }

    /// 错误状态色
    pub fn STATUS_ERROR() -> u32 {
        Self::pick(0xef4444, 0xdc2626)
    }

    /// 成功状态浅底背景色
    pub fn STATUS_SUCCESS_BG() -> u32 {
        Self::pick(0x162a22, 0xdcfce7)
    }

    /// 警告状态浅底背景色
    pub fn STATUS_WARNING_BG() -> u32 {
        Self::pick(0x2d2412, 0xfef3c7)
    }

    /// 错误状态浅底背景色
    pub fn STATUS_ERROR_BG() -> u32 {
        Self::pick(0x381e1e, 0xfee2e2)
    }
}
