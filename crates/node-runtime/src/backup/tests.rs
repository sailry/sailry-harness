use super::*;
use std::io::{Read, Write};

#[path = "tests/server.rs"]
pub(crate) mod server;

pub(super) fn snapshot() -> (tempfile::TempDir, archive::Snapshot) {
    let parent = tempfile::tempdir().unwrap();
    let profile = parent.path().canonicalize().unwrap().join("profile");
    crate::profile::private_directory(&profile).unwrap();
    let identity = sailry_link::Identity::open(&profile).unwrap();
    let mut snapshot = archive::Snapshot::new(&profile, identity.id()).unwrap();
    let database = snapshot.database_path().unwrap();
    std::fs::write(&database, b"consistent database fixture").unwrap();
    std::fs::write(
        &crate::profile::database_files(&database)[1],
        b"consistent logical log fixture",
    )
    .unwrap();
    snapshot.database(&CancellationToken::new()).unwrap();
    snapshot
        .capture(&["link.key".into()], &CancellationToken::new())
        .unwrap();
    (parent, snapshot)
}

#[test]
fn preserves_verified_private_files() {
    let (_parent, mut snapshot) = snapshot();
    let profile = snapshot.manifest.original_profile.clone();
    let workspace = format!("worktrees/{}", sailry_protocol::WorktreeId::new());
    std::fs::create_dir_all(profile.join(&workspace)).unwrap();
    std::fs::write(profile.join(&workspace).join("source.txt"), "source 内容\n").unwrap();
    snapshot
        .capture(std::slice::from_ref(&workspace), &CancellationToken::new())
        .unwrap();
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let target = tempfile::tempdir().unwrap();
    let target_path = target.path().canonicalize().unwrap();
    let manifest = archive::unpack(
        packed.file.reopen().unwrap(),
        &target_path,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(manifest.files.len(), 4);
    assert_eq!(
        std::fs::read_to_string(target_path.join(&workspace).join("source.txt")).unwrap(),
        "source 内容\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(target_path.join("link.key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
}

#[cfg(unix)]
#[test]
fn refuses_source_and_destination_symlinks() {
    let (parent, mut snapshot) = snapshot();
    let outside = parent.path().join("outside");
    std::fs::write(&outside, b"untouched").unwrap();
    std::os::unix::fs::symlink(&outside, snapshot.manifest.original_profile.join("linked"))
        .unwrap();
    assert!(
        snapshot
            .capture(&["linked".into()], &CancellationToken::new())
            .is_err()
    );
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let target = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(storage.path(), target.path().join("storage")).unwrap();
    assert!(
        archive::unpack(
            packed.file.reopen().unwrap(),
            &target.path().canonicalize().unwrap(),
            &CancellationToken::new()
        )
        .is_err()
    );
    assert!(storage.path().read_dir().unwrap().next().is_none());
    assert_eq!(std::fs::read(outside).unwrap(), b"untouched");
}

#[test]
fn rejects_undeclared_or_corrupt_entries() {
    let (_parent, snapshot) = snapshot();
    let mut manifest = snapshot.manifest.clone();
    manifest.files[0].path = "worktrees/../outside".into();
    assert!(manifest.validate().is_err());
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let mut bytes = Vec::new();
    packed
        .file
        .reopen()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes[10] ^= 0x80;
    let broken = tempfile::NamedTempFile::new().unwrap();
    broken.reopen().unwrap().write_all(&bytes).unwrap();
    let target = tempfile::tempdir().unwrap();
    assert!(
        archive::unpack(
            broken.reopen().unwrap(),
            &target.path().canonicalize().unwrap(),
            &CancellationToken::new()
        )
        .is_err()
    );
}

#[test]
fn preserves_exact_native_families() {
    let parent = tempfile::tempdir().unwrap();
    let profile = parent.path().canonicalize().unwrap().join("profile");
    crate::profile::private_directory(&profile.join("storage")).unwrap();
    let identity = sailry_link::Identity::open(&profile).unwrap();
    let mut snapshot = archive::Snapshot::new(&profile, identity.id()).unwrap();
    for name in ["node.turso", "knowledge.turso"] {
        for path in crate::profile::database_files(&profile.join("storage").join(name)) {
            let name = path.file_name().unwrap().to_str().unwrap();
            std::fs::write(&path, name.as_bytes()).unwrap();
        }
        snapshot
            .freeze(
                &profile.join("storage").join(name),
                &CancellationToken::new(),
            )
            .unwrap();
    }
    std::fs::write(
        profile.join("storage/node.sqlite3"),
        b"untouched legacy file",
    )
    .unwrap();
    snapshot
        .capture(&["link.key".into()], &CancellationToken::new())
        .unwrap();
    snapshot.manifest.validate().unwrap();
    assert_eq!(snapshot.manifest.files.len(), 9);
    let mut orphan = snapshot.manifest.clone();
    orphan
        .files
        .retain(|file| file.path != "storage/knowledge.turso");
    assert!(orphan.validate().is_err());
    for path in ["storage/node.db-log", "storage/knowledge.db-log"] {
        let mut missing = snapshot.manifest.clone();
        missing.files.retain(|file| file.path != path);
        assert!(missing.validate().is_err(), "{path}");
    }
    let mut legacy = snapshot.manifest.clone();
    legacy.files[0].path = "storage/node.sqlite3".into();
    assert!(legacy.validate().is_err());
    for path in [
        "storage/node.turso-journal",
        "storage/node.turso-log",
        "storage/knowledge.turso-log",
        "storage/node.turso-shm",
    ] {
        let mut unknown = snapshot.manifest.clone();
        unknown.files[0].path = path.into();
        assert!(unknown.validate().is_err(), "{path}");
    }
    let logical =
        &crate::profile::database_files(&snapshot.directory.path().join("storage/node.turso"))[1];
    let bytes = std::fs::read(logical).unwrap();
    std::fs::remove_file(logical).unwrap();
    assert!(
        snapshot
            .manifest
            .refresh_databases(snapshot.directory.path(), &CancellationToken::new())
            .is_err()
    );
    std::fs::write(logical, bytes).unwrap();
    let metadata = snapshot
        .manifest
        .files
        .iter()
        .map(|file| (file.path.clone(), (file.size, file.sha256.clone())))
        .collect::<std::collections::BTreeMap<_, _>>();
    for file in &mut snapshot.manifest.files {
        if file.path.starts_with("storage/") {
            file.size = 0;
            file.sha256 = "0".repeat(64);
        }
    }
    snapshot
        .manifest
        .refresh_databases(snapshot.directory.path(), &CancellationToken::new())
        .unwrap();
    assert_eq!(
        snapshot
            .manifest
            .files
            .iter()
            .map(|file| (file.path.clone(), (file.size, file.sha256.clone())))
            .collect::<std::collections::BTreeMap<_, _>>(),
        metadata
    );
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let target = tempfile::tempdir().unwrap();
    let manifest = archive::unpack(
        packed.file.reopen().unwrap(),
        &target.path().canonicalize().unwrap(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(manifest.files.len(), 9);
    for name in ["node.turso", "knowledge.turso"] {
        for path in crate::profile::database_files(&target.path().join("storage").join(name)) {
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), name.as_bytes());
        }
    }
    assert!(!target.path().join("storage/node.sqlite3").exists());
    assert_eq!(
        std::fs::read(profile.join("storage/node.sqlite3")).unwrap(),
        b"untouched legacy file"
    );
}

#[test]
fn requires_native_logical_files() {
    let parent = tempfile::tempdir().unwrap();
    let profile = parent.path().canonicalize().unwrap().join("profile");
    crate::profile::private_directory(&profile.join("storage")).unwrap();
    let path = profile.join("storage/node.turso");
    std::fs::write(&path, b"closed database fixture").unwrap();
    let mut snapshot = archive::Snapshot::new(&profile, sailry_protocol::NodeId([6; 32])).unwrap();
    assert!(snapshot.freeze(&path, &CancellationToken::new()).is_err());
    assert!(!crate::profile::database_files(&path)[1].exists());
    assert_eq!(std::fs::read(path).unwrap(), b"closed database fixture");
}

#[test]
fn refuses_nonregular_native_sidecars() {
    let parent = tempfile::tempdir().unwrap();
    let profile = parent.path().canonicalize().unwrap().join("profile");
    crate::profile::private_directory(&profile.join("storage")).unwrap();
    let identity = sailry_link::Identity::open(&profile).unwrap();
    std::fs::write(
        profile.join("storage/node.turso"),
        b"closed database fixture",
    )
    .unwrap();
    std::fs::write(
        &crate::profile::database_files(&profile.join("storage/node.turso"))[1],
        b"",
    )
    .unwrap();
    let sidecar = crate::profile::database_files(&profile.join("storage/node.turso"))[3].clone();
    std::fs::create_dir(&sidecar).unwrap();
    let mut snapshot = archive::Snapshot::new(&profile, identity.id()).unwrap();
    assert!(
        snapshot
            .freeze(
                &profile.join("storage/node.turso"),
                &CancellationToken::new()
            )
            .is_err()
    );
    std::fs::remove_dir(sidecar).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut snapshot = archive::Snapshot::new(&profile, identity.id()).unwrap();
    assert_eq!(
        snapshot
            .freeze(&profile.join("storage/node.turso"), &cancel)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(snapshot.manifest.files.is_empty());
}

#[test]
fn retains_lazy_workspace_registrations() {
    let (_parent, mut snapshot) = snapshot();
    let relative = format!("workspaces/sessions/{}", sailry_protocol::SessionId::new());
    snapshot
        .capture(std::slice::from_ref(&relative), &CancellationToken::new())
        .unwrap();
    assert!(!snapshot.manifest.original_profile.join(&relative).exists());
    assert_eq!(snapshot.manifest.files.len(), 3);
    assert!(
        snapshot
            .capture(&["attachments/missing".into()], &CancellationToken::new())
            .is_err()
    );
    snapshot.manifest.files.push(archive::File {
        path: "workspaces/sessions/invalid/source.txt".into(),
        size: 1,
        sha256: "0".repeat(64),
        mode: 0o600,
    });
    assert!(snapshot.manifest.validate().is_err());
}

#[tokio::test]
async fn roundtrips_segmented_storage_adapters() {
    for kind in [
        sailry_protocol::backup::Kind::Webdav,
        sailry_protocol::backup::Kind::S3,
    ] {
        let server = server::Server::start(kind).await;
        let (_parent, mut snapshot) = snapshot();
        let profile = snapshot.manifest.original_profile.clone();
        let workspace = format!("worktrees/{}", sailry_protocol::WorktreeId::new());
        std::fs::create_dir_all(profile.join(&workspace)).unwrap();
        let mut file = std::fs::File::create(profile.join(&workspace).join("data.bin")).unwrap();
        // Actual incompressible data forces more than one bounded remote object.
        use sha2::{Digest, Sha256};
        for index in 0u32..170_000 {
            file.write_all(&Sha256::digest(index.to_le_bytes()))
                .unwrap();
        }
        drop(file);
        snapshot
            .capture(&[workspace], &CancellationToken::new())
            .unwrap();
        let packed = snapshot.pack(&CancellationToken::new()).unwrap();
        let plaintext_hash = packed.sha256.clone();
        let id = uuid::Uuid::new_v4().to_string();
        let plan = plan(&server);
        let record = export(&plan, packed, id.clone(), &CancellationToken::new())
            .await
            .unwrap();
        assert_ne!(record.sha256, plaintext_hash);
        let expected = record.sha256.clone();
        assert!(server.parts(&id) >= 2);
        assert!(server.maximum_put() <= 4 * 1024 * 1024);
        let page = list(&plan, &CancellationToken::new()).await.unwrap();
        assert_eq!(page.records, vec![record.clone()]);
        assert!(!page.partial);
        let temporary = tempfile::tempdir().unwrap();
        let (restored, file) = restore(
            &plan,
            &id,
            sailry_protocol::backup::Location::Online,
            temporary.path(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(record, restored);
        assert_eq!(
            archive::hash(
                &mut file.reopen().unwrap(),
                record.size_bytes,
                &CancellationToken::new()
            )
            .unwrap(),
            expected
        );
        server.corrupt(&id);
        let error = restore(
            &plan,
            &id,
            sailry_protocol::backup::Location::Online,
            temporary.path(),
            &CancellationToken::new(),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }
}

#[tokio::test]
async fn cancellation_never_publishes_manifest() {
    let server = server::Server::start(sailry_protocol::backup::Kind::Webdav).await;
    let (_parent, snapshot) = snapshot();
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let id = uuid::Uuid::new_v4().to_string();
    let error = export(&plan(&server), packed, id, &cancel)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(server.writes(), 0);
}

fn plan(server: &server::Server) -> Plan {
    Plan {
        destination: Some(server.destination()),
        local: None,
        online_enabled: true,
        auth: Auth {
            identity: Some(sailry_protocol::Secret::new("fixture-user".into())),
            secret: Some(sailry_protocol::Secret::new("fixture-password".into())),
            session_token: None,
            passphrase: Some(sailry_protocol::Secret::new(
                "fixture-backup-passphrase".into(),
            )),
        },
    }
}

#[tokio::test]
async fn publishes_identical_copies_from_one_capture() {
    let server = server::Server::start(sailry_protocol::backup::Kind::Webdav).await;
    let (_parent, snapshot) = snapshot();
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let local = tempfile::tempdir().unwrap();
    let mut plan = plan(&server);
    plan.local = Some(sailry_protocol::backup::Local {
        directory: local
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        enabled: true,
    });
    let id = uuid::Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    let record = export(&plan, packed, id.clone(), &cancel).await.unwrap();
    let page = list(&plan, &cancel).await.unwrap();
    assert_eq!(page.records, vec![record.clone()]);
    assert_eq!(
        page.locations[&id],
        sailry_protocol::backup::Locations {
            local: true,
            online: true
        }
    );
    let temporary = tempfile::tempdir().unwrap();
    let (_, local_file) = restore(
        &plan,
        &id,
        sailry_protocol::backup::Location::Local,
        temporary.path(),
        &cancel,
    )
    .await
    .unwrap();
    let (_, online_file) = restore(
        &plan,
        &id,
        sailry_protocol::backup::Location::Online,
        temporary.path(),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(local_file.path()).unwrap(),
        std::fs::read(online_file.path()).unwrap()
    );
    assert!(record.size_bytes > 0);
}
