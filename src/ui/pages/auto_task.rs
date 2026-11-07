use crate::{
    config::{AppConfig, AutoTaskItem},
    message::{AppCommand, MessageBus},
    ui::components::{card_container, option_button, TextInput},
};
use gpui::{div, prelude::*, Context, Div, Entity, Render, Window};
use std::sync::Arc;

pub struct AutoTaskPage {
    bus: Arc<MessageBus>,
    config: AppConfig,
    start: Entity<TextInput>,
    end: Entity<TextInput>,
    preset: String,
    editing: Option<String>,
    deleting: Option<String>,
    restore: Option<String>,
    error: String,
}
impl AutoTaskPage {
    pub fn new(bus: Arc<MessageBus>, config: AppConfig, cx: &mut Context<Self>) -> Self {
        Self {
            bus,
            start: cx.new(|cx| TextInput::new(cx, "HH:MM", "09:00")),
            end: cx.new(|cx| TextInput::new(cx, "HH:MM", "18:00")),
            preset: config
                .presets
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_default(),
            config,
            editing: None,
            deleting: None,
            restore: None,
            error: String::new(),
        }
    }
    pub fn sync(&mut self, config: AppConfig, cx: &mut Context<Self>) {
        self.config = config;
        cx.notify();
    }
}
impl Render for AutoTaskPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut page=div().flex().flex_col().gap_4().child(div().text_xl().child("自动任务"))
            .child(div().text_sm().child("应用运行并连接显示器时生效。支持跨午夜；时段重叠时列表靠后的任务优先。结束后恢复进入任务前的设置。"));
        for (index, task) in self.config.auto_tasks.clone().into_iter().enumerate() {
            let name = self
                .config
                .presets
                .iter()
                .find(|p| p.id == task.preset_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| {
                    if task.preset_id == crate::presets::BASELINE_PRESET_ID {
                        "无预设（基线）".into()
                    } else {
                        "预设已删除".into()
                    }
                });
            let edit = task.clone();
            let id = task.id.clone();
            let row = card_container()
                .child(div().child(format!("{} – {}　{}", task.start, task.end, name)))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            option_button("编辑", false, false)
                                .id(("task-edit", index))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.editing = Some(edit.id.clone());
                                    this.preset = edit.preset_id.clone();
                                    this.start.update(cx, |input, cx| {
                                        input.set_text(edit.start.clone(), cx)
                                    });
                                    this.end.update(cx, |input, cx| {
                                        input.set_text(edit.end.clone(), cx)
                                    });
                                    cx.notify();
                                })),
                        )
                        .child(
                            option_button("删除", false, false)
                                .id(("task-delete", index))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.deleting = Some(id.clone());
                                    cx.notify();
                                })),
                        ),
                );
            page = page.child(row);
        }
        if self.config.auto_tasks.is_empty() {
            page = page.child(
                card_container().child("尚无自动任务。先在预设页面保存一组设置，再添加时间段。"),
            );
        }
        let mut editor = card_container()
            .child(div().text_lg().child(if self.editing.is_some() {
                "编辑时间段"
            } else {
                "添加时间段"
            }))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(div().flex_1().child("开始时间").child(self.start.clone()))
                    .child(div().flex_1().child("结束时间").child(self.end.clone())),
            );
        let options = self
            .config
            .presets
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .chain(std::iter::once((
                crate::presets::BASELINE_PRESET_ID.into(),
                "无预设（基线）".into(),
            )))
            .collect::<Vec<_>>();
        let mut choices = div().flex().flex_wrap().gap_2();
        for (index, (id, name)) in options.into_iter().enumerate() {
            let selected = self.preset == id;
            choices = choices.child(
                option_button(name, selected, false)
                    .id(("task-preset", index))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.preset = id.clone();
                        cx.notify();
                    })),
            );
        }
        editor = editor
            .child(choices)
            .child(div().text_sm().child(self.error.clone()))
            .child(
                option_button("保存时间段", true, false)
                    .id("save-task")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let task = AutoTaskItem {
                            id: this.editing.clone().unwrap_or_else(|| {
                                format!(
                                    "task-{}",
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_nanos()
                                )
                            }),
                            start: this.start.read(cx).text().trim().into(),
                            end: this.end.read(cx).text().trim().into(),
                            preset_id: this.preset.clone(),
                            snapshot: None,
                        };
                        if !crate::presets::scheduler::valid_task(&task) {
                            this.error = "时间格式须为 HH:MM，开始和结束不能相同".into();
                            cx.notify();
                            return;
                        }
                        if this
                            .bus
                            .cmd_sender
                            .send(AppCommand::SaveAutoTask(task))
                            .is_err()
                        {
                            this.error = "后台队列忙，请稍后重试".into();
                        } else {
                            this.editing = None;
                            this.error.clear();
                        }
                        cx.notify();
                    })),
            );
        page = page.child(editor);
        if let Some(id) = self.deleting.clone() {
            page = page.child(
                card_container()
                    .child("确认删除？尚未还原的设置会保存在恢复记录中。")
                    .child(
                        option_button("确认删除", true, false)
                            .id("confirm-delete-task")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this
                                    .bus
                                    .cmd_sender
                                    .send(AppCommand::DeleteAutoTask(id.clone()));
                                this.deleting = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        option_button("取消", false, false)
                            .id("cancel-delete-task")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.deleting = None;
                                cx.notify();
                            })),
                    ),
            );
        }
        if let Some(value) = self.config.extra.get("scheduler_recovery") {
            if let Ok(recovery) =
                crate::presets::recovery::SchedulerRecovery::from_extra_value(value)
            {
                let status = if let Some(id) = recovery.active_task_id {
                    format!("执行中：{id}")
                } else if recovery.waiting_task_id.is_some() {
                    "时段已到，等待显示器连接".into()
                } else {
                    "当前无执行中的时间段".into()
                };
                page = page.child(div().text_sm().child(status));
                if !recovery.blocked_task_ids.is_empty() {
                    page =
                        page.child(div().text_sm().child(
                            "部分失败的任务已暂停。恢复原基线后，编辑并保存任务可重新启用。",
                        ));
                }
                for (index, record) in recovery
                    .pending_restore
                    .into_iter()
                    .chain(recovery.orphaned_restores)
                    .enumerate()
                {
                    let id = record.task_id.clone();
                    page = page.child(
                        card_container()
                            .child(format!("待还原：{} · {}", record.task_id, record.reason))
                            .child(
                                option_button("确认向当前设备还原", false, false)
                                    .id(("restore-task", index))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.restore = Some(id.clone());
                                        cx.notify();
                                    })),
                            ),
                    );
                }
            }
        }
        if let Some(id) = self.restore.clone() {
            page = page.child(
                card_container()
                    .child("这会将保存的基线写入当前连接的显示器。请确认它就是原设备。")
                    .child(
                        option_button("已确认设备，开始还原", true, false)
                            .id("confirm-restore-task")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this
                                    .bus
                                    .cmd_sender
                                    .send(AppCommand::ConfirmTaskRestore(id.clone()));
                                this.restore = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        option_button("取消", false, false)
                            .id("cancel-restore-task")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.restore = None;
                                cx.notify();
                            })),
                    ),
            );
        }
        page
    }
}
pub fn render_auto_task_page(page: Entity<AutoTaskPage>) -> Div {
    div().child(page)
}
