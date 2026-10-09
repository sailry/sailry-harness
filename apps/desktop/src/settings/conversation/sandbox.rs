use crate::{
    settings::{
        group::{Group, Row},
        providers::Binding,
    },
    tr,
};
use gpui_kit::{
    component::{button::Button, switch::Switch, *},
    *,
};
use sailry_protocol::{Command, ErrorCode, Output, sandbox::Settings};
#[cfg(test)]
mod tests;

pub(super) struct Host {
    binding: Option<Binding>,
    pub(super) saved: Option<Settings>,
    observed: Option<u64>,
    pending: bool,
    error: Option<&'static str>,
}

impl Host {
    pub(super) fn new(
        binding: Option<Binding>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, _| view.error.into_iter().collect(),
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(tr(key)).action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("sandbox-reload")
                        .label(tr("sandbox_settings_reload"))
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |host, cx| host.load(window, cx));
                        }))
                })
            },
        );
        Self {
            saved: binding.is_none().then(Settings::default),
            binding,
            observed: None,
            pending: false,
            error: None,
        }
    }

    pub(super) fn accept(&mut self, revision: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.observed == Some(revision) {
            return;
        }
        self.observed = Some(revision);
        if self
            .saved
            .as_ref()
            .is_none_or(|settings| settings.revision != revision)
        {
            self.load(window, cx);
        }
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit(Command::ReadSandboxSettings, window, cx);
    }

    fn save(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut settings) = self.saved.clone() else {
            return;
        };
        settings.enabled = enabled;
        if self.binding.is_none() {
            self.saved = Some(settings);
            cx.notify();
            return;
        }
        self.submit(Command::SaveSandboxSettings(settings), window, cx);
    }

    fn submit(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(binding) = self.binding.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let writing = command.durable();
        let request = binding.client.prepare(command);
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |this, window, cx| {
                this.pending = false;
                match result {
                    Ok(Ok(Output::SandboxSettings(settings))) => {
                        this.saved = Some(settings);
                        if writing {
                            window.push_notification(
                                notification::Notification::success(tr("sandbox_settings_saved")),
                                cx,
                            );
                        }
                    }
                    Ok(Err(error)) => {
                        this.error = Some(match error.code {
                            ErrorCode::RevisionConflict => "sandbox_settings_conflict",
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable if writing => {
                                "sandbox_settings_unknown"
                            }
                            _ => "sandbox_settings_failed",
                        });
                    }
                    _ => {
                        this.error = Some(if writing {
                            "sandbox_settings_unknown"
                        } else {
                            "sandbox_settings_failed"
                        })
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
        Group::new("settings_execution").child(
            Row::new(
                "settings_command_sandbox",
                div().debug_selector(|| "command-sandbox".into()).child(
                    Switch::new("command-sandbox")
                        .checked(self.saved.as_ref().is_none_or(|settings| settings.enabled))
                        .disabled(
                            self.pending
                                || self.saved.is_none()
                                || self.error == Some("sandbox_settings_unknown"),
                        )
                        .accessibility_label(tr("settings_command_sandbox"))
                        .on_click(
                            cx.listener(|host, value, window, cx| host.save(*value, window, cx)),
                        ),
                ),
            )
            .description("settings_command_sandbox_description"),
        )
    }
}
