use super::*;
use serde_json::{Value, json};
use std::{
    io::Write as _,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

#[path = "../../tests/logging/parse.rs"]
mod parse;

fn lines(directory: &Path) -> Vec<Value> {
    std::fs::read_dir(directory)
        .unwrap()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if !path.is_file() || path.extension().is_none_or(|extension| extension != "log") {
                return Vec::new();
            }
            std::fs::read_to_string(path)
                .unwrap()
                .lines()
                .map(parse::line)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn filters_payloads_and_upstream_events() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Desktop));
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::error!(target: "sailry", area = "desktop", phase = "failed", reason = "fixture", body = "PRIVATE_BODY", credential = "PRIVATE_KEY", url = "PRIVATE_URL", error = ?"PRIVATE_ERROR", code = ?"PRIVATE_CODE");
        tracing::error!(target: "sailry", input = "PRIVATE_INPUT", output = "PRIVATE_OUTPUT", headers = "PRIVATE_AUTHORIZATION", query = "PRIVATE_SQL", path = "PRIVATE_PATH", url = "https://fixture.invalid/?token=PRIVATE_QUERY_TOKEN");
        tracing::error!(target: "reqwest::headers", "PRIVATE_HTTP_HEADER");
        tracing::error!(target: "gpui_shell::script", "PRIVATE_CONSOLE_VALUE");
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["reason"], "fixture");
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
}

#[test]
fn records_typed_artifact_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Desktop));
    let artifact = sailry_protocol::ArtifactId::new();
    tracing::dispatcher::with_default(&dispatch, || {
        let mut entry = Entry::new(Area::Artifact, Phase::Failed);
        entry.context.artifact = Some(artifact);
        entry.revision = Some(9);
        entry.reason = Some("render");
        entry.emit_debug();
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["artifact_id"], artifact.to_string());
    assert_eq!(records[0]["revision"], 9);
    assert_eq!(records[0]["level"], "DEBUG");
}

#[test]
fn records_safe_model_causes_without_empty_fields() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    tracing::dispatcher::with_default(&dispatch, || {
        let mut entry = Entry::new(Area::Model, Phase::Failed);
        entry.operation = Some("conversation");
        entry.stage = Some("request");
        entry.attempt = Some(2);
        entry.limit = Some(5);
        entry.request_mode = Some("interactions");
        entry.transport = Some("sse");
        entry.connection_reused = Some(false);
        entry.fallback_reason = Some("refused");
        entry.continuation = Some("full");
        entry.stream = Some(true);
        entry.budget = Some(4096);
        entry.parse_layer = Some("stream_event");
        entry.parse_category = Some("data");
        entry.parse_cause = Some("missing_field");
        entry.parse_field = Some("sequence_number");
        entry.parse_event = Some("response.output_text.delta".into());
        entry.parse_line = Some(7);
        entry.parse_column = Some(11);
        entry.cause_code = Some("model.fixture.rate_limited");
        entry.cause_component = Some("model");
        entry.cause_category = Some("rate_limited");
        entry.upstream_status = Some(429);
        entry.emit();
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record["operation"], "conversation");
    assert_eq!(record["stage"], "request");
    assert_eq!(record["cause_code"], "model.fixture.rate_limited");
    assert_eq!(record["cause_component"], "model");
    assert_eq!(record["cause_category"], "rate_limited");
    assert_eq!(record["upstream_status"], 429);
    assert_eq!(record["attempt"], 2);
    assert_eq!(record["limit"], 5);
    assert_eq!(record["request_mode"], "interactions");
    assert_eq!(record["transport"], "sse");
    assert_eq!(record["connection_reused"], false);
    assert_eq!(record["fallback_reason"], "refused");
    assert_eq!(record["continuation"], "full");
    assert_eq!(record["stream"], true);
    assert_eq!(record["budget"], 4096);
    assert_eq!(record["parse_layer"], "stream_event");
    assert_eq!(record["parse_category"], "data");
    assert_eq!(record["parse_cause"], "missing_field");
    assert_eq!(record["parse_field"], "sequence_number");
    assert_eq!(record["parse_event"], "response.output_text.delta");
    assert_eq!(record["parse_line"], 7);
    assert_eq!(record["parse_column"], 11);
    for absent in [
        "node",
        "package",
        "digest",
        "settings_revision",
        "command",
        "source_kind",
        "reason",
    ] {
        assert!(
            !record.as_object().unwrap().contains_key(absent),
            "{absent}"
        );
    }
    assert!(!serde_json::to_string(record).unwrap().contains("PRIVATE_"));
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Emission>>>);

