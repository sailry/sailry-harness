//! The selected execution Node owns this policy, including remote settings.
use super::{Workspace, group::Row, providers::Binding};
use crate::tr;
use gpui_kit::{
    component::{button::Button, notification::Notification, switch::Switch, *},
    *,
};
use sailry_protocol::{Command, ErrorCode, NodeId, Output, power::Settings};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    hosts: BTreeMap<NodeId, Entity<Host>>,
    preview: Option<Entity<Host>>,
}

impl Workspace {
    pub(super) fn power(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Host> {
        if let Some(live) = &self.provider_link {
            let binding = live.binding.clone();
            let host = self
                .power
                .hosts
                .entry(binding.client.target())
                .or_insert_with(|| cx.new(|_| Host::new(Some(binding.clone()))))
                .clone();
            host.update(cx, |host, cx| {
                host.binding = Some(binding);
                if !live.connected {
                    host.observed = None;
                    host.connected = false;
                } else {
                    let reconnected = !host.connected;
                    host.connected = true;
                    if !host.pending && host.observed != Some(live.power_revision) {
                        host.observed = Some(live.power_revision);
                        if reconnected
                            || host.saved.as_ref().map(|saved| saved.revision)
                                != Some(live.power_revision)
                        {
                            host.submit(Command::ReadPowerSettings, window, cx);
                        }
                    }
                }
            });
            host
        } else {
            self.power
                .preview
                .get_or_insert_with(|| cx.new(|_| Host::new(None)))
                .clone()
        }
    }
}

pub(super) struct Host {
    binding: Option<Binding>,
    saved: Option<Settings>,
    observed: Option<u64>,
    connected: bool,
    draft: bool,
    pending: bool,
}

impl Host {
    fn new(binding: Option<Binding>) -> Self {
        let saved = binding.is_none().then(Settings::default);
        Self {
            binding,
            saved,
            observed: None,
            connected: false,
            draft: true,
            pending: false,
        }
    }

    fn submit(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(binding) = self.binding.clone() else {
            return;
        };
        let writing = command.durable();
        self.pending = true;
        let request = binding.client.prepare(command);
        let runtime = binding.runtime.clone();
        let job = runtime.spawn(async move {
            let (result, known) = super::requests::execute(&binding, request).await;
            // A durable replay acknowledges the original request, not today's policy.
            let result = if writing && result.is_ok() {
                binding
                    .client
                    .execute(binding.client.prepare(Command::ReadPowerSettings))
                    .await
            } else {
                result
            };
            (result, known)
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |this, window, cx| {
                this.pending = false;
                match result {
                    Ok((Ok(Output::PowerSettings(settings)), _)) => {
                        this.draft = settings.prevent_sleep;
                        this.saved = Some(settings);
                        if writing {
                            crate::feedback::toast(
                                window,
                                "power-settings-saved".into(),
                                Notification::success(tr("power_settings_saved")),
                                cx,
                            );
                        }
                    }
                    result => {
                        let key = match result {
                            Ok((Err(error), true)) if error.code == ErrorCode::RevisionConflict => {
                                "power_settings_conflict"
                            }
                            Ok((_, false)) if writing => "power_settings_unknown",
                            Err(_) if writing => "power_settings_unknown",
                            _ => "power_settings_failed",
                        };
                        let owner = cx.weak_entity();
                        crate::feedback::toast(
                            window,
                            "power-settings-failed".into(),
                            Notification::error(tr(key)).action(move |_, _, _| {
                                let owner = owner.clone();
                                Button::new("reload-power-settings")
                                    .label(tr("power_settings_reload"))
                                    .on_click(move |_, window, cx| {
                                        _ = owner.update(cx, |this, cx| {
                                            this.submit(Command::ReadPowerSettings, window, cx)
                                        });
                                    })
                            }),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Row::new(
            "power_prevent_sleep",
            Switch::new("prevent-sleep")
                .accessibility_label(tr("power_prevent_sleep"))
                .checked(self.draft)
                .disabled(
                    self.pending
                        || self.saved.is_none()
                        || self.binding.is_some() && !self.connected,
                )
                .on_click(cx.listener(|this, enabled, window, cx| {
                    let Some(mut settings) = this.saved.clone() else {
                        return;
                    };
                    settings.prevent_sleep = *enabled;
                    this.draft = *enabled;
                    if this.binding.is_none() {
                        this.saved = Some(settings);
                        cx.notify();
                    } else {
                        this.submit(Command::SavePowerSettings(settings), window, cx);
                    }
                })),
        )
        .description("power_prevent_sleep_description")
    }
}
