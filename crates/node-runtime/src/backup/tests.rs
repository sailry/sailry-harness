use super::*;
use std::io::{Read, Write};

#[path = "tests/server.rs"]
pub(crate) mod server;

fn snapshot() -> (tempfile::TempDir, archive::Snapshot) {
    let parent = tempfile::tempdir().unwrap();
    let profile = parent.path().canonicalize().unwrap().join("profile");
    crate::profile::private_directory(&profile).unwrap();
    let identity = sailry_link::Identity::open(&profile).unwrap();
    let mut snapshot = archive::Snapshot::new(&profile, identity.id()).unwrap();
    let database = snapshot.database_path().unwrap();
    std::fs::write(database, b"consistent database fixture").unwrap();
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
    assert_eq!(manifest.files.len(), 3);
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
fn retains_lazy_workspace_registrations() {
    let (_parent, mut snapshot) = snapshot();
    let relative = format!("workspaces/sessions/{}", sailry_protocol::SessionId::new());
    snapshot
        .capture(std::slice::from_ref(&relative), &CancellationToken::new())
        .unwrap();
    assert!(!snapshot.manifest.original_profile.join(&relative).exists());
    assert_eq!(snapshot.manifest.files.len(), 2);
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
        let expected = packed.sha256.clone();
        let id = uuid::Uuid::new_v4().to_string();
        let plan = plan(&server);
        let record = export(&plan, packed, id.clone(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(record.sha256, expected);
        assert!(server.parts(&id) >= 2);
        assert!(server.maximum_put() <= 4 * 1024 * 1024);
        let page = list(&plan, &CancellationToken::new()).await.unwrap();
        assert_eq!(page.records, vec![record.clone()]);
        assert!(!page.partial);
        let temporary = tempfile::tempdir().unwrap();
        let (restored, file) = restore(&plan, &id, temporary.path(), &CancellationToken::new())
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
        let error = restore(&plan, &id, temporary.path(), &CancellationToken::new())
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
        destination: server.destination(),
        auth: Auth {
            identity: Some(sailry_protocol::Secret::new("fixture-user".into())),
            secret: Some(sailry_protocol::Secret::new("fixture-password".into())),
            session_token: None,
        },
    }
}
