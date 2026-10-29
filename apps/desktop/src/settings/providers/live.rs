use super::{
    Workspace,
    data::{Channel, Model, Preset},
};
use crate::{backend::Services, tr};
use gpui_kit::*;
use sailry_client::{Client, View};
use sailry_link::{CancellationToken, Transport};
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, ProviderId,
    conversation::{ModelApi, Provider},
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) mod quota;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(in crate::settings) struct Binding {
    pub client: Arc<Client>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub label: SharedString,
}

pub(in crate::settings) struct Live {
    pub terminal_revision: u64,
    pub power_revision: u64,
    pub sandbox_revision: u64,
    pub search_revision: u64,
    pub binding: Binding,
    pub providers: BTreeMap<usize, Provider>,
    pub snapshot: Option<sailry_protocol::Snapshot>,
    pub credentials: Vec<sailry_protocol::Credential>,
    ids: BTreeMap<ProviderId, usize>,
    pub connected: bool,
    pub pending: bool,
    pub error: Option<&'static str>,
    pub catalog: super::catalog::State,
    pub(super) directory: super::directory::State,
    stop: CancellationToken,
    _task: Task<()>,
    _python: Task<()>,
    quota_reset: Option<u64>,
    quota_clock: Option<Task<()>>,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Workspace {
    pub(crate) fn bind_providers(
        &mut self,
        transport: Arc<dyn Transport>,
        runtime: Arc<tokio::runtime::Runtime>,
        label: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.bind_client(
            Binding {
                client: Arc::new(Client::new(transport)),
                runtime,
                label,
            },
            cx,
        );
    }

    pub(in crate::settings) fn bind_client(&mut self, binding: Binding, cx: &mut Context<Self>) {
        let node = binding.client.target();
        if self
            .provider_link
            .as_ref()
            .is_some_and(|live| live.binding.client.target() == node)
        {
            return;
        }
        self.clear_models(cx);
        crate::installations::connected(binding.client.clone(), binding.runtime.clone(), cx);
        self.clear_backup(cx);
        self.extensions.release(cx);
        self.provider_link = None;
        self.providers.channels.clear();
        self.roles.clear();
        self.role_catalog = Default::default();
        self.plugin_catalog = Default::default();
        let updates = super::super::plugins::updates::bind(binding.clone(), cx);
        cx.observe(&updates, |_, _, cx| cx.notify()).detach();
        self.plugin_catalog.updates = Some(updates);
        self.extensions = super::super::extensions::State::reset(self.extensions.selected.clone());
        let metadata = cx.new(|_| {
            super::super::plugins::Metadata::new(binding.client.clone(), binding.runtime.clone())
        });
        cx.observe(&metadata, |_, _, cx| cx.notify()).detach();
        self.plugin_catalog.metadata = Some(metadata);
        let stop = CancellationToken::new();
        let python = super::super::plugins::preparation::watch(&binding, &stop, cx);
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        let client = binding.client.clone();
        let watcher = client.clone();
        let cancellation = stop.clone();
        binding.runtime.spawn(async move {
            let _ = watcher.watch(sender, cancellation).await;
        });
        let runtime = binding.runtime.clone();
        let task = cx.spawn(async move |owner, cx| {
            let mut providers = Vec::new();
            let mut credentials = Vec::new();
            let mut credential_cursor = 0;
            let mut connected = false;
            loop {
                let view = receiver.borrow_and_update().clone();
                if let Some(snapshot) = &view.snapshot
                    && (providers != snapshot.providers
                        || credential_cursor != view.credential_cursor
                        || (!connected && view.connected))
                {
                    providers = snapshot.providers.clone();
                    credential_cursor = view.credential_cursor;
                    credentials.clear();
                    // Invalidate displayed values before awaiting the public metadata read.
                    if owner
                        .update(cx, |owner, cx| {
                            if let Some(state) = &owner.provider_contributions {
                                state.invalidate(cx);
                            }
                            owner.accept_providers(node, view.clone(), &[], cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                    let client = client.clone();
                    credentials = match runtime
                        .spawn(async move {
                            client
                                .execute(client.prepare(Command::ListCredentials))
                                .await
                        })
                        .await
                    {
                        Ok(Ok(sailry_protocol::Output::Credentials(values))) => values,
                        _ => Vec::new(),
                    };
                }
                if !metadata_current(&view, &receiver.borrow()) {
                    continue;
                }
                connected = view.connected;
                if owner
                    .update(cx, |owner, cx| {
                        owner.accept_providers(node, view, &credentials, cx)
                    })
                    .is_err()
                {
                    break;
                }
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        });
        self.provider_link = Some(Live {
            terminal_revision: 0,
            power_revision: 0,
            sandbox_revision: 0,
            search_revision: 0,
            binding,
            providers: BTreeMap::new(),
            snapshot: None,
            credentials: vec![],
            ids: BTreeMap::new(),
            connected: false,
            pending: false,
            error: None,
            catalog: Default::default(),
            directory: Default::default(),
            stop,
            _task: task,
            _python: python,
            quota_reset: None,
            quota_clock: None,
        });
        cx.notify();
    }

    fn accept_providers(
        &mut self,
        node: NodeId,
        view: View,
        credentials: &[sailry_protocol::Credential],
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &mut self.provider_link else {
            return;
        };
        if live.binding.client.target() != node {
            return;
        }
        if view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.node != node)
        {
            return;
        }
        let refresh =
            !live.connected && view.connected && self.section == super::super::Section::Providers;
        live.snapshot = view.snapshot.clone();
        live.credentials = credentials.to_vec();
        live.connected = view.connected;
        if !view.connected {
            live.directory.invalidate();
            self.extensions.clear_resources();
        }
        let directory_plugins = view
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.plugins.clone());
        if let Some(snapshot) = view.snapshot.as_ref() {
            self.extensions.retain(&snapshot.plugins, cx);
            live.terminal_revision = snapshot.terminal_settings_revision;
            live.power_revision = snapshot.power_settings_revision;
            live.sandbox_revision = snapshot.sandbox_settings_revision;
            live.search_revision = snapshot.search_settings_revision;
            if snapshot.model_catalog.revision > live.catalog.status.revision {
                live.catalog.error = None;
            }
            if snapshot.model_catalog.revision >= live.catalog.status.revision {
                live.catalog.status = snapshot.model_catalog.clone();
            }
            live.providers.clear();
            for provider in &snapshot.providers {
                let next = live.ids.len();
                let id = *live.ids.entry(provider.id).or_insert(next);
                live.providers.insert(id, provider.clone());
            }
            self.providers.channels = live
                .providers
                .iter()
                .map(|(&id, provider)| {
                    let mut channel = channel(id, provider);
                    channel.account_label = provider
                        .credential
                        .as_ref()
                        .and_then(|reference| {
                            credentials.iter().find(|credential| {
                                reference.node == node
                                    && credential.id == reference.id
                                    && credential.provider == provider.id
                                    && !credential.revoked
                            })
                        })
                        .and_then(|credential| credential.account_label.clone());
                    channel.directory = super::directory::selected(
                        provider.preset.as_ref(),
                        &live.directory.entries,
                    )
                    .or_else(|| {
                        self.providers
                            .channels
                            .iter()
                            .find(|previous| previous.id == id)
                            .and_then(|previous| previous.directory.clone())
                            .filter(|entry| {
                                provider.preset.as_ref().is_some_and(|reference| {
                                    entry.source == reference.source
                                        && entry.preset.id == reference.id
                                })
                            })
                    });
                    channel.preset = preset(provider, channel.directory.as_ref());
                    channel.quota = snapshot
                        .provider_quotas
                        .iter()
                        .find(|quota| {
                            quota.provider == provider.id
                                && quota.credential.node == node
                                && provider.credential.as_ref() == Some(&quota.credential)
                        })
                        .cloned();
                    channel
                })
                .collect();
            self.providers
                .channels
                .sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
            self.roles = self.role_catalog.accept(&snapshot.roles, &live.providers);
            self.plugin_catalog.accept(snapshot, cx);
        }
        if let Some(plugins) = directory_plugins {
            self.read_channel_directory(plugins, cx);
        }
        if refresh {
            self.refresh_catalog(super::catalog::Source::Automatic, cx);
        }
        quota::refresh(self, cx);
        cx.notify();
    }

    pub(super) fn provider_action(
        &mut self,
        id: usize,
        remove: bool,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &mut self.provider_link else {
            return;
        };
        if live.pending {
            return;
        }
        let Some(mut provider) = live.providers.get(&id).cloned() else {
            return;
        };
        let command = if remove {
            Command::RemoveProvider {
                provider: provider.id,
                expected_revision: provider.revision,
            }
        } else {
            provider.enabled = enabled;
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }
        };
        let binding = live.binding.clone();
        self.submit_provider(binding, command, cx);
    }

    pub(super) fn submit_provider(
        &mut self,
        binding: Binding,
        command: Command,
        cx: &mut Context<Self>,
    ) {
        let node = binding.client.target();
        let request = binding.client.prepare(command);
        if let Some(live) = &mut self.provider_link
            && live.binding.client.target() == node
        {
            live.pending = true;
            live.error = None;
        }
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn(async move |owner, cx| {
            let result = job.await;
            let _ = owner.update(cx, |owner, cx| {
                if let Some(live) = &mut owner.provider_link
                    && live.binding.client.target() == node
                {
                    live.pending = false;
                    live.error = match result {
                        Ok(Ok(_)) => None,
                        Ok(Err(error)) => Some(error_key(&error)),
                        Err(_) => Some("provider_save_unknown"),
                    };
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}

pub(in crate::settings) fn local(owner: &mut Workspace, cx: &mut Context<Workspace>) {
    if let Some(services) = cx.try_global::<Services>().cloned() {
        owner.bind_providers(
            services.local,
            services.runtime,
            tr("composer_host_local"),
            cx,
        );
    }
}

/// The editor preset a configured provider corresponds to.
pub(super) fn preset(
    provider: &Provider,
    entry: Option<&sailry_protocol::conversation::presets::Entry>,
) -> Preset {
    if let Some(reference) = &provider.preset
        && let Some(entry) = entry
            .filter(|entry| entry.source == reference.source && entry.preset.id == reference.id)
        && let Some(route) = entry
            .preset
            .routes
            .iter()
            .find(|route| reference.route.as_ref() == Some(&route.key()))
            .or_else(|| {
                entry
                    .preset
                    .routes
                    .iter()
                    .find(|route| route.protocol.api() == provider.api)
            })
        && route.protocol.api() == provider.api
    {
        return super::presets::kind(entry, route);
    }
    match (provider.authentication, provider.api) {
        (sailry_protocol::Authentication::ChatGptOAuth, _) => Preset::ChatGptOAuth,
        (sailry_protocol::Authentication::ChatGpt, _) => Preset::ChatGpt,
        (sailry_protocol::Authentication::ClaudeOAuth, _) => Preset::ClaudeOAuth,
        (sailry_protocol::Authentication::Copilot, ModelApi::Responses) => Preset::CopilotResponses,
        (sailry_protocol::Authentication::Copilot, _) => Preset::Copilot,
        (_, api) => match api {
            ModelApi::Plugin => Preset::Plugin,
            ModelApi::AzureOpenAi => Preset::AzureOpenAi,
            ModelApi::AzureAi => Preset::AzureAi,
            ModelApi::DeepSeek => Preset::Hosted(super::vendors::Vendor::DeepSeek),
            ModelApi::OpenCodeGo => Preset::OpenCodeGo,
            ModelApi::OpenCodeZen => Preset::OpenCodeZen,
            ModelApi::ChatCompletions => Preset::CompatibleChat,
            ModelApi::Responses => Preset::CompatibleResponses,
            ModelApi::Anthropic => Preset::CompatibleAnthropic,
            ModelApi::Gemini => Preset::CompatibleGemini,
        },
    }
}

/// Logo asset path of the provider's configured type, or None for generic providers.
pub(crate) fn logo(provider: &Provider) -> Option<String> {
    if let Some(reference) = &provider.preset
        && let Some(entry) = super::presets::bundled()
            .iter()
            .find(|entry| entry.source == reference.source && entry.preset.id == reference.id)
    {
        return entry.preset.logo.clone();
    }
    super::presets::bundled()
        .into_iter()
        .find(|entry| {
            super::presets::authentication(entry) == provider.authentication
                && entry.preset.routes.iter().any(|route| {
                    route.protocol.api() == provider.api
                        && !route.endpoint.is_empty()
                        && route.endpoint.trim_end_matches('/')
                            == provider.endpoint.trim_end_matches('/')
                })
        })
        .and_then(|entry| entry.preset.logo)
        .or_else(|| preset(provider, None).category().logo())
}

pub(crate) fn channel(id: usize, provider: &Provider) -> Channel {
    let directory =
        super::directory::selected(provider.preset.as_ref(), &super::presets::bundled());
    Channel {
        options: provider.options.clone(),
        oauth: provider.oauth.clone(),
        quota: None,
        preset: preset(provider, directory.as_ref()),
        directory,
        id,
        name: provider.name.clone(),
        endpoint: provider.endpoint.clone(),
        credential_configured: provider.credential.is_some(),
        account_label: None,
        enabled: provider.enabled,
        connected: false,
        default_model: provider.default_model.clone(),
        models: provider
            .models
            .iter()
            .map(|model| Model {
                id: model.id.clone(),
                display_name: model.display_name.clone(),
                context: model.context,
                output: model.output,
                compaction_tokens: model.compaction_tokens,
                capabilities: model.capabilities.clone(),
                vision: model.vision,
                tools: model.tools,
                reasoning: model.reasoning,
                web_search: model.web_search,
                generates: model.generates.clone(),
                efforts: model.efforts.clone(),
                custom_efforts: model.custom_efforts,
                default_effort: model.default_effort,
            })
            .collect(),
    }
}

pub(super) fn model(model: &Model) -> sailry_protocol::conversation::Model {
    sailry_protocol::conversation::Model {
        id: model.id.clone(),
        display_name: model.display_name.clone(),
        context: model.context,
        output: model.output,
        compaction_tokens: model.compaction_tokens,
        capabilities: model.capabilities.clone(),
        vision: model.vision,
        tools: model.tools,
        reasoning: model.reasoning,
        web_search: model.web_search,
        generates: model.generates.clone(),
        efforts: model.efforts.clone(),
        custom_efforts: model.custom_efforts,
        default_effort: model.default_effort,
    }
}

pub(super) fn error_key(error: &Fault) -> &'static str {
    match error.code {
        ErrorCode::RevisionConflict | ErrorCode::Conflict => "provider_revision_changed",
        ErrorCode::OutcomeUnknown => "provider_save_unknown",
        ErrorCode::PermissionDenied => "provider_access_denied",
        ErrorCode::NotConfigured => "provider_not_configured",
        ErrorCode::InvalidRequest => "provider_configuration_invalid",
        _ => "provider_save_failed",
    }
}

fn metadata_current(captured: &View, current: &View) -> bool {
    captured.connected == current.connected
        && captured.credential_cursor == current.credential_cursor
        && captured
            .snapshot
            .as_ref()
            .map(|snapshot| (&snapshot.node, &snapshot.providers))
            == current
                .snapshot
                .as_ref()
                .map(|snapshot| (&snapshot.node, &snapshot.providers))
}
