use super::*;
use sentry::protocol::{Attachment, Context, Event, Exception, Frame, Mechanism, Stacktrace};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Instant};
use tracing_subscriber::prelude::*;

mod server;

#[tokio::test]
async fn skips_disabled_event_conversion() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counted<'a>(&'a AtomicUsize);
    impl std::fmt::Debug for Counted<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fetch_add(1, Ordering::Relaxed);
            formatter.write_str("PRIVATE_VALUE")
        }
    }
    let mut server = server::Server::new().await;
    let control = Control::fixture(false, true);
    let report = Report::create(control.clone(), Some(&server.dsn)).unwrap();
    let hub = Arc::new(Hub::new(
        Some(report.client.clone()),
        Arc::new(Default::default()),
    ));
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(report.layer()));
    let visits = AtomicUsize::new(0);
    Hub::run(hub.clone(), || {
        tracing::dispatcher::with_default(&dispatch, || {
            let emit = || {
                tracing::error!(target: "sailry", area = "node", phase = "failed", private = ?Counted(&visits));
            };
            emit();
            assert_eq!(visits.load(Ordering::Relaxed), 0);
            control.fixture_set(true, false);
            emit();
            assert_eq!(visits.load(Ordering::Relaxed), 0);
            control.fixture_set(true, true);
            emit();
            assert_eq!(visits.load(Ordering::Relaxed), 1);
        })
    });
    assert!(!server.next().await.to_string().contains("PRIVATE_"));
    control.fixture_set(false, true);
    Hub::run(hub, || {
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::error!(target: "sailry", area = "node", phase = "failed", private = ?Counted(&visits));
        })
    });
    assert_eq!(visits.load(Ordering::Relaxed), 1);
    assert!(report.close(Duration::from_secs(1)));
    assert!(server.empty());
}

#[tokio::test]
async fn captures_existing_and_new_threads() {
    let mut server = server::Server::new().await;
    let main = Hub::main();
    let previous = main.client();
    main.bind_client(None);
    let (ready, waiting) = std::sync::mpsc::sync_channel(1);
    let (proceed, started) = std::sync::mpsc::sync_channel(1);
    let existing = std::thread::spawn(move || {
        assert!(Hub::current().client().is_none());
        ready.send(()).unwrap();
        started.recv().unwrap();
        assert!(std::panic::catch_unwind(|| panic!("PRIVATE_EXISTING_THREAD")).is_err());
    });
    waiting.recv().unwrap();
    let report = Report::new(Control::fixture(true, true), Some(&server.dsn)).unwrap();
    proceed.send(()).unwrap();
    tokio::task::spawn_blocking(move || existing.join().unwrap())
        .await
        .unwrap();
    let new = std::thread::spawn(|| {
        assert!(std::panic::catch_unwind(|| panic!("PRIVATE_NEW_THREAD")).is_err());
    });
    tokio::task::spawn_blocking(move || new.join().unwrap())
        .await
        .unwrap();
    for _ in 0..2 {
        let event = server.next().await;
        assert_eq!(event["event"]["message"], "runtime.panic:internal");
        assert!(!event.to_string().contains("PRIVATE_"));
    }
    assert!(report.close(Duration::from_secs(1)));
    main.bind_client(previous);
}

