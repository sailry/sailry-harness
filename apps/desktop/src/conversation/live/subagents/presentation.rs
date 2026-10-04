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
        self.history
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.page.children)
            .filter(|child| child.origin.turn == turn)
            .map(|child| Row {
                key: Choice::Live(child.run.session),
                parent: None,
                name: self.child_name(child),
                status: status_key(child.run.status),
                icon: match child.run.status {
                    Status::Completed => IconName::Check,
                    Status::Failed | Status::Interrupted => IconName::CircleX,
                    Status::Cancelled => IconName::Minus,
                    Status::Queued => IconName::Ellipsis,
                    Status::Running | Status::Stopping => IconName::LoaderCircle,
                },
                order: child.run.sequence,
            })
            .collect()
    }

    pub(super) fn child_label(completed: usize, total: usize) -> String {
        rust_i18n::t!("subagent_count", completed = completed, total = total).to_string()
    }

    /// Historical delegation expands inline, like the other tool groups.
    pub(in crate::conversation) fn render_children(
        &self,
        turn: TurnId,
        children: &[SessionId],
        continuing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let first = children[0];
        let total = children.len();
        let completed = children
            .iter()
            .filter(|id| {
                self.child(**id)
                    .is_some_and(|child| child.run.status == Status::Completed)
            })
            .count();
        let active = children.iter().any(|id| {
            self.child(*id).is_some_and(|child| {
                matches!(
                    child.run.status,
                    Status::Queued | Status::Running | Status::Stopping
                )
            })
        });
        let key = format!("subagents-{first}");
        let open = self
            .expanded
            .get(&(turn, key.clone()))
            .copied()
            .unwrap_or(false);
        let rows: Vec<_> = self
            .child_rows(turn)
            .into_iter()
            .filter(|row| matches!(row.key, Choice::Live(id) if children.contains(&id)))
            .collect();
        if total == 1 {
            return self.child_row(rows.into_iter().next().unwrap(), continuing, cx);
        }
        let content = v_flex()
            .w_full()
            .min_w_0()
            .pl_4()
            .pt_2()
            .gap_2()
            .debug_selector(move || format!("live-subagent-list-{first}"))
            .children(
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| self.child_row(row, continuing && index + 1 == total, cx)),
            );
        Collapsible::new()
            .w_full()
            .min_w_0()
            .open(open)
            .child(
                crate::conversation::disclosure::trigger(
                    format!("live-turn-subagents-{first}"),
                    IconName::Bot,
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

    fn child_row(&self, row: Row, continuing: bool, cx: &mut Context<Self>) -> AnyElement {
        let Choice::Live(id) = row.key else {
            unreachable!()
        };
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
            format!("live-subagent-row-{id}"),
            crate::ui::identicon::agent(&id.to_string(), cx),
            row.name.clone(),
            crate::conversation::disclosure::Detail {
                text,
                secondary: true,
                running: continuing || matches!(row.status, "chat_running" | "chat_stopping"),
                failed: matches!(row.status, "turn_failed" | "chat_interrupted"),
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
