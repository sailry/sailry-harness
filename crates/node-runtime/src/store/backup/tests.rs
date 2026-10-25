use super::*;
use sailry_protocol::backup::Destination;
use std::time::Duration;
mod history;

fn destination() -> Destination {
    Destination {
        kind: Kind::Webdav,
        endpoint: "https://backup.example.com".into(),
        root: "profiles".into(),
        bucket: String::new(),
        region: String::new(),
    }
}

struct Fixture {
    storage: Storage,
    knowledge: Arc<Handle>,
    _root: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().canonicalize().unwrap().join("profile");
        crate::profile::private_directory(&profile.join("storage")).unwrap();
        let identity = sailry_link::Identity::open(&profile).unwrap();
        let database = Database::open(
            &profile.join("storage/node.turso"),
            identity.id(),
            Some(profile.clone()),
        )
        .await
        .unwrap();
        let transaction = turso::transaction::Transaction::new_unchecked(
            &database.connection,
            TransactionBehavior::Concurrent,
        )
        .await
        .unwrap();
        execute(
            &transaction,
            &Request::new(
                identity.id(),
                Command::SaveBackupSettings(Update {
                    settings: Settings::default(),
                    identity: SecretUpdate::Keep,
                    secret: SecretUpdate::Keep,
                    session_token: SecretUpdate::Keep,
                    passphrase: SecretUpdate::Replace(Secret::new(
                        "isolated backup fixture passphrase".into(),
                    )),
                }),
            ),
        )
        .await
        .unwrap();
        transaction
            .execute(
                "INSERT INTO counters(namespace,value) VALUES('sessions',73)",
                (),
            )
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let path = profile.join("storage/knowledge.turso");
        let knowledge = Arc::new(Handle::new(
            path.clone(),
            super::super::database::engine(&path).await.unwrap(),
        ));
        let connection = knowledge.lease(false).await.unwrap();
        connection.execute_batch("BEGIN; CREATE TABLE marker(body TEXT NOT NULL); INSERT INTO marker(body) VALUES('initial'); COMMIT;").await.unwrap();
        drop(connection);
        Self {
            storage: database.into_storage(),
            knowledge,
            _root: root,
        }
    }

    async fn close(self) {
        drop(self.knowledge.pause().await.unwrap());
        self.storage.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_frozen_databases_and_watermarks() {
    let fixture = Fixture::new().await;
    let profile = fixture.storage.profile.as_ref().unwrap();
    let legacy = profile.join("storage/node.sqlite3");
    std::fs::write(&legacy, b"untouched legacy fixture").unwrap();
    let cancel = CancellationToken::new();
    let source = source(&fixture.storage, Some(&fixture.knowledge), 1, &cancel)
        .await
        .unwrap();
    for path in ["storage/node.turso", "storage/knowledge.turso"]
        .into_iter()
        .flat_map(|name| {
            crate::profile::database_files(Path::new(name))
                .into_iter()
                .take(2)
        })
    {
        assert!(
            source
                .snapshot
                .manifest
                .files
                .iter()
                .any(|file| Path::new(&file.path) == path),
            "{}",
            path.display()
        );
    }
    assert!(
        !source
            .snapshot
            .manifest
            .files
            .iter()
            .any(|file| file.path == "storage/node.sqlite3")
    );
    let connection = fixture.storage.handle.lease(false).await.unwrap();
    let transaction = turso::transaction::Transaction::new_unchecked(
        &connection,
        TransactionBehavior::Concurrent,
    )
    .await
    .unwrap();
    assert_eq!(
        super::super::database::counters::next(
            &transaction,
            super::super::database::counters::Namespace::Sessions,
        )
        .await
        .unwrap(),
        74
    );
    transaction.commit().await.unwrap();
    drop(connection);
    let connection = fixture.knowledge.lease(false).await.unwrap();
    connection
        .execute("UPDATE marker SET body='changed'", ())
        .await
        .unwrap();
    drop(connection);
    let (_, mut snapshot, roots) = capture(source, &cancel).unwrap();
    snapshot.capture(&roots, &cancel).unwrap();
    let packed = snapshot.pack(&cancel).unwrap();
    let record = backup::record(&packed, uuid::Uuid::new_v4().to_string()).unwrap();
    let target = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("restored");
    let (restored, path) = publish(
        packed.file,
        &target,
        RecordSource::Known(record.clone()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(restored, record);
    assert_eq!(path, target);
    assert_eq!(std::fs::read(&legacy).unwrap(), b"untouched legacy fixture");
    assert!(!target.join("storage/node.sqlite3").exists());
    for name in ["node.turso", "knowledge.turso"] {
        assert!(crate::profile::database_files(&target.join("storage").join(name))[1].is_file());
    }
    let path = target.join("storage/node.turso");
    let handle = Handle::new(
        path.clone(),
        super::super::database::engine(&path).await.unwrap(),
    );
    let connection = handle.lease(false).await.unwrap();
    let transaction = turso::transaction::Transaction::new_unchecked(
        &connection,
        TransactionBehavior::Concurrent,
    )
    .await
    .unwrap();
    assert_eq!(
        super::super::database::counters::next(
            &transaction,
            super::super::database::counters::Namespace::Sessions,
        )
        .await
        .unwrap(),
        74
    );
    transaction.commit().await.unwrap();
    drop(connection);
    drop(handle.pause().await.unwrap());
    let path = target.join("storage/knowledge.turso");
    let handle = Handle::new(
        path.clone(),
        super::super::database::engine(&path).await.unwrap(),
    );
    let connection = handle.lease(true).await.unwrap();
    assert_eq!(
        connection
            .prepare("SELECT body FROM marker")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "initial"
    );
    drop(connection);
    drop(handle.pause().await.unwrap());
    fixture.close().await;
}

#[tokio::test]
async fn reopens_both_handles_after_copy_failure() {
    let fixture = Fixture::new().await;
    let cancel = CancellationToken::new();
    let profile = fixture.storage.profile.as_ref().unwrap();
    let snapshot = Snapshot::new(profile, fixture.storage.node).unwrap();
    let collision = snapshot.database_path().unwrap();
    std::fs::write(collision, b"existing destination fixture").unwrap();
    let main = fixture.storage.handle.exclusive().await.unwrap();
    let knowledge = fixture.knowledge.exclusive().await.unwrap();
    let plan = plan(main.connection(), Some(1)).await.unwrap();
    let fault = freeze(main, Some(knowledge), plan, Vec::new(), snapshot, &cancel)
        .await
        .err()
        .unwrap();
    assert_eq!(fault.code, ErrorCode::Unavailable);
    let main = fixture.storage.handle.lease(true).await.unwrap();
    assert_eq!(read(&main).await.unwrap().settings.revision, 1);
    drop(main);
    let knowledge = fixture.knowledge.lease(true).await.unwrap();
    assert_eq!(
        knowledge
            .prepare("SELECT body FROM marker")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "initial"
    );
    drop(knowledge);
    fixture.close().await;
}

#[tokio::test]
async fn cancels_with_active_knowledge_lease() {
    let fixture = Fixture::new().await;
    let knowledge = fixture.knowledge.lease(true).await.unwrap();
    let cancel = CancellationToken::new();
    let mut pending = Box::pin(source(
        &fixture.storage,
        Some(&fixture.knowledge),
        1,
        &cancel,
    ));
    tokio::select! {
        biased;
        _ = &mut pending => panic!("backup must wait for the knowledge lease"),
        _ = tokio::task::yield_now() => {}
    }
    cancel.cancel();
    assert_eq!(pending.await.err().unwrap().code, ErrorCode::Cancelled);
    drop(knowledge);
    let main = fixture.storage.handle.lease(true).await.unwrap();
    assert_eq!(read(&main).await.unwrap().settings.revision, 1);
    drop(main);
    fixture.close().await;
}

#[tokio::test]
async fn preserves_private_credentials_and_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(
        &root.path().join("node.turso"),
        sailry_protocol::NodeId([1; 32]),
        None,
    )
    .await
    .unwrap();
    let db = turso::transaction::Transaction::new_unchecked(
        &database.connection,
        TransactionBehavior::Concurrent,
    )
    .await
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
    let (output, event) = execute(&db, &request).await.unwrap();
    let public = serde_json::to_string(&(output.clone(), event)).unwrap();
    assert!(!public.contains("private-user"));
    assert!(!public.contains("private-password"));
    assert!(!public.contains("private-backup-passphrase"));
    assert_eq!(
        read(&db).await.unwrap().auth.secret.unwrap().expose(),
        "private-password"
    );
    assert_eq!(
        execute(&db, &request).await.unwrap_err().code,
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
    execute(&db, &request).await.unwrap();
    assert!(!read(&db).await.unwrap().settings.credentials_configured);
    assert!(read(&db).await.unwrap().auth.secret.is_none());
    assert!(read(&db).await.unwrap().settings.passphrase_configured);
    assert_eq!(
        read(&db).await.unwrap().auth.passphrase.unwrap().expose(),
        "private-backup-passphrase"
    );
    db.commit().await.unwrap();
    database.close().await.unwrap();
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

#[tokio::test]
async fn relocates_only_current_owned_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let original = root.join("original");
    let target = root.join("restored");
    let staging = root.join("staging");
    std::fs::create_dir_all(staging.join("storage")).unwrap();
    let path = staging.join("storage/node.turso");
    let handle = Handle::new(
        path.clone(),
        super::super::database::engine(&path).await.unwrap(),
    );
    let db = handle.lease(false).await.unwrap();
    db.execute_batch("BEGIN; CREATE TABLE projects(id TEXT PRIMARY KEY,path TEXT); CREATE TABLE worktrees(id TEXT PRIMARY KEY,path TEXT); CREATE TABLE knowledge_collections(id TEXT PRIMARY KEY,body BLOB); CREATE TABLE session_revisions(body BLOB); COMMIT;").await.unwrap();
    let owned = original.join("worktrees/example");
    let external = root.join("external");
    for table in ["projects", "worktrees"] {
        db.execute(
            &format!("INSERT INTO {table}(id,path) VALUES('owned',?1),('external',?2)"),
            params![owned.to_str().unwrap(), external.to_str().unwrap()],
        )
        .await
        .unwrap();
    }
    let capture =
        serde_json::json!({"path":owned.to_str().unwrap(),"user":"unchanged"}).to_string();
    db.execute(
        "INSERT INTO session_revisions(body) VALUES(?1)",
        [capture.as_bytes()],
    )
    .await
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
        params![
            collection.collection.id.to_string(),
            encode(&collection).unwrap()
        ],
    )
    .await
    .unwrap();
    drop(db);
    let closed = handle.pause().await.unwrap();
    relocate(&staging, &original, &target).await.unwrap();
    closed.reopen().await.unwrap();
    let db = handle.lease(true).await.unwrap();
    for table in ["projects", "worktrees"] {
        let owned = db
            .prepare(format!("SELECT path FROM {table} WHERE id='owned'"))
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<String>(0)
            .unwrap();
        assert_eq!(owned, target.join("worktrees/example").to_string_lossy());
        let untouched = db
            .prepare(format!("SELECT path FROM {table} WHERE id='external'"))
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<String>(0)
            .unwrap();
        assert_eq!(untouched, external.to_string_lossy());
    }
    let history = db
        .prepare("SELECT body FROM session_revisions")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<Vec<u8>>(0)
        .unwrap();
    assert_eq!(history, capture.as_bytes());
    let body = db
        .prepare("SELECT body FROM knowledge_collections")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<Vec<u8>>(0)
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
    drop(db);
    drop(handle.pause().await.unwrap());
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
                    features: Default::default(),
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
        let stored = artifact.clone();
        let saved_digest = digest.clone();
        node.with_storage(move |db| {
            Box::pin(async move {
                let transaction = db
                    .transaction_with_behavior(TransactionBehavior::Concurrent)
                    .await
                    .map_err(storage_error)?;
                let sequence = super::super::database::counters::next(
                    &transaction,
                    super::super::database::counters::Namespace::Artifacts,
                )
                .await?;
                transaction
                    .execute(
                        "INSERT INTO artifacts(rowid,id,session,body) VALUES(?1,?2,?3,?4)",
                        params![
                            sequence as i64,
                            stored.id.to_string(),
                            stored.session.to_string(),
                            encode(&stored)?
                        ],
                    )
                    .await
                    .map_err(storage_error)?;
                transaction.execute("INSERT INTO artifact_revisions(artifact,revision,body,digest,warnings) VALUES(?1,1,?2,?3,?4)",params![stored.id.to_string(),encode(&stored)?,saved_digest,encode(&Vec::<String>::new())?]).await.map_err(storage_error)?;
                transaction.commit().await.map_err(storage_error)
            })
        })
        .await
        .unwrap();
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
            let (plan, mut snapshot, roots) = capture(source, &stop)?;
            let _ = entered.send(());
            wait.recv().map_err(storage_error)?;
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
        let artifact_id = artifact.id;
        let (state, count) = node
            .with_storage(move |db| {
                Box::pin(async move {
                    let state = db
                        .prepare("SELECT state FROM sessions WHERE id=?1")
                        .await
                        .map_err(storage_error)?
                        .query_row([session.id.to_string()])
                        .await
                        .map_err(storage_error)?
                        .get::<String>(0)
                        .map_err(storage_error)?;
                    let count = db
                        .prepare("SELECT count(*) FROM artifacts WHERE id=?1")
                        .await
                        .map_err(storage_error)?
                        .query_row([artifact_id.to_string()])
                        .await
                        .map_err(storage_error)?
                        .get::<i64>(0)
                        .map_err(storage_error)?;
                    Ok((state, count))
                })
            })
            .await
            .unwrap();
        assert_eq!(state, "removed");
        assert_eq!(count, 0);
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
        let path = restored.join("storage/node.turso");
        let handle = Handle::new(
            path.clone(),
            super::super::database::engine(&path).await.unwrap(),
        );
        let copied = handle.lease(true).await.unwrap();
        assert_eq!(
            copied
                .prepare("PRAGMA integrity_check")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<String>(0)
                .unwrap(),
            "ok"
        );
        assert_eq!(
            copied
                .prepare("PRAGMA journal_mode")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<String>(0)
                .unwrap(),
            "mvcc"
        );
        assert_eq!(
            copied
                .prepare("SELECT state FROM sessions WHERE id=?1")
                .await
                .unwrap()
                .query_row([session.id.to_string()])
                .await
                .unwrap()
                .get::<String>(0)
                .unwrap(),
            "active"
        );
        assert_eq!(
            copied
                .prepare("SELECT digest FROM plugin_packages WHERE name='example'")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<String>(0)
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
                .prepare("SELECT count(*) FROM artifacts WHERE id=?1")
                .await
                .unwrap()
                .query_row([artifact.id.to_string()])
                .await
                .unwrap()
                .get::<i64>(0)
                .unwrap(),
            1
        );
        assert_eq!(
            copied
                .prepare("SELECT count(*) FROM plugin_packages WHERE digest=?1")
                .await
                .unwrap()
                .query_row([updated.summary.digest])
                .await
                .unwrap()
                .get::<i64>(0)
                .unwrap(),
            0
        );
        drop(copied);
        drop(handle.pause().await.unwrap());
        drop(packed);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[test]
fn separates_local_destination_from_profile() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let profile = root.join("node");
    std::fs::create_dir(&profile).unwrap();
    assert!(local_target(root.join("archives/daily").to_str().unwrap(), &profile).is_ok());
    for target in [&profile, &root, &profile.join("attachments/backups")] {
        assert_eq!(
            local_target(target.to_str().unwrap(), &profile)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
}

#[test]
fn excludes_plugin_contexts() {
    let mut request = Request::new(
        sailry_protocol::NodeId([1; 32]),
        Command::DownloadBackup {
            expected_revision: 0,
        },
    );
    controller(&request).unwrap();
    request.plugin = Some(sailry_protocol::plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: sailry_protocol::plugin::Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        worktree: None,
        session: None,
    });
    assert_eq!(
        controller(&request).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn rejects_invalid_schedule_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(
        &root.path().join("node.turso"),
        sailry_protocol::NodeId([1; 32]),
        None,
    )
    .await
    .unwrap();
    let db = turso::transaction::Transaction::new_unchecked(
        &database.connection,
        TransactionBehavior::Concurrent,
    )
    .await
    .unwrap();
    for schedule in [
        sailry_protocol::backup::Schedule {
            enabled: true,
            timezone: "Invalid/Zone".into(),
            hour: 9,
            minute: 5,
        },
        sailry_protocol::backup::Schedule {
            enabled: true,
            timezone: "UTC".into(),
            hour: 24,
            minute: 0,
        },
        sailry_protocol::backup::Schedule {
            enabled: true,
            timezone: "UTC".into(),
            hour: 9,
            minute: 60,
        },
    ] {
        let request = Request::new(
            sailry_protocol::NodeId([1; 32]),
            Command::SaveBackupSettings(Update {
                settings: Settings {
                    destination: Some(destination()),
                    schedule: Some(schedule),
                    ..Settings::default()
                },
                identity: SecretUpdate::Keep,
                secret: SecretUpdate::Keep,
                session_token: SecretUpdate::Keep,
                passphrase: SecretUpdate::Replace(Secret::new(
                    "scheduled-backup-passphrase".into(),
                )),
            }),
        );
        assert_eq!(
            execute(&db, &request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(read(&db).await.unwrap().settings, Settings::default());
        let records = db.prepare("SELECT (SELECT count(*) FROM dispatch_handlers)+(SELECT count(*) FROM dispatch_schedules)").await.unwrap().query_row(()).await.unwrap().get::<i64>(0).unwrap();
        assert_eq!(records, 0);
    }
    db.commit().await.unwrap();
    database.close().await.unwrap();
}
