//! CLI account linking keeps credentials in the execution Node's GitHub CLI storage.
use super::*;

pub(super) struct Login {
    status: Option<sailry_protocol::plugin::github::Status>,
    reading: bool,
    pub(super) pending: bool,
    pub(super) request: Option<Request>,
    pub(super) stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Login {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Editor {
    pub(super) fn read_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .info
            .as_ref()
            .and_then(|info| info.extension.as_ref())
            .is_some_and(|extension| {
                extension.credential_source
                    == Some(sailry_protocol::plugin::CredentialSource::GithubCli)
            })
        {
            self.github = None;
            return;
        }
        let stop = CancellationToken::new();
        self.github = Some(Login {
            status: None,
            reading: true,
            pending: false,
            request: None,
            stop: stop.clone(),
            task: None,
        });
        let binding = self.binding.clone();
        let request = binding.client.prepare(Command::ReadGithubLogin {
            package: self.expected.reference(),
        });
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = binding.client.execute(request) => Some(result),
            }
        });
        let stop = self.github.as_ref().unwrap().stop.clone();
        let task = cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |editor, _, cx| {
                if editor.closed || stop.is_cancelled() {
                    return;
                }
                let Some(login) = &mut editor.github else {
                    return;
                };
                login.reading = false;
                match result {
                    Ok(Some(Ok(Output::GithubLogin(status)))) => login.status = Some(status),
                    Ok(Some(Err(error))) => editor.failure(error),
                    _ => editor.error = Some("github_login_failed"),
                }
                cx.notify();
            });
        });
        self.github.as_mut().unwrap().task = Some(task);
    }

    fn import_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.loading || self.pending || self.request.is_some() {
            return;
        }
        let Some(login) = &mut self.github else {
            return;
        };
        if login.pending || login.reading {
            return;
        }
        if login.request.is_none() {
            if !login
                .status
                .as_ref()
                .is_some_and(|status| status.authenticated)
            {
                return;
            }
            login.request = Some(self.binding.client.prepare(Command::ImportGithubLogin {
                package: self.expected.reference(),
            }));
        }
        let request = login.request.clone().unwrap();
        login.pending = true;
        self.error = None;
        self.detail = None;
        self.lock(true, cx);
        let binding = self.binding.clone();
        let job = binding.runtime.spawn(async move {
            let result = binding.client.execute(request.clone()).await;
            if result.is_err() {
                match binding.client.outcome(&request).await {
                    Ok(sailry_protocol::RequestOutcome::Completed(result)) => (*result, true),
                    _ => (result, false),
                }
            } else {
                (result, true)
            }
        });
        let task = cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |editor, window, cx| {
                let Some(login) = &mut editor.github else {
                    return;
                };
                login.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok((Ok(Output::PluginSettings(_)), _)) => {
                        login.request = None;
                        editor.lock(false, cx);
                        crate::feedback::toast(
                            window,
                            tr("github_login_imported"),
                            notification::Notification::success(tr("github_login_imported")),
                            cx,
                        );
                        editor.load(true, window, cx);
                    }
                    Ok((Err(error), confirmed)) => {
                        if confirmed && error.code != ErrorCode::OutcomeUnknown {
                            login.request = None;
                            editor.lock(false, cx);
                        }
                        if confirmed {
                            editor.failure(error);
                        } else {
                            editor.error = Some("plugins_unknown");
                        }
                    }
                    _ => editor.error = Some("plugins_unknown"),
                }
                cx.notify();
            });
        });
        self.github.as_mut().unwrap().task = Some(task);
        cx.notify();
    }

    pub(super) fn github_row(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let login = self.github.as_ref()?;
        let status = login.status.as_ref();
        let label = if login.request.is_some() {
            "plugins_retry"
        } else {
            "github_login_import"
        };
        let state = if login.reading {
            "plugins_loading"
        } else if status.is_none_or(|status| !status.available) {
            "github_login_unavailable"
        } else if status.is_some_and(|status| !status.authenticated) {
            "github_login_signed_out"
        } else if status.is_some_and(|status| status.bound) {
            "github_login_linked"
        } else {
            "github_login_ready"
        };
        Some(
            h_flex()
                .id("github-login-card")
                .debug_selector(|| "github-login-card".into())
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .p_3()
                .gap_2()
                .items_center()
                .justify_between()
                .child(Icon::new(IconName::Github))
                .child(
                    v_flex().gap_1().child(tr("github_login_title")).child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                if let Some(account) =
                                    status.and_then(|status| status.account.as_ref())
                                {
                                    format!("{} · {}", tr(state), account).into()
                                } else {
                                    tr(state)
                                },
                            ),
                    ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("github-login-check")
                                .ghost()
                                .icon(IconName::RotateCw)
                                .debug_selector(|| "github-login-check".into())
                                .accessibility_label(tr("plugins_settings_reload"))
                                .tooltip(tr("plugins_settings_reload"))
                                .disabled(
                                    login.pending
                                        || login.reading
                                        || login.request.is_some()
                                        || self.loading
                                        || self.pending
                                        || self.request.is_some(),
                                )
                                .on_click(cx.listener(|editor, _, window, cx| {
                                    editor.read_github(window, cx)
                                })),
                        )
                        .child(
                            Button::new("github-login-import")
                                .ghost()
                                .icon(Icon::empty().path("icons/download.svg"))
                                .tooltip(tr(label))
                                .accessibility_label(tr(label))
                                .debug_selector(|| "github-login-import".into())
                                .disabled(
                                    login.pending
                                        || login.reading
                                        || self.loading
                                        || self.pending
                                        || self.request.is_some()
                                        || (login.request.is_none()
                                            && status.is_none_or(|status| !status.authenticated)),
                                )
                                .on_click(cx.listener(|editor, _, window, cx| {
                                    editor.import_github(window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "github/tests.rs"]
mod tests;
