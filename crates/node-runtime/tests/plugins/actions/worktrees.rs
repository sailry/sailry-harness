use super::*;

fn initial(root: &Path) -> String {
    let repository = git2::Repository::init(root).unwrap();
    fs::write(root.join("source.txt"), "captured checkout\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("source.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn confines_management_and_retains_admitted_results() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let head = initial(&root);
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[
                Action::ReadGit,
                Action::ReadWorktrees,
                Action::WriteWorktrees,
            ],
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == worktree)
            .unwrap()
            .project
            .unwrap();
        let Output::GitStatus(status) = execute(&client, Command::InspectGit { worktree }).await
        else {
            panic!("status expected")
        };
        let create = client
            .prepare(Command::CreateManagedWorktree {
                project,
                source: worktree,
                branch: "plugin-topic".into(),
                expected_head: head.clone(),
                expected_index: status.index_revision.unwrap(),
                include_changes: false,
            })
            .with_plugin(context.clone());
        let admitted = client.dispatch(create.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let result = admitted.completion.await.unwrap().unwrap();
        let Output::Worktree(created) = &result else {
            panic!("worktree expected")
        };
        assert_eq!(created.project, Some(project));
        assert!(!created.main);
        assert_eq!(
            fs::read_to_string(Path::new(&created.path).join("source.txt")).unwrap(),
            "captured checkout\n"
        );
        assert_eq!(client.execute(create.clone()).await.unwrap(), result);
        let Output::ProjectCatalog(catalog) = client
            .execute(
                client
                    .prepare(Command::ReadWorktreeCatalog { worktree })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("captured catalog expected")
        };
        assert_eq!(catalog.projects.len(), 1);
        assert_eq!(catalog.projects[0].id, project);
        assert!(catalog.worktrees.iter().any(|tree| tree.id == created.id));
        assert!(
            catalog
                .worktrees
                .iter()
                .all(|tree| tree.project == Some(project))
        );
        let list = client
            .prepare(Command::ListWorktrees { worktree })
            .with_plugin(context.clone());
        let Output::GitWorktrees(trees) = client.execute(list).await.unwrap() else {
            panic!("worktree list expected")
        };
        assert!(trees.entries.iter().any(|entry| entry.path == created.path));
        // A workspace read cannot retarget to another checkout, even in its project.
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::InspectGit {
                            worktree: created.id
                        })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for command in [
            Command::ListWorktrees {
                worktree: created.id,
            },
            Command::RegisterWorktree {
                project: ProjectId::new(),
                path: created.path.clone(),
            },
            Command::RemoveWorktree {
                worktree: WorktreeId::new(),
                expected_head: head.clone(),
                expected_branch: "plugin-topic".into(),
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let remove = client
            .prepare(Command::RemoveWorktree {
                worktree: created.id,
                expected_head: head,
                expected_branch: "plugin-topic".into(),
            })
            .with_plugin(context.clone());
        let removed = client.execute(remove.clone()).await.unwrap();
        assert!(!Path::new(&created.path).exists());
        execute(
            &client,
            Command::SetPluginEnabled {
                name: context.package.name.clone(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(client.execute(create).await.unwrap(), result);
        assert_eq!(client.execute(remove).await.unwrap(), removed);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ListWorktrees { worktree })
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
