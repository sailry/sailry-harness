use super::*;
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    message_scroller::MessageScroller,
    *,
};

pub(in crate::conversation) fn header(
    picker: AnyElement,
    stop: AnyElement,
    cx: &App,
) -> AnyElement {
    crate::header::Header::new("subagent-header", cx)
        .bordered(false)
        .gap_1()
        .child(div().flex_1().min_w_0().child(picker))
        .child(stop)
        .into_any_element()
}

impl Shell {
    pub(in crate::conversation) fn subagent_summary(
        &self,
        location: Location,
        agents: &[Agent],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let complete = agents
            .iter()
            .filter(|agent| agent.turn.status == Status::Completed)
            .count();
        let failed = agents
            .iter()
            .filter(|agent| agent.turn.status == Status::Failed)
            .count();
        v_flex()
            .gap_1()
            .items_start()
            .child(
                self.subagent_picker(
                    location,
                    "turn-subagent-picker",
                    Button::new("turn-subagents")
                        .custom(crate::theme::subtle_button(cx))
                        .icon(IconName::Bot)
                        .rounded_full()
                        .debug_selector(|| "turn-subagents".into())
                        .label(
                            rust_i18n::t!(
                                "subagent_count",
                                completed = complete,
                                total = agents.len()
                            )
                            .to_string(),
                        ),
                    cx,
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        rust_i18n::t!("subagent_summary", complete = complete, failed = failed)
                            .to_string(),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::conversation) fn subagent_activity(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = (self.host, self.session);
        let thread = &self.conversations[&session];
        let turn = thread.turns.last()?;
        let total = turn.subagents().count();
        if total == 0 {
            return None;
        }
        let complete = turn
            .subagents()
            .filter(|agent| agent.turn.status == Status::Completed)
            .count();
        Some(
            self.subagent_picker(
                Location::main(session, thread.turns.len() - 1),
                "composer-subagent-picker",
                Button::new("composer-subagents")
                    .custom(crate::theme::subtle_button(cx))
                    .rounded_full()
                    .icon(IconName::Bot)
                    .debug_selector(|| "composer-subagents".into())
                    .tooltip(tr("subagent_show"))
                    .label(
                        rust_i18n::t!("subagent_count", completed = complete, total = total)
                            .to_string(),
                    ),
                cx,
            )
            .into_any_element(),
        )
    }

    pub(crate) fn subagent_panel(&self, panel: &Panel, cx: &mut Context<Self>) -> AnyElement {
        let location = panel.location;
        let agent = self
            .transcript_turn(Location {
                child: None,
                ..location
            })
            .and_then(|turn| {
                turn.subagents()
                    .find(|agent| Some(agent.key) == location.child)
            });
        let Some(agent) = agent else {
            return div().child(tr("subagent_unavailable")).into_any_element();
        };
        let owner = cx.entity().downgrade();
        v_flex()
            .debug_selector(|| "subagent-panel".into())
            .size_full()
            .min_h_0()
            .child(header(
                self.subagent_picker(
                    location,
                    "header-subagent-picker",
                    Button::new("subagent-switch")
                        .custom(crate::theme::subtle_button(cx))
                        .icon(IconName::Bot)
                        .rounded_full()
                        .label(agent.name.clone())
                        .debug_selector(|| "subagent-switch".into())
                        .max_w_full()
                        .tooltip(tr("subagent_show")),
                    cx,
                )
                .into_any_element(),
                Button::new("subagent-stop")
                    .ghost()
                    .icon(IconName::CircleX)
                    .disabled(true)
                    .debug_selector(|| "subagent-stop".into())
                    .tooltip(tr("subagent_stop_unavailable"))
                    .accessibility_label(tr("subagent_stop_unavailable"))
                    .into_any_element(),
                cx,
            ))
            .child(
                div()
                    .px_6()
                    .pt_3()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("subagent_readonly", role = agent.role).to_string()),
            )
            .child(
                div().flex_1().min_h_0().child(
                    MessageScroller::new(
                        "subagent-transcript",
                        panel.scroller.clone(),
                        move |_, _, cx| {
                            owner
                                .update(cx, |shell, cx| shell.transcript_view(location, cx))
                                .unwrap_or_else(|_| div().into_any_element())
                        },
                    )
                    .with_list_style(StyleRefinement::default().px_0().py_6())
                    .with_jump_button_label(tr("turn_latest"))
                    .with_jump_button_renderer(|button| {
                        button
                            .without_tooltip()
                            .accessibility_label(tr("turn_latest"))
                    }),
                ),
            )
            .into_any_element()
    }
}

/// Shared state glyph for inline delegation and the compact session picker.
pub(in crate::conversation) fn status_icon(row: &Row) -> AnyElement {
    use gpui_kit::component::{Icon, IconName, Sizable as _, spinner::Spinner};
    if matches!(
        row.status,
        "chat_running" | "chat_stopping" | "subagent_running"
    ) {
        Spinner::new()
            .icon(IconName::LoaderCircle)
            .small()
            .into_any_element()
    } else {
        Icon::new(row.icon.clone())
            .size_4()
            .flex_shrink_0()
            .into_any_element()
    }
}
