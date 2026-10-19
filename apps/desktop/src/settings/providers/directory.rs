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
        .or_else(|| {
            super::presets::bundled()
                .into_iter()
                .find(|entry| entry.source == reference.source && entry.preset.id == reference.id)
        })
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
                        live.directory.entries = listing.entries;
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
        let reference = providers
            .get(&channel.id)
            .and_then(|provider| provider.preset.as_ref());
        channel.directory = selected(reference, entries).or_else(|| {
            channel.directory.take().filter(|entry| {
                reference.is_some_and(|reference| {
                    entry.source == reference.source && entry.preset.id == reference.id
                })
            })
        });
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
}
