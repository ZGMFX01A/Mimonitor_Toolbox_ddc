use crate::{
    config::AppConfig,
    message::MessageBus,
    ui::components::{card_container, option_button},
};
use gpui::{div, prelude::*, Context, Div, Entity, Render, Window};
use std::{collections::HashMap, sync::Arc};
pub struct TrayPage {
    bus: Arc<MessageBus>,
    config: AppConfig,
}
impl TrayPage {
    pub fn new(bus: Arc<MessageBus>, config: AppConfig) -> Self {
        Self { bus, config }
    }
    pub fn sync(&mut self, config: AppConfig, cx: &mut Context<Self>) {
        self.config = config;
        cx.notify();
    }
    fn save(&mut self, items: Vec<String>) {
        super::tools_panel::save_patch(
            self.bus.clone(),
            HashMap::from([("tray_items".into(), serde_json::json!(items))]),
        );
    }
}
impl Render for TrayPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let catalog = crate::tray::catalog::catalog();
        let mut page=div().flex().flex_col().gap_4().child(div().text_xl().child("托盘菜单"))
            .child(card_container().child("左键打开统一亮度弹窗，失去焦点自动收起。右键菜单保留工具箱、刷新和退出，可添加下列快捷控制。"));
        for (index, key) in self.config.tray_items.clone().into_iter().enumerate() {
            let label = catalog
                .iter()
                .find(|entry| entry.id == key)
                .map(|entry| entry.label)
                .unwrap_or(&key)
                .to_string();
            page = page.child(
                card_container().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().child(label))
                        .child(
                            option_button("上移", false, index == 0)
                                .id(("tray-up", index))
                                .on_click(cx.listener(move |this, _, _, _| {
                                    if index > 0 {
                                        let mut items = this.config.tray_items.clone();
                                        items.swap(index, index - 1);
                                        this.save(items);
                                    }
                                })),
                        )
                        .child(
                            option_button("下移", false, index + 1 == self.config.tray_items.len())
                                .id(("tray-down", index))
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let mut items = this.config.tray_items.clone();
                                    if index + 1 < items.len() {
                                        items.swap(index, index + 1);
                                        this.save(items);
                                    }
                                })),
                        )
                        .child(
                            option_button("移除", false, false)
                                .id(("tray-remove", index))
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let mut items = this.config.tray_items.clone();
                                    items.retain(|item| item != &key);
                                    this.save(items);
                                })),
                        ),
                ),
            );
        }
        let mut available = card_container().child(div().text_lg().child("添加快捷控制"));
        for (index, entry) in catalog.into_iter().enumerate() {
            let key = entry.id.to_string();
            let added = self.config.tray_items.contains(&key);
            available = available.child(
                option_button(entry.label, added, added)
                    .id(("tray-add", index))
                    .on_click(cx.listener(move |this, _, _, _| {
                        if !added {
                            let mut items = this.config.tray_items.clone();
                            items.push(key.clone());
                            this.save(items);
                        }
                    })),
            );
        }
        page.child(available)
    }
}
pub fn render_tray_page(page: Entity<TrayPage>) -> Div {
    div().child(page)
}
