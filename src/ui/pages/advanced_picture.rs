use crate::ui::theme::ThemeColors;
use crate::{
    device::picture::PictureParametersState,
    message::{actions::ToolAction, AppCommand, MessageBus},
    ui::components::{card_container, option_button, ContinuousSlider},
};
use gpui::prelude::*;
use gpui::{div, rgb, Context, Entity, Render, Window};
use std::sync::Arc;

pub struct AdvancedPicture {
    bus: Arc<MessageBus>,
    generation: u64,
    connected: bool,
    state: PictureParametersState,
    gains: Vec<Entity<ContinuousSlider>>,
    busy: bool,
    reset_confirm: bool,
}
impl AdvancedPicture {
    pub fn new(bus: Arc<MessageBus>, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        let gains = ["红色增益", "绿色增益", "蓝色增益"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                let weak = weak.clone();
                cx.new(|_| {
                    ContinuousSlider::new_param(
                        label,
                        None,
                        524,
                        1524,
                        1,
                        "",
                        true,
                        Some("请先读取".into()),
                        move |value, _, cx| {
                            if let Some(view) = weak.upgrade() {
                                view.update(cx, |this, cx| {
                                    let (Some(mut r), Some(mut g), Some(mut b)) = (
                                        this.state.red_gain,
                                        this.state.green_gain,
                                        this.state.blue_gain,
                                    ) else {
                                        return;
                                    };
                                    match index {
                                        0 => r = value,
                                        1 => g = value,
                                        _ => b = value,
                                    };
                                    this.send(
                                        ToolAction::Gains {
                                            red: r,
                                            green: g,
                                            blue: b,
                                        },
                                        cx,
                                    );
                                });
                            }
                        },
                    )
                })
            })
            .collect();
        Self {
            bus,
            generation: 0,
            connected: false,
            state: Default::default(),
            gains,
            busy: false,
            reset_confirm: false,
        }
    }
    pub fn set_connected(&mut self, connected: bool, generation: u64, cx: &mut Context<Self>) {
        if !connected || generation != self.generation {
            self.state = Default::default();
            self.busy = false;
            self.reset_confirm = false;
        }
        self.connected = connected;
        self.generation = generation;
        self.sync(cx);
    }
    pub fn update_state(&mut self, state: PictureParametersState, cx: &mut Context<Self>) {
        if !self.busy {
            self.state = state;
        }
        self.sync(cx);
    }
    pub fn completed(&mut self, cx: &mut Context<Self>) {
        self.busy = false;
        self.sync(cx);
    }
    pub fn state(&self) -> &PictureParametersState {
        &self.state
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        for (i, gain) in self.gains.iter().enumerate() {
            let v = [
                self.state.red_gain,
                self.state.green_gain,
                self.state.blue_gain,
            ][i];
            let disabled = !self.connected
                || self.busy
                || self.state.color_temp != Some(3)
                || [
                    self.state.red_gain,
                    self.state.green_gain,
                    self.state.blue_gain,
                ]
                .contains(&None);
            gain.update(cx, |s, cx| {
                s.set_value_opt(v, cx);
                s.set_disabled(
                    disabled,
                    if disabled {
                        Some("需读取完整 RGB 并选择自定义色温".into())
                    } else {
                        None
                    },
                    cx,
                );
            });
        }
        cx.notify();
    }
    fn send(&mut self, action: ToolAction, cx: &mut Context<Self>) {
        if !self.connected || self.busy {
            return;
        }
        self.busy = true;
        let _ = self.bus.cmd_sender.send(AppCommand::ToolAction {
            action,
            generation: self.generation,
        });
        self.sync(cx);
    }
}
impl Render for AdvancedPicture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = card_container().child(div().text_lg().child("高级画面设置"));
        let groups: [(&str, &str, Option<i32>, &[(i32, &str)]); 5] = [
            (
                "色温",
                "color_temp",
                self.state.color_temp,
                &[
                    (0, "冷色"),
                    (1, "标准"),
                    (2, "暖色"),
                    (8, "原色"),
                    (3, "自定义"),
                ],
            ),
            (
                "色域",
                "color_space",
                self.state.color_space.map(|v| v as i32),
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
                "HDR 色调映射",
                "hdr_tone_mapping",
                self.state.hdr_tone_mapping.map(|v| v as i32),
                &[(0, "HGiG"), (1, "层次"), (2, "动态"), (3, "明亮")],
            ),
            (
                "动态清晰度",
                "dynamic_definition",
                self.state.dynamic_definition.map(|v| v as i32),
                &[(0, "关"), (1, "低"), (2, "中"), (3, "高")],
            ),
            (
                "响应时间",
                "response_time",
                self.state.response_time.map(|v| v as i32),
                &[(1, "普通"), (2, "快速"), (3, "高速")],
            ),
        ];
        for (label, key, current, options) in groups {
            let disabled = !self.connected
                || self.busy
                || current.is_none()
                || (key == "hdr_tone_mapping"
                    && !self
                        .state
                        .mode
                        .is_some_and(crate::device::picture::is_hdr_tone_mapping_picture_mode));
            let mut row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(div().w(gpui::px(110.0)).text_sm().child(label));
            for &(value, name) in options {
                let key = key.to_string();
                row = row.child(
                    option_button(name, current == Some(value), disabled)
                        .id(gpui::SharedString::from(format!("advanced-{key}-{value}")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !disabled {
                                this.send(
                                    ToolAction::Advanced {
                                        parameter: key.clone(),
                                        value,
                                    },
                                    cx,
                                );
                            }
                        })),
                );
            }
            if current.is_none() {
                row = row.child(div().text_xs().child("未读取"));
            }
            root = root.child(row);
        }
        root = root.children(self.gains.iter().cloned()).child(
            div()
                .text_xs()
                .child("自定义色温下调整 RGB，范围 524–1524；每次保留另外两个通道。"),
        );
        let confirm = self.reset_confirm;
        let disabled = !self.connected || self.busy;
        root.child(
            option_button(
                if confirm {
                    "确认恢复当前模式默认值"
                } else {
                    "恢复当前模式默认值"
                },
                confirm,
                disabled,
            )
            .id("reset-picture")
            .on_click(cx.listener(move |this, _, _, cx| {
                if disabled {
                    return;
                }
                if this.reset_confirm {
                    this.reset_confirm = false;
                    this.send(ToolAction::ResetPicture, cx);
                } else {
                    this.reset_confirm = true;
                    cx.notify();
                }
            })),
        )
        .when(confirm, |d| {
            d.child(
                div()
                    .text_sm()
                    .text_color(rgb(ThemeColors::STATUS_WARNING()))
                    .child("此操作将重置当前画面模式的参数，再次点击确认。"),
            )
        })
    }
}
