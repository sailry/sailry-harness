//! Opt-in platform acceptance reads only this process's synthetic sentinel.
use super::super::*;
use std::{
    process::Command,
    time::{Duration, Instant},
};

#[test]
#[ignore = "Requires explicit native macOS unified-log acceptance"]
fn observes_synthetic_console_messages() {
    let directory = tempfile::tempdir().unwrap();
    let (writer, worker) = writer(directory.path(), Rotation::DAILY, 7, 8192).unwrap();
    let dispatch = tracing::Dispatch::new(subscriber(writer, Application::Host));
    let nonce = format!("sailry-console-{}", uuid::Uuid::new_v4());
    let start = (chrono::Utc::now() - chrono::Duration::seconds(2))
        .format("%Y-%m-%d %H:%M:%S%z")
        .to_string();
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!(target: "sailry", area = "logging", phase = "begin", reason = nonce.as_str(), body = "PRIVATE_NATIVE_BODY", error = ?"PRIVATE_NATIVE_ERROR", "PRIVATE_NATIVE_MESSAGE");
        tracing::warn!(target: "sailry", area = "logging", phase = "interrupted", reason = nonce.as_str());
        tracing::error!(target: "sailry", area = "logging", phase = "failed", reason = nonce.as_str());
    });
    drop(worker);
    let lines = std::fs::read_to_string(latest_file(directory.path()).unwrap().unwrap()).unwrap();
    assert_eq!(lines.lines().count(), 3);
    assert!(!lines.contains("PRIVATE_"));
    let predicate = format!(
        "subsystem == \"ai.sailry.host\" AND category == \"logging\" AND processIdentifier == {} AND eventMessage CONTAINS \"{nonce}\"",
        std::process::id(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let end = (chrono::Utc::now() + chrono::Duration::seconds(1))
            .format("%Y-%m-%d %H:%M:%S%z")
            .to_string();
        let mut command = Command::new("/usr/bin/log")
            .args([
                "show",
                "--style",
                "json",
                "--start",
                &start,
                "--end",
                &end,
                "--predicate",
                &predicate,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        while command.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                command.kill().unwrap();
                let _ = command.wait();
                panic!("native log query exceeded its deadline");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = command.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "native log query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let records: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
        let expected = [
            (format!("phase=begin reason={nonce}"), "Default"),
            (format!("phase=interrupted reason={nonce}"), "Error"),
            (format!("phase=failed reason={nonce}"), "Error"),
        ];
        let found = expected.iter().all(|(message, level)| {
            records.iter().any(|record| {
                record["eventMessage"] == *message
                    && record["subsystem"] == "ai.sailry.host"
                    && record["category"] == "logging"
                    && record["processID"] == std::process::id()
                    && record["messageType"] == *level
            })
        });
        if found {
            println!(
                "Observed three sanitized Console messages with native subsystem, category and severity"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "synthetic Console messages were not observed"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}
