use super::*;
use sailry_protocol::{Command, ErrorCode, Output};

impl Panel {
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit(Command::ReadSearchSettings, window, cx);
    }

    pub(super) fn submit(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            self.error = Some("search_target_changed");
            cx.notify();
            return;
        }
        if self.pending {
            return;
        }
        let Some(binding) = self.binding.clone() else {
            if let Command::SaveSearchSettings(update) = command {
                self.apply(update.settings, window, cx);
            }
            return;
        };
        self.pending = true;
        self.error = None;
        let writing = command.durable();
        let testing = matches!(command, Command::TestSearchDriver { .. });
        let request = binding.client.prepare(command);
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok(Ok(Output::SearchSettings(settings))) => {
                        panel.apply(settings, window, cx);
                        if writing {
                            crate::feedback::toast(
                                window,
                                crate::tr("search_saved"),
                                notification::Notification::success(crate::tr("search_saved")),
                                cx,
                            );
                        }
                    }
                    Ok(Ok(Output::SearchPage(_))) if testing => crate::feedback::toast(
                        window,
                        crate::tr("search_test_passed"),
                        notification::Notification::success(crate::tr("search_test_passed")),
                        cx,
                    ),
                    Ok(Err(error)) => panel.error = Some(error_key(error.code, writing, testing)),
                    _ => {
                        panel.error = Some(if writing {
                            "search_save_unknown"
                        } else {
                            "search_request_failed"
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

pub(super) fn error_key(code: ErrorCode, writing: bool, testing: bool) -> &'static str {
    match code {
        ErrorCode::RevisionConflict => "search_settings_conflict",
        ErrorCode::InvalidRequest => "search_settings_invalid",
        ErrorCode::NotConfigured => "search_not_configured",
        ErrorCode::PermissionDenied if testing => "search_test_auth_failed",
        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable if writing => "search_save_unknown",
        _ => "search_request_failed",
    }
}