#[tokio::test]
async fn captures_errors_and_thread_panics() {
    let mut server = server::Server::new().await;
    let control = Control::fixture(true, true);
    let report = Report::create(control.clone(), Some(&server.dsn)).unwrap();
    let hub = Arc::new(Hub::new(
        Some(report.client.clone()),
        Arc::new(Default::default()),
    ));
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(report.layer()));
    Hub::run(hub.clone(), || {
        tracing::dispatcher::with_default(&dispatch, || {
            assert!(!tracing::enabled!(target: "sailry", tracing::Level::TRACE));
            assert!(!tracing::enabled!(target: "sailry", tracing::Level::INFO));
            assert!(!tracing::enabled!(target: "vendor", tracing::Level::ERROR));
            assert!(tracing::enabled!(target: "sailry", tracing::Level::WARN));
            assert!(tracing::enabled!(target: "sailry", tracing::Level::ERROR));
            sentry::configure_scope(|scope| {
                scope.set_user(Some(sentry::User {
                    email: Some("PRIVATE_EMAIL".into()),
                    ..Default::default()
                }));
                scope.set_tag("PRIVATE_TAG", "PRIVATE_TAG_VALUE");
                scope.add_attachment(Attachment {
                    buffer: b"PRIVATE_ATTACHMENT".to_vec(),
                    filename: "PRIVATE_FILENAME".into(),
                    ..Default::default()
                });
            });
            tracing::warn!(target: "sailry", area = "model", phase = "retrying", reason = "network", body = "PRIVATE_WARN", "PRIVATE_WARN_MESSAGE");
            tracing::warn!(target: "vendor", "PRIVATE_VENDOR_WARN");
            tracing::error!(target: "vendor", "PRIVATE_VENDOR_ERROR");
            tracing::error!(target: "sailry", area = "model", phase = "begin", "PRIVATE_NOT_FAILED");
            tracing::error!(target: "sailry", area = "model", phase = "failed", reason = "request", code = "unavailable", provider_name = "PRIVATE_PROVIDER", model = "PRIVATE_MODEL", request = "PRIVATE_REQUEST", body = "PRIVATE_BODY", url = "https://PRIVATE_HOST/path?token=PRIVATE_KEY", "PRIVATE_ERROR_MESSAGE");
        });
    });
    let worker = std::thread::spawn(move || {
        Hub::run(hub, || {
            assert!(std::panic::catch_unwind(|| panic!("PRIVATE_PANIC_PAYLOAD")).is_err());
        });
    });
    tokio::task::spawn_blocking(move || worker.join().unwrap())
        .await
        .unwrap();
    let error = server.next().await;
    let panic = server.next().await;
    let serialized = format!("{error}\n{panic}");
    assert!(!serialized.contains("PRIVATE_"), "{serialized}");
    assert!(!serialized.contains("/Volumes/"));
    assert!(!serialized.contains("/Users/"));
    assert!(!serialized.contains("_telemetry_epoch"));
    let error = &error["event"];
    for event in [error, &panic["event"]] {
        assert_eq!(event["platform"], "native");
        let images = event["debug_meta"]["images"].as_array().unwrap();
        assert!(!images.is_empty());
        assert!(images.iter().any(|image| {
            image
                .get("id")
                .or_else(|| image.get("uuid"))
                .and_then(Value::as_str)
                .is_some_and(|id| {
                    id.chars()
                        .any(|value| value.is_ascii_hexdigit() && value != '0')
                })
        }));
        for image in images {
            assert_eq!(image["name"], "native");
            assert!(image.get("id").is_some() || image.get("uuid").is_some());
            assert!(image.get("code_file").is_none());
            assert!(image.get("debug_file").is_none());
        }
        let frames = event["exception"]["values"][0]["stacktrace"]["frames"]
            .as_array()
            .unwrap();
        assert!(
            frames
                .iter()
                .any(|frame| frame.get("instruction_addr").is_some())
        );
        let address = |value: &Value| {
            value
                .as_str()
                .and_then(|value| u64::from_str_radix(value.trim_start_matches("0x"), 16).ok())
        };
        assert!(frames.iter().any(|frame| {
            address(&frame["instruction_addr"]).is_some_and(|instruction| {
                images.iter().any(|image| {
                    address(&image["image_addr"]).is_some_and(|start| {
                        instruction >= start
                            && image["image_size"]
                                .as_u64()
                                .is_some_and(|size| instruction - start < size)
                    })
                })
            })
        }));
    }
    assert_eq!(error["extra"]["code"], "unavailable");
    assert_eq!(error["environment"], control.metadata().channel);
    assert!(error["extra"].get("reason").is_none());
    assert_eq!(error["breadcrumbs"]["values"].as_array().unwrap().len(), 1);
    assert_eq!(
        error["breadcrumbs"]["values"][0]["data"]["phase"],
        "retrying"
    );
    assert!(error.get("user").is_none());
    assert!(error.get("request").is_none());
    assert!(error.get("server_name").is_none());
    assert!(error.get("contexts").is_none());
    assert_eq!(panic["event"]["exception"]["values"][0]["type"], "panic");
    assert_eq!(panic["event"]["message"], "runtime.panic:internal");
    assert_eq!(error["message"], "model.operation:unavailable");
    assert!(report.close(Duration::from_secs(1)));
    assert!(server.empty());
}