struct Emission {
    level: tracing::Level,
    area: String,
    line: String,
}

impl format::Sink for Capture {
    fn emit(&self, level: tracing::Level, area: &str, line: &str) {
        self.0.lock().unwrap().push(Emission {
            level,
            area: area.into(),
            line: line.into(),
        });
    }
}

#[test]
fn shares_sanitized_fields_with_native_sink() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let capture = Capture::default();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(format::Safe {
                    application: "host",
                    sink: capture.clone(),
                })
                .with_writer(writer)
                .with_ansi(false),
        ),
    );
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::warn!(target: "sailry", area = "model", phase = "failed", provider_name = "Safe \"quoted\" name=fixture\\value\n\0\u{1b}", model = "false", budget = -1_i64, stream = false, count = 0_u64, reason = "", error = ?"PRIVATE_ERROR", code = ?"PRIVATE_DEBUG", body = "PRIVATE_BODY", "PRIVATE_MESSAGE");
        tracing::info!(target: "sailry", area = "model", budget = 0_i64, stream = true, reason = "\n\0\u{1b}");
    });
    drop(worker);
    let text = std::fs::read_to_string(latest_file(directory.path()).unwrap().unwrap()).unwrap();
    let emitted = capture.0.lock().unwrap();
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[0].level, tracing::Level::WARN);
    assert_eq!(emitted[0].area, "model");
    assert_eq!(emitted[1].level, tracing::Level::INFO);
    assert_eq!(emitted[1].area, "model");
    assert!(!text.contains("PRIVATE_"));
    assert!(
        !text
            .chars()
            .any(|character| character.is_control() && character != '\n')
    );
    let records = lines(directory.path());
    for (record, emission) in records.iter().zip(emitted.iter()) {
        let mut native = parse::line(&format!(
            "1970-01-01T00:00:00Z {} host {}",
            emission.level, emission.line
        ));
        let mut file = record.clone();
        assert!(native.get("area").is_none());
        for key in ["timestamp", "level", "application", "area"] {
            native.as_object_mut().unwrap().remove(key);
            file.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(native, file, "both sinks preserve the same safe fields");
        assert!(!emission.line.contains("PRIVATE_"));
        assert!(!emission.line.contains("1970-"));
        assert!(!emission.line.contains(" WARN "));
        assert!(!emission.line.contains(" host "));
    }
    assert_eq!(
        records[0]["provider_name"],
        "Safe \"quoted\" name=fixture\\value"
    );
    assert_eq!(records[0]["model"], "false");
    assert_eq!(records[0]["budget"], -1);
    assert_eq!(records[0]["stream"], false);
    assert_eq!(records[0]["count"], 0);
    assert_eq!(records[1]["budget"], 0);
    assert_eq!(records[1]["stream"], true);
    assert!(records.iter().all(|record| record.get("reason").is_none()));
    assert!(text.contains("budget=-1"));
    assert!(text.contains("budget=0"));
    assert!(text.contains("model=\"false\""));
}

#[test]
fn records_correlated_recovery_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    let incident = sailry_protocol::RequestId::new();
    let call = sailry_protocol::RequestId::new();
    tracing::dispatcher::with_default(&dispatch, || {
        let mut entry = Entry::new(Area::Model, Phase::Retrying);
        entry.context.incident = Some(incident);
        entry.context.call = Some(call);
        entry.communication_outcome = Some("completed");
        entry.execution_outcome = Some("unknown");
        entry.recovery_decision = Some("snapshot");
        entry.wait_ms = Some(1000);
        entry.end_to_end_us = Some(3500);
        entry.rtt_ms = Some(2);
        entry.generation = Some(4);
        entry.sequence = Some(19);
        entry.emit();
        entry.phase = Phase::Selected;
        entry.emit();
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records.len(), 2);
    for (record, phase) in records.iter().zip(["retrying", "selected"]) {
        assert_eq!(record["phase"], phase);
        assert_eq!(record["level"], "INFO");
        assert_eq!(record["incident"], incident.to_string());
        assert_eq!(record["call"], call.to_string());
        assert_eq!(record["communication_outcome"], "completed");
        assert_eq!(record["execution_outcome"], "unknown");
        assert_eq!(record["recovery_decision"], "snapshot");
        assert_eq!(record["wait_ms"], 1000);
        assert_eq!(record["end_to_end_us"], 3500);
        assert_eq!(record["rtt_ms"], 2);
        assert_eq!(record["generation"], 4);
        assert_eq!(record["sequence"], 19);
    }
}

