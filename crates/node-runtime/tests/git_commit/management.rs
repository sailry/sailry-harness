use super::actions::{git, run};
use super::*;

#[tokio::test]
async fn tracking_checkout_preserves_edits() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let root = fixture.repository.workdir().unwrap();
    git(root, &["switch", "-c", "source"]);
    fixture.stage("incoming\n");
    let incoming = fixture.commit("Incoming", false).await;
    fixture
        .repository
        .remote("origin", root.to_str().unwrap())
        .unwrap();
    fixture
        .repository
        .reference("refs/remotes/origin/incoming", incoming, false, "Fixture")
        .unwrap();
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let invite = fixture.node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (n, client) in [&fixture.client, &remote].into_iter().enumerate() {
        git(root, &["switch", "main"]);
        let draft = format!("uncommitted {n}\n");
        std::fs::write(root.join("README.md"), &draft).unwrap();
        let Output::GitStatus(status) = client
            .execute(client.prepare(Command::InspectGit {
                worktree: fixture.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        let local = format!("tracking-{n}");
        let error = client
            .execute(client.prepare(Command::RunGitAction {
                worktree: fixture.worktree,
                action: GitAction::SwitchTracking {
                    remote: "origin/incoming".into(),
                    local: local.clone(),
                    stash: false,
                },
                expected_index: status.index_revision.unwrap(),
                expected_head: status.head,
                expected_branch: status.branch,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            draft
        );
        run(
            &fixture,
            client,
            GitAction::SwitchTracking {
                remote: "origin/incoming".into(),
                local,
                stash: true,
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "incoming\n"
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
        let Output::GitOutput { text } = client
            .execute(client.prepare(Command::ReadGitStash {
                worktree: fixture.worktree,
                commit: branches.sync.stashes[0].commit.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(text.contains(draft.trim()));
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn management_local_and_remote() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    let base = fixture.commit("Base", false).await;
    let root = fixture.repository.workdir().unwrap();
    let origin = fixture.temp.path().join("origin.git");
    Repository::init_bare(&origin).unwrap();
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
        GitAction::Push {
            remote: Some("origin".into()),
            publish: true,
            force: false,
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
    for (n, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let tag = format!("v{n}.0");
        run(
            &fixture,
            client,
            GitAction::CreateTag {
                name: tag.clone(),
                commit: base.to_string(),
                message: Some("Release".into()),
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::PushTags {
                remote: "origin".into(),
            },
        )
        .await;
        let Output::GitRemoteTags(tags) = client
            .execute(client.prepare(Command::ListGitRemoteTags {
                worktree: fixture.worktree,
                remote: "origin".into(),
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        let found = tags.iter().find(|t| t.name == tag).unwrap();
        let oid = found.commit.clone();
        run(
            &fixture,
            client,
            GitAction::DeleteTag {
                name: tag.clone(),
                commit: oid.clone(),
                remote: Some("origin".into()),
            },
        )
        .await;
        assert!(
            Repository::open_bare(&origin)
                .unwrap()
                .find_reference(&format!("refs/tags/{tag}"))
                .is_err()
        );
        run(
            &fixture,
            client,
            GitAction::DeleteTag {
                name: tag.clone(),
                commit: oid,
                remote: None,
            },
        )
        .await;
        assert!(
            fixture
                .repository
                .find_reference(&format!("refs/tags/{tag}"))
                .is_err()
        );

        let branch = format!("feature-{n}");
        git(root, &["branch", &branch]);
        git(root, &["push", "origin", &branch]);
        let target = format!("origin/{branch}");
        run(
            &fixture,
            client,
            GitAction::Fetch {
                remote: None,
                prune: false,
                all: false,
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::SwitchTracking {
                remote: target.clone(),
                local: format!("tracking-{n}"),
                stash: false,
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
        assert_eq!(
            branches.sync.tracking.get(&format!("tracking-{n}")),
            Some(&target)
        );
        assert!(branches.sync.remote_urls["origin"].ends_with("origin.git"));
        git(root, &["switch", "main"]);
        run(
            &fixture,
            client,
            GitAction::DeleteRemoteBranch {
                branch: target,
                commit: base.to_string(),
            },
        )
        .await;
        assert!(
            Repository::open_bare(&origin)
                .unwrap()
                .find_branch(&branch, git2::BranchType::Local)
                .is_err()
        );
        run(
            &fixture,
            client,
            GitAction::Pull {
                rebase: false,
                remote: Some("origin".into()),
                branch: Some("main".into()),
            },
        )
        .await;
        run(&fixture, client, GitAction::Sync { rebase: true }).await;

        std::fs::write(root.join("README.md"), format!("stash-{n}\n")).unwrap();
        std::fs::write(root.join("untracked.txt"), "included\n").unwrap();
        run(
            &fixture,
            client,
            GitAction::Stash {
                mode: GitStashMode::All,
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
        let commits = branches
            .sync
            .stashes
            .iter()
            .map(|s| s.commit.clone())
            .collect::<Vec<_>>();
        let Output::GitOutput { text } = client
            .execute(client.prepare(Command::ReadGitStash {
                worktree: fixture.worktree,
                commit: commits[0].clone(),
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(text.contains("included"));
        run(&fixture, client, GitAction::DropAllStashes { commits }).await;
        run(
            &fixture,
            client,
            GitAction::AddRemote {
                name: format!("temporary-{n}"),
                url: origin.to_str().unwrap().into(),
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::RemoveRemote {
                name: format!("temporary-{n}"),
            },
        )
        .await;
        assert!(
            fixture
                .repository
                .find_remote(&format!("temporary-{n}"))
                .is_err()
        );
        std::fs::write(root.join("README.md"), "discard this edit\n").unwrap();
        std::fs::write(root.join("added.txt"), "discard this addition\n").unwrap();
        git(root, &["add", "added.txt"]);
        run(
            &fixture,
            client,
            GitAction::DiscardAll {
                paths: vec!["README.md".into(), "added.txt".into()],
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "base\n"
        );
        assert!(!root.join("added.txt").exists());
        assert!(
            fixture
                .repository
                .index()
                .unwrap()
                .get_path(Path::new("added.txt"), 0)
                .is_none()
        );
        std::fs::write(root.join("README.md"), "keep this edit\n").unwrap();
        run(
            &fixture,
            client,
            GitAction::SwitchStashing {
                branch: branch.clone(),
                commit: base.to_string(),
            },
        )
        .await;
        assert_eq!(
            fixture.repository.head().unwrap().shorthand().unwrap(),
            branch
        );
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "base\n"
        );
        git(root, &["switch", "main"]);
        let Output::GitBranches(stashed) = client
            .execute(client.prepare(Command::ListGitBranches {
                worktree: fixture.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(!stashed.sync.stashes.is_empty());
        run(
            &fixture,
            client,
            GitAction::DropAllStashes {
                commits: stashed
                    .sync
                    .stashes
                    .iter()
                    .map(|s| s.commit.clone())
                    .collect(),
            },
        )
        .await;
        run(
            &fixture,
            client,
            GitAction::Merge {
                commit: base.to_string(),
            },
        )
        .await;
        let Output::GitOutput { text } = client
            .execute(client.prepare(Command::ReadGitOutput {
                worktree: fixture.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(text.contains("git tag"));
        assert!(text.contains("[exit 0]"));
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn commit_follow_up_preserves_success() {
    let fixture = Fixture::new().await;
    let root = fixture.repository.workdir().unwrap();
    let origin = fixture.temp.path().join("origin.git");
    Repository::init_bare(&origin).unwrap();
    run(
        &fixture,
        &fixture.client,
        GitAction::AddRemote {
            name: "origin".into(),
            url: origin.to_str().unwrap().into(),
        },
    )
    .await;
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let address = controller
        .handle()
        .pair(fixture.node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (n, client) in [&fixture.client, &remote].into_iter().enumerate() {
        std::fs::write(root.join(format!("new-{n}.txt")), "new file\n").unwrap();
        let mut request = fixture.request(client, "Commit and publish", false).await;
        if let Command::CreateGitCommit { options, .. } = &mut request.command {
            options.all = true;
            options.after = GitAfterCommit::Push;
            options.push_remote = Some("origin".into());
        }
        let result = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        let Output::GitCommitCreated { id, follow_up } = result else {
            panic!()
        };
        assert!(follow_up.is_none());
        assert_eq!(
            Repository::open_bare(&origin)
                .unwrap()
                .find_reference("refs/heads/main")
                .unwrap()
                .target()
                .unwrap()
                .to_string(),
            id
        );
        fixture.stage(&format!("pending-{n}\n"));
        let mut request = fixture
            .request(client, "Commit with unavailable push", false)
            .await;
        if let Command::CreateGitCommit { options, .. } = &mut request.command {
            options.after = GitAfterCommit::Push;
            options.push_remote = Some("missing".into());
        }
        let result = client.execute(request.clone()).await.unwrap();
        let Output::GitCommitCreated { id, follow_up } = &result else {
            panic!()
        };
        assert!(follow_up.is_some());
        assert_eq!(
            fixture
                .repository
                .head()
                .unwrap()
                .target()
                .unwrap()
                .to_string(),
            *id
        );
        assert_eq!(client.execute(request).await.unwrap(), result);
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