#[test]
fn reconstructs_only_public_metadata() {
    let control = Control::fixture(true, true);
    let mut event = Event {
        message: Some("PRIVATE_MESSAGE".into()),
        server_name: Some("PRIVATE_MACHINE".into()),
        request: Some(Default::default()),
        logentry: Some(Default::default()),
        extra: [("PRIVATE_EXTRA".into(), json!("PRIVATE_EXTRA_VALUE"))]
            .into_iter()
            .collect(),
        exception: vec![Exception {
            ty: "panic".into(),
            value: Some("PRIVATE_PANIC".into()),
            mechanism: Some(Mechanism {
                ty: "panic".into(),
                ..Default::default()
            }),
            stacktrace: Some(Stacktrace {
                frames: vec![Frame {
                    function: Some("sailry_node_runtime::worker::run".into()),
                    module: Some("sailry_node_runtime".into()),
                    filename: Some("src/worker.rs".into()),
                    lineno: Some(42),
                    colno: Some(9),
                    abs_path: Some("/Users/PRIVATE_USER/project/src/worker.rs".into()),
                    package: Some("/Users/PRIVATE_USER/bin".into()),
                    context_line: Some("PRIVATE_SOURCE".into()),
                    vars: [("PRIVATE_VARIABLE".into(), json!("PRIVATE_VALUE"))]
                        .into_iter()
                        .collect(),
                    instruction_addr: Some(123_u64.into()),
                    ..Default::default()
                }],
                registers: [("PRIVATE_REGISTER".into(), 123_u64.into())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
        }]
        .into(),
        ..Default::default()
    };
    event
        .contexts
        .insert("PRIVATE_CONTEXT".into(), Context::Other(Default::default()));
    let safe =
        serde_json::to_value(privacy::event(event, &control.metadata(), false).unwrap()).unwrap();
    assert!(!safe.to_string().contains("PRIVATE_"));
    assert_eq!(safe["environment"], control.metadata().channel);
    assert_eq!(
        safe["exception"]["values"][0]["stacktrace"]["frames"][0],
        json!({
            "function": "sailry_node_runtime::worker::run",
            "module": "sailry_node_runtime",
            "filename": "src/worker.rs",
            "lineno": 42,
            "instruction_addr": "0x7b",
        })
    );
}

#[tokio::test]
async fn retains_verified_plugin_metadata() {
    use sailry_protocol::{
        Command, ErrorCode, FaultDetails, NodeId, Request, RequestId,
        notification::{DiagnosticPlugin, SystemDiagnostic, SystemSource},
        plugin,
    };
    let mut server = server::Server::new().await;
    let report = Report::create(Control::fixture(true, true), Some(&server.dsn)).unwrap();
    let hub = Arc::new(Hub::new(
        Some(report.client.clone()),
        Arc::new(Default::default()),
    ));
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(report.layer()));
    let node = NodeId([2; 32]);
    let version = crate::plugins::Host::bundled_version("web-search")
        .unwrap()
        .unwrap();
    let mut request = Request::new(
        node,
        Command::CallPlugin {
            handler: "PRIVATE_HANDLER".into(),
            input: json!({"secret":"PRIVATE_INPUT"}),
        },
    );
    request.plugin = Some(plugin::Context {
        package: plugin::Reference {
            name: "web-search".into(),
            digest: crate::plugins::Host::bundled_digest("web-search").unwrap(),
            settings_revision: 4,
        },
        invocation: None,
        session: None,
        turn: None,
        worktree: None,
        surface: Default::default(),
    });
    let mut fault = Fault::new(ErrorCode::InvalidRequest, "PRIVATE_JAVASCRIPT_ERROR");
    let mut diagnostic =
        SystemDiagnostic::new(RequestId::new(), SystemSource::PluginCallback, fault.code);
    diagnostic.plugin = Some(DiagnosticPlugin {
        name: "web-search".into(),
        version: version.clone(),
    });
    fault.details = Some(FaultDetails::SystemDiagnostic(Box::new(diagnostic)));
    Hub::run(hub, || {
        tracing::dispatcher::with_default(&dispatch, || {
            let mut span = super::super::commands::begin(node, node, &request).unwrap();
            super::super::commands::rejected(&mut span, &fault);
        })
    });
    let envelope = server.next().await;
    assert_eq!(envelope["event"]["extra"]["package"], "web-search");
    assert_eq!(envelope["event"]["extra"]["plugin_version"], version);
    assert_eq!(envelope["event"]["extra"]["code"], "invalid_request");
    assert!(!envelope.to_string().contains("PRIVATE_"));
    assert!(!envelope.to_string().contains(&node.to_string()));
    assert!(report.close(Duration::from_secs(1)));
    let safe = super::super::format::remote(
        &serde_json::from_value(json!({
            "package":"https://private.invalid/key", "plugin_version":"PRIVATE_VERSION",
        }))
        .unwrap(),
    );
    assert_eq!(
        safe,
        json!({"package":"third_party"})
            .as_object()
            .unwrap()
            .clone()
    );
}

#[test]
fn restricts_shared_business_fields() {
    let version = crate::plugins::Host::bundled_version("web-search")
        .unwrap()
        .unwrap();
    let digest = crate::plugins::Host::bundled_digest("web-search").unwrap();
    let fields = serde_json::from_value(json!({
        "area":"plugin", "phase":"failed", "operation":"callback", "code":"internal",
        "cause_code":"cancelled", "package":"web-search", "plugin_version":version,
        "app_version":"0.1.0", "duration_us":12, "elapsed_us":13, "count":2,
        "settings_revision":3, "revision":4, "generation":5, "sequence":6,
        "transport":"remote", "parse_field":"private_field", "parse_event":"private_event",
        "bytes":7, "upstream_status":401, "status":"failed", "attempt":8,
        "stage":"private_stage", "source_kind":"private_source", "platform":"private_platform",
        "wait_ms":9, "end_to_end_us":10, "rtt_ms":11, "first_token_us":14,
        "last_content_us":15, "provider_name":"private_provider", "model":"private_model",
        "handler":"private_handler", "invocation":"private_invocation", "digest":digest,
    }))
    .unwrap();
    let safe = super::super::format::remote(&fields);
    let keys: std::collections::BTreeSet<_> = safe.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        std::collections::BTreeSet::from([
            "area",
            "phase",
            "operation",
            "code",
            "cause_code",
            "package",
            "plugin_version",
            "app_version",
            "duration_us",
            "elapsed_us",
            "count",
        ])
    );
    assert_eq!(safe["package"], "web-search");
    assert_eq!(safe["plugin_version"], version);
}

#[test]
fn requires_exact_public_package_identity() {
    let version = crate::plugins::Host::bundled_version("web-search")
        .unwrap()
        .unwrap();
    let digest = crate::plugins::Host::bundled_digest("web-search").unwrap();
    let fields = json!({
        "package":"web-search", "plugin_version":version, "digest":digest,
    });
    assert_eq!(
        super::super::format::remote(fields.as_object().unwrap()),
        json!({"package":"web-search", "plugin_version":version})
            .as_object()
            .unwrap()
            .clone()
    );
    for (key, value) in [
        ("package", "private-project-tools"),
        ("plugin_version", "0.1.0+PRIVATE_VERSION"),
        ("digest", "PRIVATE_DIGEST"),
    ] {
        let mut fields = fields.clone();
        fields[key] = value.into();
        let safe = super::super::format::remote(fields.as_object().unwrap());
        assert_eq!(safe["package"], "third_party");
        assert!(!safe.contains_key("plugin_version"));
        assert!(!safe.contains_key("digest"));
        assert!(!serde_json::to_string(&safe).unwrap().contains(value));
    }
    let newer = super::super::format::remote(
        json!({"package":"web-search", "plugin_version":"99.0.0", "digest":"unknown"})
            .as_object()
            .unwrap(),
    );
    assert_eq!(newer["package"], "third_party");
    assert!(!newer.contains_key("plugin_version"));
}

#[tokio::test]
async fn groups_private_plugin_failures() {
    let mut server = server::Server::new().await;
    let report = Report::create(Control::fixture(true, true), Some(&server.dsn)).unwrap();
    let hub = Arc::new(Hub::new(
        Some(report.client.clone()),
        Arc::new(Default::default()),
    ));
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(report.layer()));
    Hub::run(hub, || {
        tracing::dispatcher::with_default(&dispatch, || {
            for package in ["PRIVATE_PACKAGE", "web-search"] {
                tracing::error!(target: "sailry", area = "plugin", phase = "failed", operation = "callback", code = "internal", package, plugin_version = "0.1.0+PRIVATE_VERSION", digest = "PRIVATE_DIGEST");
            }
        })
    });
    let first = server.next().await;
    let second = server.next().await;
    for envelope in [&first, &second] {
        assert_eq!(envelope["event"]["extra"]["package"], "third_party");
        assert!(envelope["event"]["extra"].get("plugin_version").is_none());
        assert!(!envelope.to_string().contains("PRIVATE_"));
        assert!(!envelope.to_string().contains("web-search"));
    }
    assert_eq!(
        first["event"]["fingerprint"],
        second["event"]["fingerprint"]
    );
    assert!(report.close(Duration::from_secs(1)));
    assert!(server.empty());
}

