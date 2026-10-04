use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Output, conversation::discovery};

#[cfg(test)]
mod tests;

pub(super) struct Probe {
    stop: CancellationToken,
    _task: Task<()>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Editor {
    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        self.probe = None;
        self.request = None;
        self.original_key = None;
        self.credential
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn source(&self, cx: &App) -> Result<discovery::Source, &'static str> {
        if self.preset.oauth() {
            let provider = self
                .provider
                .as_ref()
                .filter(|provider| provider.credential.is_some())
                .ok_or("provider_login_required")?;
            return Ok(discovery::Source::Saved {
                provider: provider.id,
                expected_revision: provider.revision,
            });
        }
        let (api, endpoint) = self.connection(cx)?;
        let key = self.credential.read(cx).value();
        if !self.key_ready && key.trim().is_empty() {
            return Err("provider_key_failed");
        }
        if !self.preset.custom_endpoint() && key.trim().is_empty() {
            return Err("provider_key_required");
        }
        Ok(discovery::Source::Draft(discovery::Draft {
            provider: self.provider_id,
            api,
            endpoint,
            credential: self
                .provider
                .as_ref()
                .and_then(|provider| provider.credential.clone())
                .filter(|_| self.configured && !key.trim().is_empty()),
            secret: self.draft_key(cx),
        }))
    }

    pub(super) fn discover_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preset.cloud() {
            self.fill_metadata(window, cx);
            return;
        }
        let source = match self.source(cx) {
            Ok(source) => source,
            Err(error) => {
                self.report_error(error, window, cx);
                cx.notify();
                return;
            }
        };
        self.query(source, window, cx);
    }

    fn fill_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed || self.probe.is_some() {
            return;
        }
        let drafts = self
            .models
            .iter()
            .filter_map(|draft| draft.value(cx).ok().map(|value| (draft.key, value)))
            .collect::<Vec<_>>();
        if drafts.is_empty() {
            self.report_error("provider_model_required", window, cx);
            return;
        }
        let models = drafts
            .iter()
            .map(|(_, model)| discovery::Model {
                id: model.id.clone(),
                context: None,
                output: None,
                capabilities: None,
            })
            .collect::<Vec<_>>();
        let binding = self
            .binding
            .clone()
            .expect("metadata lookup has a Node binding");
        let preset = self.preset;
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = cancelled.cancelled() => None,
                result = super::catalog::read(&binding.client, preset, &models) => Some(result),
            }
        });
        self.probe = Some(Probe {
            stop,
            _task: cx.spawn_in(window, async move |editor, cx| {
                let result = job.await;
                let _ = editor.update_in(cx, |editor, window, cx| {
                    editor.probe = None;
                    if editor.closed || editor.preset != preset {
                        return;
                    }
                    match result {
                        Ok(Some(metadata)) if !metadata.failed => {
                            for (key, snapshot) in drafts {
                                let Some(reference) = metadata.models.get(&snapshot.id) else {
                                    continue;
                                };
                                let Some(draft) = editor.models.iter_mut().find(|draft| {
                                    draft.key == key
                                        && draft.value(cx).ok().as_ref() == Some(&snapshot)
                                }) else {
                                    continue;
                                };
                                draft.fill(None, Some(reference), preset, window, cx);
                            }
                        }
                        _ => editor.report_error("provider_catalog_read_failed", window, cx),
                    }
                    cx.notify();
                });
            }),
        });
        cx.notify();
    }

    fn query(&mut self, source: discovery::Source, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending || self.probe.is_some() {
            return;
        }
        let binding = self.binding.clone().expect("query has a Node binding");
        let preset = self.preset;
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let request = binding
            .client
            .prepare(Command::DiscoverModels(Box::new(source.clone())));
        let job = binding.runtime.spawn(async move {
            let query = async {
                let output = binding.client.execute(request).await?;
                let metadata = if let Output::DiscoveredModels(catalog) = &output {
                    super::catalog::read(&binding.client, preset, &catalog.models).await
                } else {
                    super::catalog::Metadata::default()
                };
                Ok::<_, sailry_protocol::Fault>((output, metadata))
            };
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => None,
                result = query => Some(result),
            }
        });
        self.error = None;
        self.probe = Some(Probe {
            stop,
            _task: cx.spawn_in(window, async move |editor, cx| {
                let result = job.await;
                let _ = editor.update_in(cx, |editor, window, cx| {
                    editor.probe = None;
                    if editor.closed {
                        return;
                    }
                    if editor.source(cx).ok().as_ref() != Some(&source) {
                        editor.report_error("provider_query_changed", window, cx);
                        cx.notify();
                        return;
                    }
                    match result {
                        Ok(Some(Ok((Output::DiscoveredModels(catalog), metadata)))) => {
                            if preset.custom_endpoint() {
                                editor.endpoint.update(cx, |input, cx| {
                                    input.set_value(catalog.endpoint, window, cx)
                                });
                            }
                            for model in catalog.models {
                                let reference = metadata.models.get(&model.id);
                                if let Some(draft) = editor
                                    .models
                                    .iter_mut()
                                    .find(|draft| draft.id.read(cx).value().trim() == model.id)
                                {
                                    draft.fill(Some(&model), reference, preset, window, cx);
                                    continue;
                                }
                                let draft = Draft::discovered(
                                    editor.next_model,
                                    model,
                                    reference,
                                    preset,
                                    window,
                                    cx,
                                );
                                editor.models.push(draft);
                                editor.next_model += 1;
                            }
                            if metadata.failed {
                                editor.report_error("provider_catalog_read_failed", window, cx);
                            }
                        }
                        Ok(Some(Err(error))) => editor.report_error(
                            match error.code {
                                ErrorCode::RevisionConflict => "provider_revision_changed",
                                ErrorCode::PermissionDenied | ErrorCode::NotConfigured
                                    if preset.oauth() =>
                                {
                                    "provider_account_unavailable"
                                }
                                ErrorCode::OutcomeUnknown if preset.oauth() => {
                                    "provider_account_uncertain"
                                }
                                ErrorCode::PermissionDenied | ErrorCode::NotConfigured => {
                                    "provider_credential_unavailable"
                                }
                                ErrorCode::InvalidRequest => "provider_configuration_invalid",
                                ErrorCode::Busy => "provider_query_busy",
                                _ => "provider_query_failed",
                            },
                            window,
                            cx,
                        ),
                        _ => editor.report_error("provider_query_failed", window, cx),
                    }
                    cx.notify();
                });
            }),
        });
        cx.notify();
    }
}
