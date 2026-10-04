//! Shared status rows compose Kit ListItem, Button, Spinner, Collapsible and scrolling.
//! Packages supply content and actions; this module owns only presentation state.
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
        v_flex()
            .w_full()
            .min_w_0()
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
                let details_key = format!("{}-details", row.id);
                let text_key = format!("status-details-{}", row.id);
                let shimmer_key = format!("status-label-{}", row.id);
                let text = row.text.clone();
                let content = h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .text_color(cx.theme().foreground)
                    .child(
                        div()
                            .flex_shrink_0()
                            .child(if row.state == State::InProgress {
                                Spinner::new()
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
                            .debug_selector(move || label_key.clone())
                            .child(if row.highlight {
                                shimmer::ShimmerText::new(row.text.clone())
                                    .id(shimmer_key)
                                    .into_any_element()
                            } else {
                                row.text.clone().into_any_element()
                            }),
                    );
                let trigger = if row.details {
                    let action = expanded.clone();
                    button::Button::new(row.id.clone())
                        .ghost()
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
                                    &text_key, &row.text, cx,
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
    pub footer: Vec<AnyElement>,
}

impl RenderOnce for Content {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        v_flex()
            .debug_selector(move || id.clone())
            .tab_group()
            .w(px(320.).min(window.viewport_size().width - px(48.)))
            .min_w_0()
            .child(crate::conversation::surface::group_scroll(
                format!("{}-scroll", self.id),
                if self.items.is_empty() {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.empty)
                        .into_any_element()
                } else {
                    Rows {
                        id: self.id.clone(),
                        items: self.items,
                    }
                    .into_any_element()
                },
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
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .gap_2()
                        .justify_end()
                        .children(self.footer),
                )
            })
    }
}
