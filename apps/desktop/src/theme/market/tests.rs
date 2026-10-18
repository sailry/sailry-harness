use super::*;
use crate::theme::package::Loaded;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::path::Path;

mod fixture;
use fixture::Server;
const FIRST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SECOND: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn offers_only_newer_official_versions() {
    let mut entry = catalog::bundled().themes.remove(0);
    let mut installed = Installed {
        id: entry.id.clone(),
        version: Some("0.1.0".into()),
        digest: "sha256:changed".into(),
        official: true,
        commit: Some(FIRST.into()),
    };
    entry.package_version = "0.1.1".into();
    assert!(entry.update_available(&installed));
    for candidate in ["0.1.0", "0.1.0+new-build", "0.0.9", "0.1.0-alpha.1"] {
        entry.package_version = candidate.into();
        assert!(!entry.update_available(&installed));
    }
    entry.package_version = "0.1.1".into();
    installed.official = false;
    assert!(!entry.update_available(&installed));
    installed.official = true;
    for version in [None, Some("old local label".into())] {
        installed.version = version;
        assert!(!entry.update_available(&installed));
    }
}

#[test]
fn rejects_unsafe_catalog_resources() {
    let entry = catalog::bundled().themes.remove(0);
    for path in [
        "../escape",
        "assets/../escape",
        "assets//bad",
        "assets/file?secret",
        "https://example.com/file",
    ] {
        let mut value = entry.clone();
        value.resources[0].path = path.into();
        assert!(value.validate().is_err());
    }
    let mut value = entry.clone();
    value.resources.push(value.resources[0].clone());
    assert!(value.validate().is_err());
    value = entry;
    value.resources[0].bytes = PACKAGE_BYTES + 1;
    assert!(value.validate().is_err());
}

#[tokio::test]
async fn restores_verified_offline_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let mut server = Server::start(&directory.path().join("installed")).await;
    server.publish(&source, FIRST);
    let listing = server
        .service
        .list(false, &CancellationToken::new())
        .await
        .unwrap();
    assert!(!listing.unavailable);
    assert_eq!(listing.commit.as_deref(), Some(FIRST));
    assert!(listing.selection("tide").is_some());
    assert_eq!(
        server.requests.lock().unwrap().clone(),
        vec![
            "/commits/main".to_owned(),
            format!("/raw/{FIRST}/catalog.json")
        ]
    );
    let mut reopened = Service::new(directory.path().join("installed")).unwrap();
    reopened.source = server.service.source.clone();
    server.offline().await;
    let restored = reopened
        .list(false, &CancellationToken::new())
        .await
        .unwrap();
    assert!(restored.unavailable);
    assert_eq!(restored.selection("tide").unwrap().commit, FIRST);
    assert_eq!(
        restored.catalog.themes[0].digest,
        listing.catalog.themes[0].digest
    );
    let mut empty = Service::new(directory.path().join("empty")).unwrap();
    empty.source = reopened.source;
    let fallback = empty.list(false, &CancellationToken::new()).await.unwrap();
    assert!(fallback.unavailable);
    assert!(fallback.commit.is_none());
    assert!(fallback.selection(&fallback.catalog.themes[0].id).is_none());
}

#[tokio::test]
async fn installs_manifest_declared_resources() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let server = Server::start(&directory.path().join("installed")).await;
    server.publish_without_hints(&source, FIRST);
    let stop = CancellationToken::new();
    let first = server.service.list(false, &stop).await.unwrap();
    assert!(first.catalog.themes[0].resources.is_empty());
    let selected = first.selection("tide").unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("theme.json")).unwrap()).unwrap();
    manifest["package_version"] = json!("0.1.1");
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    server.publish_without_hints(&source, SECOND);
    // The old index shape still downloads only its selected immutable commit.
    let installed = server
        .service
        .install(
            server.service.download(selected, &stop).await.unwrap(),
            &stop,
        )
        .await
        .unwrap();
    assert_eq!(installed.installed.commit.as_deref(), Some(FIRST));
    assert_eq!(installed.installed.version.as_deref(), Some("0.1.0"));
    assert_eq!(
        u32::from(installed.loaded.images["assets/light.png"].size(0).width),
        240
    );
    let path = directory.path().join("installed/tide/theme.json");
    let original = std::fs::read(&path).unwrap();
    let listing = server.service.list(true, &stop).await.unwrap();
    let selected = listing.selection("tide").unwrap();
    let remote = format!("/raw/{SECOND}/tide/theme.json");
    let valid = server
        .routes
        .lock()
        .unwrap()
        .insert(remote.clone(), b"{}".to_vec())
        .unwrap();
    assert!(matches!(
        server.service.download(selected.clone(), &stop).await,
        Err(Failure::Invalid)
    ));
    server.routes.lock().unwrap().insert(remote, valid);
    let mut corrupt = selected.clone();
    corrupt.entry.digest = format!("sha256:{}", "0".repeat(64));
    assert!(matches!(
        server.service.download(corrupt, &stop).await,
        Err(Failure::Invalid)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        server.service.installed("tide").await.unwrap(),
        Some(installed.installed)
    );
    let updated = server
        .service
        .install(
            server.service.download(selected, &stop).await.unwrap(),
            &stop,
        )
        .await
        .unwrap();
    assert_eq!(updated.installed.commit.as_deref(), Some(SECOND));
    assert_eq!(updated.installed.version.as_deref(), Some("0.1.1"));
    let requests = server.requests.lock().unwrap();
    let resources: Vec<_> = requests
        .iter()
        .filter(|path| path.starts_with("/raw/") && !path.ends_with("catalog.json"))
        .collect();
    assert!(
        resources[..3]
            .iter()
            .all(|path| path.starts_with(&format!("/raw/{FIRST}/tide/")))
    );
    assert!(
        resources[3..]
            .iter()
            .all(|path| path.starts_with(&format!("/raw/{SECOND}/tide/")))
    );
}

