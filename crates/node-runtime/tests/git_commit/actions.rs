use super::*;
use std::process::Command as Process;

pub(super) fn git(root: &Path, args: &[&str]) {
    let output = Process::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Git fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
pub(super) async fn run(fixture: &Fixture, client: &Client, action: GitAction) -> Output {
    let Output::GitStatus(status) = client
        .execute(client.prepare(Command::InspectGit {
            worktree: fixture.worktree,
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    let request = client.prepare(Command::RunGitAction {
        worktree: fixture.worktree,
        action,
        expected_index: status.index_revision.unwrap(),
        expected_head: status.head,
        expected_branch: status.branch,
    });
    let result = client.execute(request.clone()).await.unwrap();
    assert_eq!(
        client.execute(request).await.unwrap(),
        result,
        "durable replay must not repeat an operation"
    );
    result
}

#[tokio::test]
async fn sync_local_and_remote() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let root = fixture.repository.workdir().unwrap();
    let origin = fixture.temp.path().join("origin.git");
    let backup = fixture.temp.path().join("backup.git");
    Repository::init_bare(&origin).unwrap();
    Repository::init_bare(&backup).unwrap();
    run(
        &fixture,
        &fixture.client,
        GitAction::AddRemote {
            name: "origin".into(),
            url: origin.to_str().unwrap().into(),
        },
    )
    .await;
    run(
        &fixture,
        &fixture.client,
        GitAction::AddRemote {
            name: "backup".into(),
            url: backup.to_str().unwrap().into(),
        },
    )
    .await;
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let invite = fixture.node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (position, client) in [&fixture.client, &remote].into_iter().enumerate() {
        run(
            &fixture,
            client,
            GitAction::Push {
                remote: Some("origin".into()),
                publish: true,
                force: false,
            },
        )
        .await;
        let Output::GitBranches(branches) = client
            .execute(client.prepare(Command::ListGitBranches {
                worktree: fixture.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(branches.sync.upstream.as_deref(), Some("origin/main"));
        assert_eq!(branches.sync.remotes.len(), 2);
        run(
            &fixture,
            client,
            GitAction::Push {
                remote: Some("backup".into()),
                publish: false,
                force: false,
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::Fetch {
                remote: None,
                prune: true,
                all: true,
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::Fetch {
                remote: Some("origin".into()),
                prune: true,
                all: false,
            },
        )
        .await;
        let peer = fixture.temp.path().join(format!("peer-{position}"));
        git(
            fixture.temp.path(),
            &[
                "clone",
                "--branch",
                "main",
                origin.to_str().unwrap(),
                peer.to_str().unwrap(),
            ],
        );
        git(&peer, &["config", "user.name", "Peer"]);
        git(&peer, &["config", "user.email", "peer@example.invalid"]);
        git(&peer, &["config", "commit.gpgsign", "false"]);
        std::fs::write(peer.join(format!("peer-{position}.txt")), "from origin\n").unwrap();
        git(&peer, &["add", "."]);
        git(&peer, &["commit", "-m", "Remote edit"]);
        git(&peer, &["push"]);
        run(
            &fixture,
            client,
            GitAction::Fetch {
                remote: Some("origin".into()),
                prune: true,
                all: false,
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::Pull {
                rebase: position == 1,
                remote: None,
                branch: None,
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(root.join(format!("peer-{position}.txt"))).unwrap(),
            "from origin\n"
        );
        std::fs::write(root.join("README.md"), format!("tracked {position}\n")).unwrap();
        std::fs::write(root.join("untracked.txt"), "must stay untracked\n").unwrap();
        let mut request = fixture.request(client, "Tracked with signoff", false).await;
        if let Command::CreateGitCommit { options, .. } = &mut request.command {
            options.tracked = true;
            options.signoff = true;
        }
        let Output::GitCommitCreated { id, .. } = client.execute(request).await.unwrap() else {
            panic!()
        };
        let commit = fixture
            .repository
            .find_commit(Oid::from_str(&id).unwrap())
            .unwrap();
        assert!(
            commit
                .message()
                .unwrap()
                .contains("Signed-off-by: Execution Node <node@example.invalid>")
        );
        assert!(commit.tree().unwrap().get_name("untracked.txt").is_none());
        run(
            &fixture,
            client,
            GitAction::Push {
                remote: None,
                publish: false,
                force: false,
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::Track {
                branch: "origin/main".into(),
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::SwitchRemote {
                branch: "origin/main".into(),
            },
        )
        .await;
        run(&fixture, client, GitAction::UndoCommit).await;
        let request = fixture.request(client, "Replacement", false).await;
        client.execute(request).await.unwrap();
        run(
            &fixture,
            client,
            GitAction::Push {
                remote: None,
                publish: false,
                force: true,
            },
        )
        .await;
        std::fs::write(root.join("README.md"), "discard me\n").unwrap();
        git(root, &["add", "README.md"]);
        run(
            &fixture,
            client,
            GitAction::Discard {
                paths: vec!["README.md".into()],
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            format!("tracked {position}\n")
        );
        let remote_branch = format!("remote-{position}");
        git(
            &peer,
            &[
                "push",
                "origin",
                &format!("HEAD:refs/heads/{remote_branch}"),
            ],
        );
        run(
            &fixture,
            client,
            GitAction::Fetch {
                remote: Some("origin".into()),
                prune: true,
                all: false,
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::SwitchRemote {
                branch: format!("origin/{remote_branch}"),
            },
        )
        .await;
        assert_eq!(
            fixture.repository.head().unwrap().shorthand().unwrap(),
            remote_branch
        );
        git(root, &["switch", "main"]);
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn respects_hooks_and_skip_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    let hook = fixture.repository.path().join("hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    let request = fixture
        .request(&fixture.client, "Hook rejects", false)
        .await;
    assert!(fixture.client.execute(request).await.is_err());
    assert!(fixture.repository.head().is_err());
    let mut request = fixture.request(&fixture.client, "Skip hook", false).await;
    if let Command::CreateGitCommit { options, .. } = &mut request.command {
        options.skip_hooks = true;
    }
    assert!(matches!(
        fixture.client.execute(request).await.unwrap(),
        Output::GitCommitCreated { .. }
    ));
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn branches_and_stashes_local_and_remote() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let root = fixture.repository.workdir().unwrap();
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let invite = fixture.node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (position, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let branch = format!("feature-{position}");
        git(root, &["switch", "-c", &branch]);
        let path = format!("feature-{position}.txt");
        std::fs::write(root.join(&path), "feature\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "Feature"]);
        let feature = fixture
            .repository
            .head()
            .unwrap()
            .target()
            .unwrap()
            .to_string();
        git(root, &["switch", "main"]);
        let path = format!("main-{position}.txt");
        std::fs::write(root.join(&path), "main\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "Main diverges"]);
        run(
            &fixture,
            client,
            GitAction::Rebase {
                commit: feature.clone(),
            },
        )
        .await;
        assert_eq!(
            fixture
                .repository
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .parent_id(0)
                .unwrap()
                .to_string(),
            feature
        );
        git(root, &["switch", &branch]);
        std::fs::write(root.join(format!("pick-{position}.txt")), "pick\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "Pick"]);
        let pick = fixture
            .repository
            .head()
            .unwrap()
            .target()
            .unwrap()
            .to_string();
        git(root, &["switch", "main"]);
        run(&fixture, client, GitAction::CherryPick { commit: pick }).await;
        assert!(root.join(format!("pick-{position}.txt")).exists());
        std::fs::write(root.join("README.md"), "stashed\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        run(
            &fixture,
            client,
            GitAction::Stash {
                mode: GitStashMode::All,
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "base\n"
        );
        assert!(
            !root.join("new.txt").exists(),
            "stash left file, position {position}, status: {:?}",
            Process::new("git")
                .current_dir(root)
                .args(["status", "--porcelain", "--ignored"])
                .output()
                .unwrap()
        );
        let Output::GitBranches(branches) = client
            .execute(client.prepare(Command::ListGitBranches {
                worktree: fixture.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(branches.sync.stashes.len(), 1);
        run(
            &fixture,
            client,
            GitAction::ApplyStash {
                commit: branches.sync.stashes[0].commit.clone(),
                pop: true,
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "stashed\n"
        );
        assert!(root.join("new.txt").exists());
        run(
            &fixture,
            client,
            GitAction::Discard {
                paths: vec!["README.md".into()],
            },
        )
        .await;
        std::fs::remove_file(root.join("new.txt")).unwrap();
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn sequencer_local_and_remote() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let root = fixture.repository.workdir().unwrap();
    git(root, &["switch", "-c", "conflict"]);
    std::fs::write(root.join("README.md"), "other\n").unwrap();
    git(root, &["commit", "-am", "Other"]);
    let target = fixture
        .repository
        .head()
        .unwrap()
        .target()
        .unwrap()
        .to_string();
    git(root, &["switch", "main"]);
    std::fs::write(root.join("README.md"), "ours\n").unwrap();
    git(root, &["commit", "-am", "Ours"]);
    let head = fixture
        .repository
        .head()
        .unwrap()
        .target()
        .unwrap()
        .to_string();
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let invite = fixture.node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&fixture.client, &remote] {
        for abort in [true, false] {
            git(root, &["reset", "--hard", &head]);
            let Output::GitStatus(status) = client
                .execute(client.prepare(Command::InspectGit {
                    worktree: fixture.worktree,
                }))
                .await
                .unwrap()
            else {
                panic!()
            };
            let request = client.prepare(Command::RunGitAction {
                worktree: fixture.worktree,
                action: GitAction::CherryPick {
                    commit: target.clone(),
                },
                expected_index: status.index_revision.unwrap(),
                expected_head: status.head,
                expected_branch: status.branch,
            });
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            let Output::GitBranches(branches) = client
                .execute(client.prepare(Command::ListGitBranches {
                    worktree: fixture.worktree,
                }))
                .await
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(branches.sync.operation.as_deref(), Some("cherry-pick"));
            if !abort {
                std::fs::write(root.join("README.md"), "resolved\n").unwrap();
                git(root, &["add", "README.md"]);
            }
            run(
                &fixture,
                client,
                if abort {
                    GitAction::Abort
                } else {
                    GitAction::Continue
                },
            )
            .await;
            assert_eq!(fixture.repository.state(), git2::RepositoryState::Clean);
            assert_eq!(
                std::fs::read_to_string(root.join("README.md")).unwrap(),
                if abort { "ours\n" } else { "resolved\n" }
            );
        }
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn undo_root_local_and_remote() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let invite = fixture.node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&fixture.client, &remote] {
        let request = fixture.request(client, "Root", false).await;
        client.execute(request).await.unwrap();
        run(&fixture, client, GitAction::UndoCommit).await;
        let Output::GitStatus(status) = client
            .execute(client.prepare(Command::InspectGit {
                worktree: fixture.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(status.kind, RepositoryKind::Unborn);
        assert!(
            status.entries.iter().any(
                |entry| entry.path == "README.md" && entry.staged == Some(GitChangeKind::Added)
            )
        );
        assert_eq!(
            std::fs::read_to_string(fixture.repository.workdir().unwrap().join("README.md"))
                .unwrap(),
            "base\n"
        );
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