#[test]
fn records_complete_public_node_ids() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    tracing::dispatcher::with_default(&dispatch, || {
        let mut entry = Entry::new(Area::Command, Phase::Begin);
        entry.context.node = Some(sailry_protocol::NodeId([1; 32]));
        entry.context.caller = Some(sailry_protocol::NodeId([255; 32]));
        entry.context.request = Some(sailry_protocol::RequestId::new());
        entry.context.provider_name = Some("x".repeat(300));
        entry.emit();
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records[0]["node"], "01".repeat(32));
    assert_eq!(records[0]["caller"], "ff".repeat(32));
    assert_eq!(records[0]["provider_name"], "x".repeat(256));
    assert_eq!(records[0]["request"].as_str().unwrap().len(), 36);
}

#[test]
fn bounds_oversized_unicode_fields_without_losing_failures() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    tracing::dispatcher::with_default(&dispatch, || {
        let mut entry = Entry::new(Area::Model, Phase::Failed);
        entry.context.model = Some("🙂".repeat(MAX_FILE_BYTES));
        entry.code = Some(sailry_protocol::ErrorCode::OutcomeUnknown);
        entry.emit();
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["model"].as_str().unwrap().chars().count(), 256);
    assert_eq!(records[0]["phase"], "failed");
    assert_eq!(records[0]["code"], "outcome_unknown");
    assert!(
        std::fs::metadata(latest_file(directory.path()).unwrap().unwrap())
            .unwrap()
            .len()
            <= MAX_FILE_BYTES as u64
    );
}

#[test]
fn retains_read_failures_and_slow_completions() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    tracing::dispatcher::with_default(&dispatch, || {
        let node = sailry_protocol::NodeId([1; 32]);
        let request = sailry_protocol::Request::new(node, sailry_protocol::Command::ListSsh);
        let mut fast = commands::begin(node, node, &request).unwrap();
        fast.record(Phase::Admitted);
        fast.finish(Phase::Completed, None);
        let mut failed = commands::begin(node, node, &request).unwrap();
        failed.finish(Phase::Failed, Some(sailry_protocol::ErrorCode::Busy));
        let mut slow = commands::begin(node, node, &request).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        slow.finish(Phase::Completed, None);
        let mut unknown = commands::begin(node, node, &request).unwrap();
        unknown.finish(
            Phase::Interrupted,
            Some(sailry_protocol::ErrorCode::OutcomeUnknown),
        );
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["code"], "busy");
    assert_eq!(records[1]["phase"], "completed");
    assert!(records[1]["duration_us"].as_u64().unwrap() >= 150_000);
    assert_eq!(records[2]["level"], "WARN");
    assert!(records.iter().all(|record| record["command"] == "list_ssh"));
}

#[test]
fn retains_cancelled_cleanup_uncertainty() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    tracing::dispatcher::with_default(&dispatch, || {
        let mut entry = Entry::new(Area::Run, Phase::Cancelled);
        entry.status = Some(sailry_protocol::conversation::Status::Cancelled);
        entry.code = Some(sailry_protocol::ErrorCode::OutcomeUnknown);
        entry.emit();
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records[0]["phase"], "cancelled");
    assert_eq!(records[0]["status"], "cancelled");
    assert_eq!(records[0]["code"], "outcome_unknown");
    assert_eq!(records[0]["level"], "WARN");
}

#[test]
fn flushes_concurrent_complete_lines() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    let workers = (0..4)
        .map(|_| {
            let dispatch = dispatch.clone();
            std::thread::spawn(move || {
                tracing::dispatcher::with_default(&dispatch, || {
                    for index in 0..32 {
                        let mut entry = Entry::new(Area::Desktop, Phase::Begin);
                        entry.count = Some(index);
                        entry.emit();
                    }
                })
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records.len(), 128);
    assert!(
        records
            .iter()
            .all(|record| record["application"] == "host" && record["count"].is_u64())
    );
}

#[test]
fn retires_only_owned_dates() {
    let directory = tempfile::tempdir().unwrap();
    for day in 1..=10 {
        std::fs::write(directory.path().join(format!("2020-01-{day:02}.log")), "").unwrap();
    }
    std::fs::write(directory.path().join("notes.txt"), "preserve").unwrap();
    std::fs::write(directory.path().join("2020-01-01"), "legacy").unwrap();
    std::fs::create_dir(directory.path().join("2020-01-11.log")).unwrap();
    rolling::retire(directory.path()).unwrap();
    let (mut writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    writer.write_all(b"owned diagnostic\n").unwrap();
    drop(worker);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("notes.txt")).unwrap(),
        "preserve"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("2020-01-01")).unwrap(),
        "legacy"
    );
    assert!(directory.path().join("2020-01-11.log").is_dir());
    let owned = std::fs::read_dir(directory.path())
        .unwrap()
        .filter(|entry| {
            let entry = entry.as_ref().unwrap();
            entry.file_type().unwrap().is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "log")
        })
        .count();
    assert!(owned <= RETAINED_FILES);
}

