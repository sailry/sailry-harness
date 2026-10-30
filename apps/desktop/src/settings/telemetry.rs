//! Desktop consent always uses its local profile, even while a remote Node is selected.
use super::{
    Workspace,
    group::{Group, Row},
    providers::Binding,
};
use crate::{backend::Services, tr};
use gpui_kit::{
    component::{button::Button, notification::Notification, switch::Switch, *},
    *,
};
use sailry_protocol::{
    Command, ErrorCode, Output,
    telemetry::{Settings, Update},
};
use std::sync::Arc;

impl Workspace {
    pub(super) fn telemetry(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Panel> {
        self.telemetry
            .get_or_insert_with(|| {
                let binding = cx.try_global::<Services>().map(bind);
                cx.new(|cx| {
                    let mut panel = Panel::new(binding);
                    panel.submit(Command::ReadTelemetrySettings, window, cx);
                    panel
                })
            })
            .clone()
    }
}

fn bind(services: &Services) -> Binding {
    Binding {
        client: Arc::new(sailry_client::Client::new(services.local.clone())),
        runtime: services.runtime.clone(),
        label: tr("app"),
    }
}

pub(super) struct Panel {
    binding: Option<Binding>,
    saved: Option<Settings>,
    pending: bool,
}

impl Panel {
    fn new(binding: Option<Binding>) -> Self {
        Self {
            saved: binding.is_none().then(Settings::default),
            binding,
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
        let success = match command {
            Command::SaveTelemetrySettings(_) => Some("telemetry_saved"),
            Command::ResetTelemetryIdentity { .. } => Some("telemetry_reset"),
            Command::SendDiagnostics => Some("telemetry_diagnostics_sent"),
            _ => None,
        };
        let diagnostics = matches!(command, Command::SendDiagnostics);
        let writing = command.durable();
        self.pending = true;
        let request = binding.client.prepare(command);
        let runtime = binding.runtime.clone();
        let job = runtime.spawn(async move { super::requests::execute(&binding, request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |this, window, cx| {
                this.pending = false;
                let key = match result {
                    Ok((Ok(Output::TelemetrySettings(settings)), _)) if !diagnostics => {
                        this.saved = Some(settings);
                        None
                    }
                    Ok((Ok(Output::DiagnosticsSent), _)) if diagnostics => None,
                    Ok((Err(error), true)) if error.code == ErrorCode::RevisionConflict => {
                        Some("telemetry_conflict")
                    }
                    Ok((_, false)) | Err(_) if writing => Some("telemetry_unknown"),
                    _ if diagnostics => Some("telemetry_diagnostics_failed"),
                    _ => Some("telemetry_failed"),
                };
                if let Some(key) = key {
                    let owner = cx.weak_entity();
                    crate::feedback::toast(
                        window,
                        "telemetry-failed".into(),
                        Notification::error(tr(key)).action(move |_, _, _| {
                            let owner = owner.clone();
                            Button::new("reload-telemetry")
                                .label(tr("telemetry_reload"))
                                .on_click(move |_, window, cx| {
                                    _ = owner.update(cx, |this, cx| {
                                        this.submit(Command::ReadTelemetrySettings, window, cx)
                                    });
                                })
                        }),
                        cx,
                    );
                } else if let Some(key) = success {
                    crate::feedback::toast(
                        window,
                        "telemetry-saved".into(),
                        Notification::success(tr(key)),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set(
        &mut self,
        errors: Option<bool>,
        metrics: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(settings) = self.saved.as_ref() else {
            return;
        };
        let update = Update {
            revision: settings.revision,
            errors: errors.unwrap_or(settings.errors),
            metrics: metrics.unwrap_or(settings.metrics),
        };
        if self.binding.is_none() {
            let settings = self.saved.as_mut().unwrap();
            settings.errors = update.errors;
            settings.metrics = update.metrics;
            cx.notify();
        } else {
            self.submit(Command::SaveTelemetrySettings(update), window, cx);
        }
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.pending || self.saved.is_none();
        Group::new("telemetry_title")
            .child(
                Row::new(
                    "telemetry_errors",
                    Switch::new("telemetry-errors")
                        .accessibility_label(tr("telemetry_errors"))
                        .checked(self.saved.as_ref().is_none_or(|settings| settings.errors))
                        .disabled(disabled)
                        .on_change(cx.listener(|this, enabled, window, cx| {
                            this.set(Some(*enabled), None, window, cx)
                        })),
                )
                .description("telemetry_errors_description"),
            )
            .child(
                Row::new(
                    "telemetry_metrics",
                    Switch::new("telemetry-metrics")
                        .accessibility_label(tr("telemetry_metrics"))
                        .checked(self.saved.as_ref().is_none_or(|settings| settings.metrics))
                        .disabled(disabled)
                        .on_change(cx.listener(|this, enabled, window, cx| {
                            this.set(None, Some(*enabled), window, cx)
                        })),
                )
                .description("telemetry_metrics_description"),
            )
            .child(Row::new(
                "telemetry_identity",
                Button::new("reset-telemetry-identity")
                    .label(tr("telemetry_reset_action"))
                    .outline()
                    .small()
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let Some(settings) = this.saved.as_ref() else {
                            return;
                        };
                        this.submit(
                            Command::ResetTelemetryIdentity {
                                expected_revision: settings.revision,
                            },
                            window,
                            cx,
                        );
                    })),
            ))
            .child(
                div()
                    .py_3()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("telemetry_excludes")),
            )
    }
}

#[cfg(test)]
mod tests;
