//! This process installs a pre-existing log bridge before Sailry's tracing sink.
use sailry_node_runtime::logging::{self, Area, Entry, Phase};
use serde_json::Value;

#[test]
fn retains_sink_with_existing_logger() {
    tracing_log::LogTracer::init().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let guard = logging::install(directory.path(), logging::Application::Host).unwrap();
    let mut entry = Entry::new(Area::Desktop, Phase::Begin);
    entry.reason = Some("fixture");
    entry.emit();
    assert_eq!(guard.dropped_lines(), 0);
    drop(guard);
    let records = std::fs::read_dir(directory.path().join("logs/host"))
        .unwrap()
        .flat_map(|entry| {
            std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(
        records
            .iter()
            .any(|record| record["reason"] == "log_bridge" && record["level"] == "WARN")
    );
    assert!(records.iter().any(|record| record["reason"] == "fixture"));
    assert!(tracing_log::LogTracer::init().is_err());
}
