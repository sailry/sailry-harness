use super::*;

fn repository(root: &Path) -> git2::Repository {
    let repository = git2::Repository::init(root).unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Plugin fixture").unwrap();
    config
        .set_str("user.email", "plugin@example.invalid")
        .unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    fs::write(root.join("notes.txt"), "First revision\n").unwrap();
    repository
}

async fn status(client: &Client, context: &Context) -> GitStatus {
    let Output::GitStatus(status) = client
        .execute(
            client
                .prepare(Command::InspectGit {
                    worktree: context.worktree.unwrap(),
                })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("Git status expected")
    };
    status
}

fn stage(client: &Client, context: &Context, status: &GitStatus) -> Request {
    client
        .prepare(Command::UpdateGitIndex {
            worktree: context.worktree.unwrap(),
            operation: GitIndexChange::Stage,
            paths: vec!["notes.txt".into()],
            expected_index: status.index_revision.clone().unwrap(),
            expected_head: status.head.clone(),
        })
        .with_plugin(context.clone())
}

fn commit(client: &Client, context: &Context, status: &GitStatus) -> Request {
    client
        .prepare(Command::CreateGitCommit {
            worktree: context.worktree.unwrap(),
            message: "Plugin commit".into(),
            amend: false,
            options: Default::default(),
            expected_index: status.index_revision.clone().unwrap(),
            expected_head: status.head.clone(),
            expected_branch: status.branch.clone(),
        })
        .with_plugin(context.clone())
}

#[tokio::test]
#[cfg(unix)]
async fn shares_index_commit_and_recovery() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let repository = repository(&root);
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadGit, Action::WriteGit],
        )
        .await;
        let before = status(&client, &context).await;
        let staged = stage(&client, &context, &before);
        let admitted = client.dispatch(staged.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let result = admitted.completion.await.unwrap().unwrap();
        assert!(matches!(result, Output::GitIndex(_)));
        assert_eq!(client.execute(staged.clone()).await.unwrap(), result);
        // A new request with stale state must not re-read and overwrite the index.
        assert_eq!(
            client
                .execute(stage(&client, &context, &before))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let request = commit(&client, &context, &status(&client, &context).await);
        let result = client.execute(request.clone()).await.unwrap();
        let Output::GitCommitCreated { id, .. } = &result else {
            panic!("commit expected")
        };
        assert_eq!(
            repository.head().unwrap().target().unwrap().to_string(),
            *id
        );
        assert_eq!(
            repository
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .parent_count(),
            0
        );
        let tree = repository.head().unwrap().peel_to_tree().unwrap();
        let entry = tree.get_path(Path::new("notes.txt")).unwrap();
        assert_eq!(
            repository.find_blob(entry.id()).unwrap().content(),
            b"First revision\n"
        );
        drop(entry);
        drop(tree);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: context.package.name.clone(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        assert_eq!(
            client
                .execute(stage(&client, &context, &before))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(
            repository.head().unwrap().target().unwrap().to_string(),
            *id
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn requires_write_grant_and_captured_worktree() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let repository = repository(&root);
        let read = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadGit],
        )
        .await;
        let before = status(&client, &read).await;
        for request in [
            stage(&client, &read, &before),
            commit(&client, &read, &before),
        ] {
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            1,
            &[Action::ReadGit, Action::WriteGit],
        )
        .await;
        assert_eq!(
            client
                .execute(stage(&client, &read, &before))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        for captured in [None, Some(WorktreeId::new())] {
            let mut outside = context.clone();
            outside.worktree = captured;
            for mut request in [
                stage(&client, &context, &before),
                commit(&client, &context, &before),
            ] {
                request.plugin = Some(outside.clone());
                assert_eq!(
                    client.execute(request).await.unwrap_err().code,
                    ErrorCode::PermissionDenied
                );
            }
        }
        assert_eq!(repository.index().unwrap().len(), 0);
        assert!(repository.head().is_err());
        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).unwrap(),
            "First revision\n"
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn manages_branches_with_captured_revisions() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let repository = repository(&root);
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadGit, Action::WriteGit],
        )
        .await;
        client
            .execute(stage(&client, &context, &status(&client, &context).await))
            .await
            .unwrap();
        client
            .execute(commit(&client, &context, &status(&client, &context).await))
            .await
            .unwrap();
        let before = status(&client, &context).await;
        let head = before.head.clone().unwrap();
        let create = client
            .prepare(Command::CreateGitBranch {
                worktree,
                name: "topic".into(),
                commit: head.clone(),
            })
            .with_plugin(context.clone());
        let result = client.execute(create.clone()).await.unwrap();
        assert_eq!(client.execute(create).await.unwrap(), result);
        client
            .execute(
                client
                    .prepare(Command::RenameGitBranch {
                        worktree,
                        name: "topic".into(),
                        new_name: "renamed".into(),
                        commit: head.clone(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
        for command in [
            Command::ListGitBranches { worktree },
            Command::ResolveGitRevision {
                worktree,
                revision: "renamed".into(),
            },
            Command::ReadGitLog {
                worktree,
                limit: 100,
                cursor: None,
            },
            Command::ReadGitCommit {
                worktree,
                commit: head.clone(),
            },
            Command::ReadGitOutput { worktree },
            Command::ReadGitDiff {
                worktree,
                path: "notes.txt".into(),
                scope: GitDiffScope::All,
            },
        ] {
            let expected = execute(&client, command.clone()).await;
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap(),
                expected
            );
        }
        client
            .execute(
                client
                    .prepare(Command::SwitchGitBranch {
                        worktree,
                        name: "renamed".into(),
                        commit: head.clone(),
                        expected_head: before.head.clone(),
                        expected_branch: before.branch.clone(),
                        expected_index: before.index_revision.clone().unwrap(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
        assert_eq!(repository.head().unwrap().shorthand().unwrap(), "renamed");
        let stale = client
            .prepare(Command::DeleteGitBranch {
                worktree,
                name: before.branch.unwrap(),
                commit: head.clone(),
                expected_head: head,
                expected_branch: Some("stale".into()),
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let outside = WorktreeId::new();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ListGitBranches { worktree: outside })
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
