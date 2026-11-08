use crate::{
    config::AppConfig,
    hotkey::{AdjustHotkeyConfig, AdjustParam, CycleAction, HotkeyBindingConfig},
    message::MessageBus,
    ui::components::{option_button, TextInput},
    ui::theme::ThemeColors,
};
use gpui::{div, prelude::*, Context, Entity, FontWeight, Render, SharedString, Window};
use std::{collections::HashMap, sync::Arc};

pub struct AdjustRow {
    pub id: usize,
    pub param: String,
    pub direction: String,
    pub key: Entity<TextInput>,
    pub step: Entity<TextInput>,
    pub param_menu_open: bool,
}

pub struct HotkeyForm {
    bus: Arc<MessageBus>,
    cycles: Vec<Entity<TextInput>>,
    adjusts: Vec<AdjustRow>,
    next_row_id: usize,
    expanded: bool,
    error: String,
}

fn binding(modifier: &str, key: &str) -> String {
    if key.is_empty() || key == "无" {
        String::new()
    } else if modifier.is_empty() || modifier == "无" {
        key.into()
    } else {
        format!("{modifier}+{key}")
    }
}

fn split_binding(text: &str) -> HotkeyBindingConfig {
    let text = text.trim();
    if let Some((modifier, key)) = text.rsplit_once('+') {
        HotkeyBindingConfig {
            modifier: modifier.trim().into(),
            key: key.trim().into(),
        }
    } else {
        HotkeyBindingConfig {
            modifier: String::new(),
            key: text.into(),
        }
    }
}

fn has_binding_errors(plan: &[crate::hotkey::PlannedRegistrationItem]) -> bool {
    plan.iter().any(|item| item.pre_error.as_ref().is_some_and(|status|
        !matches!(status, crate::hotkey::RegistrationStatus::SkippedUnbound)))
}

impl HotkeyForm {
    pub fn new(bus: Arc<MessageBus>, config: &AppConfig, cx: &mut Context<Self>) -> Self {
        let cycles = CycleAction::ALL
            .into_iter()
            .map(|action| {
                let cfg = config
                    .extra
                    .get("hotkeys")
                    .and_then(|v| v.get(action.as_str()))
                    .and_then(|v| serde_json::from_value::<HotkeyBindingConfig>(v.clone()).ok())
                    .unwrap_or_default();
                cx.new(|cx| {
                    TextInput::new(
                        cx,
                        "未绑定，例如 Ctrl+Alt+F6",
                        binding(&cfg.modifier, &cfg.key),
                    )
                })
            })
            .collect();

        let saved = config
            .extra
            .get("adjust_hotkeys")
            .and_then(|v| serde_json::from_value::<Vec<AdjustHotkeyConfig>>(v.clone()).ok())
            .unwrap_or_default();

        let mut next_row_id = 0;
        let adjusts = saved
            .into_iter()
            .map(|cfg| {
                let id = next_row_id;
                next_row_id += 1;
                let key = cx.new(|cx| {
                    TextInput::new(
                        cx,
                        "未绑定，例如 Ctrl+Alt+Up",
                        binding(&cfg.modifier, &cfg.key),
                    )
                });
                let step = cx.new(|cx| TextInput::new(cx, "步长", cfg.step.unwrap_or(1).to_string()));
                AdjustRow {
                    id,
                    param: cfg.param,
                    direction: cfg.direction,
                    key,
                    step,
                    param_menu_open: false,
                }
            })
            .collect();

        Self {
            bus,
            cycles,
            adjusts,
            next_row_id,
            expanded: false,
            error: String::new(),
        }
    }
}

