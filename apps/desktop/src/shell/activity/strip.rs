//! Header shortcuts share the sidebar's activity projection and navigation.
//! Kit b79f4ce's AvatarGroup only accepts Avatar children, not focusable buttons.
//! Compose Kit Buttons in overlapping flex rows instead.
use super::*;
use gpui_kit::component::{
    badge::Badge, hover_card::HoverCard, popover::Popover, scroll::ScrollableElement,
};
use std::time::Duration;

#[cfg(test)]
mod tests;

const OVERLAP: f32 = 0.3;
// The 20 px glyphs overlap, while their 28 px pointer targets stay generous.
const STEP_REMS: f32 = 1.25 * (1. - OVERLAP);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Running,
    Waiting,
    Completed,
}

impl Group {
    fn of(session: &Session) -> Option<Self> {
        match lane(session) {
            Lane::Running => Some(Self::Running),
            Lane::Waiting | Lane::Failed => Some(Self::Waiting),
            Lane::Completed => Some(Self::Completed),
            Lane::Idle => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Running => "activity_running",
            Self::Waiting => "activity_waiting",
            Self::Completed => "activity_completed",
        }
    }
}

impl Shell {
    pub(in crate::shell) fn activity_strip(
        &self,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let sessions = self.active_sessions();
        if sessions.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .debug_selector(|| "header-activity".into())
                .flex_shrink_0()
                .gap_3()
                .children(
                    [Group::Waiting, Group::Running, Group::Completed]
                        .into_iter()
                        .filter_map(|group| {
                            let entries: Vec<_> = sessions
                                .iter()
                                .filter(|(_, session)| Group::of(session) == Some(group))
                                .collect();
                            if entries.is_empty() {
                                return None;
                            }
                            // Keep header tools reachable in the narrow conversation column.
                            let slots = if width < px(480.) { 2 } else { 3 };
                            let visible = if entries.len() > slots {
                                slots - 1
                            } else {
                                entries.len()
                            };
                            Some(
                                h_flex()
                                    .debug_selector(move || format!("header-{}", group.key()))
                                    .flex_shrink_0()
                                    // Negative margins collapse intrinsic group measurement;
                                    // derive its minimum extent from the rendered slots only.
                                    .min_w(rems(
                                        1.75 + STEP_REMS * (entries.len().min(slots) - 1) as f32,
                                    ))
                                    .children(entries.iter().take(visible).enumerate().map(
                                        |(index, (node, session))| {
                                            self.activity_avatar(*node, session, cx)
                                                .when(index > 0, |button| {
                                                    button.ml(rems(STEP_REMS - 1.75))
                                                })
                                        },
                                    ))
                                    .when(entries.len() > visible, |row| {
                                        row.child(self.activity_overflow(
                                            group,
                                            entries.len() - visible,
                                            cx,
                                        ))
                                    }),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    fn activity_avatar(&self, node: NodeId, session: &Session, cx: &mut Context<Self>) -> Div {
        let id = session.id;
        let state = lane(session);
        let label = label(session);
        let button = Button::new(format!("header-session-{node:?}-{id}"))
            .debug_selector(move || format!("header-session-{node:?}-{id}"))
            .text()
            .relative()
            .size_7()
            .p_0()
            .flex_shrink_0()
            .accessibility_label(label)
            .child(avatar_icon(&id.to_string(), state, cx))
            .on_click(cx.listener(move |shell, _, window, cx| {
                shell.open_activity(
                    node,
                    sailry_client::activity::Target::Session(id),
                    window,
                    cx,
                );
            }));
        div().size_7().flex_shrink_0().child(self.session_preview(
            format!("header-session-{node:?}-{id}"),
            node,
            session,
            button,
        ))
    }

    fn activity_overflow(
        &self,
        group: Group,
        count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let popover = Popover::new(format!("header-{}-overflow", group.key()))
            .anchor(Anchor::TopRight)
            .w(px(300.))
            .p_2()
            .trigger(
                Button::new(format!("header-{}-more", group.key()))
                    .debug_selector(move || format!("header-{}-more", group.key()))
                    .text()
                    .relative()
                    .size_7()
                    .p_0()
                    .text_xs()
                    .when(group == Group::Waiting, |button| {
                        button.text_color(cx.theme().warning)
                    })
                    .when(group == Group::Completed, |button| {
                        button.text_color(cx.theme().success)
                    })
                    .label(format!("+{count}"))
                    .tooltip(tr(group.key()))
                    .accessibility_label(format!("{} · +{count}", tr(group.key()))),
            )
            .content(move |_, window, cx| {
                let Some(shell) = owner.upgrade() else {
                    return div().into_any_element();
                };
                v_flex()
                    .id("header-activity-list")
                    .debug_selector(|| "header-activity-list".into())
                    .gap_1()
                    .max_h(px(320.).min(window.viewport_size().height - px(96.)))
                    .overflow_y_scrollbar()
                    .children(
                        shell
                            .read(cx)
                            .active_sessions()
                            .into_iter()
                            .filter(|(_, session)| Group::of(session) == Some(group))
                            .map(|(node, session)| {
                                let target = shell.clone();
                                let popup = cx.entity().downgrade();
                                let id = session.id;
                                let button =
                                    Button::new(format!("header-activity-row-{node:?}-{id}"))
                                        .debug_selector(move || {
                                            format!("header-activity-row-{node:?}-{id}")
                                        })
                                        .ghost()
                                        .w_full()
                                        .h_9()
                                        .px_2()
                                        .flex_shrink_0()
                                        .accessibility_label(label(&session))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .min_w_0()
                                                .gap_2()
                                                .child(crate::ui::identicon::agent(
                                                    &id.to_string(),
                                                    cx,
                                                ))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .truncate()
                                                        .child(crate::activity::title(&session)),
                                                )
                                                .child(
                                                    crate::activity::indicator(lane(&session), cx)
                                                        .size_4()
                                                        .flex_shrink_0(),
                                                ),
                                        )
                                        .on_click(move |_, window, cx| {
                                            let _ = popup
                                                .update(cx, |popup, cx| popup.dismiss(window, cx));
                                            target.update(cx, |shell, cx| {
                                                shell.open_activity(
                                                    node,
                                                    sailry_client::activity::Target::Session(id),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        });
                                shell.read(cx).session_preview(
                                    format!("header-activity-row-{node:?}-{id}"),
                                    node,
                                    &session,
                                    button,
                                )
                            }),
                    )
                    .into_any_element()
            });
        div()
            .size_7()
            .ml(rems(STEP_REMS - 1.75))
            .flex_shrink_0()
            .child(popover)
    }

    fn session_preview(
        &self,
        id: String,
        node: NodeId,
        session: &Session,
        trigger: Button,
    ) -> HoverCard {
        let title = crate::activity::title(session);
        let project = self.session_project_name(node, session);
        let selector = format!("{id}-preview");
        HoverCard::new(selector.clone())
            .anchor(Anchor::TopRight)
            // HoverCard anchors at the trigger's top edge. Clear both the
            // 28 px avatar and 36 px overflow row without blocking their clicks.
            .top_10()
            .trigger(trigger)
            .content(move |_, window, cx| {
                crate::ui::session_preview::content(&id, title.clone(), project.clone(), window, cx)
            })
    }
}

fn rotation() -> Animation {
    // Kit's Spinner fixes its speed at 0.8 seconds; use its Icon animation API
    // for a 450 ms revolution followed by a 550 ms pause, once per second.
    Animation::new(Duration::from_secs(1))
        .repeat()
        .with_easing(|phase| {
            let progress = (phase / 0.45).min(1.);
            progress * progress * (3. - 2. * progress)
        })
}

fn avatar_icon(key: &str, state: Lane, cx: &App) -> Div {
    let icon = crate::ui::identicon::agent(key, cx).size_5();
    let (name, content) = match state {
        Lane::Running => (
            "running",
            icon.with_animation("session-rotation", rotation(), |icon, phase| {
                icon.transform(Transformation::rotate(percentage(phase)))
            })
            .into_any_element(),
        ),
        Lane::Idle => ("idle", icon.into_any_element()),
        state => {
            let (name, symbol, color) = match state {
                Lane::Waiting => ("waiting", IconName::TriangleAlert, cx.theme().warning),
                Lane::Completed => ("completed", IconName::Check, cx.theme().success),
                Lane::Failed => ("failed", IconName::Close, cx.theme().danger),
                _ => unreachable!(),
            };
            (
                name,
                Badge::new()
                    .small()
                    .color(color)
                    .icon(Icon::new(symbol).size_2())
                    .child(icon.text_color(color))
                    .into_any_element(),
            )
        }
    };
    let selector = format!("header-avatar-{name}-{key}");
    div()
        .debug_selector(move || selector.clone())
        .size_5()
        .flex_shrink_0()
        .child(content)
}

fn label(session: &Session) -> SharedString {
    format!(
        "{} · {}",
        crate::activity::title(session),
        crate::activity::status(session)
    )
    .into()
}
