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
use sailry_protocol::{Command, ErrorCode, Output, conversation::background::Settings};

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
                    Button::new("background-settings-reload")
                        .label(tr("background_settings_reload"))
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
        self.submit(Command::ReadBackgroundSettings, window, cx);
    }

    fn save(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut settings) = self.saved.clone() else {
            return;
        };
        settings.resume = enabled;
        if self.binding.is_none() {
            self.saved = Some(settings);
            cx.notify();
            return;
        }
        self.submit(Command::SaveBackgroundSettings(settings), window, cx);
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
                    Ok(Ok(Output::BackgroundSettings(settings))) => {
                        this.saved = Some(settings);
                        if writing {
                            window.push_notification(
                                notification::Notification::success(tr(
                                    "background_settings_saved",
                                )),
                                cx,
                            );
                        }
                    }
                    Ok(Err(error)) => {
                        this.error = Some(match error.code {
                            ErrorCode::RevisionConflict => "background_settings_conflict",
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable if writing => {
                                "background_settings_unknown"
                            }
                            _ => "background_settings_failed",
                        });
                    }
                    _ => {
                        this.error = Some(if writing {
                            "background_settings_unknown"
                        } else {
                            "background_settings_failed"
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
        Group::new("background_settings_group").child(
            Row::new(
                "background_settings_title",
                div().debug_selector(|| "background-settings".into()).child(
                    Switch::new("background-settings")
                        .checked(self.saved.as_ref().is_none_or(|settings| settings.resume))
                        .disabled(
                            self.pending
                                || self.saved.is_none()
                                || self.error == Some("background_settings_unknown"),
                        )
                        .accessibility_label(tr("background_settings_title"))
                        .on_click(
                            cx.listener(|host, value, window, cx| host.save(*value, window, cx)),
                        ),
                ),
            )
            .description("background_settings_description"),
        )
    }
}
