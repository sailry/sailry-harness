#![cfg(unix)]
use super::*;
use crate::code::tests::network::Sources;
use std::collections::BTreeMap;

fn routes(correct: bool) -> BTreeMap<String, Vec<u8>> {
    let commit = "a".repeat(40);
    let mason = "b".repeat(40);
    let tag = "fixture-release";
    let registry = include_bytes!("../../upstream/registry.json.zip").to_vec();
    let checksum = if correct {
        digest(&registry)
    } else {
        "0".repeat(64)
    };
    BTreeMap::from([
        (
            "/api.github.com/repos/helix-editor/helix/commits/master".into(),
            serde_json::to_vec(&json!({"sha":commit})).unwrap(),
        ),
        (
            "/api.github.com/repos/mason-org/mason-registry/releases/latest".into(),
            serde_json::to_vec(&json!({"tag_name":tag})).unwrap(),
        ),
        (
            format!("/raw.githubusercontent.com/helix-editor/helix/{commit}/languages.toml"),
            include_bytes!("../../upstream/languages.toml").to_vec(),
        ),
        (
            format!(
                "/github.com/mason-org/mason-registry/releases/download/{tag}/registry.json.zip"
            ),
            registry,
        ),
        (
            format!("/github.com/mason-org/mason-registry/releases/download/{tag}/checksums.txt"),
            format!("{checksum}  registry.json.zip\n").into_bytes(),
        ),
        (
            format!("/api.github.com/repos/mason-org/mason-registry/git/ref/tags/{tag}"),
            serde_json::to_vec(&json!({"object":{"type":"commit","sha":mason}})).unwrap(),
        ),
        (
            format!("/raw.githubusercontent.com/helix-editor/helix/{commit}/LICENSE"),
            include_bytes!("../../upstream/helix-LICENSE").to_vec(),
        ),
        (
            format!("/raw.githubusercontent.com/mason-org/mason-registry/{mason}/LICENSE"),
            include_bytes!("../../upstream/mason-LICENSE").to_vec(),
        ),
    ])
}

#[tokio::test]
async fn caches_original_rules_for_offline_reuse() {
    let profile = tempfile::tempdir().unwrap();
    let sources = Sources::new(routes(true), None).await;
    let network = Arc::new(Network::default());
    network.fixture(&sources.endpoint);
    let registry = Registry::new(Some(profile.path().into()), network.clone());
    let original = registry.get().unwrap();
    assert!(!registry.status().unwrap().cached);
    let status = registry.refresh(&CancellationToken::new()).await.unwrap();
    assert!(status.cached);
    assert_eq!(status.mason_tag, "fixture-release");
    assert_eq!(
        registry
            .get()
            .unwrap()
            .language("main.rs", "")
            .unwrap()
            .name,
        "rust"
    );
    assert!(original.language("main.rs", "").is_some());
    let count = sources.requests.lock().await.len();
    let offline = Registry::new(Some(profile.path().into()), network);
    assert_eq!(offline.status().unwrap(), status);
    assert_eq!(sources.requests.lock().await.len(), count);
    assert_eq!(
        offline.get().unwrap().packages.len(),
        original.packages.len()
    );
    std::fs::write(profile.path().join("code/rules/mason-LICENSE"), "modified").unwrap();
    let corrupt = Registry::new(Some(profile.path().into()), Arc::new(Network::default()));
    assert!(!corrupt.status().unwrap().cached);
    assert_eq!(
        corrupt.get().unwrap().packages.len(),
        original.packages.len()
    );
}

#[tokio::test]
async fn rejects_invalid_updates_without_replacing_cached_rules() {
    let profile = tempfile::tempdir().unwrap();
    let sources = Sources::new(routes(false), None).await;
    let network = Arc::new(Network::default());
    network.fixture(&sources.endpoint);
    let registry = Registry::new(Some(profile.path().into()), network);
    let before = registry.status().unwrap();
    let captured = registry.get().unwrap();
    assert!(registry.refresh(&CancellationToken::new()).await.is_err());
    assert_eq!(registry.status().unwrap(), before);
    assert!(Arc::ptr_eq(&captured, &registry.get().unwrap()));
    assert!(!profile.path().join("code/rules").exists());
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        registry.refresh(&stop).await.unwrap_err().code,
        ErrorCode::Cancelled
    );
    assert_eq!(registry.status().unwrap(), before);
}
