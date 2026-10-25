use super::*;
use crate::conversation::subagent::{Choice, Row, activity_picker};
use gpui_kit::component::{button::Button, collapsible::Collapsible};

impl View {
    pub(super) fn child_name(&self, child: &Child) -> SharedString {
        sailry_client::conversation::tools::display::child_title(
            self.history
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.page.as_ref()),
            child,
        )
        .map(|title| SharedString::from(title.into_owned()))
        .unwrap_or_else(|| tr("chat_child"))
    }

    fn row(&self, child: &Child) -> Row {
        Row {
            key: Choice::Live(child.run.session),
            parent: None,
            name: self.child_name(child),
            status: status_key(child.run.status),
            running: is_running(child.run.status),
            icon: match child.run.status {
                Status::Completed => IconName::Check,
                Status::Continued => IconName::ArrowRight,
                Status::Failed | Status::Interrupted => IconName::CircleX,
                Status::Cancelled => IconName::Minus,
                Status::Queued => IconName::Ellipsis,
                Status::Running | Status::Stopping => IconName::LoaderCircle,
            },
            order: child.run.sequence,
            elapsed: None,
        }
    }

    pub(in crate::conversation) fn activity_rows(&self) -> Vec<Row> {
        self.activity_children()
            .into_iter()
            .map(|child| {
                let mut row = self.row(child);
                row.elapsed = super::super::messages::elapsed(
                    &child.run,
                    chrono::Utc::now().timestamp_millis(),
                )
                .map(Into::into);
                row
            })
            .collect()
    }

    pub(super) fn child_label(completed: usize, total: usize) -> String {
        rust_i18n::t!("subagent_count", completed = completed, total = total).to_string()
    }

    /// Show each session once; its complete task history remains in the child view.
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
        if total == 1 {
            return self.render_child(agents[0], continuing, cx);
        }
        let list = first.clone();
        let content = v_flex()
            .w_full()
            .min_w_0()
            .pl_4()
            .pt_2()
            .gap_2()
            .debug_selector(move || format!("live-subagent-list-{list}"))
            .children(agents.iter().copied().map(|child| {
                self.render_child(
                    child,
                    continuing && child.run.session == children.last().unwrap().run.session,
                    cx,
                )
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
        crate::conversation::disclosure::trigger_with_icons(
            format!("live-subagent-row-{}", key(child)),
            crate::ui::identicon::animated(
                crate::ui::identicon::child(&id.to_string(), row.running, cx),
                row.running,
            ),
            row.name.clone(),
            crate::conversation::disclosure::Detail {
                text,
                secondary: true,
                running: continuing || row.running,
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
        let session = self.session()?;
        let total = self.activity_children().len();
        if total == 0 {
            return None;
        }
        let id = "live-composer-subagents";
        let button = Button::new(id)
            .outline()
            .small()
            .px_3()
            .rounded_full()
            .debug_selector(move || id.into())
            .font_normal()
            .text_color(cx.theme().muted_foreground)
            .label(rust_i18n::t!("composer_subagent_count", count = total).to_string());
        Some(
            h_flex()
                .child(activity_picker(
                    cx.entity().downgrade(),
                    session,
                    id,
                    button,
                    cx,
                ))
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
            let running = is_running(child.run.status);
            let state = format!("{id}-{}", if running { "rotating" } else { "static" });
            div()
                .id(id)
                .debug_selector(move || selector.clone())
                .size_4()
                .flex_shrink_0()
                .child(div().debug_selector(move || state.clone()).size_4().child(
                    crate::ui::identicon::animated(
                        crate::ui::identicon::child(&child.run.session.to_string(), running, cx),
                        running,
                    ),
                ))
        }))
}