#[tokio::test]
async fn installs_and_returns_updated_images() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let server = Server::start(&directory.path().join("installed")).await;
    let initial = server.publish(&source, FIRST);
    let stop = CancellationToken::new();
    let listing = server.service.list(false, &stop).await.unwrap();
    let selection = listing.selection("tide").unwrap();
    let preview = server
        .service
        .preview(&selection, "light", &stop)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        preview,
        std::fs::read(source.join("assets/light.png")).unwrap()
    );
    // Moving main after selection cannot mix the frozen resource snapshot.
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("theme.json")).unwrap()).unwrap();
    manifest["package_version"] = json!("0.1.1");
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    image::RgbaImage::from_pixel(40, 20, image::Rgba([12u8, 33, 90, 255]))
        .save(source.join("assets/light.png"))
        .unwrap();
    let candidate = server.publish(&source, SECOND);
    let first = server
        .service
        .install(
            server.service.download(selection, &stop).await.unwrap(),
            &stop,
        )
        .await
        .unwrap();
    assert!(first.installed.official);
    assert_eq!(first.loaded.manifest.id, "tide");
    assert_eq!(
        first.loaded.manifest.package_version.as_deref(),
        Some("0.1.0")
    );
    assert_eq!(
        u32::from(first.loaded.images["assets/light.png"].size(0).width),
        240
    );
    for version in ["0.1.0", "0.1.0+new-build", "0.0.9"] {
        let mut rejected = listing.selection("tide").unwrap();
        rejected.entry.package_version = version.into();
        rejected.entry.digest = format!("sha256:{}", "0".repeat(64));
        let before = server.requests.lock().unwrap().len();
        assert!(!rejected.entry.update_available(&first.installed));
        assert!(matches!(
            server.service.download(rejected, &stop).await,
            Err(Failure::Conflict)
        ));
        assert_eq!(server.requests.lock().unwrap().len(), before);
    }
    let exported = directory.path().join("exported.zip");
    first.loaded.export(&exported).unwrap();
    assert_eq!(
        Loaded::read(&exported, true).unwrap().digest(),
        initial.digest
    );
    let destination = directory.path().join("installed/tide");
    std::fs::write(destination.join("assets/obsolete.png"), b"obsolete").unwrap();
    assert!(candidate.update_available(&first.installed));
    let listing = server.service.list(true, &stop).await.unwrap();
    let updated = server
        .service
        .install(
            server
                .service
                .download(listing.selection("tide").unwrap(), &stop)
                .await
                .unwrap(),
            &stop,
        )
        .await
        .unwrap();
    assert_eq!(updated.installed.id, first.installed.id);
    assert_ne!(updated.installed.digest, initial.digest);
    assert_eq!(
        u32::from(updated.loaded.images["assets/light.png"].size(0).width),
        40
    );
    assert_eq!(
        updated.loaded.manifest.package_version.as_deref(),
        Some("0.1.1")
    );
    assert!(!destination.join("assets/obsolete.png").exists());
    assert_eq!(
        server.service.installed("tide").await.unwrap(),
        Some(updated.installed)
    );
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.starts_with("/raw/"))
            .all(|path| path.starts_with(&format!("/raw/{FIRST}/"))
                || path.starts_with(&format!("/raw/{SECOND}/")))
    );
}

