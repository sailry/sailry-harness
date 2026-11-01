//! Live child concurrency belongs to the selected execution Node, not a team capture.
use super::{Binding, Workspace};
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
        notification::Notification,
        *,
    },
    *,
};
use sailry_protocol::{Command, ErrorCode, NodeId, Output, Request, team::Settings};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    hosts: BTreeMap<NodeId, Entity<Host>>,
    preview: Option<Entity<Host>>,
}

impl Workspace {
    pub(super) fn team_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Host> {
        let state = &mut self.teams.settings;
        if let Some(live) = &self.provider_link {
            let binding = live.binding.clone();
            let host = state
                .hosts
                .entry(binding.client.target())
                .or_insert_with(|| cx.new(|cx| Host::new(Some(binding.clone()), window, cx)))
                .clone();
            host.update(cx, |host, cx| {
                host.binding = Some(binding);
                let connected = live.connected && live.snapshot.is_some();
                if host.connected != connected {
                    host.connected = connected;
                    host.observed = None;
                    cx.notify();
                }
                if let Some(snapshot) = live.snapshot.as_ref().filter(|_| connected) {
                    let revision = snapshot.team_settings_revision;
                    if !host.pending
                        && host.request.is_none()
                        && !host.dirty(cx)
                        && host.observed != Some(revision)
                    {
                        host.observed = Some(revision);
                        if host
                            .saved
                            .as_ref()
                            .is_none_or(|saved| saved.revision < revision)
                        {
                            host.load(window, cx);
                        }
                    }
                }
            });
            host
        } else {
            state
                .preview
                .get_or_insert_with(|| cx.new(|cx| Host::new(None, window, cx)))
                .clone()
        }
    }
}

pub(super) struct Host {
    binding: Option<Binding>,
    saved: Option<Settings>,
    observed: Option<u64>,
    connected: bool,
    pending: bool,
    request: Option<Request>,
    input: Entity<InputState>,
}

