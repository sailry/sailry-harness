//! Reconnect saved accounts without applying editor drafts; new accounts are saved first.
use super::*;
use sailry_protocol::{Command, Output, conversation::Provider};

impl Editor {
    pub(super) fn load_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.preset.oauth() || !self.configured {
            self.account_label = None;
            return;
        }
        let Some(binding) = self.binding.clone() else {
            return;
        };
        let Some(reference) = self
            .provider
            .as_ref()
            .and_then(|provider| provider.credential.clone())
        else {
            self.account_label = None;
            return;
        };
        if reference.node != binding.client.target() {
            self.account_label = None;
            return;
        }
        let provider = self.provider_id;
        let client = binding.client.clone();
        let job = binding.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::ListCredentials))
                .await
        });
        self.account_task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                if editor.closed
                    || editor.provider_id != provider
                    || editor
                        .provider
                        .as_ref()
                        .and_then(|provider| provider.credential.as_ref())
                        != Some(&reference)
                {
                    return;
                }
                match result {
                    Ok(Ok(Output::Credentials(credentials))) => {
                        editor.account_label = credentials
                            .into_iter()
                            .find(|credential| {
                                credential.id == reference.id
                                    && credential.provider == provider
                                    && !credential.revoked
                            })
                            .and_then(|credential| credential.account_label)
                            .map(SharedString::from);
                    }
                    Ok(Err(error)) => {
                        editor.report_error(super::super::live::error_key(&error), window, cx)
                    }
                    _ => editor.report_error("provider_save_unknown", window, cx),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn authorizing(&self, cx: &App) -> bool {
        self.authorization
            .as_ref()
            .is_some_and(|login| login.read(cx).active())
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.binding.is_none() || self.pending || self.closed || self.authorizing(cx) {
            return;
        }
        if let Some(provider) = self.provider.clone().filter(|provider| {
            provider.authentication == self.preset.authentication()
                && provider.api == self.preset.api()
        }) {
            self.probe = None;
            self.error = None;
            self.start_authorization(provider, window, cx);
            return;
        }
        self.persist(true, window, cx);
    }

    pub(super) fn start_authorization(
        &mut self,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let binding = self
            .binding
            .clone()
            .expect("authorization has a Node binding");
        let login = super::super::login::embedded(binding, provider, window, cx);
        self._authorization = Some(cx.subscribe_in(
            &login,
            window,
            |editor, _, _: &super::super::login::Event, window, cx| {
                editor.refresh_provider(window, cx);
            },
        ));
        cx.observe(&login, |_, _, cx| cx.notify()).detach();
        self.authorization = Some(login);
        cx.notify();
    }

    fn refresh_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let binding = self
            .binding
            .clone()
            .expect("authorization has a Node binding");
        let provider = self
            .owner
            .read(cx)
            .provider_link
            .as_ref()
            .filter(|live| live.binding.client.target() == binding.client.target())
            .and_then(|live| {
                live.providers.values().find(|provider| {
                    provider.id == self.provider_id
                        && provider.credential.is_some()
                        && provider.revision
                            > self.provider.as_ref().map_or(0, |saved| saved.revision)
                })
            })
            .cloned();
        if let Some(provider) = provider {
            self.accept_provider(provider, window, cx);
            return;
        }
        let client = binding.client.clone();
        let request = client.prepare(Command::ListProviders);
        let job = binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.pending = true;
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::Providers(providers))) => {
                        if let Some(provider) = providers
                            .into_iter()
                            .find(|provider| provider.id == editor.provider_id)
                        {
                            editor.accept_provider(provider, window, cx);
                        } else {
                            editor.report_error("provider_login_changed", window, cx);
                        }
                    }
                    Ok(Err(error)) => {
                        editor.report_error(super::super::live::error_key(&error), window, cx)
                    }
                    _ => editor.report_error("provider_save_unknown", window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn accept_provider(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        if self.models.is_empty() && !self.configured {
            let channel = super::super::live::channel(0, &provider);
            self.models = channel
                .models
                .iter()
                .enumerate()
                .map(|(key, model)| Draft::new(key, model.clone(), window, cx))
                .collect();
            self.default_model = self
                .models
                .iter()
                .position(|draft| draft.id.read(cx).value().as_ref() == provider.default_model)
                .unwrap_or(0);
            self.next_model = self.models.len();
        }
        self.configured = provider.credential.is_some();
        self.provider = Some(provider);
        self.error = None;
        self.load_account(window, cx);
        cx.notify();
    }

    pub(super) fn authorization(&self, cx: &Context<Self>) -> AnyElement {
        let login = self
            .authorization
            .as_ref()
            .filter(|login| !login.read(cx).finished());
        v_flex()
            .id("provider-authorization")
            .debug_selector(|| "provider-authorization".into())
            .w_full()
            .gap_2()
            .when_some(self.account_label.clone(), |body, label| {
                body.child(
                    div()
                        .debug_selector(|| "provider-account".into())
                        .truncate()
                        .child(label),
                )
            })
            .when_some(login, |body, login| {
                body.child(login.clone())
                    .child(login.read(cx).footer(login, cx))
            })
            .when(login.is_none(), |body| {
                body.child(
                    Button::new("provider-connect")
                        .debug_selector(|| "provider-connect".into())
                        .label(tr(if self.configured {
                            "provider_reconnect"
                        } else if self.preset == Preset::ChatGpt {
                            "provider_continue_chatgpt"
                        } else {
                            "provider_connect"
                        }))
                        .disabled(self.pending || self.binding.is_none())
                        .on_click(cx.listener(|editor, _, window, cx| editor.connect(window, cx))),
                )
            })
            .into_any_element()
    }
}