#[tokio::test]
async fn diagnostics_bypasses_only_error_preference() {
    let mut server = server::Server::new().await;
    let control = Control::fixture(false, true);
    let report = Report::create(control.clone(), Some(&server.dsn)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("current.log");
    let mut lines = String::from("2026-10-30T00:00:00Z ERROR host reason=discarded\n");
    for count in 0..500 {
        lines.push_str(&format!("2026-10-30T00:00:00Z ERROR host area=model phase=failed count={count} reason=\"PRIVATE_REASON\" request=PRIVATE_REQUEST provider_name=PRIVATE_PROVIDER body=PRIVATE_BODY package=PRIVATE_PACKAGE plugin_version=0.1.0+PRIVATE_VERSION digest=PRIVATE_DIGEST\n"));
    }
    std::fs::write(&path, lines).unwrap();
    report.diagnostics_from(Some(path)).await.unwrap();
    let envelope = server.next().await;
    assert!(!envelope.to_string().contains("PRIVATE_"));
    let attachment = envelope["attachment"].as_str().unwrap();
    assert_eq!(attachment.lines().count(), 500);
    assert!(!attachment.contains("discarded"));
    let first: Value = serde_json::from_str(attachment.lines().next().unwrap()).unwrap();
    assert_eq!(first["package"], "third_party");
    assert!(first.get("plugin_version").is_none());
    assert_eq!(
        serde_json::from_str::<Value>(attachment.lines().last().unwrap()).unwrap()["count"],
        499
    );
    control.fixture_set(false, false);
    assert!(report.diagnostics_from(None).await.is_err());
    assert!(report.close(Duration::from_secs(1)));
    assert!(server.empty());
}

#[tokio::test]
async fn rejects_stale_and_disabled_work() {
    let control = Control::fixture(true, true);
    assert!(Report::create(control.clone(), None).is_none());
    assert!(Report::create(control.clone(), Some("PRIVATE_INVALID_DSN")).is_none());
    let mut server = server::Server::new().await;
    let report = Report::create(control.clone(), Some(&server.dsn)).unwrap();
    let epoch = control.epoch();
    control.fixture_set(false, true);
    let envelope: sentry::Envelope = Event::default().into();
    assert!(report.transport.explicit(envelope, epoch).await.is_err());
    let hub = Arc::new(Hub::new(
        Some(report.client.clone()),
        Arc::new(Default::default()),
    ));
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(report.layer()));
    Hub::run(hub.clone(), || {
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::error!(target: "sailry", area = "node", phase = "failed", reason = "fixture");
        })
    });
    control.fixture_set(true, false);
    Hub::run(hub, || {
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::error!(target: "sailry", area = "node", phase = "failed", code = "internal");
        })
    });
    assert!(report.diagnostics_from(None).await.is_err());
    assert!(report.close(Duration::from_secs(1)));
    assert!(server.empty());
}