#[tokio::test]
async fn rejects_corrupt_and_invalid_packages() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let server = Server::start(&directory.path().join("installed")).await;
    server.publish(&source, FIRST);
    let stop = CancellationToken::new();
    let selection = server
        .service
        .list(false, &stop)
        .await
        .unwrap()
        .selection("tide")
        .unwrap();
    server.routes.lock().unwrap().insert(
        format!("/raw/{FIRST}/tide/assets/light.png"),
        b"corrupt".to_vec(),
    );
    assert!(matches!(
        server.service.download(selection.clone(), &stop).await,
        Err(Failure::Invalid)
    ));
    // Now the index and file hashes agree, but strict image decoding must still reject it.
    let mut invalid = selection;
    let resource = invalid
        .entry
        .resources
        .iter_mut()
        .find(|resource| resource.path == "assets/light.png")
        .unwrap();
    resource.bytes = 7;
    resource.digest = format!("sha256:{:x}", Sha256::digest(b"corrupt"));
    assert!(matches!(
        server.service.download(invalid, &stop).await,
        Err(Failure::Invalid)
    ));
    assert!(server.service.installed("tide").await.unwrap().is_none());
}

#[tokio::test]
async fn cancellation_preserves_installed_resources() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let server = Server::start(&directory.path().join("installed")).await;
    server.publish(&source, FIRST);
    let stop = CancellationToken::new();
    let selection = server
        .service
        .list(false, &stop)
        .await
        .unwrap()
        .selection("tide")
        .unwrap();
    let prepared = server.service.download(selection, &stop).await.unwrap();
    let first = server.service.install(prepared, &stop).await.unwrap();
    let original = std::fs::read(directory.path().join("installed/tide/theme.json")).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("theme.json")).unwrap()).unwrap();
    manifest["package_version"] = json!("0.1.1");
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    server.publish(&source, SECOND);
    let selected = server
        .service
        .list(true, &stop)
        .await
        .unwrap()
        .selection("tide")
        .unwrap();
    let prepared = server
        .service
        .download(selected.clone(), &stop)
        .await
        .unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        server.service.install(prepared, &cancelled).await,
        Err(Failure::Cancelled)
    ));
    assert_eq!(
        server.service.installed("tide").await.unwrap(),
        Some(first.installed.clone())
    );
    let path = format!("/raw/{SECOND}/tide/assets/dark.png");
    *server.held.lock().unwrap() = Some(path.clone());
    let service = server.service.clone();
    let cancel = stop.clone();
    let job = tokio::spawn(async move { service.download(selected, &cancel).await });
    server.wait(&path, 2).await;
    stop.cancel();
    assert!(matches!(job.await.unwrap(), Err(Failure::Cancelled)));
    assert_eq!(
        server.service.installed("tide").await.unwrap(),
        Some(first.installed)
    );
    assert_eq!(
        std::fs::read(directory.path().join("installed/tide/theme.json")).unwrap(),
        original
    );
}

#[tokio::test]
async fn never_overwrites_manual_installations() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let root = directory.path().join("installed");
    let server = Server::start(&root).await;
    server.publish(&source, FIRST);
    let stop = CancellationToken::new();
    let selection = server
        .service
        .list(false, &stop)
        .await
        .unwrap()
        .selection("tide")
        .unwrap();
    let prepared = server
        .service
        .download(selection.clone(), &stop)
        .await
        .unwrap();
    Loaded::read(&source, true).unwrap().install(&root).unwrap();
    let original = std::fs::read(root.join("tide/theme.json")).unwrap();
    assert!(
        !server
            .service
            .installed("tide")
            .await
            .unwrap()
            .unwrap()
            .official
    );
    assert!(matches!(
        server.service.install(prepared, &stop).await,
        Err(Failure::Conflict)
    ));
    assert!(matches!(
        server.service.download(selection, &stop).await,
        Err(Failure::Conflict)
    ));
    assert_eq!(
        std::fs::read(root.join("tide/theme.json")).unwrap(),
        original
    );
}

#[tokio::test]
async fn refuses_modified_official_resources() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let root = directory.path().join("installed");
    let server = Server::start(&root).await;
    server.publish(&source, FIRST);
    let stop = CancellationToken::new();
    let mut selected = server
        .service
        .list(false, &stop)
        .await
        .unwrap()
        .selection("tide")
        .unwrap();
    let first = server
        .service
        .install(
            server
                .service
                .download(selected.clone(), &stop)
                .await
                .unwrap(),
            &stop,
        )
        .await
        .unwrap();
    assert!(first.installed.official);
    let path = root.join("tide/assets/light.png");
    image::RgbaImage::from_pixel(40, 20, image::Rgba([12u8, 33, 90, 255]))
        .save(&path)
        .unwrap();
    let modified = std::fs::read(&path).unwrap();
    let installed = server.service.installed("tide").await.unwrap().unwrap();
    assert!(!installed.official);
    selected.entry.package_version = "0.1.1".into();
    assert!(!selected.entry.update_available(&installed));
    let before = server.requests.lock().unwrap().len();
    assert!(matches!(
        server.service.download(selected, &stop).await,
        Err(Failure::Conflict)
    ));
    assert_eq!(server.requests.lock().unwrap().len(), before);
    assert_eq!(std::fs::read(path).unwrap(), modified);
}
