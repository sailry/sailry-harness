use super::*;
use std::sync::{Arc, Mutex};
use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::{Layer, layer::SubscriberExt as _};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<serde_json::Value>>>);

#[derive(Default)]
struct Fields(serde_json::Map<String, serde_json::Value>);

#[test]
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "expires_read_waits_without_uncertain_effects",
            (|scenario: &'static str| {
                let request = RequestId::new();
                let read = Command::ReadGitChange {
                    worktree: WorktreeId::new(),
                    snapshot: GitChangeSnapshot {
                        target: GitChangeState::Worktree {
                            scope: GitDiffScope::All,
                            head: None,
                            index_revision: "a".repeat(64),
                        },
                        metadata_revision: "b".repeat(64),
                    },
                    path: "file.rs".into(),
                    expected_metadata_revision: "c".repeat(64),
                    expected_revision: None,
                    offset: 0,
                    limit: 500,
                };
                assert!(!read.durable(), "{scenario}");
                assert_eq!(
                    interrupted(read.durable(), request, true).code,
                    ErrorCode::Expired,
                    "{scenario}"
                );
                assert_eq!(
                    interrupted(true, request, true).code,
                    ErrorCode::OutcomeUnknown,
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "retains_failed_persistence_identity",
            (|scenario: &'static str| {
                use notification::{SystemDiagnostic, SystemSource};
                let capture = Capture::default();
                let dispatch =
                    tracing::Dispatch::new(tracing_subscriber::registry().with(capture.clone()));
                let mut report = SystemDiagnostic::new(
                    RequestId::new(),
                    SystemSource::PluginCallback,
                    ErrorCode::OutcomeUnknown,
                );
                report.request = Some(RequestId::new());
                report.session = Some(SessionId::new());
                report.turn = Some(TurnId::new());
                report.worktree = Some(WorktreeId::new());
                let info: plugin::Info = serde_json::from_value(serde_json::json!({
                "summary":{"name":"fixture","revision":1,"digest":"a".repeat(64),
                    "settings_revision":2,"enabled":true,"version":"0.1.0","description":null},
                "origin":null,"icon":null,"extension":null,"settings":null
                    }))
                .unwrap();
                tracing::dispatcher::with_default(&dispatch, || {
                    persist_failed(
                        NodeId([1; 32]),
                        NodeId([2; 32]),
                        &report,
                        &info,
                        &Fault::new(ErrorCode::Unavailable, "PRIVATE_ERROR_WITH_CREDENTIAL"),
                    );
                });
                let records = capture.0.lock().unwrap();
                assert_eq!(records.len(), 1, "{scenario}");
                let record = &records[0];
                assert_eq!(record["target"], "sailry", "{scenario}");
                assert_eq!(record["level"], "WARN", "{scenario}");
                assert_eq!(record["area"], "plugin", "{scenario}");
                assert_eq!(record["phase"], "failed", "{scenario}");
                assert_eq!(record["operation"], "diagnostic_persist", "{scenario}");
                assert_eq!(record["stage"], "store", "{scenario}");
                assert_eq!(record["code"], "unavailable", "{scenario}");
                assert_eq!(
                    record["incident"],
                    report.incident.to_string(),
                    "{scenario}"
                );
                assert_eq!(
                    record["request"],
                    report.request.unwrap().to_string(),
                    "{scenario}"
                );
                assert_eq!(
                    record["session"],
                    report.session.unwrap().to_string(),
                    "{scenario}"
                );
                assert_eq!(
                    record["turn"],
                    report.turn.unwrap().to_string(),
                    "{scenario}"
                );
                assert_eq!(
                    record["worktree"],
                    report.worktree.unwrap().to_string(),
                    "{scenario}"
                );
                assert_eq!(record["package"], "fixture", "{scenario}");
                assert_eq!(record["node"], "01".repeat(32), "{scenario}");
                assert_eq!(record["caller"], "02".repeat(32), "{scenario}");
                assert!(!record.to_string().contains("PRIVATE_"), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if !value.is_empty() {
            self.0.insert(field.name().into(), value.into());
        }
    }
}

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        fields
            .0
            .insert("target".into(), event.metadata().target().into());
        fields
            .0
            .insert("level".into(), event.metadata().level().as_str().into());
        self.0.lock().unwrap().push(fields.0.into());
    }
}
