//! Native search usage shares the canonical history writer and ordinary statistics.
use super::{Ingress, Job, unavailable};
use crate::agent::canonical;
use sailry_protocol::{Fault, SessionId, TurnId, conversation};
use tokio::sync::oneshot;

impl Ingress {
    pub(crate) async fn record_search_usage(
        &self,
        session: SessionId,
        turn: TurnId,
        provider: &conversation::Provider,
        model: &str,
        usage: &canonical::Usage,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(
            crate::store::agent::Operation::Append {
                session,
                turn,
                event: event(turn, provider, model, usage),
                context: None,
                reply,
            },
        )))
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
}

fn event(
    turn: TurnId,
    provider: &conversation::Provider,
    model: &str,
    usage: &canonical::Usage,
) -> canonical::Record {
    canonical::Record {
        id: sailry_protocol::RequestId::new().to_string(),
        created: chrono::Utc::now(),
        turn,
        provenance: canonical::Provenance {
            author: "system".into(),
            branch: String::new(),
        },
        metadata: canonical::Metadata {
            execution: Some(canonical::Execution {
                purpose: Some(canonical::Purpose::Search),
                source: Some(canonical::ModelSource {
                    id: provider.id,
                    name: provider.name.clone(),
                    api: provider.api,
                    authentication: provider.authentication,
                    model: model.into(),
                }),
                ..Default::default()
            }),
            runtime: Some(canonical::Runtime {
                user_visible: false,
                agent_visible: false,
                ..Default::default()
            }),
            ..Default::default()
        },
        kind: canonical::Kind::Message(canonical::Message {
            model: Some(model.into()),
            usage: Some(usage.clone()),
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{Authentication, ProviderId, conversation::ModelApi};
    use serde_json::json;

    #[test]
    fn retains_only_model_identity() {
        let provider: conversation::Provider = serde_json::from_value(json!({
            "id": ProviderId::new(),
            "revision": 1,
            "name": "Search provider",
            "api": ModelApi::Responses,
            "authentication": Authentication::ApiKey,
            "endpoint": "https://search.invalid/v1?token=protected-endpoint",
            "enabled": true,
            "default_model": "search-model",
            "credential": null
        }))
        .unwrap();
        let turn = TurnId::new();
        let usage = canonical::Usage {
            input: Some(10),
            output: Some(4),
            cache_read: Some(2),
            cache_write: Some(3),
            reasoning: Some(1),
            ..Default::default()
        };
        let event = event(turn, &provider, "search-model", &usage);
        canonical::validate(&event).unwrap();
        assert_eq!(event.turn, turn);
        assert_eq!(event.provenance.author, "system");
        let runtime = event.metadata.runtime.as_ref().unwrap();
        assert!(!runtime.user_visible);
        assert!(!runtime.agent_visible);
        let body = serde_json::to_value(&event).unwrap();
        assert_eq!(body["metadata"]["execution"]["purpose"], "search");
        assert_eq!(
            body["metadata"]["execution"]["source"],
            json!({
                "id": provider.id,
                "name": "Search provider",
                "api": "responses",
                "authentication": "api_key",
                "model": "search-model"
            })
        );
        assert_eq!(body["model"], "search-model");
        assert_eq!(
            body["usage"],
            json!({"input":10,"output":4,"cache_read":2,"cache_write":3,"reasoning":1})
        );
        assert!(!body.to_string().contains("protected-endpoint"));
        assert!(event.message().unwrap().content.is_empty());
    }
}
