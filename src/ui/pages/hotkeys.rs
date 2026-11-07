use crate::{
    config::AppConfig,
    hotkey::{AdjustHotkeyConfig, AdjustParam, CycleAction, HotkeyBindingConfig},
    message::MessageBus,
    ui::components::{card_container, option_button, TextInput},
};
use gpui::{div, prelude::*, Context, Entity, Render, Window};
use std::{collections::HashMap, sync::Arc};
pub struct HotkeyForm {
    bus: Arc<MessageBus>,
    cycles: Vec<Entity<TextInput>>,
    adjusts: Vec<(Entity<TextInput>, Entity<TextInput>)>,
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
        let adjusts = AdjustParam::ALL
            .into_iter()
            .flat_map(|param| ["decrease", "increase"].map(move |direction| (param, direction)))
            .map(|(param, direction)| {
                let cfg = saved
                    .iter()
                    .find(|cfg| cfg.param == param.as_str() && cfg.direction == direction)
                    .cloned()
                    .unwrap_or_default();
                (
                    cx.new(|cx| {
                        TextInput::new(
                            cx,
                            "未绑定，例如 Ctrl+Alt+Up",
                            binding(&cfg.modifier, &cfg.key),
                        )
                    }),
                    cx.new(|cx| TextInput::new(cx, "步长", cfg.step.unwrap_or(1).to_string())),
                )
            })
            .collect();
        Self {
            bus,
            cycles,
            adjusts,
            expanded: false,
            error: String::new(),
        }
    }
}
impl Render for HotkeyForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = card_container()
            .child(div().text_lg().child("全局快捷键"))
            .child(
                div()
                    .text_sm()
                    .child("输入组合键，例如 Ctrl+Alt+F6；留空解除绑定。保存时会检查冲突。"),
            );
        for (index, action) in CycleAction::ALL.into_iter().enumerate() {
            root = root.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().w(gpui::px(180.0)).text_sm().child(action.label()))
                    .child(div().flex_1().child(self.cycles[index].clone())),
            );
        }
        let expanded = self.expanded;
        root = root.child(
            option_button(
                if expanded {
                    "收起数值快捷键"
                } else {
                    "展开数值快捷键（20 项）"
                },
                false,
                false,
            )
            .id("expand-adjust")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.expanded = !expanded;
                cx.notify();
            })),
        );
        if expanded {
            for (index, (key, step)) in self.adjusts.iter().enumerate() {
                let param = AdjustParam::ALL[index / 2];
                root = root.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(gpui::px(150.0)).text_sm().child(format!(
                            "{} {}",
                            param.label(),
                            if index % 2 == 0 { "减小" } else { "增大" }
                        )))
                        .child(div().flex_1().child(key.clone()))
                        .child(div().w(gpui::px(65.0)).child(step.clone())),
                );
            }
        }
        root.child(div().text_sm().child(self.error.clone())).child(option_button("保存并注册快捷键",true,false).id("save-shortcuts").on_click(cx.listener(|this,_,_,cx|{
            let cycles=CycleAction::ALL.into_iter().enumerate().map(|(i,action)|(action.as_str().to_string(),split_binding(this.cycles[i].read(cx).text()))).collect::<HashMap<_,_>>();
            let mut adjusts=Vec::new();for(index,(key,step))in this.adjusts.iter().enumerate(){let b=split_binding(key.read(cx).text());if b.key.is_empty(){continue;}let Ok(step)=step.read(cx).text().parse::<u32>()else{this.error="步长必须是正整数".into();cx.notify();return;};let param=AdjustParam::ALL[index/2];if step==0||step>(param.max()-param.min())as u32{this.error=format!("{}的步长超出范围",param.label());cx.notify();return;}adjusts.push(AdjustHotkeyConfig{param:param.as_str().into(),direction:if index%2==0{"decrease"}else{"increase"}.into(),step:Some(step),modifier:b.modifier,key:b.key});}
            let planned=crate::hotkey::validate_and_plan_bindings(&cycles,&adjusts);if planned.iter().any(|p|p.pre_error.is_some()){this.error="组合键无效或重复。支持 Ctrl+Alt / Ctrl+Shift / Alt+Shift / Win+Shift，及字母、数字、F 键或方向键。".into();cx.notify();return;}
            super::tools_panel::save_patch(this.bus.clone(),HashMap::from([("hotkeys".into(),serde_json::json!(cycles)),("adjust_hotkeys".into(),serde_json::json!(adjusts))]));this.error="已保存，后台正在注册；被系统占用时会显示提示。".into();cx.notify();
        })))
    }
}
