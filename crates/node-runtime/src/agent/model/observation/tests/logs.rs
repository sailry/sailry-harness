use super::*;
use adk_core::AdkError;
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
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
}

#[test]
fn retains_failed_cause_after_recovery() {
    let records = Arc::new(Mutex::new(Vec::new()));
    let dispatch =
        tracing::Dispatch::new(tracing_subscriber::registry().with(Records(records.clone())));
    let capture = capture(None);
    let call = capture.lock().unwrap().context.call.unwrap().to_string();
    let mut error = AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Unavailable,
        "model.http.connection",
        "PRIVATE_CONNECT_MESSAGE",
    )
    .with_upstream_status(503);
    error
        .details
        .metadata
        .insert("body".into(), "PRIVATE_BODY".into());
    tracing::dispatcher::with_default(&dispatch, || {
        let first = Capture::observer(&capture);
        started(&first);
        first(wire::Event {
            attempt: 1,
            kind: wire::Kind::Finished {
                elapsed: Duration::from_millis(2),
                outcome: wire::Outcome::Failed,
                failure: Some((&error).into()),
            },
        });
        finished(&first, wire::Outcome::Failed);
        let next = Capture::observer(&capture);
        started(&next);
        finished(&next, wire::Outcome::Succeeded);
        assert!(
            records.lock().unwrap().is_empty(),
            "callbacks cannot write diagnostics"
        );
        let pending = capture.lock().unwrap().diagnostics();
        for entry in pending {
            entry.emit();
        }
        assert!(capture.lock().unwrap().diagnostics().is_empty());
    });
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 4);
    assert!(
        records
            .iter()
            .all(|record| record["call"] == call && record["operation"] == "model_attempt")
    );
    assert_eq!(records[1]["phase"], "failed");
    assert_eq!(records[1]["source_kind"], "deferred_observation");
    assert_eq!(records[1]["attempt"], "1");
    assert_eq!(records[1]["duration_us"], "2000");
    assert_eq!(records[1]["cause_code"], "model.http.connection");
    assert_eq!(records[1]["cause_category"], "unavailable");
    assert_eq!(records[1]["upstream_status"], "503");
    assert_eq!(records[3]["phase"], "completed");
    assert_eq!(records[3]["attempt"], "2");
    assert_eq!(records[3]["cause_code"], "");
    assert!(
        !serde_json::to_string(&*records)
            .unwrap()
            .contains("PRIVATE_")
    );
}
