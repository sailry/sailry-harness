use super::*;
use sailry_protocol::{Command, Output, conversation::Provider};

#[cfg(test)]
mod tests;

impl Editor {
    pub(super) fn connection(
        &self,
        cx: &App,
    ) -> Result<(sailry_protocol::conversation::ModelApi, String), &'static str> {
        let api = self.preset.api();
        let endpoint = self.endpoint.read(cx).value().trim().to_owned();
        let endpoint = match self.preset {
            Preset::OpenAi => "https://api.openai.com/v1".into(),
            Preset::OpenCodeGo => "https://opencode.ai/zen/go/v1".into(),
            Preset::OpenCodeZen => "https://opencode.ai/zen/v1".into(),
            Preset::ChatGpt => "https://chatgpt.com/backend-api/codex".into(),
            Preset::Copilot | Preset::CopilotResponses => "https://api.githubcopilot.com".into(),
            Preset::Hosted(vendor) => vendor.endpoint().into(),
            Preset::Bedrock | Preset::BedrockIam | Preset::Vertex | Preset::VertexAdc
                if endpoint.is_empty() =>
            {
                self.cloud.endpoint(self.preset, cx)?
            }
            Preset::Anthropic if endpoint.is_empty() => "https://api.anthropic.com".into(),
            Preset::Gemini if endpoint.is_empty() => {
                "https://generativelanguage.googleapis.com/v1beta".into()
            }
            _ => endpoint,
        };
        if endpoint.is_empty() {
            return Err("provider_endpoint_required");
        }
        Ok((api, endpoint))
    }

    pub(super) fn save_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        if self.preset.key_auth()
            && !self.key_ready
            && self.credential.read(cx).value().trim().is_empty()
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
            options: channel.options,
            oauth: channel.oauth,
            id: self.provider_id,
            revision: self
                .provider
                .as_ref()
                .map_or(0, |provider| provider.revision),
            name: channel.name,
            api,
            authentication: self.preset.authentication(),
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
                    Ok(Ok(Output::Provider(_))) => {
                        editor.request = None;
                        editor.owner.update(cx, |owner, cx| {
                            if owner.provider_link.as_ref().is_some_and(|live| {
                                live.binding.client.target()
                                    == editor.binding.as_ref().unwrap().client.target()
                            }) {
                                owner.providers.category = editor.preset.category();
                                cx.notify();
                            }
                        });
                        editor.close(window, cx);
                        window.close_dialog(cx);
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
}