impl Host {
    fn new(binding: Option<Binding>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = binding.is_none().then(Settings::default);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    saved
                        .as_ref()
                        .map(|saved| saved.child_concurrency.to_string())
                        .unwrap_or_default(),
                )
                .validate(|text, _| text.bytes().all(|byte| byte.is_ascii_digit()))
        });
        // Kit emits step intent; integral settings must never round through f64.
        input.update(cx, |input, cx| input.set_step(None, window, cx));
        cx.subscribe_in(
            &input,
            window,
            |host, input, event: &NumberInputEvent, window, cx| {
                if host.locked() {
                    return;
                }
                let NumberInputEvent::Step(action) = event;
                if let Some(value) = stepped(&input.read(cx).value(), *action) {
                    input.update(cx, |input, cx| {
                        input.set_value(value.to_string(), window, cx)
                    });
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &input,
            window,
            |host, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => host.save(window, cx),
                _ => {}
            },
        )
        .detach();
        Self {
            binding,
            saved,
            observed: None,
            connected: false,
            pending: false,
            request: None,
            input,
        }
    }

    fn locked(&self) -> bool {
        !self.connected || self.pending || self.request.is_some() || self.saved.is_none()
    }

    fn dirty(&self, cx: &App) -> bool {
        self.saved.as_ref().is_some_and(|saved| {
            value(&self.input.read(cx).value()) != Some(saved.child_concurrency)
        })
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.connected || self.request.is_some() {
            return;
        }
        let Some(binding) = &self.binding else {
            return;
        };
        self.execute(
            binding.client.prepare(Command::ReadTeamSettings),
            window,
            cx,
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.connected {
            return;
        }
        if let Some(request) = self.request.clone() {
            self.execute(request, window, cx);
            return;
        }
        let (Some(saved), Some(binding)) = (&self.saved, &self.binding) else {
            return;
        };
        let Some(child_concurrency) = value(&self.input.read(cx).value()) else {
            self.fail("teams_concurrency_invalid", window, cx);
            return;
        };
        if saved.child_concurrency == child_concurrency {
            return;
        }
        let settings = Settings {
            revision: saved.revision,
            child_concurrency,
        };
        let request = binding.client.prepare(Command::SaveTeamSettings(settings));
        self.request = Some(request.clone());
        self.execute(request, window, cx);
    }

    fn execute(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let Some(binding) = self.binding.clone() else {
            return;
        };
        let writing = request.command.durable();
        let preserve = self.dirty(cx);
        self.pending = true;
        let runtime = binding.runtime.clone();
        let job = runtime
            .spawn(async move { crate::settings::requests::execute(&binding, request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |host, window, cx| {
                host.pending = false;
                match result {
                    Ok((Ok(Output::TeamSettings(settings)), _)) => {
                        host.request = None;
                        if writing || !preserve {
                            host.input.update(cx, |input, cx| {
                                input.set_value(settings.child_concurrency.to_string(), window, cx)
                            });
                        }
                        host.observed = Some(settings.revision);
                        host.saved = Some(settings);
                        if writing {
                            crate::feedback::toast(
                                window,
                                "team-settings-saved".into(),
                                Notification::success(tr("teams_settings_saved")),
                                cx,
                            );
                        }
                    }
                    result => {
                        let key = match result {
                            Ok((Err(error), true))
                                if !matches!(
                                    error.code,
                                    ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                                ) =>
                            {
                                host.request = None;
                                match error.code {
                                    _ if !writing => "teams_settings_load_failed",
                                    ErrorCode::RevisionConflict => "teams_settings_conflict",
                                    ErrorCode::InvalidRequest => "teams_concurrency_invalid",
                                    _ => "teams_settings_failed",
                                }
                            }
                            _ if writing => "teams_settings_unknown",
                            _ => "teams_settings_load_failed",
                        };
                        host.fail(key, window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn fail(&self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.weak_entity();
        crate::feedback::toast(
            window,
            "team-settings-failed".into(),
            Notification::error(tr(key)).action(move |_, _, _| {
                let owner = owner.clone();
                Button::new("team-settings-retry")
                    .label(tr("teams_settings_retry"))
                    .on_click(move |_, window, cx| {
                        _ = owner.update(cx, |host, cx| {
                            if host.request.is_some() {
                                host.save(window, cx);
                            } else {
                                host.load(window, cx);
                            }
                        });
                    })
            }),
            cx,
        );
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Group::new("teams_concurrency").heading(false).child(
            Row::new(
                "teams_concurrency",
                h_flex()
                    .gap_2()
                    .child(
                        NumberInput::new(&self.input)
                            .w_40()
                            .small()
                            .accessibility_label(tr("teams_concurrency"))
                            .disabled(self.locked()),
                    )
                    .child(
                        Button::new("team-settings-save")
                            .small()
                            .primary()
                            .label(tr(if self.request.is_some() {
                                "teams_settings_retry"
                            } else {
                                "settings_save"
                            }))
                            .disabled(
                                !self.connected
                                    || self.pending
                                    || self.saved.is_none()
                                    || (self.request.is_none() && !self.dirty(cx)),
                            )
                            .on_click(cx.listener(|host, _, window, cx| host.save(window, cx))),
                    ),
            )
            .description("teams_concurrency_description"),
        )
    }
}

fn value(text: &str) -> Option<u64> {
    text.parse().ok().filter(|value| *value > 0)
}

fn stepped(text: &str, action: StepAction) -> Option<u64> {
    let current = value(text)?;
    match action {
        StepAction::Increment => current.checked_add(1),
        StepAction::Decrement => current.checked_sub(1).filter(|value| *value > 0),
    }
}

#[cfg(test)]
mod tests {
    use super::{StepAction, stepped, value};

    #[test]
    fn keeps_exact_positive_integers() {
        assert_eq!(value("18446744073709551615"), Some(u64::MAX));
        for invalid in ["", "0", "-1", "1.5", "18446744073709551616"] {
            assert_eq!(value(invalid), None);
        }
    }

    #[test]
    fn steps_without_rounding_or_overflow() {
        assert_eq!(
            stepped("9007199254740993", StepAction::Increment),
            Some(9007199254740994)
        );
        assert_eq!(stepped("18446744073709551615", StepAction::Increment), None);
        assert_eq!(stepped("1", StepAction::Decrement), None);
        assert_eq!(stepped("8", StepAction::Decrement), Some(7));
    }
}
