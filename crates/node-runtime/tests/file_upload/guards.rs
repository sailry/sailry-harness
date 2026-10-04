use super::*;

#[tokio::test]
async fn validates_target_identity() {
    let fixture = Fixture::start().await;
    std::fs::write(fixture.root.join("existing"), "original").unwrap();
    std::fs::create_dir(fixture.root.join("folder")).unwrap();
    let revision = blake3::hash(b"original").to_hex().to_string();
    for transport in &fixture.transports {
        let client = Client::new(transport.clone());
        for path in [
            "",
            "../escape",
            "/absolute",
            "a/../b",
            ".git/config",
            "nested/.GiT/config",
            "missing/file",
            "folder",
            "existing",
        ] {
            assert!(
                client
                    .execute(client.prepare(Command::UploadFile(spec(
                        fixture.worktree,
                        path,
                        b"new",
                        None
                    ))))
                    .await
                    .is_err(),
                "{path}"
            );
        }
        for expected in [
            Some("invalid"),
            Some(blake3::hash(b"wrong").to_hex().as_str()),
        ] {
            assert!(
                client
                    .execute(client.prepare(Command::UploadFile(spec(
                        fixture.worktree,
                        "existing",
                        b"new",
                        expected
                    ))))
                    .await
                    .is_err()
            );
        }
        let mut invalid = spec(fixture.worktree, "new", b"new", None);
        invalid.revision = "invalid".into();
        assert_eq!(
            client
                .execute(client.prepare(Command::UploadFile(invalid)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::UploadFile(spec(
                    WorktreeId::new(),
                    "new",
                    b"new",
                    None
                ))))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut wrong = client.prepare(Command::UploadFile(spec(
            fixture.worktree,
            "new",
            b"new",
            None,
        )));
        wrong.target = NodeId([0; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        let upload = prepare(
            &client,
            spec(fixture.worktree, "existing", b"new", Some(&revision)),
        )
        .await;
        stage(&client, &upload, b"new").await;
        for (worktree, path) in [(WorktreeId::new(), "existing"), (fixture.worktree, "other")] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::FinishFileUpload {
                        worktree,
                        path: path.into(),
                        stream: upload.stream
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
        client
            .execute(client.prepare(Command::CancelFileTransfer {
                stream: upload.stream,
            }))
            .await
            .unwrap();
    }
    assert_eq!(
        std::fs::read(fixture.root.join("existing")).unwrap(),
        b"original"
    );
    fixture.close().await;
}

#[tokio::test]
async fn preserves_external_changes() {
    let fixture = Fixture::start().await;
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        for mode in ["content", "identity", "parent"] {
            let folder = format!("{index}-{mode}");
            let path = format!("{folder}/file");
            std::fs::create_dir(fixture.root.join(&folder)).unwrap();
            std::fs::write(fixture.root.join(&path), "original").unwrap();
            let revision = blake3::hash(b"original").to_hex().to_string();
            let upload = prepare(
                &client,
                spec(fixture.worktree, &path, b"ours", Some(&revision)),
            )
            .await;
            stage(&client, &upload, b"ours").await;
            match mode {
                "content" => std::fs::write(fixture.root.join(&path), "external").unwrap(),
                "identity" => {
                    std::fs::rename(
                        fixture.root.join(&path),
                        fixture.root.join(&folder).join("old"),
                    )
                    .unwrap();
                    std::fs::write(fixture.root.join(&path), "original").unwrap();
                }
                _ => {
                    std::fs::rename(
                        fixture.root.join(&folder),
                        fixture.root.join(format!("{folder}-old")),
                    )
                    .unwrap();
                    std::fs::create_dir(fixture.root.join(&folder)).unwrap();
                    std::fs::write(fixture.root.join(&path), "original").unwrap();
                }
            }
            assert_eq!(
                client
                    .execute(finish(&client, &upload))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            assert_eq!(
                std::fs::read(fixture.root.join(&path)).unwrap(),
                if mode == "content" {
                    b"external"
                } else {
                    b"original"
                }
            );
        }
    }
    fixture.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn protects_restricted_targets() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::start().await;
    let outside = fixture._temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), "keep").unwrap();
    symlink(&outside, fixture.root.join("link")).unwrap();
    symlink(outside.join("sentinel"), fixture.root.join("leaf")).unwrap();
    std::fs::write(fixture.root.join("readonly"), "keep").unwrap();
    std::fs::set_permissions(
        fixture.root.join("readonly"),
        std::fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    let revision = blake3::hash(b"keep").to_hex().to_string();
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        for path in ["link/sentinel", "leaf", "readonly"] {
            assert!(
                client
                    .execute(client.prepare(Command::UploadFile(spec(
                        fixture.worktree,
                        path,
                        b"ours",
                        Some(&revision)
                    ))))
                    .await
                    .is_err(),
                "{path}"
            );
        }
        // Replacement detaches this hard link without writing its other names.
        let hard = format!("hard-{index}");
        std::fs::hard_link(outside.join("sentinel"), fixture.root.join(&hard)).unwrap();
        let upload = prepare(
            &client,
            spec(fixture.worktree, &hard, b"ours", Some(&revision)),
        )
        .await;
        stage(&client, &upload, b"ours").await;
        client.execute(finish(&client, &upload)).await.unwrap();
        assert_eq!(std::fs::read(fixture.root.join(&hard)).unwrap(), b"ours");
        assert_eq!(std::fs::read(outside.join("sentinel")).unwrap(), b"keep");
        let changed = format!("permissions-{index}");
        std::fs::write(fixture.root.join(&changed), b"keep").unwrap();
        let upload = prepare(
            &client,
            spec(fixture.worktree, &changed, b"ours", Some(&revision)),
        )
        .await;
        stage(&client, &upload, b"ours").await;
        std::fs::set_permissions(
            fixture.root.join(&changed),
            std::fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        assert_eq!(
            client
                .execute(finish(&client, &upload))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(std::fs::read(fixture.root.join(&changed)).unwrap(), b"keep");
        if index == 0 {
            client
                .execute(client.prepare(Command::RegisterProject {
                    name: "Parent".into(),
                    path: fixture._temp.path().to_str().unwrap().into(),
                }))
                .await
                .unwrap();
        }
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| {
                tree.path
                    == fixture
                        ._temp
                        .path()
                        .canonicalize()
                        .unwrap()
                        .to_str()
                        .unwrap()
            })
            .unwrap()
            .id;
        std::fs::write(fixture.profile.join("sentinel"), "keep").unwrap();
        for (path, expected) in [
            ("node/new", None),
            ("node/sentinel", Some(revision.as_str())),
        ] {
            assert_eq!(
                client
                    .execute(
                        client
                            .prepare(Command::UploadFile(spec(worktree, path, b"ours", expected)))
                    )
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert_eq!(
            std::fs::read(fixture.profile.join("sentinel")).unwrap(),
            b"keep"
        );
    }
    assert_eq!(std::fs::read(outside.join("sentinel")).unwrap(), b"keep");
    fixture.close().await;
}
