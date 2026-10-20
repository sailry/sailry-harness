use sailry_client::{Apply, Client, Projection};
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
        for phrase in ["a", "1234567", "密码太短", "validlength\n"] {
            let error = writer
                .execute(writer.prepare(Command::SaveBackupSettings(Update {
                    settings: desired.clone(),
                    identity: SecretUpdate::Keep,
                    secret: SecretUpdate::Keep,
                    session_token: SecretUpdate::Keep,
                    passphrase: SecretUpdate::Replace(Secret::new(phrase.into())),
                })))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(settings(reader).await, previous);
        }
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
                features: Default::default(),
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                options: Default::default(),
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
                    location: sailry_protocol::backup::Location::Online,
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
                    location: sailry_protocol::backup::Location::Online,
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
                location: sailry_protocol::backup::Location::Online,
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
                    location: sailry_protocol::backup::Location::Online,
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
            // The controller can observe changes newer than the backup it restores.
            for _ in 0..3 {
                execute(
                    &client,
                    Command::SaveBackupSettings(Update {
                        settings: settings(&client).await,
                        identity: SecretUpdate::Keep,
                        secret: SecretUpdate::Keep,
                        session_token: SecretUpdate::Keep,
                        passphrase: SecretUpdate::Keep,
                    }),
                )
                .await;
            }
            let Output::Snapshot(current) = execute(&client, Command::Snapshot).await else {
                unreachable!()
            };
            let mut projection = Projection::new(node.id(), 1);
            projection
                .apply(1, sailry_protocol::Update::Snapshot(current.clone()))
                .unwrap();
            let id = node.id();
            node.shutdown().await.unwrap();
            let restored = Node::start_with_plugin_catalog(&target, "http://127.0.0.1:9/")
                .await
                .unwrap();
            assert_eq!(restored.id(), id);
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
            projection.reconnect(2).unwrap();
            reader.recover(&mut projection, 2).await.unwrap();
            let recovered = projection.snapshot().unwrap();
            assert!(recovered.cursor < current.cursor);
            let mut updates = reader.subscribe().await.unwrap();
            assert_eq!(
                projection.apply(2, updates.next().await.unwrap()).unwrap(),
                Apply::Applied
            );
            drop(updates);
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
            controller.close().await.unwrap();
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
        let Output::Project(_) = execute(
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
        let config = SessionConfig {
            features: Default::default(),
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            options: Default::default(),
            permission: Permission::Ask,
            credential: None,
        };
        let Output::Session(session) = execute(
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
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.id == session.worktree)
            .unwrap()
            .clone();
        let original = Path::new(&worktree.path);
        assert!(original.starts_with(node.profile()));
        fs::create_dir_all(original).unwrap();
        assert!(fs::read_dir(original).unwrap().next().is_none());
        fs::remove_dir(original).unwrap();
        let base = repository.find_commit(commit).unwrap();
        let branch = repository
            .branch("backup-managed", &base, false)
            .unwrap()
            .into_reference();
        repository
            .worktree(
                &worktree.id.to_string(),
                original,
                Some(git2::WorktreeAddOptions::new().reference(Some(&branch))),
            )
            .unwrap();
        drop(branch);
        drop(base);
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
                location: sailry_protocol::backup::Location::Online,
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

#[tokio::test]
async fn transfers_archives_to_controller_files() {
    use tokio::io::AsyncWriteExt as _;
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let profile = root.join("node");
        let node = Node::start_empty(&profile).await.unwrap();
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
        let (client, other) = if remote {
            (&paired, &local)
        } else {
            (&local, &paired)
        };
        let Output::BackupSettings(saved) = execute(
            client,
            Command::SaveBackupSettings(Update {
                settings: Settings {
                    local: Some(backup::Local {
                        directory: profile.to_str().unwrap().into(),
                        enabled: true,
                    }),
                    schedule: Some(backup::Schedule {
                        enabled: false,
                        timezone: "Asia/Shanghai".into(),
                        hour: 9,
                        minute: 5,
                    }),
                    ..Settings::default()
                },
                identity: SecretUpdate::Keep,
                secret: SecretUpdate::Keep,
                session_token: SecretUpdate::Keep,
                passphrase: SecretUpdate::Replace(Secret::new(
                    "controller-archive-passphrase".into(),
                )),
            }),
        )
        .await
        else {
            unreachable!()
        };
        assert_eq!(
            client
                .execute(client.prepare(Command::ExportBackup {
                    expected_revision: saved.revision,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let mut desired = saved;
        desired.local.as_mut().unwrap().enabled = false;
        let Output::BackupSettings(saved) = execute(
            client,
            Command::SaveBackupSettings(Update {
                settings: desired,
                identity: SecretUpdate::Keep,
                secret: SecretUpdate::Keep,
                session_token: SecretUpdate::Keep,
                passphrase: SecretUpdate::Keep,
            }),
        )
        .await
        else {
            unreachable!()
        };
        let Output::BackupDownload(download) = execute(
            client,
            Command::DownloadBackup {
                expected_revision: saved.revision,
            },
        )
        .await
        else {
            unreachable!()
        };
        assert_eq!(download.record.node, node.id());
        assert_eq!(settings(other).await, saved);
        assert!(other.open(download.stream).await.is_err());
        let path = root.join("controller-backup.age");
        let mut writer = tokio::fs::File::create(&path).await.unwrap();
        client
            .download_backup(
                &download,
                &mut writer,
                sailry_link::CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        writer.flush().await.unwrap();
        drop(writer);
        assert_eq!(fs::metadata(&path).unwrap().len(), download.size);
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"age-encryption.org/v1\n"));
        assert!(
            !bytes
                .windows(b"controller-archive-passphrase".len())
                .any(|part| part == b"controller-archive-passphrase")
        );
        assert!(client.open(download.stream).await.is_err());
        let Output::BackupUpload(upload) = execute(
            client,
            Command::UploadBackup {
                size: bytes.len() as u64,
                revision: blake3::hash(&bytes).to_hex().to_string(),
            },
        )
        .await
        else {
            unreachable!()
        };
        let mut input = std::io::Cursor::new(bytes);
        client
            .upload(
                &upload,
                &mut input,
                sailry_link::CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        let target = root.join("restored");
        let command = Command::RestoreBackupFile {
            stream: upload.stream,
            expected_revision: saved.revision,
            target: target.to_str().unwrap().into(),
            passphrase: None,
        };
        let request = client.prepare(command);
        // Another authenticated controller cannot consume this caller's staged archive.
        assert_eq!(
            other.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(!target.exists());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let result = admission.completion.await.unwrap().unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        let Output::BackupRestored { record, .. } = result else {
            unreachable!()
        };
        assert_eq!(record.node, node.id());
        assert_eq!(
            fs::read(profile.join("link.key")).unwrap(),
            fs::read(target.join("link.key")).unwrap()
        );
        let id = node.id();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let restored = Node::start_empty(&target).await.unwrap();
        assert_eq!(restored.id(), id);
        restored.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn executes_overdue_backup_after_restart() {
    use chrono::Timelike as _;
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let profile = root.join("node");
        let destination = root.join("automatic-backups");
        let scheduled = chrono::Utc::now() - chrono::Duration::minutes(5);
        let hour = scheduled.hour() as u8;
        let minute = scheduled.minute() as u8;
        let node = Node::start_empty(&profile).await.unwrap();
        let identity = node.id();
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
        let Output::BackupSettings(saved) = execute(
            &client,
            Command::SaveBackupSettings(Update {
                settings: Settings {
                    local: Some(backup::Local {
                        directory: destination.to_str().unwrap().into(),
                        enabled: true,
                    }),
                    online_enabled: false,
                    schedule: Some(backup::Schedule {
                        enabled: true,
                        timezone: "UTC".into(),
                        hour,
                        minute,
                    }),
                    ..Settings::default()
                },
                identity: SecretUpdate::Clear,
                secret: SecretUpdate::Clear,
                session_token: SecretUpdate::Clear,
                passphrase: SecretUpdate::Replace(Secret::new(
                    "automatic-backup-passphrase".into(),
                )),
            }),
        )
        .await
        else {
            panic!("backup settings expected")
        };
        drop(client);
        node.shutdown().await.unwrap();

        // Reuse the calendar service fixture's isolated downtime setup. Keep
        // the registered handler, weekly rule, identity, and revision unchanged.
        let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let body: Vec<u8> = database
            .query_row(
                "SELECT body FROM dispatch_schedules WHERE package=''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut schedule: dispatch::Schedule = serde_json::from_slice(&body).unwrap();
        let original = schedule.clone();
        let due = (chrono::Utc::now() - chrono::Duration::days(1))
            .date_naive()
            .and_hms_opt(u32::from(hour), u32::from(minute), 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        assert!(original.next_ms.unwrap() > chrono::Utc::now().timestamp_millis());
        schedule.next_ms = Some(due);
        assert_eq!(
            database
                .execute(
                    "UPDATE dispatch_schedules SET next_ms=?1,body=?2 WHERE package='' AND id=?3",
                    rusqlite::params![
                        due,
                        serde_json::to_vec(&schedule).unwrap(),
                        schedule.id.to_string()
                    ],
                )
                .unwrap(),
            1
        );
        database.close().unwrap();

        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        // No controller submits ExportBackup: the original global worker must
        // deliver the due native callback and publish the encrypted archive.
        let record = tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                let Output::Backups(page) = execute(&client, Command::ListBackups).await else {
                    panic!("backup page expected")
                };
                assert!(page.records.len() <= 1);
                if let Some(record) = page.records.first() {
                    assert_eq!(
                        page.locations[&record.id],
                        backup::Locations {
                            local: true,
                            online: false
                        }
                    );
                    break record.clone();
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .expect("automatic backup deadline");
        assert_eq!(record.node, identity);
        let manifest = destination.join(format!("{}.json", record.id));
        let bytes = fs::read(&manifest).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest["record"], serde_json::to_value(&record).unwrap());
        assert!(!manifest["parts"].as_array().unwrap().is_empty());
        assert!(destination.join(&record.id).is_dir());

        let database = rusqlite::Connection::open_with_flags(
            profile.join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let body: Vec<u8> = database
            .query_row(
                "SELECT body FROM dispatch_jobs WHERE package=''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let job: dispatch::Job = serde_json::from_slice(&body).unwrap();
        database.close().unwrap();
        assert_eq!(job.event.schedule, Some(schedule.id));
        assert!(job.event.scheduled_ms.unwrap() >= due);
        let mut request = Request::new(
            identity,
            Command::ExportBackup {
                expected_revision: saved.revision,
            },
        );
        request.id = job.request;
        let local = Client::new(node.local());
        let completed = tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                match local.outcome(&request).await.unwrap() {
                    RequestOutcome::Completed(output) => break *output,
                    RequestOutcome::Unknown => panic!("automatic backup outcome is unknown"),
                    _ => tokio::time::sleep(Duration::from_millis(250)).await,
                }
            }
        })
        .await
        .expect("automatic backup completion deadline");
        assert_eq!(completed, Ok(Output::Backup(record.clone())));
        drop(local);
        drop(client);
        node.shutdown().await.unwrap();

        let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let body: Vec<u8> = database
            .query_row(
                "SELECT body FROM dispatch_schedules WHERE package=''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let advanced: dispatch::Schedule = serde_json::from_slice(&body).unwrap();
        assert_eq!(advanced.id, original.id);
        assert_eq!(advanced.revision, original.revision);
        assert_eq!(advanced.timing, original.timing);
        assert!(advanced.next_ms.unwrap() > chrono::Utc::now().timestamp_millis());
        database.close().unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Backups(page) = execute(&client, Command::ListBackups).await else {
            panic!("backup page expected")
        };
        assert_eq!(page.records, vec![record]);
        drop(client);
        node.shutdown().await.unwrap();
        let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let count: i64 = database
            .query_row(
                "SELECT count(*) FROM dispatch_jobs WHERE package=''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        database.close().unwrap();
        controller.close().await.unwrap();
    }
}
