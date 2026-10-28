use super::super::*;
use std::{collections::BTreeMap, sync::Mutex};
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::{Layer, layer::SubscriberExt};

struct Complete;

#[async_trait]
impl Llm for Complete {
    fn name(&self) -> &str {
        "completion-fixture"
    }

    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        Ok(Box::pin(futures::stream::iter([Ok(LlmResponse {
            content: Some(Content::new("model").with_text("Done")),
            turn_complete: true,
            ..Default::default()
        })])))
    }
}

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
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields
            .0
            .get("operation")
            .is_some_and(|value| value == "model_recovery")
        {
            self.0.lock().unwrap().push(fields.0);
        }
    }
}

#[tokio::test]
async fn final_response_survives_immediate_stream_drop() {
    let profile = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(profile.path().join("node"))
        .await
        .unwrap();
    let model = Model {
        inner: Arc::new(Complete),
        ingress: node.ingress.clone(),
        turn: None,
        session: SessionId::new(),
        stop: CancellationToken::new(),
        limit: 1,
        compaction: false,
    };
    let records = Arc::new(Mutex::new(Vec::new()));
    let dispatch =
        tracing::Dispatch::new(tracing_subscriber::registry().with(Records(records.clone())));
    async {
        let mut responses = model
            .generate_content(LlmRequest::new("completion-fixture", Vec::new()), true)
            .await
            .unwrap();
        assert!(responses.next().await.unwrap().unwrap().turn_complete);
        drop(responses);
    }
    .with_subscriber(dispatch)
    .await;
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["phase"], "begin");
    assert_eq!(records[1]["phase"], "completed");
    assert_eq!(records[1]["recovery_decision"], "succeeded");
    assert_eq!(records[1]["code"], "");
    drop(records);
    node.shutdown().await.unwrap();
}
