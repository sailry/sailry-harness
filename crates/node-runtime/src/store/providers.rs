use super::{Ingress, Job, agent, unavailable};
pub(super) mod adapters;
pub(in crate::store) mod authentication;
pub(super) mod catalog;
pub(in crate::store) mod completion;
pub(in crate::store) mod configuration;
pub(crate) mod consent;
pub(in crate::store) mod presets;
pub(in crate::store) mod quota;
pub(in crate::store) mod resources;
pub(in crate::store) use configuration::{Stored, validate_stored};
use sailry_link::Admission;
use sailry_protocol::{
    conversation::{Provider, discovery},
    *,
};
use tokio::sync::oneshot;

pub(super) fn current(
    db: &rusqlite::Connection,
    id: ProviderId,
    revision: u64,
) -> Result<Provider, Fault> {
    let provider = agent::providers::get(db, id)?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "provider does not exist"))?;
    if provider.revision != revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "provider revision changed",
        ));
    }
    Ok(provider)
}

pub(super) fn inspect(
    db: &rusqlite::Connection,
    id: ProviderId,
    revision: u64,
) -> Result<Provider, Fault> {
    let provider = current(db, id, revision)?;
    if let Some(reference) = &provider.credential {
        authentication::validate_authentication(
            db,
            reference.id,
            provider.id,
            provider.authentication,
        )?;
    }
    Ok(provider)
}