#[test]
fn resolves_active_file() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    drop(worker);
    drop(writer);
    let actual = latest_file(directory.path()).unwrap().unwrap();
    assert_eq!(actual.extension().unwrap(), "log");
    assert_eq!(actual.file_name().unwrap(), rolling::CURRENT);
    for name in ["2099-01-01", "2099-02-30.log", "notes.log"] {
        std::fs::write(directory.path().join(name), "").unwrap();
    }
    std::fs::create_dir(directory.path().join("2099-01-01.log")).unwrap();
    assert_eq!(latest_file(directory.path()).unwrap(), Some(actual));
}

#[test]
fn ignores_other_application_files() {
    let directory = tempfile::tempdir().unwrap();
    let desktop = directory.path().join("desktop");
    let host = directory.path().join("host");
    std::fs::create_dir_all(&desktop).unwrap();
    std::fs::create_dir_all(&host).unwrap();
    std::fs::write(desktop.join("2020-01-01.log"), "").unwrap();
    std::fs::write(desktop.join("2020-01-02.log"), "").unwrap();
    std::fs::write(host.join("2099-01-01.log"), "").unwrap();
    assert_eq!(
        latest_file(&desktop).unwrap(),
        Some(desktop.join("2020-01-02.log"))
    );
}

struct Held {
    started: std::sync::mpsc::Sender<()>,
    release: Arc<(Mutex<bool>, Condvar)>,
}
impl std::io::Write for Held {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let _ = self.started.send(());
        let (released, changed) = &*self.release;
        let mut released = released.lock().unwrap();
        while !*released {
            released = changed.wait(released).unwrap();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn reports_bounded_queue_loss() {
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let (started, received) = std::sync::mpsc::channel();
    let (mut writer, worker) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(1)
        .lossy(true)
        .finish(Held {
            started,
            release: release.clone(),
        });
    writer.write_all(b"first\n").unwrap();
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    for _ in 0..32 {
        writer.write_all(b"queued\n").unwrap();
    }
    assert!(writer.error_counter().dropped_lines() > 0);
    let (released, changed) = &*release;
    *released.lock().unwrap() = true;
    changed.notify_all();
    drop(worker);
}

#[test]
fn records_script_errors_without_values() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    let result = tracing::dispatcher::with_default(&dispatch, || {
        let context = sailry_protocol::plugin::Context {
            package: sailry_protocol::plugin::Reference {
                name: "fixture".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            invocation: Some(sailry_protocol::RequestId::new()),
            session: None,
            turn: None,
            worktree: None,
            surface: Default::default(),
        };
        crate::plugins::script::run(
            crate::plugins::script::Bundle {
                entry: "main.js".into(),
                files: std::collections::BTreeMap::from([(
                    "main.js".into(),
                    "export function run(input) { throw { message: input.private }; }".into(),
                )]),
            },
            "run",
            json!({"private":"PRIVATE_PLUGIN_VALUE"}),
            context,
            sailry_protocol::NodeId([1; 32]),
            sailry_link::CancellationToken::new(),
            |_| panic!("unexpected SDK call"),
        )
    });
    assert!(result.is_err());
    drop(worker);
    let records = lines(directory.path());
    assert!(
        records
            .iter()
            .any(|record| record["area"] == "plugin" && record["phase"] == "failed")
    );
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_PLUGIN_VALUE")
    );
}

#[test]
fn records_provider_http_metadata() {
    use sailry_protocol::{
        Command, ErrorCode, Fault, NodeId, Output, ProviderId, Receipt, Request, plugin,
    };

    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    let node = NodeId([1; 32]);
    let provider = ProviderId::new();
    let request = Request::new(
        node,
        Command::RequestProviderHttp {
            provider,
            expected_revision: 7,
            request: "usage".into(),
            input: Default::default(),
        },
    )
    .with_plugin(plugin::Context {
        package: plugin::Reference {
            name: "fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        invocation: None,
        session: None,
        turn: None,
        worktree: None,
        surface: Default::default(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    tracing::dispatcher::with_default(&dispatch, || {
        runtime.block_on(async {
            for status in [200, 403, 429] {
                let span = commands::begin(node, node, &request).unwrap();
                let (reply, completion) = tokio::sync::oneshot::channel();
                let response = plugin::http::Response {
                    status,
                    headers: std::collections::BTreeMap::from([(
                        "x-private".into(),
                        vec!["PRIVATE_HEADER".into()],
                    )]),
                    body: "PRIVATE_BODY".into(),
                };
                let admission = commands::admitted(
                    span,
                    sailry_link::Admission {
                        receipt: Receipt {
                            id: request.id,
                            durable: false,
                        },
                        completion,
                    },
                );
                reply
                    .send(Ok(Output::PluginHttp(response.clone())))
                    .unwrap();
                assert_eq!(
                    admission.completion.await.unwrap().unwrap(),
                    Output::PluginHttp(response)
                );
            }
            for code in [ErrorCode::PermissionDenied, ErrorCode::OutcomeUnknown] {
                let mut span = commands::begin(node, node, &request).unwrap();
                commands::rejected(&mut span, &Fault::new(code, "PRIVATE_FAILURE"));
            }
        })
    });
    drop(worker);
    let records = lines(directory.path());
    let completed = records
        .iter()
        .filter(|entry| entry["phase"] == "completed")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 3);
    for (entry, status) in completed.into_iter().zip([200, 403, 429]) {
        assert_eq!(entry["stage"], "provider_http");
        assert_eq!(entry["upstream_status"], status);
        assert_eq!(
            entry["reason"],
            if status == 200 {
                "http_success"
            } else {
                "http_rejected"
            }
        );
        assert_eq!(entry["package"], "fixture");
        assert_eq!(entry["provider"], provider.to_string());
    }
    let failures = records
        .iter()
        .filter(|entry| entry["phase"] == "failed")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2);
    for (entry, code) in failures
        .into_iter()
        .zip(["permission_denied", "outcome_unknown"])
    {
        assert_eq!(entry["stage"], "provider_http");
        assert_eq!(entry["code"], code);
        assert!(entry.get("upstream_status").is_none());
    }
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
}

#[test]
fn keeps_background_read_failures_visible() {
    use sailry_protocol::{Command, ErrorCode, NodeId, Request, WorktreeId, plugin};

    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(
        directory.path(),
        MAX_FILE_BYTES,
        RETAINED_FILES,
        QUEUED_LINES,
    )
    .unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Desktop));
    let node = NodeId([9; 32]);
    let worktree = WorktreeId::new();
    let request =
        Request::new(node, Command::InspectGit { worktree }).with_plugin(plugin::Context {
            package: plugin::Reference {
                name: "fixture".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            invocation: None,
            session: None,
            turn: None,
            worktree: Some(worktree),
            surface: Default::default(),
        });
    tracing::dispatcher::with_default(&dispatch, || {
        let mut span = commands::begin(node, node, &request).unwrap();
        span.record(Phase::Admitted);
        span.finish(Phase::Completed, None);
        for command in [
            Command::ListPlugins,
            Command::ListPluginKeys {
                prefix: String::new(),
                after: None,
                limit: 10,
            },
            Command::SearchPluginValues(serde_json::from_value(json!({})).unwrap()),
            Command::ReadPluginValue {
                key: "fixture".into(),
            },
        ] {
            let mut read = request.clone();
            read.command = command;
            let mut span = commands::begin(node, node, &read).unwrap();
            span.record(Phase::Admitted);
            span.finish(Phase::Completed, None);
        }
        let mut span = commands::begin(node, node, &request).unwrap();
        span.finish(Phase::Failed, Some(ErrorCode::Unavailable));
        let mut write = request.clone();
        write.command = Command::StopCommands {
            session: sailry_protocol::SessionId::new(),
        };
        let mut span = commands::begin(node, node, &write).unwrap();
        span.finish(Phase::Completed, None);
    });
    drop(worker);
    let records = lines(directory.path());
    assert_eq!(records[0]["phase"], "admitted");
    assert_eq!(records[0]["level"], "DEBUG");
    assert_eq!(records[1]["phase"], "completed");
    assert_eq!(records[1]["level"], "DEBUG");
    assert_eq!(records.len(), 13);
    for records in records[..10].as_chunks::<2>().0 {
        assert_eq!(records[0]["phase"], "admitted");
        assert_eq!(records[1]["phase"], "completed");
        assert!(records.iter().all(|record| record["level"] == "DEBUG"));
    }
    assert_eq!(records[10]["phase"], "failed");
    assert_eq!(records[10]["level"], "ERROR");
    assert_eq!(records[11]["phase"], "begin");
    assert_eq!(records[11]["level"], "INFO");
    assert_eq!(records[12]["phase"], "completed");
    assert_eq!(records[12]["level"], "INFO");
}
