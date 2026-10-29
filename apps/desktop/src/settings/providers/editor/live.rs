use super::*;
use sailry_protocol::{Command, Output, conversation::Provider};

impl Editor {
    pub(super) fn connection(
        &self,
        cx: &App,
    ) -> Result<(sailry_protocol::conversation::ModelApi, String), &'static str> {
        connection(
            self.selected_route(),
            self.provider.as_ref(),
            self.preset,
            &self.endpoint.read(cx).value(),
        )
    }

    pub(super) fn save_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        if self.uses_key() && !self.key_ready && self.credential.read(cx).value().trim().is_empty()
        {
            self.report_error("provider_key_failed", window, cx);
            cx.notify();
            return;
        }
        let channel = match self.channel(cx) {
            Ok(channel) => channel,
            Err(error) => {
                self.report_error(error, window, cx);
                cx.notify();
                return;
            }
        };
        let (api, endpoint) = match self.connection(cx) {
            Ok(connection) => connection,
            Err(error) => {
                self.report_error(error, window, cx);
                cx.notify();
                return;
            }
        };
        let binding = self
            .binding
            .clone()
            .expect("live editor is bound to a Node");
        self.probe = None;
        let provider = Provider {
            request_max_retries: self
                .provider
                .as_ref()
                .and_then(|provider| provider.request_max_retries),
            stream_max_retries: self
                .provider
                .as_ref()
                .and_then(|provider| provider.stream_max_retries),
            preset: self.preset_ref(),
            binding: self
                .provider
                .as_ref()
                .filter(|provider| provider.preset.as_ref() == self.preset_ref().as_ref())
                .filter(|provider| {
                    provider.preset.as_ref().is_some_and(|reference| {
                        matches!(
                            &reference.source,
                            sailry_protocol::conversation::presets::Source::Partner
                        )
                    })
                })
                .and_then(|provider| provider.binding.clone()),
            consent: self
                .provider
                .as_ref()
                .filter(|provider| provider.preset.as_ref() == self.preset_ref().as_ref())
                .and_then(|provider| provider.consent.clone()),
            options: channel.options,
            oauth: channel.oauth,
            id: self.provider_id,
            revision: self
                .provider
                .as_ref()
                .map_or(0, |provider| provider.revision),
            name: channel.name,
            api,
            authentication: self.authentication(),
            endpoint,
            enabled: self
                .provider
                .as_ref()
                .is_none_or(|provider| provider.enabled),
            models: channel
                .models
                .iter()
                .map(super::super::live::model)
                .collect(),
            default_model: channel.default_model,
            credential: self
                .provider
                .as_ref()
                .and_then(|provider| provider.credential.clone())
                .filter(|_| channel.credential_configured),
        };
        let command = Command::SaveProvider {
            expected_revision: provider.revision,
            provider,
            secret: self.draft_key(cx),
        };
        let request = self
            .request
            .as_ref()
            .filter(|request| request.command == command)
            .cloned()
            .unwrap_or_else(|| binding.client.prepare(command));
        self.request = Some(request.clone());
        self.pending = true;
        self.error = None;
        let login_binding = binding.clone();
        let login = self.editing.is_none() && self.uses_login() && !self.configured;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::Provider(provider))) => {
                        editor.request = None;
                        editor.provider = Some(provider.clone());
                        editor.owner.update(cx, |owner, cx| {
                            if owner.provider_link.as_ref().is_some_and(|live| {
                                live.binding.client.target()
                                    == editor.binding.as_ref().unwrap().client.target()
                            }) {
                                if owner.providers.category != super::super::data::Selection::All {
                                    owner.providers.category =
                                        super::super::families::for_reference(
                                            provider.preset.as_ref(),
                                            editor.preset,
                                            editor.selected.as_ref(),
                                        );
                                }
                                cx.notify();
                            }
                        });
                        editor.close(window, cx);
                        window.close_dialog(cx);
                        if login {
                            super::super::login::open_for(
                                login_binding,
                                provider,
                                editor.selected.clone(),
                                window,
                                cx,
                            );
                        }
                    }
                    Ok(Err(error)) => {
                        let key = super::super::live::error_key(&error);
                        editor.report_error(key, window, cx)
                    }
                    _ => editor.report_error("provider_save_unknown", window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn connection(
    route: Option<&sailry_protocol::conversation::presets::Route>,
    provider: Option<&Provider>,
    preset: Preset,
    endpoint: &str,
) -> Result<(sailry_protocol::conversation::ModelApi, String), &'static str> {
    let api = route.map_or_else(
        || provider.map_or(preset.api(), |provider| provider.api),
        |route| route.protocol.api(),
    );
    let endpoint = endpoint.trim().to_owned();
    if endpoint.is_empty() {
        return Err("provider_endpoint_required");
    }
    Ok((api, endpoint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::conversation::{ModelApi, presets::Protocol};

    #[test]
    fn preserves_custom_endpoints() {
        for entry in super::super::super::presets::bundled() {
            for route in &entry.preset.routes {
                let kind = super::super::super::presets::kind(&entry, route);
                assert_eq!(
                    connection(Some(route), None, kind, "https://custom.invalid/v1").unwrap(),
                    (route.protocol.api(), "https://custom.invalid/v1".into())
                );
                assert_eq!(
                    connection(None, None, kind, "https://custom.invalid/v1").unwrap(),
                    (kind.api(), "https://custom.invalid/v1".into())
                );
            }
        }
    }

    #[test]
    fn saved_api_survives_missing_declaration() {
        let provider: Provider = serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::ProviderId::new(),"revision":3,"name":"Saved provider",
            "api":"chat_completions","authentication":"api_key","endpoint":"https://saved.invalid/v1",
            "enabled":true,"models":[],"default_model":"","credential":null
        })).unwrap();
        assert_eq!(
            connection(None, Some(&provider), Preset::OpenAi, &provider.endpoint).unwrap(),
            (ModelApi::ChatCompletions, provider.endpoint.clone())
        );
        let mut route = super::super::super::presets::bundled()
            .remove(0)
            .preset
            .routes
            .remove(0);
        route.protocol = Protocol::Anthropic;
        assert_eq!(
            connection(
                Some(&route),
                Some(&provider),
                Preset::OpenAi,
                "https://custom.invalid"
            )
            .unwrap()
            .0,
            ModelApi::Anthropic
        );
    }

    #[test]
    fn empty_input_does_not_restore_defaults() {
        let entry = super::super::super::presets::bundled().remove(0);
        assert_eq!(
            connection(Some(&entry.preset.routes[0]), None, Preset::OpenAi, " "),
            Err("provider_endpoint_required")
        );
    }
}
