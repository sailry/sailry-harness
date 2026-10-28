//! Append reasoning changes to canonical input without rewriting request configuration.
use super::*;
use adk_core::{
    ConfigurationUpdate, Event, LlmRequest, LlmResponseStream, Part, ProviderData, ProviderOrigin,
    ReasoningEffort, SchemaAdapter,
};
use adk_session::{GetRequest, SessionService};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct State {
    base: Effort,
    current: Effort,
}

#[derive(Clone)]
pub(crate) struct Capture {
    origin: ProviderOrigin,
    key: String,
    state: State,
    previous: Effort,
}

impl Capture {
    pub(crate) const PREFIX: &str = "sailry.responses.configuration.";

    pub(crate) fn key(origin: &ProviderOrigin) -> String {
        format!(
            "{}{hash}",
            Self::PREFIX,
            hash = blake3::hash(&serde_json::to_vec(origin).expect("provider origin serializes"))
                .to_hex()
        )
    }

    pub(crate) fn compact_state(
        value: serde_json::Value,
        current: Option<Effort>,
    ) -> Result<serde_json::Value, Fault> {
        let mut state: State = serde_json::from_value(value)
            .map_err(|_| Fault::invalid("captured reasoning configuration is invalid"))?;
        state.current = current.unwrap_or(state.base);
        Ok(serde_json::to_value(state).expect("reasoning configuration serializes"))
    }

    pub(in crate::agent) async fn load(
        ingress: &Arc<Ingress>,
        turn: Option<TurnId>,
        session: sailry_protocol::SessionId,
        origin: ProviderOrigin,
        current: Effort,
        supported: bool,
    ) -> Result<Option<Self>, Fault> {
        let Some(turn) = turn.filter(|_| supported && discrete(current).is_some()) else {
            return Ok(None);
        };
        let session = ingress
            .sessions(turn)
            .get(GetRequest {
                app_name: APP.into(),
                user_id: USER.into(),
                session_id: session.to_string(),
                num_recent_events: Some(0),
                after: None,
            })
            .await
            .map_err(super::error)?;
        let key = Self::key(&origin);
        let mut state = session
            .state()
            .get(&key)
            .map(serde_json::from_value::<State>)
            .transpose()
            .map_err(|_| Fault::invalid("captured reasoning configuration is invalid"))?
            .unwrap_or(State {
                base: current,
                current,
            });
        let previous = state.current;
        state.current = current;
        Ok(Some(Self {
            origin,
            key,
            state,
            previous,
        }))
    }

    pub(in crate::agent) fn base(&self) -> Effort {
        self.state.base
    }

    pub(in crate::agent) fn input(&self) -> Option<Part> {
        (self.previous != self.state.current).then(|| self.part())
    }

    fn part(&self) -> Part {
        Part::ProviderData {
            provider_data: ProviderData {
                origin: Some(self.origin.clone()),
                parts: Box::new([Part::ConfigurationUpdate {
                    configuration_update: ConfigurationUpdate {
                        effort: discrete(self.state.current).expect("captured effort is discrete"),
                    },
                }]),
            },
        }
    }

    pub(crate) fn record(&self, event: &mut Event) {
        if event.author == "user" {
            event.actions.state_delta.insert(
                self.key.clone(),
                serde_json::to_value(&self.state).expect("reasoning configuration serializes"),
            );
        }
    }

    pub(in crate::agent) fn compact(&self, event: &mut Event) {
        event.actions.state_delta.insert(
            self.key.clone(),
            serde_json::to_value(&self.state).expect("reasoning configuration serializes"),
        );
        if (self.state.current != self.state.base || self.previous != self.state.current)
            && let Some(compaction) = &mut event.actions.compaction
        {
            compaction.compacted_content.parts.insert(0, self.part());
        }
    }

    fn restore(&self, request: &mut LlmRequest) {
        let current = request
            .contents
            .iter()
            .flat_map(|content| &content.parts)
            .filter_map(|part| match part {
                Part::ProviderData { provider_data }
                    if provider_data.origin.as_ref() == Some(&self.origin) =>
                {
                    match &provider_data.parts[0] {
                        Part::ConfigurationUpdate {
                            configuration_update,
                        } => Some(configuration_update.effort),
                        _ => None,
                    }
                }
                _ => None,
            })
            .last()
            .or_else(|| discrete(self.state.base));
        if current == discrete(self.state.current) {
            return;
        }
        if let Some(content) = request.contents.last_mut() {
            // A rebuilt prefix may already contain an older update. Preserve
            // that prefix and apply the captured setting after its final item.
            content.parts.push(self.part());
            super::history::clear_prepared(request);
        }
    }
}

fn discrete(effort: Effort) -> Option<ReasoningEffort> {
    Some(match effort {
        Effort::Disabled => ReasoningEffort::None,
        Effort::Minimal => ReasoningEffort::Minimal,
        Effort::Low => ReasoningEffort::Low,
        Effort::Medium => ReasoningEffort::Medium,
        Effort::High => ReasoningEffort::High,
        Effort::XHigh => ReasoningEffort::XHigh,
        Effort::Max => ReasoningEffort::Max,
        Effort::Default | Effort::Budget(_) => return None,
    })
}

pub(super) fn supported(
    provider: &Provider,
    model: Option<&sailry_protocol::conversation::Model>,
) -> bool {
    provider.api == ModelApi::Responses
        && model
            .and_then(|model| model.capabilities.as_ref())
            .and_then(|capabilities| capabilities.configuration_updates)
            == Some(true)
}

struct Model {
    inner: Arc<dyn Llm>,
    capture: Option<Capture>,
}

pub(super) fn wrap(inner: Arc<dyn Llm>, capture: Option<Capture>) -> Arc<dyn Llm> {
    Arc::new(Model { inner, capture })
}

#[async_trait]
impl Llm for Model {
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
        mut request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        if let Some(capture) = &self.capture {
            capture.restore(&mut request);
        } else {
            let mut changed = false;
            for content in &mut request.contents {
                content.parts.retain(|part| {
                    let keep = !matches!(part.provider_part(), Part::ConfigurationUpdate { .. });
                    changed |= !keep;
                    keep
                });
            }
            if changed {
                super::history::clear_prepared(&mut request);
            }
        }
        self.inner.generate_content(request, stream).await
    }
}

#[cfg(test)]
mod tests;
