//! Cached local directory metadata for channel presentation, never execution policy.
use super::{Workspace, data::Channel, live::Binding};
use gpui_kit::*;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output,
    conversation::{
        Provider,
        presets::{Entry, PresetRef},
    },
    plugin::Summary,
};

#[derive(Default)]
pub(super) struct State {
    pub plugins: Option<Vec<Summary>>,
    pub entries: Vec<Entry>,
    pub loaded: bool,
    read: Option<Read>,
}

struct Read {
    stop: CancellationToken,
    _task: Task<()>,
}
impl Drop for Read {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn selected(reference: Option<&PresetRef>, entries: &[Entry]) -> Option<Entry> {
    let reference = reference?;
    entries
        .iter()
        .find(|entry| entry.source == reference.source && entry.preset.id == reference.id)
        .cloned()
}

impl Workspace {
    pub(super) fn read_channel_directory(&mut self, plugins: Vec<Summary>, cx: &mut Context<Self>) {
        let Some(live) = &mut self.provider_link else {
            return;
        };
        if !live.connected || live.directory.plugins.as_ref() == Some(&plugins) {
            return;
        }
        live.directory.read = None;
        live.directory.plugins = Some(plugins.clone());
        let Binding {
            client, runtime, ..
        } = live.binding.clone();
        let node = client.target();
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let job = runtime.spawn(async move {
            tokio::select! { biased;
                _ = cancelled.cancelled() => None,
                result = client.execute(client.prepare(Command::ListChannelPresets {refresh_partners:false})) => Some(result)
            }
        });
        live.directory.read = Some(Read {
            stop,
            _task: cx.spawn(async move |owner, cx| {
                let result = job.await;
                _ = owner.update(cx, |owner, cx| {
                    let Some(live) = &mut owner.provider_link else {
                        return;
                    };
                    if live.binding.client.target() != node
                        || live.directory.plugins.as_ref() != Some(&plugins)
                    {
                        return;
                    }
                    live.directory.read = None;
                    if let Ok(Some(Ok(Output::ChannelPresets(listing)))) = result {
                        live.directory.entries = super::presets::presentation(listing.entries);
                        live.directory.loaded = true;
                        apply(
                            &mut owner.providers.channels,
                            &live.providers,
                            &live.directory.entries,
                        );
                    } else {
                        // A failed observation must not pin an empty directory across reconnects.
                        live.directory.plugins = None;
                    }
                    cx.notify();
                });
            }),
        });
    }
}
fn apply(
    channels: &mut [Channel],
    providers: &std::collections::BTreeMap<usize, Provider>,
    entries: &[Entry],
) {
    for channel in channels {
        let provider = providers.get(&channel.id);
        let reference = provider.and_then(|provider| provider.preset.as_ref());
        channel.directory = selected(reference, entries).or_else(|| {
            channel.directory.take().filter(|entry| {
                reference.is_some_and(|reference| {
                    entry.source == reference.source && entry.preset.id == reference.id
                })
            })
        });
        if let Some(provider) = provider {
            channel.preset = super::live::preset(provider, channel.directory.as_ref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn matches_source_and_stable_identity() {
        let mut entry = super::super::presets::bundled().remove(0);
        entry.source = sailry_protocol::conversation::presets::Source::Package {
            name: "fixture".into(),
        };
        let mut reference = PresetRef {
            source: entry.source.clone(),
            id: entry.preset.id.clone(),
            route: None,
        };
        assert_eq!(
            selected(Some(&reference), std::slice::from_ref(&entry))
                .unwrap()
                .source,
            entry.source
        );
        assert!(selected(Some(&reference), &[]).is_none());
        reference.source = sailry_protocol::conversation::presets::Source::Partner;
        assert!(selected(Some(&reference), &[entry]).is_none());
    }

    #[test]
    fn loaded_directory_never_borrows_local_entries() {
        let entry = super::super::presets::bundled().remove(0);
        let reference = super::super::presets::reference(&entry, &entry.preset.routes[0]);
        assert!(selected(Some(&reference), &[]).is_none());
    }

    #[test]
    fn refresh_preserves_saved_overrides() {
        use sailry_protocol::conversation::{
            ModelApi,
            connection::{Options, Product},
        };
        let mut entry = super::super::presets::bundled()
            .into_iter()
            .find(|entry| entry.preset.id == "qwen")
            .unwrap();
        let reference = super::super::presets::reference(&entry, &entry.preset.routes[0]);
        let credential = sailry_protocol::CredentialRef {
            node: sailry_protocol::NodeId::new(),
            id: sailry_protocol::CredentialId::new(),
        };
        let model = super::super::live::model(&super::super::data::Model::example("saved-model"));
        let provider: Provider = serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::ProviderId::new(),"revision":7,"name":"Saved custom name",
            "api":"chat_completions","authentication":"api_key","endpoint":"https://custom.invalid/v1",
            "enabled":true,"models":[model],"default_model":"saved-model","credential":credential,"preset":reference,
            "options":Options::Hosted { product: Product::Metered }
        })).unwrap();
        assert_eq!(provider.api, ModelApi::ChatCompletions);
        let channel = super::super::live::channel(7, &provider);
        let before = channel.clone();
        entry.preset.name = "Execution Node preset".into();
        entry.preset.locales.clear();
        entry.preset.brand_display = None;
        entry.preset.routes[0].endpoint = "https://new-default.invalid/v1".into();
        let entries = super::super::presets::presentation(vec![entry.clone()]);
        let mut channels = [channel];
        apply(&mut channels, &[(7, provider.clone())].into(), &entries);
        let current = &channels[0];
        assert_eq!(current.directory.as_ref(), Some(&entries[0]));
        assert_eq!(
            super::super::presets::brand_label(current.directory.as_ref().unwrap()),
            "Execution Node preset"
        );
        assert_eq!(current.name, before.name);
        assert_eq!(current.endpoint, before.endpoint);
        assert_eq!(current.models, before.models);
        assert_eq!(current.default_model, before.default_model);
        assert_eq!(current.credential_configured, before.credential_configured);
        assert_eq!(
            current.options,
            Some(Options::Hosted {
                product: Product::Metered
            })
        );
        assert_eq!(current.label().as_ref(), "Execution Node preset");
        assert_eq!(provider.endpoint, "https://custom.invalid/v1");
        assert_eq!(provider.preset.as_ref(), Some(&reference));
        assert_eq!(provider.credential.as_ref(), Some(&credential));
    }
}
