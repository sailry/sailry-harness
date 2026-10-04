use super::*;

#[tokio::test]
async fn scopes_monorepo_exports() {
    for remote in [false, true] {
        let git = source::Git::new();
        add_unrelated_files(&git);
        let (_directory, node, controller, client, _worktree) = fixture(remote).await;
        let Output::PluginSource(selected) = execute(
            &client,
            Command::InspectPluginSource {
                source: plugin::skills::Source {
                    repository: git.url.clone(),
                    git_ref: Some("release/v1".into()),
                    path: Some("plugin".into()),
                },
            },
        )
        .await
        else {
            panic!("plugin source expected")
        };
        assert_eq!(selected.info.summary.name, "generic-plugin");
        assert!(selected.info.issues.is_empty());
        let discovery = discover(
            &client,
            plugin::skills::Source {
                repository: git.url.clone(),
                git_ref: Some("release/v1".into()),
                path: Some("skills/analysis".into()),
            },
        )
        .await;
        assert_eq!(discovery.skills.len(), 1);
        let installed = info(
            execute(
                &client,
                source::install(&discovery, &discovery.skills[0], 0),
            )
            .await,
        );
        let root = node
            .profile()
            .join("plugins/packages")
            .join(installed.summary.digest);
        assert_eq!(
            fs::read_to_string(root.join("SOURCE-LICENSE")).unwrap(),
            "Fixture license notice"
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

fn add_unrelated_files(git: &source::Git) {
    let path = url::Url::parse(&git.url).unwrap().to_file_path().unwrap();
    let repository = git2::Repository::open_bare(path).unwrap();
    let parent = repository.head().unwrap().peel_to_commit().unwrap();
    let blob = repository.blob(b"Unrelated content").unwrap();
    let mut unrelated = repository.treebuilder(None).unwrap();
    for index in 0..10_001 {
        unrelated
            .insert(format!("file-{index}"), blob, 0o100644)
            .unwrap();
    }
    let mut tree = repository
        .treebuilder(Some(&parent.tree().unwrap()))
        .unwrap();
    tree.insert("unrelated", unrelated.write().unwrap(), 0o040000)
        .unwrap();
    let tree = repository.find_tree(tree.write().unwrap()).unwrap();
    let signature = git2::Signature::now("Skill fixture", "skill@example.invalid").unwrap();
    repository
        .commit(
            Some("refs/heads/release/v1"),
            &signature,
            &signature,
            "Add unrelated files",
            &tree,
            &[&parent],
        )
        .unwrap();
}

#[tokio::test]
async fn manages_generic_git_commits() {
    for remote in [false, true] {
        let git = source::Git::new();
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let original = discover(&client, git.source()).await;
        assert_eq!(original.source.repository, git.url);
        assert_eq!(original.source.git_ref, "release/v1");
        assert_eq!(original.source.commit, git.first);
        assert_eq!(original.skills.len(), 3);
        let selected = original
            .skills
            .iter()
            .find(|skill| skill.skill.name == "analysis")
            .unwrap();
        assert!(inventory(&client).await.is_empty());
        let second = git.update();
        let request = client.prepare(source::install(&original, selected, 0));
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let installed = info(admitted.completion.await.unwrap().unwrap());
        assert_eq!(installed.skill.as_ref().unwrap().source, original.source);
        assert_eq!(
            installed.summary.version.as_deref(),
            Some(git.first.as_str())
        );
        assert_eq!(
            info(client.execute(request.clone()).await.unwrap()),
            installed
        );
        let root = node
            .profile()
            .join("plugins/packages")
            .join(&installed.summary.digest);
        assert_eq!(
            fs::read_to_string(root.join("skills/analysis/SKILL.md")).unwrap(),
            source::body("first")
        );
        assert_eq!(
            fs::read_to_string(root.join("SOURCE-LICENSE")).unwrap(),
            "Fixture license notice"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                fs::metadata(root.join("skills/analysis/scripts/check.sh"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0
            );
        }
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: selected.name.clone(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await,
        );
        assert!(!disabled.summary.enabled);
        let next = discover(&client, git.source()).await;
        assert_eq!(next.source.commit, second);
        let candidate = next
            .skills
            .iter()
            .find(|skill| skill.name == selected.name)
            .unwrap();
        let conflict = client
            .execute(client.prepare(source::install(&next, candidate, 1)))
            .await
            .unwrap_err();
        assert_eq!(conflict.code, ErrorCode::RevisionConflict);
        let updated = info(execute(&client, source::install(&next, candidate, 2)).await);
        assert!(!updated.summary.enabled);
        assert_ne!(updated.summary.digest, installed.summary.digest);
        assert_eq!(updated.skill.as_ref().unwrap().source.commit, second);
        let unsafe_skill = next
            .skills
            .iter()
            .find(|skill| skill.skill.name == "unsafe")
            .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(source::install(&next, unsafe_skill, 0)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            inventory(&client).await.as_slice(),
            std::slice::from_ref(&updated.summary)
        );
        assert_eq!(info(client.execute(request).await.unwrap()), installed);
        execute(
            &client,
            Command::RemovePlugin {
                name: selected.name.clone(),
                expected_revision: 3,
            },
        )
        .await;
        assert!(inventory(&client).await.is_empty());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn manages_generic_plugin_commits() {
    for remote in [false, true] {
        let git = source::Git::new();
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let inspect = || Command::InspectPluginSource {
            source: plugin::skills::Source {
                repository: git.url.clone(),
                git_ref: Some("release/v1".into()),
                path: Some("plugin".into()),
            },
        };
        let admitted = client.dispatch(client.prepare(inspect())).await.unwrap();
        assert!(!admitted.receipt.durable);
        let Output::PluginSource(original) = admitted.completion.await.unwrap().unwrap() else {
            panic!("plugin source expected");
        };
        assert_eq!(original.source.commit, git.first);
        assert_eq!(original.path, "plugin");
        assert_eq!(original.info.summary.name, "generic-plugin");
        git.update();
        let command =
            |source: &plugin::catalog::Selection, revision| Command::InstallPluginSource {
                source: source.source.clone(),
                path: source.path.clone(),
                name: source.info.summary.name.clone(),
                expected_revision: revision,
            };
        let request = client.prepare(command(&original, 0));
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let installed = info(admitted.completion.await.unwrap().unwrap());
        assert_eq!(
            installed.origin,
            Some(plugin::Origin::Online {
                source: original.source.clone(),
                path: "plugin".into()
            })
        );
        assert!(installed.skill.is_none());
        let root = node
            .profile()
            .join("plugins/packages")
            .join(&installed.summary.digest);
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "Plugin first\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("LICENSE")).unwrap(),
            "Plugin fixture license"
        );
        assert_eq!(
            info(client.execute(request.clone()).await.unwrap()),
            installed
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: installed.summary.name.clone(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        let Output::PluginSource(next) = execute(&client, inspect()).await else {
            panic!("plugin source expected");
        };
        assert_ne!(next.source.commit, original.source.commit);
        assert_eq!(
            client
                .execute(client.prepare(command(&next, 1)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let updated = info(execute(&client, command(&next, 2)).await);
        assert_eq!(updated.summary.revision, 3);
        assert!(!updated.summary.enabled);
        assert_ne!(updated.summary.digest, installed.summary.digest);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            inventory(&client).await.as_slice(),
            std::slice::from_ref(&updated.summary)
        );
        assert_eq!(info(client.execute(request).await.unwrap()), installed);
        execute(
            &client,
            Command::RemovePlugin {
                name: updated.summary.name,
                expected_revision: 3,
            },
        )
        .await;
        assert!(inventory(&client).await.is_empty());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
