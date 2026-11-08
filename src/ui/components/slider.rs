use crate::ui::theme::ThemeColors;
use gpui::prelude::*;
use gpui::{
    div, px, rgb, App, Bounds, Context, CursorStyle, Element, ElementId, Entity, GlobalElementId,
    IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Render, Style, Window,
};

/// 通用可连续拖拽的参数滑块视图（支持 min/max/step/unit/未知/禁用）
pub struct ContinuousSlider {
    pub label: String,
    pub value: Option<u32>,
    pub min: u32,
    pub max: u32,
    pub step: u32,
    pub unit: String,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
    is_dragging: bool,
    pub last_bounds: Option<Bounds<Pixels>>,
    on_change: Option<Box<dyn Fn(u32, &mut Window, &mut Context<Self>) + 'static>>,
}

impl ContinuousSlider {
    /// 全功能通用构造方法
    pub fn new_param(
        label: impl Into<String>,
        initial_value: Option<u32>,
        min: u32,
        max: u32,
        step: u32,
        unit: impl Into<String>,
        disabled: bool,
        disabled_reason: Option<String>,
        on_change: impl Fn(u32, &mut Window, &mut Context<Self>) + 'static,
    ) -> Self {
        let min_val = min;
        let max_val = max.max(min_val + 1);
        let step_val = step.max(1);
        let clamped_val = initial_value.map(|v| v.clamp(min_val, max_val));

        Self {
            label: label.into(),
            value: clamped_val,
            min: min_val,
            max: max_val,
            step: step_val,
            unit: unit.into(),
            disabled,
            disabled_reason,
            is_dragging: false,
            last_bounds: None,
            on_change: Some(Box::new(on_change)),
        }
    }

    /// 便捷背光滑块构造方法（范围 1..100，单位 %）
    pub fn new_backlight(
        initial_value: u32,
        disabled: bool,
        disabled_reason: Option<String>,
        on_change: impl Fn(u32, &mut Window, &mut Context<Self>) + 'static,
    ) -> Self {
        Self::new_param(
            "背光",
            if initial_value == 0 && disabled {
                None
            } else {
                Some(initial_value.clamp(1, 100))
            },
            1,
            100,
            1,
            "%",
            disabled,
            disabled_reason,
            on_change,
        )
    }

    /// 向后兼容的便捷构造方法（默认背光 1..100，%）
    pub fn new(
        initial_value: u32,
        disabled: bool,
        disabled_reason: Option<String>,
        on_change: impl Fn(u32, &mut Window, &mut Context<Self>) + 'static,
    ) -> Self {
        Self::new_backlight(initial_value, disabled, disabled_reason, on_change)
    }

    /// 设置具体数值
    pub fn set_value(&mut self, val: u32, cx: &mut Context<Self>) {
        let clamped = val.clamp(self.min, self.max);
        if !self.is_dragging && self.value != Some(clamped) {
            self.value = Some(clamped);
            cx.notify();
        }
    }

    /// 设置可选数值（支持 None 未知值）
    pub fn set_value_opt(&mut self, val: Option<u32>, cx: &mut Context<Self>) {
        let clamped = val.map(|v| v.clamp(self.min, self.max));
        if !self.is_dragging && self.value != clamped {
            self.value = clamped;
            cx.notify();
        }
    }

    /// 设置禁用状态与禁用原因
    pub fn set_disabled(&mut self, disabled: bool, reason: Option<String>, cx: &mut Context<Self>) {
        if self.disabled != disabled || self.disabled_reason != reason {
            self.disabled = disabled;
            self.disabled_reason = reason;
            if disabled {
                self.is_dragging = false;
            }
            cx.notify();
        }
    }

