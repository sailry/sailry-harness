use super::*;
use sailry_protocol::backup::Destination;

fn destination() -> Destination {
    Destination {
        kind: Kind::Webdav,
        endpoint: "https://backup.example.com".into(),
        root: "profiles".into(),
        bucket: String::new(),
        region: String::new(),
    }
}

#[test]
fn preserves_private_credentials_and_conflicts() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE backup_settings(singleton INTEGER PRIMARY KEY,body BLOB NOT NULL)",
    )
    .unwrap();
    let mut settings = Settings {
        destination: Some(destination()),
        ..Settings::default()
    };
    let command = Update {
        settings: settings.clone(),
        identity: SecretUpdate::Replace(Secret::new("private-user".into())),
        secret: SecretUpdate::Replace(Secret::new("private-password".into())),
        session_token: SecretUpdate::Clear,
        passphrase: SecretUpdate::Replace(Secret::new("private-backup-passphrase".into())),
    };
    let request = Request::new(
        sailry_protocol::NodeId([1; 32]),
        Command::SaveBackupSettings(command),
    );
    let (output, event) = execute(&db, &request).unwrap();
    let public = serde_json::to_string(&(output.clone(), event)).unwrap();
    assert!(!public.contains("private-user"));
    assert!(!public.contains("private-password"));
    assert!(!public.contains("private-backup-passphrase"));
    assert_eq!(
        read(&db).unwrap().auth.secret.unwrap().expose(),
        "private-password"
    );
    assert_eq!(
        execute(&db, &request).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let Output::BackupSettings(saved) = output else {
        unreachable!()
    };
    settings.revision = saved.revision;
    settings.destination.as_mut().unwrap().kind = Kind::S3;
    settings.destination.as_mut().unwrap().bucket = "bucket".into();
    settings.destination.as_mut().unwrap().region = "us-east-1".into();
    let request = Request::new(
        sailry_protocol::NodeId([1; 32]),
        Command::SaveBackupSettings(Update {
            settings,
            identity: SecretUpdate::Keep,
            secret: SecretUpdate::Keep,
            session_token: SecretUpdate::Keep,
            passphrase: SecretUpdate::Keep,
        }),
    );
    execute(&db, &request).unwrap();
    assert!(!read(&db).unwrap().settings.credentials_configured);
    assert!(read(&db).unwrap().auth.secret.is_none());
    assert!(read(&db).unwrap().settings.passphrase_configured);
    assert_eq!(
        read(&db).unwrap().auth.passphrase.unwrap().expose(),
        "private-backup-passphrase"
    );
}

#[test]
fn confines_empty_restore_targets() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let active = root.join("active");
    std::fs::create_dir(&active).unwrap();
    assert!(target_path("relative", Some(&active)).is_err());
    assert_eq!(
        target_path(active.to_str().unwrap(), Some(&active))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let target = root.join("restored");
    assert_eq!(
        target_path(target.to_str().unwrap(), Some(&active)).unwrap(),
        target
    );
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("user.txt"), b"preserved").unwrap();
    assert_eq!(
        target_path(target.to_str().unwrap(), Some(&active))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    #[cfg(unix)]
    {
        let linked = root.join("linked");
        std::os::unix::fs::symlink(&target, &linked).unwrap();
        assert!(target_path(linked.to_str().unwrap(), Some(&active)).is_err());
    }
    assert_eq!(
        std::fs::read(target.join("user.txt")).unwrap(),
        b"preserved"
    );
}