impl Render for HotkeyForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .flex()
            .flex_col()
            .gap_3()
            .w_full()
            .min_w_0()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::BOLD)
                    .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                    .child("全局快捷键"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(ThemeColors::TEXT_SECONDARY()))
                    .child("输入组合键，例如 Ctrl+Alt+F6；留空解除绑定。保存时会检查冲突。"),
            );

        for (index, action) in CycleAction::ALL.into_iter().enumerate() {
            root = root.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(gpui::px(160.0))
                            .min_w(gpui::px(120.0))
                            .text_sm()
                            .text_color(gpui::rgb(ThemeColors::TEXT_PRIMARY()))
                            .child(action.label()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::px(180.0))
                            .child(self.cycles[index].clone()),
                    ),
            );
        }

        let expanded = self.expanded;
        let adjust_count = self.adjusts.len();
        root = root.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt_1()
                .child(
                    option_button(
                        if expanded {
                            "收起数值快捷键".to_string()
                        } else {
                            format!("展开数值快捷键（{} 项）", adjust_count)
                        },
                        false,
                        false,
                    )
                    .id("expand-adjust")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.expanded = !expanded;
                        cx.notify();
                    })),
                ),
        );

        if expanded {
            for row in &self.adjusts {
                let row_id = row.id;
                let is_inc = row.direction == "increase";
                let is_dec = row.direction == "decrease";
                let param_label = AdjustParam::ALL
                    .iter()
                    .find(|p| p.as_str() == row.param)
                    .map(|p| p.label().to_string())
                    .unwrap_or_else(|| format!("未知({})", row.param));

                let mut row_container = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2p5()
                    .rounded_md()
                    .bg(gpui::rgb(ThemeColors::BG_ROOT()))
                    .border_1()
                    .border_color(gpui::rgb(ThemeColors::BORDER()));

                let left_controls = div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        option_button(&format!("参数: {param_label}"), row.param_menu_open, false)
                            .id(SharedString::from(format!("param-btn-{}", row_id)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(r) = this.adjusts.iter_mut().find(|r| r.id == row_id) {
                                    r.param_menu_open = !r.param_menu_open;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                option_button("减小", is_dec, false)
                                    .id(SharedString::from(format!("dir-dec-{}", row_id)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(r) = this.adjusts.iter_mut().find(|r| r.id == row_id) {
                                            r.direction = "decrease".into();
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                option_button("增大", is_inc, false)
                                    .id(SharedString::from(format!("dir-inc-{}", row_id)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(r) = this.adjusts.iter_mut().find(|r| r.id == row_id) {
                                            r.direction = "increase".into();
                                            cx.notify();
                                        }
                                    })),
                            ),
                    );

                let right_controls = div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap_2()
                    .flex_1()
                    .min_w(gpui::px(260.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .flex_1()
                            .min_w(gpui::px(160.0))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui::rgb(ThemeColors::TEXT_MUTED()))
                                    .child("按键:"),
                            )
                            .child(div().flex_1().min_w_0().child(row.key.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .flex_shrink_0()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui::rgb(ThemeColors::TEXT_MUTED()))
                                    .child("步长:"),
                            )
                            .child(div().w(gpui::px(55.0)).child(row.step.clone())),
                    )
                    .child(
                        option_button("删除", false, false)
                            .id(SharedString::from(format!("del-row-{}", row_id)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.adjusts.retain(|r| r.id != row_id);
                                cx.notify();
                            })),
                    );

                let row_line = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .child(left_controls)
                    .child(right_controls);

                row_container = row_container.child(row_line);

                if row.param_menu_open {
                    let mut menu_div = div()
                        .flex()
                        .flex_wrap()
                        .gap_1p5()
                        .p_2()
                        .rounded_md()
                        .bg(gpui::rgb(ThemeColors::BG_CARD()))
                        .border_1()
                        .border_color(gpui::rgb(ThemeColors::BORDER()));
                    for p in AdjustParam::ALL {
                        let p_str = p.as_str().to_string();
                        let p_label = p.label();
                        let is_selected = row.param == p_str;
                        menu_div = menu_div.child(
                            option_button(p_label, is_selected, false)
                                .id(SharedString::from(format!("param-opt-{}-{}", row_id, p_str)))
                                .on_click(cx.listener({
                                    let p_str = p_str.clone();
                                    move |this, _, _, cx| {
                                        if let Some(r) = this.adjusts.iter_mut().find(|r| r.id == row_id) {
                                            r.param = p_str.clone();
                                            r.param_menu_open = false;
                                            cx.notify();
                                        }
                                    }
                                })),
                        );
                    }
                    row_container = row_container.child(menu_div);
                }

                root = root.child(row_container);
            }

            root = root.child(
                div()
                    .flex()
                    .justify_start()
                    .child(
                        option_button("+ 添加数值快捷键", false, false)
                            .id("add-adjust")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let row_id = this.next_row_id;
                                this.next_row_id += 1;
                                let default_param = AdjustParam::ALL
                                    .iter()
                                    .find(|p| {
                                        p.label().contains("背光")
                                            || p.label().contains("亮度")
                                            || p.as_str() == "backlight"
                                            || p.as_str() == "brightness"
                                    })
                                    .map(|p| p.as_str())
                                    .unwrap_or(AdjustParam::ALL[0].as_str())
                                    .to_string();
                                let key = cx.new(|cx| TextInput::new(cx, "未绑定，例如 Ctrl+Alt+Up", ""));
                                let step = cx.new(|cx| TextInput::new(cx, "步长", "1"));
                                this.adjusts.push(AdjustRow {
                                    id: row_id,
                                    param: default_param,
                                    direction: "increase".to_string(),
                                    key,
                                    step,
                                    param_menu_open: false,
                                });
                                cx.notify();
                            })),
                    ),
            );
        }

        if !self.error.is_empty() {
            let is_success = self.error.starts_with("已保存");
            let (bg, border, text_c) = if is_success {
                (
                    ThemeColors::STATUS_SUCCESS_BG(),
                    ThemeColors::STATUS_SUCCESS(),
                    ThemeColors::STATUS_SUCCESS(),
                )
            } else {
                (
                    ThemeColors::STATUS_ERROR_BG(),
                    ThemeColors::STATUS_ERROR(),
                    ThemeColors::STATUS_ERROR(),
                )
            };
            root = root.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .p_2p5()
                    .rounded_md()
                    .bg(gpui::rgb(bg))
                    .border_1()
                    .border_color(gpui::rgb(border))
                    .text_xs()
                    .text_color(gpui::rgb(text_c))
                    .min_w_0()
                    .child(self.error.clone()),
            );
        }

        root.child(
            div()
                .flex()
                .justify_end()
                .pt_2()
                .child(
                    option_button("保存并注册快捷键", true, false)
                        .id("save-shortcuts")
                        .on_click(cx.listener(|this, _, _, cx| {
                    let cycles = CycleAction::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(i, action)| {
                            (
                                action.as_str().to_string(),
                                split_binding(this.cycles[i].read(cx).text()),
                            )
                        })
                        .collect::<HashMap<_, _>>();

                    let mut adjusts = Vec::new();
                    for row in &this.adjusts {
                        let b = split_binding(row.key.read(cx).text());
                        if b.key.is_empty() {
                            continue;
                        }

                        let Some(param) = AdjustParam::ALL.iter().find(|p| p.as_str() == row.param) else {
                            this.error = format!("未知参数: {}", row.param);
                            cx.notify();
                            return;
                        };

                        if row.direction != "increase" && row.direction != "decrease" {
                            this.error = format!("方向必须是增大或减小: {}", row.direction);
                            cx.notify();
                            return;
                        }

                        let Ok(step) = row.step.read(cx).text().trim().parse::<u32>() else {
                            this.error = "步长必须是正整数".into();
                            cx.notify();
                            return;
                        };

                        if step == 0 || step > (param.max() - param.min()) as u32 {
                            this.error = format!("{}的步长超出范围", param.label());
                            cx.notify();
                            return;
                        }

                        adjusts.push(AdjustHotkeyConfig {
                            param: param.as_str().into(),
                            direction: row.direction.clone(),
                            step: Some(step),
                            modifier: b.modifier,
                            key: b.key,
                        });
                    }

                    let planned = crate::hotkey::validate_and_plan_bindings(&cycles, &adjusts);
                    if has_binding_errors(&planned) {
                        this.error = "组合键无效或重复。支持 Ctrl+Alt / Ctrl+Shift / Alt+Shift / Win+Shift，及字母、数字、F 键或方向键。".into();
                        cx.notify();
                        return;
                    }

                    super::tools_panel::save_patch(
                        this.bus.clone(),
                        HashMap::from([
                            ("hotkeys".into(), serde_json::json!(cycles)),
                            ("adjust_hotkeys".into(), serde_json::json!(adjusts)),
                        ]),
                    );
                    this.error = "已保存，后台正在注册；被系统占用时会显示提示。".into();
                    cx.notify();
                })),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_shortcuts_can_be_saved_while_invalid_and_duplicate_keys_are_rejected() {
        let mut cycles = HashMap::new();
        assert!(!has_binding_errors(&crate::hotkey::validate_and_plan_bindings(&cycles, &[])));
        let action = CycleAction::ALL[0].as_str().to_string();
        cycles.insert(action.clone(), split_binding("Ctrl+Alt+F6"));
        assert!(!has_binding_errors(&crate::hotkey::validate_and_plan_bindings(&cycles, &[])));
        cycles.insert(CycleAction::ALL[1].as_str().into(), split_binding("Ctrl+Alt+F6"));
        assert!(has_binding_errors(&crate::hotkey::validate_and_plan_bindings(&cycles, &[])));
        cycles.remove(CycleAction::ALL[1].as_str());
        cycles.insert(action, split_binding("Invalid+UnknownKey"));
        assert!(has_binding_errors(&crate::hotkey::validate_and_plan_bindings(&cycles, &[])));
    }

    #[test]
    fn test_binding_and_split() {
        assert_eq!(binding("Ctrl+Alt", "F6"), "Ctrl+Alt+F6");
        assert_eq!(binding("", "F6"), "F6");
        assert_eq!(binding("Ctrl", ""), "");
        assert_eq!(binding("Ctrl", "无"), "");

        let s1 = split_binding("Ctrl+Alt+F6");
        assert_eq!(s1.modifier, "Ctrl+Alt");
        assert_eq!(s1.key, "F6");

        let s2 = split_binding("F6");
        assert_eq!(s2.modifier, "");
        assert_eq!(s2.key, "F6");
    }

}
