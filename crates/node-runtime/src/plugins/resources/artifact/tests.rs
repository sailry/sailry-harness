use super::*;
use std::{fs, time::Duration};

fn fixture() -> (tempfile::TempDir, Host, Info) {
    let (directory, host, source) = crate::plugins::tests::fixture();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("plugin.json")).unwrap()).unwrap();
    manifest["extensions"] = serde_json::json!({"dev.sailry.platform": {
        "api_version":"v1", "actions":["artifacts.read", "artifacts.write"],
        "artifact_view":{"template":"dev.sailry.platform/index.html",
            "styles":["dev.sailry.platform/theme.css"],
            "scripts":["dev.sailry.platform/main.js"]}
    }});
    fs::write(source.join("plugin.json"), manifest.to_string()).unwrap();
    for (path, content) in [
        ("index.html", "<main>{{body}}</main>"),
        ("theme.css", "main { color: inherit; }"),
        ("main.js", "export default class Example {}"),
    ] {
        fs::write(source.join("dev.sailry.platform").join(path), content).unwrap();
    }
    let info = host.install(&source, "", "example").unwrap();
    assert!(info.extension.as_ref().unwrap().artifact_view.is_some());
    (directory, host, info)
}

#[tokio::test]
async fn waits_for_resource_capacity() {
    let (_directory, host, info) = fixture();
    let permit = host.reads.clone().acquire_many_owned(2).await.unwrap();
    let reader = host.clone();
    let task = tokio::spawn(async move {
        reader
            .read_artifact_view(info, CancellationToken::new())
            .await
    });
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    assert_eq!(host.reads.available_permits(), 0);
    drop(permit);
    let content = task.await.unwrap().unwrap().unwrap();
    assert_eq!(content.template, "<main>{{body}}</main>");
    assert_eq!(content.styles, ["main { color: inherit; }"]);
    assert_eq!(content.scripts, ["export default class Example {}"]);
    assert_eq!(host.reads.available_permits(), 2);
}

#[tokio::test]
async fn cancels_while_waiting() {
    let (_directory, host, info) = fixture();
    let permit = host.reads.clone().acquire_many_owned(2).await.unwrap();
    let stop = CancellationToken::new();
    let cancellation = stop.clone();
    let reader = host.clone();
    let task = tokio::spawn(async move { reader.read_artifact_view(info, stop).await });
    tokio::task::yield_now().await;
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap();
    assert_eq!(result.unwrap().unwrap_err().code, ErrorCode::Cancelled);
    assert_eq!(host.reads.available_permits(), 0);
    drop(permit);
    assert_eq!(host.reads.available_permits(), 2);
}

#[tokio::test(start_paused = true)]
async fn bounds_capacity_wait() {
    let (_directory, host, info) = fixture();
    let permit = host.reads.clone().acquire_many_owned(2).await.unwrap();
    let start = tokio::time::Instant::now();
    assert_eq!(
        host.read_artifact_view(info, CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    assert_eq!(host.reads.available_permits(), 0);
    drop(permit);
    assert_eq!(host.reads.available_permits(), 2);
}