#[tokio::test]
async fn rejects_http_without_leaks() {
    let mut server = server::Server::rejected().await;
    let report = Report::create(Control::fixture(false, true), Some(&server.dsn)).unwrap();
    let fault = report.diagnostics_from(None).await.unwrap_err();
    assert_eq!(fault.code, sailry_protocol::ErrorCode::Unavailable);
    assert!(!fault.to_string().contains("PRIVATE_"));
    assert!(!fault.to_string().contains(&server.dsn));
    assert_eq!(
        server.next().await["event"]["message"],
        "runtime.diagnostics:requested"
    );
    assert!(report.close(Duration::from_secs(1)));
}

#[test]
fn filters_malformed_diagnostic_lines() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "PRIVATE_RAW_MESSAGE\n2026-10-30T00:00:00Z ERROR host reason=\"unterminated\n2026-10-30T00:00:00Z ERROR host reason=safe count=7 path=/Users/PRIVATE_USER\n").unwrap();
    let attachment = diagnostics::attachment(Some(file.path().to_owned())).unwrap();
    let text = String::from_utf8(attachment.buffer).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(!text.contains("PRIVATE_"));
    assert_eq!(
        serde_json::from_str::<Value>(text.trim()).unwrap()["count"],
        7
    );
    assert!(diagnostics::attachment(Some(PathBuf::from("/PRIVATE_MISSING_FILE"))).is_err());
}

