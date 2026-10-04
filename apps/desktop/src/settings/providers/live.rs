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

#[derive(Clone)]
pub(in crate::settings) struct Binding {
    pub client: Arc<Client>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub label: SharedString,
}

pub(in crate::settings) struct Live {
    pub terminal_revision: u64,
    pub binding: Binding,
    pub providers: BTreeMap<usize, Provider>,
    ids: BTreeMap<ProviderId, usize>,
    pub connected: bool,
    pub pending: bool,
    pub error: Option<&'static str>,
    pub catalog: super::catalog::State,
    stop: CancellationToken,
    _task: Task<()>,
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
        let node = transport.target();
        if self
            .provider_link
            .as_ref()
            .is_some_and(|live| live.binding.client.target() == node)
        {
            return;
        }
        self.provider_link = None;
        self.providers.channels.clear();
        self.roles.clear();
        self.role_catalog = Default::default();
        self.plugin_catalog = Default::default();
        self.extensions = super::super::extensions::State::reset(self.extensions.selected.clone());
        let binding = Binding {
            client: Arc::new(Client::new(transport)),
            runtime,
            label,
        };
        let metadata = cx.new(|_| {
            super::super::plugins::Metadata::new(binding.client.clone(), binding.runtime.clone())
        });
        cx.observe(&metadata, |_, _, cx| cx.notify()).detach();
        self.plugin_catalog.metadata = Some(metadata);
        let stop = CancellationToken::new();
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        let client = binding.client.clone();
        let cancellation = stop.clone();
        binding.runtime.spawn(async move {
            let _ = client.watch(sender, cancellation).await;
        });
        let task = cx.spawn(async move |owner, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if owner
                    .update(cx, |owner, cx| owner.accept_providers(node, view, cx))
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
            binding,
            providers: BTreeMap::new(),
            ids: BTreeMap::new(),
            connected: false,
            pending: false,
            error: None,
            catalog: Default::default(),
            stop,
            _task: task,
        });
        cx.notify();
    }

    fn accept_providers(&mut self, node: NodeId, view: View, cx: &mut Context<Self>) {
        let Some(live) = &mut self.provider_link else {
            return;
        };
        if live.binding.client.target() != node {
            return;
        }
        let refresh =
            !live.connected && view.connected && self.section == super::super::Section::Providers;
        live.connected = view.connected;
        if let Some(snapshot) = view.snapshot.as_ref() {
            live.terminal_revision = snapshot.terminal_settings_revision;
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
                .map(|(&id, provider)| channel(id, provider))
                .collect();
            self.providers
                .channels
                .sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
            self.roles = self.role_catalog.accept(&snapshot.roles, &live.providers);
            self.plugin_catalog.accept(snapshot, cx);
        }
        if refresh {
            self.refresh_catalog(cx);
        }
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

pub(crate) fn channel(id: usize, provider: &Provider) -> Channel {
    Channel {
        options: provider.options.clone(),
        id,
        name: provider.name.clone(),
        preset: match (provider.authentication, provider.api) {
            (sailry_protocol::Authentication::Host, ModelApi::Bedrock) => Preset::BedrockIam,
            (sailry_protocol::Authentication::Host, ModelApi::Vertex) => Preset::VertexAdc,
            (sailry_protocol::Authentication::ChatGpt, _) => Preset::ChatGpt,
            (sailry_protocol::Authentication::Copilot, ModelApi::Responses) => {
                Preset::CopilotResponses
            }
            (sailry_protocol::Authentication::Copilot, _) => Preset::Copilot,
            (_, api) => match api {
                ModelApi::AzureOpenAi => Preset::AzureOpenAi,
                ModelApi::AzureAi => Preset::AzureAi,
                ModelApi::Bedrock => Preset::Bedrock,
                ModelApi::Vertex => Preset::Vertex,
                ModelApi::DeepSeek => Preset::Hosted(super::vendors::Vendor::DeepSeek),
                ModelApi::OpenCodeGo => Preset::OpenCodeGo,
                ModelApi::OpenCodeZen => Preset::OpenCodeZen,
                ModelApi::ChatCompletions => {
                    super::vendors::Vendor::from_endpoint(&provider.endpoint)
                        .map(Preset::Hosted)
                        .unwrap_or(Preset::CompatibleChat)
                }
                ModelApi::Responses if provider.endpoint == "https://api.openai.com/v1" => {
                    Preset::OpenAi
                }
                ModelApi::Responses => Preset::CompatibleResponses,
                ModelApi::Anthropic
                    if provider.endpoint.trim_end_matches('/') == "https://api.anthropic.com" =>
                {
                    Preset::Anthropic
                }
                ModelApi::Anthropic => Preset::CompatibleAnthropic,
                ModelApi::Gemini
                    if provider.endpoint.trim_end_matches('/')
                        == "https://generativelanguage.googleapis.com/v1beta" =>
                {
                    Preset::Gemini
                }
                ModelApi::Gemini => Preset::CompatibleGemini,
            },
        },
        endpoint: provider.endpoint.clone(),
        credential_configured: provider.credential.is_some(),
        enabled: provider.enabled,
        connected: false,
        default_model: provider.default_model.clone(),
        models: provider
            .models
            .iter()
            .map(|model| Model {
                id: model.id.clone(),
                context: model.context,
                output: model.output,
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
        context: model.context,
        output: model.output,
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
        ErrorCode::PermissionDenied => "provider_credential_unavailable",
        ErrorCode::InvalidRequest => "provider_configuration_invalid",
        _ => "provider_save_failed",
    }
}
