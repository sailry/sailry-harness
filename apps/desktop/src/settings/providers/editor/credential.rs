//! Private API keys belong to the open editor, never the provider projection.
use super::*;
use sailry_protocol::{Command, Output, Secret};

impl Editor {
    pub(super) fn load_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(binding) = self.binding.clone() else {
            return;
        };
        let Some(provider) = self.provider.as_ref().filter(|provider| {
            self.preset.key_auth()
                && provider.authentication == sailry_protocol::Authentication::ApiKey
                && provider.credential.is_some()
        }) else {
            return;
        };
        let request = binding.client.prepare(Command::ReadProviderKey {
            provider: provider.id,
            expected_revision: provider.revision,
        });
        let kind = self.preset.kind();
        self.key_ready = false;
        self.pending = true;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if editor.closed || editor.preset.kind() != kind {
                    return;
                }
                match result {
                    Ok(Ok(Output::ProviderKey(key))) => {
                        if editor.credential.read(cx).value().is_empty() {
                            editor.credential.update(cx, |input, cx| {
                                input.set_value(
                                    key.as_ref().map_or("", Secret::expose),
                                    window,
                                    cx,
                                );
                            });
                        }
                        editor.original_key = key;
                        editor.key_ready = true;
                    }
                    Ok(Err(error)) => editor.report_error(
                        if error.code == sailry_protocol::ErrorCode::RevisionConflict {
                            "provider_revision_changed"
                        } else {
                            "provider_key_failed"
                        },
                        window,
                        cx,
                    ),
                    _ => editor.report_error("provider_key_failed", window, cx),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn draft_key(&self, cx: &App) -> Option<Secret> {
        let value = self.credential.read(cx).value();
        (self.preset.key_auth()
            && !value.trim().is_empty()
            && self.original_key.as_ref().map(Secret::expose) != Some(value.as_ref()))
        .then(|| Secret::new(value.to_string()))
    }
}
