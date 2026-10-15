//! Shared status rows compose Kit ListItem, Button, Spinner, Collapsible and scrolling.
//! Packages supply content and actions; this module owns only presentation state.
#[cfg(test)]
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{collapsible::Collapsible, list::ListItem, spinner::Spinner, *},
    *,
};
use serde::Deserialize;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Paused,
    Blocked,
    Failed,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
    pub id: String,
    pub text: String,
    pub state: State,
    #[serde(default)]
    pub details: bool,
    #[serde(default)]
    pub highlight: bool,
}

#[derive(IntoElement)]
pub(crate) struct Rows {
    pub id: String,
    pub items: Vec<Row>,
}

impl RenderOnce for Rows {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let expanded =
            window.use_keyed_state((ElementId::from(self.id.clone()), "details"), cx, |_, _| {
                None::<(String, String)>
            });
        let width =
            window.use_keyed_state(
                (ElementId::from(self.id), "width"),
                cx,
                |_, _| None::<Pixels>,
            );
        let content_width = width
            .read(cx)
            .map(|width| (width - window.rem_size() * 0.5).max(px(0.)));
        v_flex()
            .w_full()
            .min_w_0()
            .on_prepaint(move |bounds, window, cx| {
                if width
                    .read(cx)
                    .is_some_and(|width| (width - bounds.size.width).abs() <= px(0.5))
                {
                    return;
                }
                let width = width.clone();
                window.defer(cx, move |window, cx| {
                    width.update(cx, |width, cx| {
                        *width = Some(bounds.size.width);
                        cx.notify();
                    });
                    window.refresh();
                });
            })
            .children(self.items.into_iter().map(|row| {
                let settled = matches!(row.state, State::Completed | State::Skipped);
                let (icon, tone) = match row.state {
                    State::Pending => (IconName::CircleCheck, cx.theme().muted_foreground),
                    State::InProgress => (IconName::LoaderCircle, cx.theme().muted_foreground),
                    State::Completed => (IconName::CircleCheck, cx.theme().success),
                    State::Skipped => (IconName::Minus, cx.theme().muted_foreground),
                    State::Paused => (IconName::Pause, cx.theme().warning),
                    State::Blocked => (IconName::TriangleAlert, cx.theme().warning),
                    State::Failed => (IconName::CircleX, cx.theme().danger),
                };
                let selected = expanded
                    .read(cx)
                    .as_ref()
                    .is_some_and(|(id, text)| *id == row.id && *text == row.text);
                let key = row.id.clone();
                let label_key = format!("{}-label", row.id);
                let icon_key = format!("{}-icon", row.id);
                let caret_key = format!("{}-caret", row.id);
                let group = SharedString::from(row.id.clone());
                let details_key = format!("{}-details", row.id);
                let text_key = format!("status-details-{}", row.id);
                let shimmer_key = format!("status-label-{}", row.id);
                let text = row.text.clone();
                let content = h_flex()
                    .w_full()
                    // Kit ListItem's internal content wrapper has an intrinsic minimum.
                    // Constrain our content to the measured row so native text ellipsis owns clipping.
                    .when_some(content_width, |content, width| content.w(width))
                    .min_w_0()
                    .gap_2()
                    .text_color(if row.details {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().foreground
                    })
                    .group_hover(group.clone(), |style| {
                        style.text_color(cx.theme().foreground)
                    })
                    .child(
                        div()
                            .debug_selector(move || icon_key.clone())
                            .flex_shrink_0()
                            .child(if row.state == State::InProgress {
                                Spinner::new()
                                    .ease(linear)
                                    .icon(icon)
                                    .with_size(px(16.))
                                    .color(tone)
                                    .into_any_element()
                            } else {
                                Icon::new(icon).size_4().text_color(tone).into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .when(settled, |text| {
                                text.text_color(cx.theme().muted_foreground).line_through()
                            })
                            .when(row.details, |text| {
                                text.group_hover(group.clone(), |style| {
                                    style.text_color(cx.theme().foreground)
                                })
                            })
                            .debug_selector(move || label_key.clone())
                            .child(if row.highlight {
                                shimmer::ShimmerText::new(row.text.clone())
                                    .id(shimmer_key)
                                    .into_any_element()
                            } else {
                                row.text.clone().into_any_element()
                            }),
                    )
                    .when(row.details, |content| {
                        content.child(
                            div()
                                .debug_selector(move || caret_key.clone())
                                .flex_shrink_0()
                                .child(
                                    Icon::new(if selected {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size_3(),
                                ),
                        )
                    });
                let trigger = if row.details {
                    let action = expanded.clone();
                    button::Button::new(row.id.clone())
                        .group(group)
                        .custom(
                            button::ButtonCustomVariant::new(cx)
                                .foreground(cx.theme().muted_foreground),
                        )
                        .accessibility_label(row.text.clone())
                        .w_full()
                        .h_8()
                        .min_w_0()
                        .pl_0()
                        .pr_2()
                        .py_1()
                        .child(content)
                        .on_click(move |_, window, cx| {
                            action.update(cx, |value, cx| {
                                *value = if selected {
                                    None
                                } else {
                                    Some((row.id.clone(), text.clone()))
                                };
                                cx.notify();
                            });
                            window.refresh();
                        })
                        .into_any_element()
                } else {
                    ListItem::new(row.id.clone())
                        .disabled(true)
                        .accessibility_label(row.text.clone())
                        .w_full()
                        .h_8()
                        .min_w_0()
                        .pl_0()
                        .pr_2()
                        .py_1()
                        .child(content)
                        .into_any_element()
                };
                v_flex()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .child(
                        div()
                            .debug_selector(move || key.clone())
                            .w_full()
                            .min_w_0()
                            .h_8()
                            .flex_shrink_0()
                            .child(trigger),
                    )
                    .child(
                        Collapsible::new().open(selected && row.details).content(
                            div()
                                .id(details_key.clone())
                                .debug_selector(move || details_key.clone())
                                .w_full()
                                .min_w_0()
                                .max_h_48()
                                .pl_6()
                                .py_2()
                                .child(crate::conversation::surface::code(
                                    &text_key,
                                    &row.text,
                                    cx.theme().mono_font_size,
                                    cx,
                                )),
                        ),
                    )
            }))
    }
}

#[derive(IntoElement)]
pub(crate) struct Content {
    pub id: String,
    pub items: Vec<Row>,
    pub empty: SharedString,
    pub body: Option<SharedString>,
    pub footer: Vec<AnyElement>,
}

impl RenderOnce for Content {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        let body = if self.body.is_none() && self.items.is_empty() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(self.empty)
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .min_w_0()
                .when_some(self.body, |body, text| {
                    let key = format!("{}-body", self.id);
                    let selector = key.clone();
                    body.child(
                        div()
                            .id(key.clone())
                            .debug_selector(move || selector.clone())
                            .role(Role::Label)
                            .aria_label(text.clone())
                            .w_full()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(gpui_kit::base::SelectableText::new(key, text))
                            .map(|body| {
                                #[cfg(test)]
                                let body = body.test_support();
                                body
                            }),
                    )
                })
                .when(!self.items.is_empty(), |body| {
                    body.child(div().pl_2().child(Rows {
                        id: self.id.clone(),
                        items: self.items,
                    }))
                })
                .into_any_element()
        };
        v_flex()
            .debug_selector(move || id.clone())
            .tab_group()
            .w(px(320.).min(window.viewport_size().width - px(48.)))
            .min_w_0()
            .child(crate::conversation::surface::list_scroll(
                format!("{}-scroll", self.id),
                body,
            ))
            .when(!self.footer.is_empty(), |panel| {
                panel.child(
                    h_flex()
                        .debug_selector({
                            let id = self.id;
                            move || format!("{id}-footer")
                        })
                        .w_full()
                        .flex_shrink_0()
                        .pt_2()
                        .gap_2()
                        .justify_end()
                        .children(self.footer),
                )
            })
    }
}
