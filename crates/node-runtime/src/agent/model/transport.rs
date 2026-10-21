//! Session sockets and endpoint observations are Node-private transport optimizations.
use super::*;
use adk_core::{AdkError, LlmRequest, LlmResponseStream, SchemaAdapter};
use adk_model::openai::{TransportEvent, WsOptions, WsRefusal};
use async_trait::async_trait;
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Capability {
    #[default]
    Unknown,
    Supported(u8),
    Unsupported,
}

impl Capability {
    fn observe(&mut self, event: TransportEvent) {
        match event {
            TransportEvent::WebSocket { .. } => *self = Self::Supported(0),
            TransportEvent::HttpCompleted {
                refusal: WsRefusal::Definitive,
            } => {
                *self = match *self {
                    Self::Supported(0) => Self::Supported(1),
                    _ => Self::Unsupported,
                }
            }
            TransportEvent::HttpCompleted {
                refusal: WsRefusal::Transient,
            } => {
                if matches!(self, Self::Supported(_)) {
                    *self = Self::Supported(0);
                }
            }
            TransportEvent::Fallback { .. } | TransportEvent::Http => {}
        }
    }
}

#[derive(Default)]
pub(super) struct Capabilities {
    states: Mutex<BTreeMap<String, Capability>>,
    #[cfg(any(test, feature = "test-support"))]
    fixture: Mutex<Option<WsOptions>>,
}

impl Capabilities {
    #[cfg(any(test, feature = "test-support"))]
    fn configure_fixture(&self, options: WsOptions) {
        *self.fixture.lock().unwrap() = Some(options);
    }

    pub(super) fn options(
        self: &Arc<Self>,
        provider: &Provider,
        model: &str,
        session: sailry_protocol::SessionId,
    ) -> Option<WsOptions> {
        if provider.api != ModelApi::Responses
            || !matches!(
                provider.authentication,
                Authentication::ApiKey | Authentication::ChatGptOAuth
            )
        {
            return None;
        }
        let key = format!(
            "{}:{}:{}",
            provider.id, provider.revision, provider.endpoint
        );
        let enabled = self.clone();
        let observed = self.clone();
        let permit = key.clone();
        let identity = crate::logging::Context {
            provider: Some(provider.id),
            session: Some(session),
            model: Some(model.to_owned()),
            ..Default::default()
        };
        #[cfg(any(test, feature = "test-support"))]
        let options = self.fixture.lock().unwrap().clone().unwrap_or_default();
        #[cfg(not(any(test, feature = "test-support")))]
        let options = WsOptions::default();
        Some(WsOptions {
            enabled: Arc::new(move || {
                enabled.states.lock().unwrap().get(&permit) != Some(&Capability::Unsupported)
            }),
            observe: Arc::new(move |event| {
                {
                    let mut states = observed.states.lock().unwrap();
                    if states.len() >= 256 && !states.contains_key(&key) {
                        states.clear();
                    }
                    let state = states.entry(key.clone()).or_default();
                    state.observe(event);
                }
                let mut entry = crate::logging::Entry::new(
                    crate::logging::Area::Model,
                    crate::logging::Phase::Completed,
                );
                entry.context = identity.clone();
                entry.operation = Some("transport");
                match event {
                    TransportEvent::Http => entry.transport = Some("sse"),
                    TransportEvent::WebSocket {
                        reused,
                        incremental,
                    } => {
                        entry.transport = Some("websocket");
                        entry.connection_reused = Some(reused);
                        entry.continuation = Some(if incremental { "incremental" } else { "full" });
                    }
                    TransportEvent::Fallback { refusal }
                    | TransportEvent::HttpCompleted { refusal } => {
                        entry.transport = Some("sse");
                        entry.fallback_reason = Some(match refusal {
                            WsRefusal::Definitive => "refused",
                            WsRefusal::Transient => "transient",
                        });
                    }
                }
                entry.emit();
            }),
            credential_revision: Arc::new(authorization::revision),
            ..options
        })
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn configure_fixture(registry: &registry::Registry, options: WsOptions) {
    registry.capabilities.configure_fixture(options);
}

struct Http {
    inner: Arc<dyn Llm>,
}
pub(in crate::agent) fn http(inner: Arc<dyn Llm>) -> Arc<dyn Llm> {
    Arc::new(Http { inner })
}
#[async_trait]
impl Llm for Http {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        self.inner.schema_adapter()
    }
    fn uses_interactions_api(&self) -> bool {
        self.inner.uses_interactions_api()
    }
    fn max_cost_micro_usd(&self, request: &LlmRequest) -> Option<u64> {
        self.inner.max_cost_micro_usd(request)
    }
    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> Result<LlmResponseStream, AdkError> {
        let responses =
            adk_model::openai::http_only(self.inner.generate_content(request, stream)).await?;
        Ok(Box::pin(futures::stream::unfold(
            responses,
            |mut responses| async move {
                let next = adk_model::openai::http_only(responses.next()).await;
                next.map(|next| (next, responses))
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remembers_only_confirmed_refusals() {
        let mut state = Capability::Unknown;
        state.observe(TransportEvent::Fallback {
            refusal: WsRefusal::Definitive,
        });
        assert!(state == Capability::Unknown);
        state.observe(TransportEvent::HttpCompleted {
            refusal: WsRefusal::Definitive,
        });
        assert!(state == Capability::Unsupported);
        state.observe(TransportEvent::WebSocket {
            reused: false,
            incremental: false,
        });
        state.observe(TransportEvent::HttpCompleted {
            refusal: WsRefusal::Definitive,
        });
        assert!(state == Capability::Supported(1));
        state.observe(TransportEvent::HttpCompleted {
            refusal: WsRefusal::Definitive,
        });
        assert!(state == Capability::Unsupported);
    }
}