#[test]
fn limits_each_fingerprint() {
    let limiter = privacy::Limiter::default();
    let event = privacy::event(
        Event::default(),
        &Control::fixture(false, true).metadata(),
        true,
    )
    .unwrap();
    for _ in 0..10 {
        assert!(limiter.admit(&event));
    }
    assert!(!limiter.admit(&event));
}

#[tokio::test]
async fn cancels_inflight_on_opt_out() {
    let mut server = server::Server::paused().await;
    let control = Control::fixture(false, true);
    let report = Arc::new(Report::create(control.clone(), Some(&server.dsn)).unwrap());
    let sending = {
        let report = report.clone();
        tokio::spawn(async move { report.diagnostics_from(None).await })
    };
    server.next().await;
    let started = Instant::now();
    control.fixture_set(false, false);
    assert!(
        tokio::time::timeout(Duration::from_millis(250), sending)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_millis(250));
    assert!(report.close(Duration::from_secs(1)));
}

#[tokio::test]
async fn cancels_abandoned_diagnostics() {
    let mut server = server::Server::paused().await;
    let report =
        Arc::new(Report::create(Control::fixture(false, true), Some(&server.dsn)).unwrap());
    let sending = {
        let report = report.clone();
        tokio::spawn(async move { report.diagnostics_from(None).await })
    };
    server.next().await;
    sending.abort();
    assert!(sending.await.unwrap_err().is_cancelled());
    assert!(
        !tokio::time::timeout(Duration::from_millis(250), report.transport.accepted(1))
            .await
            .unwrap()
    );
    assert!(report.close(Duration::from_secs(1)));
}

#[tokio::test]
async fn bounds_shutdown_drain() {
    let mut server = server::Server::paused().await;
    let control = Control::fixture(false, true);
    let report = Arc::new(Report::create(control, Some(&server.dsn)).unwrap());
    let sending = {
        let report = report.clone();
        tokio::spawn(async move { report.diagnostics_from(None).await })
    };
    server.next().await;
    let started = Instant::now();
    assert!(!report.close(Duration::from_millis(20)));
    assert!(started.elapsed() < Duration::from_millis(250));
    assert!(sending.await.unwrap().is_err());
}

#[tokio::test]
#[ignore = "requires SAILRY_SENTRY_DSN and live Sentry ingestion"]
async fn live_acceptance() {
    let dsn = std::env::var("SAILRY_SENTRY_DSN").expect("SAILRY_SENTRY_DSN is required");
    assert_ne!(
        std::env::var("DO_NOT_TRACK").ok().as_deref(),
        Some("1"),
        "live reporting is disabled"
    );
    assert_ne!(
        std::env::var("SAILRY_TELEMETRY").ok().as_deref(),
        Some("off"),
        "live reporting is disabled"
    );
    for application in ["desktop", "host"] {
        let main = Hub::main();
        let previous = main.client();
        main.bind_client(None);
        let (ready, waiting) = std::sync::mpsc::sync_channel(1);
        let (proceed, started) = std::sync::mpsc::sync_channel(1);
        let existing = std::thread::spawn(move || {
            assert!(Hub::current().client().is_none());
            ready.send(()).unwrap();
            started.recv().unwrap();
            assert!(std::panic::catch_unwind(|| panic!("synthetic telemetry probe")).is_err());
        });
        waiting.recv().unwrap();
        let control = Control::fixture(true, true);
        let mut metadata = control.metadata();
        metadata.installation_id = uuid::Uuid::new_v4().to_string();
        metadata.channel = "dev";
        metadata.application = application;
        control.fixture_metadata(metadata);
        let report = Report::new(control, Some(&dsn)).expect("Sentry configuration must be valid");
        let hub = Arc::new(Hub::new(
            Some(report.client.clone()),
            Arc::new(Default::default()),
        ));
        let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(report.layer()));
        Hub::run(hub, || {
            tracing::dispatcher::with_default(&dispatch, || {
                tracing::error!(target: "sailry", area = "telemetry", phase = "failed", operation = "probe", code = "internal");
            })
        });
        proceed.send(()).unwrap();
        existing.join().unwrap();
        std::thread::spawn(|| {
            assert!(std::panic::catch_unwind(|| panic!("synthetic telemetry probe")).is_err());
        })
        .join()
        .unwrap();
        let accepted = report.transport.accepted(3).await;
        let closed = report.close(Duration::from_secs(2));
        main.bind_client(previous);
        assert!(accepted, "Sentry must accept all three synthetic envelopes");
        assert!(closed, "report transport must finish shutdown");
    }
}
