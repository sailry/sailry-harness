//! Session sockets and endpoint observations are Node-private transport optimizations.
use super::*;
use adk_model::openai::{TransportEvent, WsOptions, WsRefusal};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

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
    fallback: Mutex<BTreeSet<sailry_protocol::SessionId>>,
    #[cfg(any(test, feature = "test-support"))]
    fixture: Mutex<Option<WsOptions>>,
}

impl Capabilities {
    pub(super) fn fallback(&self, session: sailry_protocol::SessionId) -> bool {
        self.fallback.lock().unwrap().insert(session)
    }

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
        #[cfg(any(test, feature = "test-support"))]
        if !(options.enabled)() {
            return None;
        }
        Some(WsOptions {
            fallback_to_http: false,
            enabled: Arc::new(move || {
                !enabled.fallback.lock().unwrap().contains(&session)
                    && enabled.states.lock().unwrap().get(&permit) != Some(&Capability::Unsupported)
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
                if matches!(event, TransportEvent::HttpCompleted { .. }) {
                    return;
                }
                let mut entry = crate::logging::Entry::new(
                    crate::logging::Area::Model,
                    crate::logging::Phase::Selected,
                );
                entry.context = super::call::current().unwrap_or_else(|| identity.clone());
                // Preconnect has no logical generation; request selections reuse
                // the identity scoped around both generation and stream polling.
                entry.stage = Some(if entry.context.call.is_some() {
                    "request"
                } else {
                    "preconnect"
                });
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
                    TransportEvent::Fallback { refusal } => {
                        entry.transport = Some("sse");
                        entry.fallback_reason = Some(match refusal {
                            WsRefusal::Definitive => "refused",
                            WsRefusal::Transient => "transient",
                        });
                    }
                    TransportEvent::HttpCompleted { .. } => unreachable!(),
                }
                entry.emit();
            }),
            ..options
        })
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn configure_fixture(registry: &registry::Registry, options: WsOptions) {
    registry.capabilities.configure_fixture(options);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn logs_one_fallback_selection() {
        use std::collections::BTreeMap;
        use tracing_subscriber::{Layer, layer::SubscriberExt};
        #[derive(Default)]
        struct Fields(BTreeMap<String, String>);
        impl tracing::field::Visit for Fields {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                self.0.insert(field.name().into(), value.into());
            }
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.insert(field.name().into(), format!("{value:?}"));
            }
        }
        struct Records(Arc<Mutex<Vec<BTreeMap<String, String>>>>);
        impl<S: tracing::Subscriber> Layer<S> for Records {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _: tracing_subscriber::layer::Context<'_, S>,
            ) {
                let mut fields = Fields::default();
                event.record(&mut fields);
                self.0.lock().unwrap().push(fields.0);
            }
        }
        let provider: Provider = serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"Fixture", "api":"responses",
            "authentication":"api_key","endpoint":"https://fixture.invalid/v1","enabled":true,
            "models":[],"default_model":"fixture"
        })).unwrap();
        let capabilities = Arc::new(Capabilities::default());
        let options = capabilities
            .options(&provider, "fixture", sailry_protocol::SessionId::new())
            .unwrap();
        let records = Arc::new(Mutex::new(Vec::new()));
        let dispatch =
            tracing::Dispatch::new(tracing_subscriber::registry().with(Records(records.clone())));
        let identity = super::super::call::identity(crate::logging::Context::default());
        super::super::call::scope(identity.clone(), async {
            tracing::dispatcher::with_default(&dispatch, || {
                (options.observe)(TransportEvent::Fallback {
                    refusal: WsRefusal::Definitive,
                });
                assert!((options.enabled)());
                (options.observe)(TransportEvent::HttpCompleted {
                    refusal: WsRefusal::Definitive,
                });
                assert!(!(options.enabled)());
            });
        })
        .await;
        {
            let records = records.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0]["call"], identity.call.unwrap().to_string());
            assert_eq!(records[0]["phase"], "selected");
            assert_eq!(records[0]["stage"], "request");
            assert_eq!(records[0]["fallback_reason"], "refused");
        }
        tracing::dispatcher::with_default(&dispatch, || (options.observe)(TransportEvent::Http));
        let records = records.lock().unwrap();
        assert_eq!(records[1]["call"], "");
        assert_eq!(records[1]["phase"], "selected");
        assert_eq!(records[1]["stage"], "preconnect");
    }

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
