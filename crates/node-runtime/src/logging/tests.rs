use super::*;
use serde_json::{Value, json};
use std::{
    io::Write as _,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

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
                .map(|line| serde_json::from_str(line).unwrap())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn filters_payloads_and_upstream_events() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(directory.path(), Rotation::DAILY, 7, 8192).unwrap();
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
    let (writer, worker) = writer(directory.path(), Rotation::DAILY, 7, 8192).unwrap();
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
fn retains_cancelled_cleanup_uncertainty() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(directory.path(), Rotation::DAILY, 7, 8192).unwrap();
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
    let (writer, worker) = writer(directory.path(), Rotation::DAILY, 7, 8192).unwrap();
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
fn prunes_only_owned_dates() {
    let directory = tempfile::tempdir().unwrap();
    for day in 1..=10 {
        std::fs::write(directory.path().join(format!("2020-01-{day:02}.log")), "").unwrap();
    }
    std::fs::write(directory.path().join("notes.txt"), "preserve").unwrap();
    std::fs::write(directory.path().join("2020-01-01"), "legacy").unwrap();
    std::fs::create_dir(directory.path().join("2020-01-11.log")).unwrap();
    let (mut writer, worker) = writer(
        directory.path(),
        Rotation::DAILY,
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
fn rolls_at_a_calendar_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let (mut writer, worker) =
        writer(directory.path(), Rotation::MINUTELY, 2, QUEUED_LINES).unwrap();
    writer.write_all(b"before\n").unwrap();
    let started = Instant::now();
    let before = loop {
        if let Some(path) = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| std::fs::read_to_string(path).unwrap() == "before\n")
        {
            break path.file_name().unwrap().to_str().unwrap().to_owned();
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "initial write deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    while chrono::Utc::now().format("%Y-%m-%d-%H-%M.log").to_string() == before {
        assert!(
            started.elapsed() < Duration::from_secs(65),
            "calendar rollover deadline"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    writer.write_all(b"after\n").unwrap();
    drop(worker);
    let contents = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(contents.len(), 2);
    assert!(contents.iter().any(|text| text == "before\n"));
    assert!(contents.iter().any(|text| text == "after\n"));
}

#[test]
fn resolves_latest_daily_file() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(directory.path(), Rotation::DAILY, 7, 8192).unwrap();
    drop(worker);
    drop(writer);
    let actual = latest_file(directory.path()).unwrap().unwrap();
    assert_eq!(actual.extension().unwrap(), "log");
    chrono::NaiveDate::parse_from_str(actual.file_stem().unwrap().to_str().unwrap(), "%Y-%m-%d")
        .unwrap();
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
    let (writer, worker) = writer(directory.path(), Rotation::DAILY, 7, 8192).unwrap();
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
