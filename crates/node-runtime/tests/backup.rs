use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    backup::{Kind, Settings, Update},
    plugin::settings::SecretUpdate,
    *,
};
use std::{fs, path::Path, time::Duration};
#[path = "../src/backup/tests/server.rs"]
mod server;

async fn execute(client: &Client, command: Command) -> Output {
    tokio::time::timeout(
        Duration::from_secs(60),
        client.execute(client.prepare(command)),
    )
    .await
    .expect("backup operation deadline")
    .unwrap()
}
async fn settings(client: &Client) -> Settings {
    let Output::BackupSettings(settings) = execute(client, Command::ReadBackupSettings).await
    else {
        panic!("backup settings expected")
    };
    settings
}

#[tokio::test]
async fn shares_private_configuration_and_receipts() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let node = Node::start_with_plugin_catalog(root.join("node"), "http://127.0.0.1:9/")
            .await
            .unwrap();
        let controller = Link::controller(root.join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let local = Client::new(node.local());
        let paired = Client::new(controller.handle().remote(address));
        let (writer, reader) = if remote {
            (&paired, &local)
        } else {
            (&local, &paired)
        };
        let server = server::Server::start(Kind::Webdav).await;
        let previous = settings(writer).await;
        let mut desired = previous.clone();
        desired.destination = Some(server.destination());
        let request = writer.prepare(Command::SaveBackupSettings(Update {
            settings: desired,
            identity: SecretUpdate::Replace(Secret::new("fixture-user".into())),
            secret: SecretUpdate::Replace(Secret::new("fixture-password".into())),
            session_token: SecretUpdate::Clear,
            passphrase: SecretUpdate::Replace(Secret::new("fixture-backup-passphrase".into())),
        }));
        let admission = writer.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let saved = admission.completion.await.unwrap().unwrap();
        assert_eq!(writer.execute(request.clone()).await.unwrap(), saved);
        assert_eq!(execute(reader, Command::ReadBackupSettings).await, saved);
        let public = serde_json::to_string(&saved).unwrap();
        assert!(!public.contains("fixture-user"));
        assert!(!public.contains("fixture-password"));
        assert!(!public.contains("fixture-backup-passphrase"));
        assert_eq!(
            reader.outcome(&request).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        assert_eq!(
            writer
                .execute(writer.prepare(Command::SaveBackupSettings(Update {
                    settings: previous,
                    identity: SecretUpdate::Keep,
                    secret: SecretUpdate::Keep,
                    session_token: SecretUpdate::Keep,
                    passphrase: SecretUpdate::Replace(Secret::new(
                        "fixture-backup-passphrase".into()
                    )),
                })))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let mut wrong = request.clone();
        wrong.id = RequestId::new();
        wrong.target = NodeId([9; 32]);
        assert_eq!(
            writer.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        let Output::Snapshot(snapshot) = execute(reader, Command::Snapshot).await else {
            unreachable!()
        };
        let public = serde_json::to_string(&snapshot).unwrap();
        assert!(!public.contains("fixture-password"));
        assert!(!public.contains("fixture-backup-passphrase"));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_new_profiles_without_external_mutation() {
    for remote in [false, true] {
        for kind in [Kind::Webdav, Kind::S3] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            let node = Node::start_with_plugin_catalog(root.join("node"), "http://127.0.0.1:9/")
                .await
                .unwrap();
            let controller = Link::controller(root.join("controller"), NetworkScope::default())
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
            let external = root.join("project");
            fs::create_dir(&external).unwrap();
            fs::write(
                external.join("source.txt"),
                b"external source stays unchanged",
            )
            .unwrap();
            let Output::Project(project) = execute(
                &client,
                Command::RegisterProject {
                    name: "Retained project".into(),
                    path: external.to_str().unwrap().into(),
                },
            )
            .await
            else {
                unreachable!()
            };
            let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
                unreachable!()
            };
            let worktree = snapshot
                .worktrees
                .iter()
                .find(|worktree| worktree.project == Some(project.id))
                .unwrap()
                .id;
            let config = SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                credential: None,
            };
            let Output::Session(session) = execute(
                &client,
                Command::CreateSession {
                    project: Some(project.id),
                    worktree: Some(worktree),
                    config: Some(config.clone()),
                },
            )
            .await
            else {
                unreachable!()
            };
            let Output::Session(scratch) = execute(
                &client,
                Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(config),
                },
            )
            .await
            else {
                unreachable!()
            };
            let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
                unreachable!()
            };
            let managed = snapshot
                .worktrees
                .iter()
                .find(|worktree| worktree.id == scratch.worktree)
                .unwrap();
            let source = Path::new(&managed.path).join("scratch.txt");
            fs::create_dir_all(source.parent().unwrap()).unwrap();
            fs::write(&source, b"retained managed scratch content").unwrap();
            let relative = source.strip_prefix(node.profile()).unwrap().to_owned();
            for path in ["tmp/disposable.txt", "models/cache.txt", "python/cache.txt"] {
                let path = node.profile().join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"excluded cache").unwrap();
            }
            assert!(node.profile().join("storage/knowledge.db").exists());
            let db =
                rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
            let captured: Vec<u8> = db
                .query_row(
                    "SELECT config FROM session_revisions WHERE session=?1",
                    [session.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            let scratch_capture: Vec<u8> = db
                .query_row(
                    "SELECT config FROM session_revisions WHERE session=?1",
                    [scratch.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            drop(db);
            let server = server::Server::start(kind).await;
            let mut configuration = settings(&client).await;
            configuration.destination = Some(server.destination());
            let Output::BackupSettings(saved) = execute(
                &client,
                Command::SaveBackupSettings(Update {
                    settings: configuration,
                    identity: SecretUpdate::Replace(Secret::new("fixture-user".into())),
                    secret: SecretUpdate::Replace(Secret::new("fixture-password".into())),
                    session_token: SecretUpdate::Clear,
                    passphrase: SecretUpdate::Replace(Secret::new(
                        "fixture-backup-passphrase".into(),
                    )),
                }),
            )
            .await
            else {
                unreachable!()
            };
            let export = client.prepare(Command::ExportBackup {
                expected_revision: saved.revision,
            });
            let admission = client.dispatch(export.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let result = admission.completion.await.unwrap().unwrap();
            let Output::Backup(record) = &result else {
                panic!("backup expected")
            };
            assert!(server.maximum_put() <= 4 * 1024 * 1024);
            assert!(server.parts(&record.id) > 0);
            let writes = server.writes();
            assert_eq!(client.execute(export.clone()).await.unwrap(), result);
            assert_eq!(server.writes(), writes);
            assert_eq!(
                client.outcome(&export).await.unwrap(),
                RequestOutcome::Completed(Box::new(Ok(result.clone())))
            );
            let Output::Backups(page) = execute(&client, Command::ListBackups).await else {
                unreachable!()
            };
            assert_eq!(page.records, vec![record.clone()]);
            assert!(!page.partial);
            let original = fs::read(node.profile().join("link.key")).unwrap();
            let busy = root.join("nonempty");
            fs::create_dir(&busy).unwrap();
            fs::write(busy.join("user.txt"), b"must remain").unwrap();
            let error = client
                .execute(client.prepare(Command::RestoreBackup {
                    passphrase: None,
                    id: record.id.clone(),
                    expected_revision: saved.revision,
                    target: busy.to_str().unwrap().into(),
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Conflict);
            assert_eq!(fs::read(busy.join("user.txt")).unwrap(), b"must remain");
            let target = root.join("restored");
            let wrong_target = root.join("wrong-passphrase");
            let error = client
                .execute(client.prepare(Command::RestoreBackup {
                    id: record.id.clone(),
                    expected_revision: saved.revision,
                    target: wrong_target.to_str().unwrap().into(),
                    passphrase: Some(Secret::new("wrong-backup-passphrase".into())),
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert!(!wrong_target.exists());
            let restore = client.prepare(Command::RestoreBackup {
                passphrase: None,
                id: record.id.clone(),
                expected_revision: saved.revision,
                target: target.to_str().unwrap().into(),
            });
            let restored = client.execute(restore.clone()).await.unwrap();
            assert!(matches!(restored, Output::BackupRestored { .. }));
            assert_eq!(client.execute(restore).await.unwrap(), restored);
            assert_eq!(fs::read(target.join("link.key")).unwrap(), original);
            assert_eq!(
                fs::read(target.join(&relative)).unwrap(),
                b"retained managed scratch content"
            );
            assert_eq!(
                fs::read(&source).unwrap(),
                b"retained managed scratch content"
            );
            for path in [
                "models",
                "python",
                "tmp",
                "storage/knowledge.db",
                "node.lock",
            ] {
                assert!(!target.join(path).exists(), "excluded {path}");
            }
            let db = rusqlite::Connection::open(target.join("storage/node.sqlite3")).unwrap();
            let retained: Vec<u8> = db
                .query_row(
                    "SELECT config FROM session_revisions WHERE session=?1",
                    [session.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(retained, captured);
            let retained: Vec<u8> = db
                .query_row(
                    "SELECT config FROM session_revisions WHERE session=?1",
                    [scratch.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(retained, scratch_capture);
            let current: String = db
                .query_row(
                    "SELECT path FROM worktrees WHERE id=?1",
                    [managed.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                Path::new(&current),
                target.join(
                    Path::new(&managed.path)
                        .strip_prefix(node.profile())
                        .unwrap()
                )
            );
            let retained: String = db
                .query_row(
                    "SELECT path FROM projects WHERE id=?1",
                    [project.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(Path::new(&retained), external);
            drop(db);
            assert_eq!(
                fs::read(external.join("source.txt")).unwrap(),
                b"external source stays unchanged"
            );
            server.corrupt(&record.id);
            let rejected = root.join("corrupt-restore");
            let error = client
                .execute(client.prepare(Command::RestoreBackup {
                    passphrase: None,
                    id: record.id.clone(),
                    expected_revision: saved.revision,
                    target: rejected.to_str().unwrap().into(),
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert!(!rejected.exists());
            assert_eq!(fs::read(node.profile().join("link.key")).unwrap(), original);
            assert_eq!(
                fs::read(&source).unwrap(),
                b"retained managed scratch content"
            );
            assert_eq!(
                fs::read(external.join("source.txt")).unwrap(),
                b"external source stays unchanged"
            );
            let id = node.id();
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
            let restored = Node::start_with_plugin_catalog(&target, "http://127.0.0.1:9/")
                .await
                .unwrap();
            assert_eq!(restored.id(), id);
            let reader = Client::new(restored.local());
            let Output::Session(loaded) = execute(
                &reader,
                Command::ReadSession {
                    session: session.id,
                },
            )
            .await
            else {
                unreachable!()
            };
            assert_eq!(loaded.id, session.id);
            assert!(settings(&reader).await.credentials_configured);
            restored.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn restores_independent_managed_git_checkouts() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let external = root.join("repository");
        let repository = git2::Repository::init(&external).unwrap();
        repository.set_head("refs/heads/main").unwrap();
        fs::write(external.join("tracked.txt"), b"base\n").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
            .unwrap();
        drop(tree);
        drop(index);
        let profile = root.join("node");
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let controller = Link::controller(root.join("controller"), NetworkScope::default())
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
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: "Git source".into(),
                path: external.to_str().unwrap().into(),
            },
        )
        .await
        else {
            unreachable!()
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            unreachable!()
        };
        let source = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.project == Some(project.id))
            .unwrap()
            .id;
        let Output::GitStatus(base) =
            execute(&client, Command::InspectGit { worktree: source }).await
        else {
            unreachable!()
        };
        let Output::Worktree(worktree) = execute(
            &client,
            Command::CreateManagedWorktree {
                project: project.id,
                source,
                branch: "backup-managed".into(),
                expected_head: commit.to_string(),
                expected_index: base.index_revision.unwrap(),
                include_changes: false,
            },
        )
        .await
        else {
            unreachable!()
        };
        let original = Path::new(&worktree.path);
        assert!(original.join(".git").is_file());
        let linked = git2::Repository::open(original).unwrap();
        fs::write(original.join("tracked.txt"), b"staged\n").unwrap();
        let mut index = linked.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        drop(index);
        drop(linked);
        fs::write(original.join("tracked.txt"), b"working\n").unwrap();
        fs::write(original.join("untracked.txt"), b"untracked\n").unwrap();
        let Output::GitStatus(before) = execute(
            &client,
            Command::InspectGit {
                worktree: worktree.id,
            },
        )
        .await
        else {
            unreachable!()
        };
        let metadata = files(repository.path());
        let server = server::Server::start(Kind::Webdav).await;
        let mut settings = settings(&client).await;
        settings.destination = Some(server.destination());
        let Output::BackupSettings(settings) = execute(
            &client,
            Command::SaveBackupSettings(Update {
                settings,
                identity: SecretUpdate::Replace(Secret::new("fixture-user".into())),
                secret: SecretUpdate::Replace(Secret::new("fixture-password".into())),
                session_token: SecretUpdate::Keep,
                passphrase: SecretUpdate::Replace(Secret::new("fixture-backup-passphrase".into())),
            }),
        )
        .await
        else {
            unreachable!()
        };
        let Output::Backup(record) = execute(
            &client,
            Command::ExportBackup {
                expected_revision: settings.revision,
            },
        )
        .await
        else {
            unreachable!()
        };
        let target = root.join("restored");
        execute(
            &client,
            Command::RestoreBackup {
                passphrase: None,
                id: record.id,
                expected_revision: settings.revision,
                target: target.to_str().unwrap().into(),
            },
        )
        .await;
        assert_eq!(files(repository.path()), metadata);
        assert!(original.join(".git").is_file());
        let relative = original.strip_prefix(node.profile()).unwrap().to_owned();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        fs::rename(&profile, root.join("old-profile-unavailable")).unwrap();
        let restored_root = target.join(relative);
        assert!(restored_root.join(".git").is_dir());
        let owned = git2::Repository::open(&restored_root).unwrap();
        assert_eq!(owned.workdir().unwrap(), restored_root);
        assert!(!owned.path().join("objects/info/alternates").exists());
        drop(owned);
        let restored = Node::start_with_plugin_catalog(&target, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let controller =
            Link::controller(root.join("restored-controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(restored.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let reader = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            restored.local()
        });
        let Output::GitStatus(after) = execute(
            &reader,
            Command::InspectGit {
                worktree: worktree.id,
            },
        )
        .await
        else {
            unreachable!()
        };
        assert_eq!(after.kind, RepositoryKind::Ready);
        assert_eq!(after.branch, before.branch);
        assert_eq!(after.head, before.head);
        assert_eq!(after.entries, before.entries);
        let Output::GitDiff(staged) = execute(
            &reader,
            Command::ReadGitDiff {
                worktree: worktree.id,
                path: "tracked.txt".into(),
                scope: GitDiffScope::Staged,
            },
        )
        .await
        else {
            unreachable!()
        };
        assert!(staged.text.contains("+staged"));
        let Output::GitDiff(unstaged) = execute(
            &reader,
            Command::ReadGitDiff {
                worktree: worktree.id,
                path: "tracked.txt".into(),
                scope: GitDiffScope::Unstaged,
            },
        )
        .await
        else {
            unreachable!()
        };
        assert!(unstaged.text.contains("+working"));
        assert_eq!(files(repository.path()), metadata);
        assert_eq!(fs::read(external.join("tracked.txt")).unwrap(), b"base\n");
        restored.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

fn files(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn collect(
        root: &Path,
        relative: &Path,
        files: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        for entry in fs::read_dir(root.join(relative)).unwrap() {
            let entry = entry.unwrap();
            let path = relative.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                collect(root, &path, files);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                files.insert(path.clone(), fs::read(root.join(path)).unwrap());
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    collect(root, Path::new(""), &mut files);
    files
}
