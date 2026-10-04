use super::{Ingress, Job, agent, unavailable};
pub(in crate::store) mod authentication;
pub(super) mod catalog;
pub(in crate::store) mod completion;
pub(in crate::store) mod configuration;
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
                    } => {
                        let provider = self.provider(*provider, *expected_revision).await?;
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
        if crate::providers::cloud::deployment(provider.api) {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "cloud deployments require an explicit model ID",
            ));
        }
        let models = if provider.authentication == Authentication::ApiKey {
            self.draft_models(&discovery::Draft {
                provider: provider.id,
                api: provider.api,
                endpoint: provider.endpoint.clone(),
                credential: provider.credential.clone(),
                secret: None,
            })
            .await?
        } else {
            let reference = provider.credential.clone().ok_or_else(|| {
                Fault::new(ErrorCode::NotConfigured, "provider sign-in is required")
            })?;
            let grant = self
                .authorization(
                    reference,
                    provider.id,
                    provider.authentication,
                    &self.closed,
                )
                .await?;
            let endpoint = grant.endpoint();
            #[cfg(any(test, feature = "test-support"))]
            let endpoint = self.authorization_endpoint.as_deref().unwrap_or(endpoint);
            let models = self
                .discovery
                .authorized(provider.api, endpoint, &grant, self.closed.clone())
                .await?;
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
        let key = self.discovery_key(source).await?;
        let models = self
            .discovery
            .read(
                source.api,
                &source.endpoint,
                key.as_ref(),
                self.closed.clone(),
            )
            .await?;
        self.discovery_key(source).await?;
        Ok(models)
    }

    async fn discovery_key(&self, source: &discovery::Draft) -> Result<Option<Secret>, Fault> {
        if let Some(secret) = &source.secret {
            if secret.expose().is_empty() || secret.expose().len() > 65536 {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "provider credential is invalid",
                ));
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
