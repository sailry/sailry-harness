use super::*;

pub(in crate::settings::providers) fn open(
    owner: Entity<Workspace>,
    id: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((binding, provider)) = binding(&owner, id, cx) else {
        return;
    };
    let view = cx.new(|_| Revoke {
        binding,
        provider,
        request: None,
        pending: false,
        done: false,
        error: None,
    });
    view.update(cx, |view, cx| view.prompt(window, cx));
}

struct Revoke {
    binding: Binding,
    provider: Provider,
    request: Option<Request>,
    pending: bool,
    done: bool,
    error: Option<&'static str>,
}

impl Revoke {
    fn prompt(&self, window: &mut Window, cx: &mut Context<Self>) {
        let detail = format!("{}\n{}", self.provider.name, tr("provider_logout_effect"));
        if let Some(error) = self.error {
            use gpui_kit::component::notification::Notification;
            crate::feedback::toast(
                window,
                tr(error),
                Notification::error(tr(error)).id1::<Self>(("error", cx.entity_id())),
                cx,
            );
        }
        if self.error == Some("provider_login_changed") {
            crate::feedback::error(&tr("provider_disconnect"), &detail, window, cx);
            return;
        }
        let view = cx.entity();
        crate::prompts::confirm(
            &tr("provider_disconnect"),
            &detail,
            tr(if self.error.is_some() {
                "provider_login_retry"
            } else {
                "provider_disconnect"
            }),
            window,
            cx,
            move |window, cx| view.update(cx, |view, cx| view.submit(window, cx)),
        );
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.done {
            return;
        }
        let Some(reference) = self.provider.credential.clone() else {
            self.done = true;
            cx.notify();
            return;
        };
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let provider = self.provider.id;
        let request = self.request.clone();
        let job = self.binding.runtime.spawn(async move {
            if let Some(request) = request {
                return Ok(Some(request));
            }
            let Output::Credentials(credentials) = client
                .execute(client.prepare(Command::ListCredentials))
                .await?
            else {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "credential metadata response expected",
                ));
            };
            let credential = credentials
                .into_iter()
                .find(|credential| credential.id == reference.id && credential.provider == provider)
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotFound, "provider credential is unavailable")
                })?;
            if credential.revoked {
                return Ok(None);
            }
            Ok(Some(client.prepare(Command::RevokeCredential {
                id: credential.id,
                expected_revision: credential.revision,
            })))
        });
        let owner = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |view, window, cx| {
                match result {
                    Ok(Ok(Some(request))) => {
                        // Retain the exact admitted request when its result is uncertain.
                        view.request = Some(request);
                        view.execute(window, cx);
                    }
                    Ok(Ok(None)) => {
                        view.pending = false;
                        view.done = true;
                    }
                    _ => {
                        view.pending = false;
                        view.error = Some("provider_logout_failed");
                    }
                }
                if view.error.is_some() {
                    view.prompt(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn execute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.binding.client.clone();
        let request = self.request.clone().unwrap();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        let owner = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |view, window, cx| {
                view.pending = false;
                match result {
                    Ok(Ok(Output::Credential(credential))) if credential.revoked => {
                        view.done = true
                    }
                    Ok(Err(Fault {
                        code: ErrorCode::RevisionConflict | ErrorCode::Conflict,
                        ..
                    })) => view.error = Some("provider_login_changed"),
                    _ => view.error = Some("provider_logout_unknown"),
                }
                if view.error.is_some() {
                    view.prompt(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
