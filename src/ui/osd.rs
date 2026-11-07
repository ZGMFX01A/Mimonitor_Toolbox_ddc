use crate::message::{AppCommand, MessageBus};
use gpui::{div, prelude::*, rgb, Context, Render, Window};
use std::sync::Arc;
pub struct Osd {
    pub message: String,
    bus: Arc<MessageBus>,
}
impl Osd {
    pub fn new(message: String, bus: Arc<MessageBus>) -> Self {
        Self { message, bus }
    }
}
impl Render for Osd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_4()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(rgb(0x00bcd4))
            .bg(rgb(crate::ui::theme::ThemeColors::BG_CARD()))
            .text_color(rgb(0xffffff))
            .child(div().text_lg().child(self.message.clone()))
            .child(
                crate::ui::components::option_button("取消此次调整", false, false)
                    .id("cancel-osd")
                    .on_click(cx.listener(|this, _, _, _| {
                        let _ = this.bus.cmd_sender.send(AppCommand::CancelHotkey);
                    })),
            )
    }
}