impl Ingress {
    async fn capture_channel(&self, provider: &Provider) -> Result<Provider, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::CaptureChannel {
                provider: Box::new(provider.clone()),
                reply,
            })
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
    pub(crate) async fn model_adapter(
        &self,
        provider: &Provider,
        messages: bool,
    ) -> Result<sailry_protocol::plugin::model_adapter::Declaration, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::ModelAdapter {
                provider: Box::new(provider.clone()),
                messages,
                reply,
            })
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(super) async fn discover_models(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let (models, saved) = match &request.command {
            Command::ValidateProvider {
                provider,
                expected_revision,
            } => {
                let provider = self.provider(*provider, *expected_revision).await?;
                (self.saved_models(&provider).await?, Some(provider))
            }
            Command::DiscoverModels(source) => {
                let models = match source.as_ref() {
                    discovery::Source::Draft(draft) => self.draft_models(draft).await?,
                    discovery::Source::Saved {
                        provider,
                        expected_revision,
                        oauth,
                    } => {
                        let mut provider = self.provider(*provider, *expected_revision).await?;
                        if let Some(options) = oauth {
                            provider.oauth = Some(options.clone());
                            crate::providers::login::validate(&provider)?;
                        }
                        self.saved_models(&provider).await?
                    }
                };
                (models, None)
            }
            _ => {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "model discovery command expected",
                ));
            }
        };
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let output = if let Some(provider) = saved {
            Output::ProviderValidation(crate::providers::validate(&provider, &models.models))
        } else {
            Output::DiscoveredModels(models)
        };
        let (sender, completion) = oneshot::channel();
        let _ = sender.send(Ok(output));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    async fn saved_models(&self, provider: &Provider) -> Result<discovery::Catalog, Fault> {
        if provider.api == sailry_protocol::conversation::ModelApi::Plugin {
            let captured = self.capture_channel(provider).await?;
            let models =
                crate::agent::discover_plugin_models(self, &captured, None, &self.closed).await?;
            self.provider(provider.id, provider.revision).await?;
            if self.capture_channel(provider).await?.binding != captured.binding {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "channel declaration changed during model discovery",
                ));
            }
            return Ok(models);
        }
        if crate::providers::cloud::deployment(provider.api) {
            return Err(Fault::invalid(
                "cloud deployments require an explicit model ID",
            ));
        }
        let models = if provider.authentication == Authentication::ApiKey {
            let captured = self.capture_channel(provider).await?;
            let key = match &provider.credential {
                Some(reference) => Some(
                    self.resolve_credential(reference.clone(), provider.id)
                        .await?,
                ),
                None => None,
            };
            let injection = captured
                .binding
                .as_ref()
                .and_then(|binding| binding.key_injection.as_ref());
            let headers = crate::providers::headers(provider.api, key.as_ref(), injection)?;
            let models = self
                .discovery
                .read(
                    provider.api,
                    &provider.endpoint,
                    headers,
                    self.closed.clone(),
                )
                .await?;
            if let Some(reference) = &provider.credential {
                self.resolve_credential(reference.clone(), provider.id)
                    .await?;
            }
            if self.capture_channel(provider).await?.binding != captured.binding {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "channel declaration changed during model discovery",
                ));
            }
            models
        } else {
            let captured = if provider.authentication == Authentication::Plugin {
                Some(self.capture_channel(provider).await?)
            } else {
                None
            };
            let options =
                crate::providers::login::effective(captured.as_ref().unwrap_or(provider))?;
            let reference = provider.credential.clone().ok_or_else(|| {
                Fault::new(ErrorCode::NotConfigured, "provider sign-in is required")
            })?;
            let grant = self
                .authorization(
                    reference,
                    provider.id,
                    provider.authentication,
                    &options,
                    &self.closed,
                )
                .await?
                .grant;
            let endpoint = grant.endpoint();
            #[cfg(any(test, feature = "test-support"))]
            let endpoint = self.authorization_endpoint.as_deref().unwrap_or(endpoint);
            let models = self
                .discovery
                .authorized(
                    provider.api,
                    endpoint,
                    &grant,
                    &options,
                    self.closed.clone(),
                )
                .await?;
            if let Some(captured) = captured
                && self.capture_channel(provider).await?.binding != captured.binding
            {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "channel declaration changed during model discovery",
                ));
            }
            discovery::Catalog {
                endpoint: endpoint.to_owned(),
                models,
            }
        };
        // Revocation, account replacement or a changed provider revision invalidates
        // a late result without starting another refresh after the completed query.
        self.provider(provider.id, provider.revision).await?;
        Ok(models)
    }

    async fn draft_models(&self, source: &discovery::Draft) -> Result<discovery::Catalog, Fault> {
        let entry = self.draft_preset(source).await?;
        let key = self.discovery_key(source).await?;
        if let Some(sailry_protocol::conversation::presets::Entry {
            preset:
                sailry_protocol::conversation::presets::Preset {
                    models: sailry_protocol::conversation::presets::Models::Static { ids },
                    ..
                },
            ..
        }) = &entry
        {
            return Ok(discovery::Catalog {
                endpoint: source.endpoint.clone(),
                models: ids
                    .iter()
                    .map(|id| discovery::Model {
                        id: id.clone(),
                        display_name: None,
                        context: None,
                        output: None,
                        capabilities: None,
                    })
                    .collect(),
            });
        }
        if source.api == sailry_protocol::conversation::ModelApi::Plugin {
            let provider = Provider {
                id: source.provider,
                revision: 0,
                preset: source.preset.clone(),
                binding: None,
                consent: None,
                name: "Model discovery".into(),
                api: source.api,
                authentication: Authentication::ApiKey,
                endpoint: source.endpoint.clone(),
                options: None,
                oauth: None,
                enabled: true,
                models: Vec::new(),
                default_model: String::new(),
                credential: source.credential.clone(),
            };
            let captured = self.capture_channel(&provider).await?;
            let models =
                crate::agent::discover_plugin_models(self, &captured, key, &self.closed).await?;
            if self.draft_preset(source).await? != entry {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "channel declaration changed during model discovery",
                ));
            }
            return Ok(models);
        }
        let injection = entry.as_ref().and_then(|entry| match &entry.preset.auth {
            sailry_protocol::conversation::presets::Auth::ApiKey { header, prefix }
                if header.is_some() || prefix.is_some() =>
            {
                Some(plugin::model_auth::Injection {
                    header: header.clone().unwrap_or_else(|| "authorization".into()),
                    template: format!("{}{{key}}", prefix.as_deref().unwrap_or("")),
                })
            }
            _ => None,
        });
        let headers = crate::providers::headers(source.api, key.as_ref(), injection.as_ref())?;
        let models = self
            .discovery
            .read(source.api, &source.endpoint, headers, self.closed.clone())
            .await?;
        self.discovery_key(source).await?;
        if self.draft_preset(source).await? != entry {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "channel declaration changed during model discovery",
            ));
        }
        Ok(models)
    }

    async fn draft_preset(
        &self,
        source: &discovery::Draft,
    ) -> Result<Option<sailry_protocol::conversation::presets::Entry>, Fault> {
        let Some(reference) = &source.preset else {
            return Ok(None);
        };
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::ChannelPresets {
                catalog: None,
                unavailable: false,
                reply,
            })
            .map_err(|_| unavailable())?;
        let listing = response.await.map_err(|_| unavailable())??;
        let entry = listing
            .entries
            .into_iter()
            .find(|entry| entry.source == reference.source && entry.preset.id == reference.id)
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "channel preset is unavailable on this Node",
                )
            })?;
        if !entry.preset.routes.iter().any(|route| {
            route.protocol.api() == source.api
                && reference
                    .route
                    .as_ref()
                    .is_none_or(|key| route.key() == *key)
        }) {
            return Err(Fault::invalid(
                "model discovery protocol does not match its channel preset",
            ));
        }
        if !matches!(
            entry.preset.auth,
            sailry_protocol::conversation::presets::Auth::ApiKey { .. }
                | sailry_protocol::conversation::presets::Auth::None
        ) {
            return Err(Fault::invalid(
                "channel authentication requires a saved sign-in",
            ));
        }
        Ok(Some(entry))
    }

    async fn discovery_key(&self, source: &discovery::Draft) -> Result<Option<Secret>, Fault> {
        if let Some(secret) = &source.secret {
            if secret.expose().is_empty() || secret.expose().len() > 65536 {
                return Err(Fault::invalid("provider credential is invalid"));
            }
            return Ok(Some(secret.clone()));
        }
        match &source.credential {
            Some(reference) => self
                .resolve_credential(reference.clone(), source.provider)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    async fn provider(&self, id: ProviderId, revision: u64) -> Result<Provider, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::Provider {
                id,
                revision,
                reply,
            })
            .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        response.await.map_err(|_| unavailable())?
    }
}
