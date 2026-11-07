use crate::{
    hotkey::{AdjustDirection, AdjustParam, HotkeyEvent},
    message::{actions::ToolAction, AppCommand},
};
#[derive(Debug, Clone)]
pub enum Choice {
    Value(&'static str, i32),
    Adjust(AdjustParam, AdjustDirection),
}
pub struct Entry {
    pub id: &'static str,
    pub label: &'static str,
    pub choices: Vec<(String, Choice)>,
}
pub fn catalog() -> Vec<Entry> {
    let options: [(&str, &str, &[(i32, &str)]); 7] = [
        (
            "picture_mode",
            "画面模式",
            &[(14, "标准"), (10, "游戏"), (9, "电影")],
        ),
        (
            "local_dimming",
            "精密控光",
            &[(0, "关"), (1, "低"), (2, "中"), (3, "高")],
        ),
        (
            "color_space",
            "色域",
            &[
                (0, "自动"),
                (3, "sRGB"),
                (6, "DCI-P3"),
                (4, "Adobe RGB"),
                (5, "BT.2020"),
                (7, "BT.709"),
            ],
        ),
        (
            "color_temp",
            "色温",
            &[
                (0, "冷色"),
                (1, "标准"),
                (2, "暖色"),
                (8, "原色"),
                (3, "自定义"),
            ],
        ),
        (
            "response_time",
            "响应时间",
            &[(1, "普通"), (2, "快速"), (3, "高速")],
        ),
        ("freesync", "FreeSync", &[(0, "关"), (1, "开")]),
        (
            "input_source",
            "信号源",
            &[(23, "HDMI 1"), (24, "HDMI 2"), (29, "DP"), (30, "Type-C")],
        ),
    ];
    let mut entries = options
        .into_iter()
        .map(|(id, label, values)| Entry {
            id,
            label,
            choices: values
                .iter()
                .map(|(v, label)| (label.to_string(), Choice::Value(id, *v)))
                .collect(),
        })
        .collect::<Vec<_>>();
    for param in AdjustParam::ALL {
        entries.push(Entry {
            id: param.as_str(),
            label: param.label(),
            choices: vec![
                (
                    "减小".into(),
                    Choice::Adjust(param, AdjustDirection::Decrease),
                ),
                (
                    "增大".into(),
                    Choice::Adjust(param, AdjustDirection::Increase),
                ),
            ],
        });
    }
    entries
}
impl Choice {
    pub fn command(&self, generation: u64) -> AppCommand {
        match self {
            Self::Adjust(param, direction) => AppCommand::Hotkey {
                event: HotkeyEvent::Adjust {
                    param: *param,
                    direction: *direction,
                    step: 1,
                },
                generation,
            },
            Self::Value(key, value) => match *key {
                "picture_mode" => AppCommand::SetPictureMode {
                    mode: *value,
                    generation,
                    revision: 0,
                },
                "local_dimming" => AppCommand::SetLocalDimming {
                    value: *value as u32,
                    generation,
                    revision: 0,
                },
                "freesync" => AppCommand::SetGameFreeSync {
                    enabled: *value == 1,
                    restore_mode: None,
                    generation,
                },
                "input_source" => AppCommand::SwitchSource {
                    source: crate::device::source::VideoSource::from_id(*value as u32).unwrap(),
                    generation,
                },
                _ => AppCommand::ToolAction {
                    action: ToolAction::Advanced {
                        parameter: key.to_string(),
                        value: *value,
                    },
                    generation,
                },
            },
        }
    }
}
