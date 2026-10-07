use super::*;
use crate::conversation::subagent::{Choice, Row, live_picker};
use gpui_kit::component::{button::Button, collapsible::Collapsible};
use sailry_protocol::conversation::Part;

impl View {
    pub(super) fn child_name(&self, child: &Child) -> SharedString {
        self.history
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .page
                    .entries
                    .iter()
                    .find(|entry| entry.id == child.origin.entry)
            })
            .and_then(|entry| entry.parts.get(child.origin.index))
            .and_then(|part| match part {
                Part::ToolCall { arguments, .. } => arguments.get("title")?.as_str(),
                _ => None,
            })
            .filter(|title| !title.trim().is_empty())
            .map(SharedString::from)
            .or_else(|| child.name.clone().map(SharedString::from))
            .unwrap_or_else(|| tr("chat_child"))
    }

    pub(in crate::conversation) fn child_rows(&self, turn: TurnId) -> Vec<Row> {
        self.current_children(turn)
            .into_iter()
            .map(|child| self.row(child))
            .collect()
    }

    fn row(&self, child: &Child) -> Row {
        Row {
            key: Choice::Live(child.run.session),
            parent: None,
            name: self.child_name(child),
            status: status_key(child.run.status),
            running: matches!(child.run.status, Status::Running | Status::Stopping),
            icon: match child.run.status {
                Status::Completed => IconName::Check,
                Status::Continued => IconName::ArrowRight,
                Status::Failed | Status::Interrupted => IconName::CircleX,
                Status::Cancelled => IconName::Minus,
                Status::Queued => IconName::Ellipsis,
                Status::Running | Status::Stopping => IconName::LoaderCircle,
            },
            order: child.run.sequence,
        }
    }

    pub(super) fn child_label(completed: usize, total: usize) -> String {
        rust_i18n::t!("subagent_count", completed = completed, total = total).to_string()
    }

    /// Historical delegation expands inline, like the other tool groups.
    pub(in crate::conversation) fn render_children(
        &self,
        turn: TurnId,
        children: &[&Child],
        continuing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let first = key(children[0]);
        let mut agents = latest(children.iter().copied());
        agents.sort_by_key(|agent| {
            children
                .iter()
                .position(|child| child.run.session == agent.run.session)
                .unwrap()
        });
        let total = agents.len();
        let completed = agents
            .iter()
            .filter(|child| child.run.status == Status::Completed)
            .count();
        let active = agents.iter().any(|child| {
            matches!(
                child.run.status,
                Status::Queued | Status::Running | Status::Stopping
            )
        });
        let key = format!("subagents-{first}");
        let open = self
            .expanded
            .get(&(turn, key.clone()))
            .copied()
            .unwrap_or(false);
        if children.len() == 1 {
            return self.render_child(children[0], continuing, cx);
        }
        let list = first.clone();
        let content = v_flex()
            .w_full()
            .min_w_0()
            .pl_4()
            .pt_2()
            .gap_2()
            .debug_selector(move || format!("live-subagent-list-{list}"))
            .children(children.iter().copied().enumerate().map(|(index, child)| {
                self.render_child(child, continuing && index + 1 == children.len(), cx)
            }));
        Collapsible::new()
            .w_full()
            .min_w_0()
            .open(open)
            .child(
                crate::conversation::disclosure::trigger_with_icons(
                    format!("live-turn-subagents-{first}"),
                    icons(&agents, cx),
                    Self::child_label(completed, total).into(),
                    crate::conversation::disclosure::Detail {
                        running: active || continuing,
                        ..Default::default()
                    },
                    open,
                    cx,
                )
                .on_click(
                    cx.listener(move |view, _, _, cx| view.expand(turn, key.clone(), !open, cx)),
                ),
            )
            .content(content)
            .into_any_element()
    }

    fn render_child(&self, child: &Child, continuing: bool, cx: &mut Context<Self>) -> AnyElement {
        let row = self.row(child);
        let id = child.run.session;
        let text = self
            .child_session(id)
            .map(|session| {
                format!(
                    "{} {}",
                    session.config.model,
                    crate::reasoning::label(session.config.effort)
                )
                .into()
            })
            .unwrap_or_default();
        crate::conversation::disclosure::trigger(
            format!("live-subagent-row-{}", key(child)),
            crate::ui::identicon::agent(&id.to_string(), cx),
            row.name.clone(),
            crate::conversation::disclosure::Detail {
                text,
                secondary: true,
                running: continuing || row.running,
                rotate_icon: row.running,
                failed: matches!(child.run.status, Status::Failed | Status::Interrupted),
                ..Default::default()
            },
            false,
            cx,
        )
        .accessibility_label(format!("{}: {}", row.name, tr(row.status)))
        .on_click(cx.listener(move |view, _, _, cx| view.open_child(id, cx)))
        .into_any_element()
    }

    pub(in crate::conversation) fn render_activity(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let turn = self.active()?;
        let (active, _, _, total) = self.child_counts(turn)?;
        if active == 0 {
            return None;
        }
        let id = "live-composer-subagents";
        let button = Button::new(id)
            .outline()
            .small()
            .px_3()
            .child(gpui_kit::component::spinner::Spinner::new().small())
            .rounded_full()
            .debug_selector(move || id.into())
            .font_normal()
            .text_color(cx.theme().muted_foreground)
            .label(rust_i18n::t!("composer_subagent_count", count = total).to_string());
        Some(
            h_flex()
                .child(live_picker(cx.entity().downgrade(), turn, None, id, button))
                .into_any_element(),
        )
    }
}

fn icons(children: &[&Child], cx: &App) -> impl IntoElement {
    let id = format!("live-subagent-icons-{}", key(children[0]));
    h_flex()
        .id(id.clone())
        .debug_selector(move || id.clone())
        .flex_shrink_0()
        .gap_1()
        .children(children.iter().map(|child| {
            let id = format!("live-subagent-icon-{}", key(child));
            let selector = id.clone();
            let running = matches!(child.run.status, Status::Running | Status::Stopping);
            let state = format!("{id}-{}", if running { "rotating" } else { "static" });
            div()
                .id(id)
                .debug_selector(move || selector.clone())
                .size_4()
                .flex_shrink_0()
                .child(div().debug_selector(move || state.clone()).size_4().child(
                    crate::ui::identicon::animated(
                        crate::ui::identicon::agent(&child.run.session.to_string(), cx),
                        running,
                    ),
                ))
        }))
}

#[cfg(test)]
mod tests;