    /// 从鼠标水平坐标换算对应数值（支持 step 量化对齐）
    fn update_value_from_mouse_x(
        &mut self,
        mouse_x: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        if let Some(bounds) = self.last_bounds {
            let width = bounds.size.width;
            if width > px(0.0) {
                let offset_x = (mouse_x - bounds.left()).clamp(px(0.0), width);
                let ratio = (offset_x / width).clamp(0.0, 1.0);
                let span = (self.max - self.min) as f32;
                let raw_val = self.min as f32 + ratio * span;

                // 按照 step 对齐
                let step_f = self.step.max(1) as f32;
                let stepped =
                    ((raw_val - self.min as f32) / step_f).round() * step_f + self.min as f32;
                let final_val = (stepped as u32).clamp(self.min, self.max);

                self.value = Some(final_val);
                if let Some(cb) = &self.on_change {
                    cb(final_val, window, cx);
                }
                cx.notify();
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.is_dragging = true;
        self.update_value_from_mouse_x(event.position.x, window, cx);
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_dragging {
            if event.pressed_button != Some(MouseButton::Left) {
                self.is_dragging = false;
                cx.notify();
                return;
            }
            self.update_value_from_mouse_x(event.position.x, window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_dragging {
            self.is_dragging = false;
            cx.notify();
        }
    }
}

struct SliderBoundsTracker {
    slider: Entity<ContinuousSlider>,
}

impl IntoElement for SliderBoundsTracker {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SliderBoundsTracker {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.0).into();
        style.size.height = gpui::relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        _window: &mut Window,
        cx: &mut App,
    ) {
        self.slider.update(cx, |s, _| {
            s.last_bounds = Some(bounds);
        });
    }
}

impl Render for ContinuousSlider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_disabled = self.disabled;

        let (fraction, display_val) = match self.value {
            Some(val) => {
                let span = self.max.saturating_sub(self.min);
                let frac = if span == 0 {
                    0.0
                } else {
                    ((val.saturating_sub(self.min)) as f32 / span as f32).clamp(0.0, 1.0)
                };
                (frac, format!("{}{}", val, self.unit))
            }
            None => (
                0.0,
                if is_disabled {
                    "--".to_string()
                } else {
                    "未知".to_string()
                },
            ),
        };

        let thumb_size = px(14.0);
        let thumb_left = if let Some(bounds) = self.last_bounds {
            if bounds.size.width > thumb_size {
                (bounds.size.width - thumb_size) * fraction
            } else {
                px(0.0)
            }
        } else {
            px(0.0)
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .gap_1()
            // 确保拖拽释放到边界外也能安全结束拖拽
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1_5()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(if is_disabled {
                                        ThemeColors::TEXT_DISABLED()
                                    } else {
                                        ThemeColors::TEXT_PRIMARY()
                                    }))
                                    .child(self.label.clone()),
                            )
                            .children(self.disabled_reason.as_ref().map(|reason| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(ThemeColors::TEXT_MUTED()))
                                    .child(format!("({})", reason))
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(if is_disabled {
                                ThemeColors::TEXT_DISABLED()
                            } else if self.value.is_none() {
                                ThemeColors::TEXT_MUTED()
                            } else {
                                ThemeColors::ACCENT_CYAN()
                            }))
                            .child(display_val),
                    ),
            )
            // 可拖拽的滑块轨道容器（高度适度加宽至28px提升命中率，不改数据映射）
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .h(px(28.0))
                    .cursor(if is_disabled {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::PointingHand
                    })
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(6.0))
                            .bg(rgb(ThemeColors::BG_TRACK()))
                            .rounded_full()
                            .child(SliderBoundsTracker {
                                slider: cx.entity(),
                            })
                            // 已填充高亮段
                            .child(
                                div()
                                    .absolute()
                                    .top(px(0.0))
                                    .left(px(0.0))
                                    .h_full()
                                    .w(gpui::DefiniteLength::Fraction(fraction))
                                    .bg(rgb(if is_disabled {
                                        ThemeColors::TEXT_DISABLED()
                                    } else {
                                        ThemeColors::ACCENT_CYAN()
                                    }))
                                    .rounded_full(),
                            )
                            // 滑块拇指 Thumb (仅在有确定数值且未禁用时显示在对应位置，居中且两端不溢出)
                            .children(if !is_disabled && self.value.is_some() {
                                Some(
                                    div()
                                        .absolute()
                                        .top(px(-4.0))
                                        .left(thumb_left)
                                        .w(thumb_size)
                                        .h(thumb_size)
                                        .rounded_full()
                                        .bg(rgb(0xFFFFFF))
                                        .border_2()
                                        .border_color(rgb(if self.is_dragging {
                                            ThemeColors::ACCENT_HOVER()
                                        } else {
                                            ThemeColors::ACCENT_CYAN()
                                        })),
                                )
                            } else {
                                None
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_continuous_slider_param_clamping_and_fraction() {
        let slider = ContinuousSlider::new_param(
            "红色增益",
            Some(1000),
            524,
            1524,
            1,
            "",
            false,
            None,
            |_, _, _| {},
        );

        assert_eq!(slider.min, 524);
        assert_eq!(slider.max, 1524);
        assert_eq!(slider.value, Some(1000));
        assert_eq!(slider.unit, "");
        assert_eq!(slider.label, "红色增益");
    }

    #[test]
    fn test_continuous_slider_backlight_convenience() {
        let slider = ContinuousSlider::new_backlight(50, false, None, |_, _, _| {});

        assert_eq!(slider.min, 1);
        assert_eq!(slider.max, 100);
        assert_eq!(slider.value, Some(50));
        assert_eq!(slider.unit, "%");
        assert_eq!(slider.label, "背光");
    }

    #[test]
    fn test_continuous_slider_none_value_handling() {
        let slider =
            ContinuousSlider::new_param("对比度", None, 0, 100, 1, "", false, None, |_, _, _| {});

        assert_eq!(slider.value, None);
    }
}
