use super::*;
use std::fs;

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
async fn ignores_remote_read_capacity() {
    let (_directory, host, info) = fixture();
    let _permit = host.reads.clone().acquire_many_owned(2).await.unwrap();
    let content = host
        .read_artifact_view(info, CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(content.template, "<main>{{body}}</main>");
    assert_eq!(content.styles, ["main { color: inherit; }"]);
    assert_eq!(content.scripts, ["export default class Example {}"]);
    assert_eq!(host.reads.available_permits(), 0);
}

#[tokio::test]
async fn cancels_before_reading() {
    let (_directory, host, info) = fixture();
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        host.read_artifact_view(info, stop).await.unwrap_err().code,
        ErrorCode::Cancelled
    );
}
