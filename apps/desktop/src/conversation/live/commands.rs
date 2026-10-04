//! Background command controls beside the composer activity buttons.
use super::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    accordion::Accordion,
    button::{Button, ButtonVariants},
    popover::Popover,
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::{
    RequestId,
    process::{Info, Outcome, Status as CommandStatus},
};
use tokio::sync::watch;

fn active(info: &Info) -> bool {
    matches!(
        info.status,
        CommandStatus::Running | CommandStatus::Stopping
    )
}

pub(super) struct Panel {
    binding: Binding,
    session: Option<SessionId>,
    state: sailry_client::commands::View,
    selected: watch::Sender<Option<RequestId>>,
    stop: CancellationToken,
    watcher: Option<Task<()>>,
    action: Option<Task<()>>,
    error: Option<&'static str>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Panel {
    pub fn new(binding: Binding) -> Self {
        Self {
            binding,
            session: None,
            state: Default::default(),
            selected: watch::channel(None).0,
            stop: CancellationToken::new(),
            watcher: None,
            action: None,
            error: None,
        }
    }

    fn sync(&mut self, view: &View, cx: &mut Context<Self>) {
        if self.session == view.session()
            && self.binding.client.target() == view.binding.client.target()
        {
            return;
        }
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.binding = view.binding.clone();
        self.session = view.session();
        self.state = Default::default();
        self.action = None;
        self.error = None;
        self.watcher = None;
        let (selected, selection) = watch::channel(None);
        self.selected = selected;
        let Some(session) = self.session else {
            return;
        };
        let (updates, mut receiver) = watch::channel(sailry_client::commands::View::default());
        let stop = self.stop.clone();
        let client = self.binding.client.clone();
        self.binding.runtime.spawn(async move {
            client
                .watch_commands(session, selection, updates, stop)
                .await;
        });
        self.watcher = Some(cx.spawn(async move |panel, cx| {
            while receiver.changed().await.is_ok() {
                let state = receiver.borrow_and_update().clone();
                if panel
                    .update(cx, |panel, cx| {
                        panel.state = state;
                        panel.sync_selection();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn sync_selection(&mut self) {
        let selected = *self.selected.borrow();
        if selected.is_some_and(|id| {
            !self
                .state
                .items
                .iter()
                .any(|info| info.id == id && active(info))
        }) {
            self.selected.send_replace(None);
            self.state.output = None;
        }
    }

    fn stop_command(&mut self, id: RequestId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session else {
            return;
        };
        if self.action.is_some() || !self.state.connected {
            return;
        }
        let client = self.binding.client.clone();
        let request = client.prepare(Command::StopCommand { session, id });
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.error = None;
        self.action = Some(cx.spawn_in(window, async move |panel, cx| {
            let result = job.await;
            let _ = panel.update_in(cx, |panel, window, cx| {
                panel.action = None;
                match result {
                    Ok(Ok(Output::CommandOutput(output))) => {
                        if let Some(info) = panel.state.items.iter_mut().find(|info| info.id == id)
                        {
                            *info = output.info.clone();
                        }
                        if *panel.selected.borrow() == Some(id) {
                            panel.state.output = Some(output);
                        }
                    }
                    Ok(Err(error)) if error.code == sailry_protocol::ErrorCode::OutcomeUnknown => {
                        panel.error = Some("background_stop_unknown")
                    }
                    _ => panel.error = Some("background_stop_failed"),
                }
                if let Some(error) = panel.error {
                    crate::feedback::error("", &tr(error), window, cx);
                }
                panel.sync_selection();
                panel.selected.send_modify(|_| {});
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn row(&self, info: &Info, cx: &mut Context<Self>) -> AnyElement {
        let id = info.id;
        let group = SharedString::from(format!("background-header-{id}"));
        let status = match &info.status {
            CommandStatus::Running => tr("chat_running"),
            CommandStatus::Stopping => tr("chat_stopping"),
            CommandStatus::Finished(Outcome::Exited(0)) => tr("turn_completed"),
            CommandStatus::Finished(Outcome::Cancelled) => tr("turn_cancelled"),
            CommandStatus::Finished(_) | CommandStatus::Failed(_) => tr("turn_failed"),
        };
        h_flex()
            .id(group.clone())
            .group(group.clone())
            .debug_selector(move || format!("background-header-{id}"))
            .relative()
            .w_full()
            .h_6()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .id(SharedString::from(format!("background-status-{id}")))
                    .child(Icon::new(IconName::SquareTerminal).small())
                    .tooltip(move |window, cx| Tooltip::new(status.clone()).build(window, cx)),
            )
            .child(
                div()
                    .debug_selector(move || format!("background-command-{id}"))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(
                        info.command
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("background-stop-{id}")))
                    .debug_selector(move || format!("background-stop-{id}"))
                    .ghost()
                    .small()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bg(cx.theme().popover)
                    .opacity(0.)
                    .group_hover(group, |style| style.opacity(1.))
                    .focus(|style| style.opacity(1.))
                    .label(tr("background_end"))
                    .accessibility_label(tr("background_end"))
                    .disabled(
                        !self.state.connected
                            || self.action.is_some()
                            || info.status == CommandStatus::Stopping,
                    )
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        cx.stop_propagation();
                        panel.stop_command(id, window, cx);
                    })),
            )
            .into_any_element()
    }
}

impl View {
    pub(crate) fn bind_ports(
        &mut self,
        ports: Entity<crate::ports::Workspace>,
        cx: &mut Context<Self>,
    ) {
        if self
            .ports
            .as_ref()
            .is_some_and(|(current, _)| *current == ports)
        {
            return;
        }
        let observer = cx.observe(&ports, |_, _, cx| cx.notify());
        self.ports = Some((ports, observer));
        cx.notify();
    }

    pub(crate) fn stop_background(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session() else {
            return;
        };
        let client = self.binding.client.clone();
        let request = client.prepare(Command::StopCommands { session });
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        // Explicit pane closure stops its commands; switching or unmounting does not.
        cx.spawn_in(window, async move |_, cx| {
            let message = match job.await {
                Ok(Ok(Output::Commands(items)))
                    if items.iter().all(|info| {
                        matches!(
                            info.status,
                            CommandStatus::Finished(
                                Outcome::Cancelled | Outcome::Exited(_) | Outcome::Signal(_)
                            )
                        )
                    }) =>
                {
                    return;
                }
                Ok(Ok(Output::Commands(_))) => "background_stop_unknown",
                Ok(Err(error)) if error.code == sailry_protocol::ErrorCode::OutcomeUnknown => {
                    "background_stop_unknown"
                }
                _ => "background_stop_failed",
            };
            let _ = cx.update(|window, cx| {
                crate::feedback::toast(
                    window,
                    tr(message),
                    gpui_kit::component::notification::Notification::error(tr(message)),
                    cx,
                )
            });
        })
        .detach();
    }

    pub(super) fn command_activity(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.commands.update(cx, |panel, cx| panel.sync(self, cx));
        let panel = self.commands.read(cx);
        let count = panel.state.items.iter().filter(|info| active(info)).count();
        if count == 0 {
            return None;
        }
        let services = self
            .ports
            .as_ref()
            .filter(|(ports, _)| ports.read(cx).node() == self.binding.client.target())
            .map(|(ports, _)| {
                let sources = panel
                    .state
                    .items
                    .iter()
                    .filter(|info| active(info))
                    .flat_map(|info| {
                        info.services
                            .iter()
                            .map(move |service| crate::ports::Source {
                                session: info.session,
                                command: info.id,
                                service: service.clone(),
                            })
                    })
                    .collect();
                crate::ports::Workspace::service_buttons(ports, sources, panel.state.connected, cx)
            })
            .unwrap_or_default();
        let panel = self.commands.clone();
        Some(
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(
                    Popover::new("background-popover")
                        .anchor(Anchor::BottomLeft)
                        .bottom_2()
                        .p_2()
                        .trigger(
                            Button::new("background-commands")
                                .outline()
                                .small()
                                .rounded_full()
                                .px_3()
                                .font_normal()
                                .text_color(cx.theme().muted_foreground)
                                .icon(IconName::SquareTerminal)
                                .debug_selector(|| "composer-background".into())
                                .label(
                                    rust_i18n::t!("background_count", count = count).to_string(),
                                ),
                        )
                        .content(move |_, _, _| panel.clone()),
                )
                .children(services)
                .into_any_element(),
        )
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = *self.selected.borrow();
        let ids: Vec<_> = self
            .state
            .items
            .iter()
            .filter(|info| active(info))
            .map(|info| info.id)
            .collect();
        let mut commands = Accordion::new("background-accordion")
            .bordered(false)
            .w_full()
            .h_auto()
            .flex_shrink_0();
        for info in self.state.items.iter().filter(|info| active(info)) {
            let title = self.row(info, cx);
            let output = self
                .state
                .output
                .as_ref()
                .filter(|output| Some(info.id) == selected && output.info.id == info.id);
            commands = commands.item(|item| {
                item.title(title)
                    .open(selected == Some(info.id))
                    .bg(cx.theme().transparent)
                    .title_style(
                        StyleRefinement::default()
                            .min_w_0()
                            .font_normal()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .content_style(StyleRefinement::default().pb_0())
                    .when_some(output, |item, output| {
                        let text = [&output.stdout.text, &output.stderr.text]
                            .into_iter()
                            .filter(|text| !text.is_empty())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        item.child(crate::conversation::surface::scroll(
                            "background-output".into(),
                            div()
                                .debug_selector(|| "background-output-content".into())
                                .font_family(cx.theme().mono_font_family.clone())
                                .line_height(px(crate::conversation::surface::LINE_HEIGHT))
                                .text_color(cx.theme().muted_foreground)
                                .child(gpui_kit::base::SelectableText::new(
                                    format!("background-output-{}", info.id),
                                    text,
                                )),
                        ))
                    })
            });
        }
        v_flex()
            .debug_selector(|| "background-list".into())
            .w(px(480.).min(window.viewport_size().width - px(48.)))
            .min_w_0()
            .gap_2()
            .text_sm()
            .child(
                div()
                    .id("background-rows")
                    .max_h(px(320.).min(window.viewport_size().height * 0.5))
                    .overflow_y_scrollbar()
                    .child(commands.on_toggle_click(cx.listener(
                        move |panel, open: &[usize], _, cx| {
                            let selected = open.first().and_then(|index| ids.get(*index)).copied();
                            panel.state.output = None;
                            panel.selected.send_replace(selected);
                            cx.notify();
                        },
                    ))),
            )
            .when(self.state.error.is_some(), |panel| {
                panel.child(
                    div()
                        .text_color(cx.theme().danger)
                        .child(tr("background_unavailable")),
                )
            })
    }
}

#[cfg(test)]
mod tests;