#[test]
fn relocates_only_current_owned_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let original = root.join("original");
    let target = root.join("restored");
    let staging = root.join("staging");
    std::fs::create_dir_all(staging.join("storage")).unwrap();
    let db = Connection::open(staging.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,path TEXT); CREATE TABLE worktrees(id TEXT PRIMARY KEY,path TEXT); CREATE TABLE knowledge_collections(id TEXT PRIMARY KEY,body BLOB); CREATE TABLE session_revisions(body BLOB);").unwrap();
    let owned = original.join("worktrees/example");
    let external = root.join("external");
    for table in ["projects", "worktrees"] {
        db.execute(
            &format!("INSERT INTO {table}(id,path) VALUES('owned',?1),('external',?2)"),
            rusqlite::params![owned.to_str().unwrap(), external.to_str().unwrap()],
        )
        .unwrap();
    }
    let capture =
        serde_json::json!({"path":owned.to_str().unwrap(),"user":"unchanged"}).to_string();
    db.execute(
        "INSERT INTO session_revisions(body) VALUES(?1)",
        [capture.as_bytes()],
    )
    .unwrap();
    let collection = super::super::knowledge::Stored {
        collection: sailry_protocol::knowledge::Collection {
            id: sailry_protocol::KnowledgeId::new(),
            revision: 1,
            rebuild: 0,
            owner: sailry_protocol::knowledge::Owner::Controller,
            definition: sailry_protocol::knowledge::Definition {
                name: "Sources".into(),
                source: sailry_protocol::knowledge::Source::File {
                    path: owned.join("source.txt").to_string_lossy().into(),
                },
                model: None,
                globs: vec![],
                tags: vec![],
                shared_with: vec![],
            },
            paused: true,
            progress: Default::default(),
        },
        capture: super::super::knowledge::Capture {
            package: None,
            worktree: None,
            session: None,
        },
        automatic: false,
    };
    db.execute(
        "INSERT INTO knowledge_collections(id,body) VALUES(?1,?2)",
        rusqlite::params![
            collection.collection.id.to_string(),
            encode(&collection).unwrap()
        ],
    )
    .unwrap();
    drop(db);
    relocate(&staging, &original, &target).unwrap();
    let db = Connection::open(staging.join("storage/node.sqlite3")).unwrap();
    for table in ["projects", "worktrees"] {
        let owned: String = db
            .query_row(
                &format!("SELECT path FROM {table} WHERE id='owned'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owned, target.join("worktrees/example").to_string_lossy());
        let untouched: String = db
            .query_row(
                &format!("SELECT path FROM {table} WHERE id='external'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(untouched, external.to_string_lossy());
    }
    let history: Vec<u8> = db
        .query_row("SELECT body FROM session_revisions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(history, capture.as_bytes());
    let body: Vec<u8> = db
        .query_row("SELECT body FROM knowledge_collections", [], |row| {
            row.get(0)
        })
        .unwrap();
    let relocated: super::super::knowledge::Stored = serde_json::from_slice(&body).unwrap();
    let sailry_protocol::knowledge::Source::File { path } = relocated.collection.definition.source
    else {
        unreachable!()
    };
    assert_eq!(
        path,
        target
            .join("worktrees/example/source.txt")
            .to_string_lossy()
    );
    assert_eq!(relocated.capture.worktree, collection.capture.worktree);
    assert_eq!(
        relocated.collection.revision,
        collection.collection.revision
    );
    assert!(!original.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copies_consistent_state_without_blocking_commands() {
    use sailry_client::Client;
    use sailry_link::{Link, NetworkScope};
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = crate::Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let Output::BackupSettings(saved) = client
            .execute(client.prepare(Command::SaveBackupSettings(Update {
                settings: Settings {
                    destination: Some(destination()),
                    ..Default::default()
                },
                identity: SecretUpdate::Clear,
                secret: SecretUpdate::Clear,
                session_token: SecretUpdate::Clear,
                passphrase: SecretUpdate::Replace(Secret::new("isolated-backup-passphrase".into())),
            })))
            .await
            .unwrap()
        else {
            panic!("backup settings expected")
        };
        let project = directory.path().join("project");
        std::fs::create_dir_all(project.join("package")).unwrap();
        let manifest = |version: &str| serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example","version":version,"extensions":{"dev.sailry.platform":{"api_version":"v1"}}});
        std::fs::write(
            project.join("package/plugin.json"),
            manifest("0.1.0").to_string(),
        )
        .unwrap();
        let Output::Project(registered) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Snapshot".into(),
                path: project.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(current) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = current
            .worktrees
            .iter()
            .find(|entry| entry.project == Some(registered.id))
            .unwrap()
            .id;
        let install = |revision| Command::InstallPlugin {
            worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: revision,
            expected_digest: None,
        };
        let Output::Plugin(original) = client.execute(client.prepare(install(0))).await.unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(original.issues.is_empty());
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(registered.id),
                worktree: Some(worktree),
                config: Some(sailry_protocol::SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: sailry_protocol::ProviderId::new(),
                    model: "fixture".into(),
                    effort: sailry_protocol::Effort::Low,
                    mode: sailry_protocol::WorkMode::Code,
                    options: Default::default(),
                    permission: sailry_protocol::Permission::Ask,
                    credential: None,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Artifact fixture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        client
            .execute(client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        let profile = node.profile().to_owned();
        let html = "<!doctype html><html><body>Recorded document</body></html>";
        let artifact = sailry_protocol::artifact::Summary {
            id: sailry_protocol::ArtifactId::new(),
            session: session.id,
            revision: 1,
            title: "Recorded document".into(),
            summary: "Original content".into(),
            kind: sailry_protocol::artifact::Kind::Document,
            turn: turn.id,
            package: original.summary.reference(),
            size: html.len() as u64,
            created_at: 1,
            updated_at: 1,
        };
        let digest =
            crate::files::artifacts::write(&profile, session.id, artifact.id, 1, html).unwrap();
        let db = Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        db.execute(
            "INSERT INTO artifacts(id,session,body) VALUES(?1,?2,?3)",
            rusqlite::params![
                artifact.id.to_string(),
                session.id.to_string(),
                encode(&artifact).unwrap()
            ],
        )
        .unwrap();
        db.execute("INSERT INTO artifact_revisions(artifact,revision,body,digest,warnings) VALUES(?1,1,?2,?3,?4)",rusqlite::params![artifact.id.to_string(),encode(&artifact).unwrap(),&digest,encode(&Vec::<String>::new()).unwrap()]).unwrap();
        drop(db);
        let resources = node.ingress.backup_resources.clone().read_owned().await;
        let (reply, source) = tokio::sync::oneshot::channel();
        node.ingress
            .send_internal(Job::BackupSnapshot {
                revision: saved.revision,
                cancel: CancellationToken::new(),
                reply,
            })
            .await
            .unwrap();
        let source = source.await.unwrap().unwrap();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let worker = tokio::task::spawn_blocking(move || {
            let stop = CancellationToken::new();
            let (plan, mut snapshot, roots) = capture_with(source, &stop, move |db| {
                assert_eq!(
                    db.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                        .unwrap(),
                    "wal"
                );
                let _ = entered.send(());
                wait.recv().map_err(storage_error)?;
                Ok(())
            })?;
            snapshot.capture(&roots, &stop)?;
            drop(resources);
            Ok::<_, Fault>((plan, snapshot.pack(&stop)?, roots))
        });
        ready.await.unwrap();
        let Output::Session(removing) = client
            .execute(client.prepare(Command::ReadSession {
                session: session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let mut removal = client
            .dispatch(client.prepare(Command::RemoveSession {
                session: session.id,
                expected_revision: removing.revision,
            }))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while node.ingress.backup_resources.try_read().is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("physical cleanup must wait for the snapshot lease");
        let current = Connection::open_with_flags(
            profile.join("storage/node.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            current
                .query_row(
                    "SELECT state FROM sessions WHERE id=?1",
                    [session.id.to_string()],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "removed"
        );
        assert_eq!(
            current
                .query_row(
                    "SELECT count(*) FROM artifacts WHERE id=?1",
                    [artifact.id.to_string()],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        drop(current);
        assert!(matches!(
            removal.completion.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(
            crate::files::artifacts::read(
                &profile,
                session.id,
                artifact.id,
                1,
                artifact.size,
                &digest
            )
            .unwrap(),
            html
        );
        std::fs::write(
            project.join("package/plugin.json"),
            manifest("0.1.1").to_string(),
        )
        .unwrap();
        let update = tokio::time::timeout(Duration::from_secs(5), async {
            assert!(matches!(
                client
                    .execute(client.prepare(Command::Snapshot))
                    .await
                    .unwrap(),
                Output::Snapshot(_)
            ));
            client
                .execute(client.prepare(install(original.summary.revision)))
                .await
                .unwrap()
        })
        .await;
        let _ = release.send(());
        let Output::Plugin(updated) =
            update.expect("read snapshot must not block Store reads or durable writes")
        else {
            panic!("updated plugin expected")
        };
        assert!(updated.issues.is_empty());
        assert_ne!(updated.summary.digest, original.summary.digest);
        let (_, packed, roots) = worker.await.unwrap().unwrap();
        assert!(matches!(
            removal.completion.await.unwrap().unwrap(),
            Output::SessionsRemoved(_)
        ));
        assert!(
            !profile
                .join("artifacts")
                .join(session.id.to_string())
                .exists()
        );
        let restore = tempfile::tempdir().unwrap();
        let restored = restore.path().canonicalize().unwrap();
        backup::archive::unpack(
            packed.file.reopen().unwrap(),
            &restored,
            &CancellationToken::new(),
        )
        .unwrap();
        let copied = Connection::open_with_flags(
            restored.join("storage/node.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            copied
                .pragma_query_value(None, "integrity_check", |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert_eq!(
            copied
                .query_row(
                    "SELECT state FROM sessions WHERE id=?1",
                    [session.id.to_string()],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "active"
        );
        assert_eq!(
            copied
                .query_row(
                    "SELECT digest FROM plugin_packages WHERE name='example'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            original.summary.digest
        );
        assert!(roots.contains(&format!("plugins/packages/{}", original.summary.digest)));
        assert!(!roots.contains(&format!("plugins/packages/{}", updated.summary.digest)));
        assert_eq!(
            std::fs::read(
                restored
                    .join("plugins/packages")
                    .join(&original.summary.digest)
                    .join("plugin.json")
            )
            .unwrap(),
            manifest("0.1.0").to_string().as_bytes()
        );
        assert_eq!(
            crate::files::artifacts::read(
                &restored,
                session.id,
                artifact.id,
                1,
                artifact.size,
                &digest
            )
            .unwrap(),
            html
        );
        assert_eq!(
            copied
                .query_row(
                    "SELECT count(*) FROM artifacts WHERE id=?1",
                    [artifact.id.to_string()],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            copied
                .query_row(
                    "SELECT count(*) FROM plugin_packages WHERE digest=?1",
                    [updated.summary.digest],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        drop(copied);
        drop(packed);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
