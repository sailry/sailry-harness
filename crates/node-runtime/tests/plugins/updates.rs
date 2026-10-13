use super::*;

async fn check(client: &Client, package: &Summary) -> plugin::updates::Report {
    let request = client.prepare(Command::CheckPluginUpdate {
        force: true,
        name: package.name.clone(),
        expected_revision: package.revision,
    });
    assert!(!request.command.durable());
    let admission = client.dispatch(request).await.unwrap();
    assert!(!admission.receipt.durable);
    let Output::PluginUpdate(report) = admission.completion.await.unwrap().unwrap() else {
        panic!("plugin update report expected");
    };
    assert_eq!(report.current, *package);
    report
}

fn packages(node: &Node) -> std::collections::BTreeSet<std::ffi::OsString> {
    fs::read_dir(node.profile().join("plugins/packages"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect()
}

#[tokio::test]
async fn checks_reject_stale_revisions_without_installing() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let installed = info(execute(&client, install(worktree, 0)).await);
        let stored = packages(&node);
        let report = check(&client, &installed.summary).await;
        assert!(!report.manual && report.available.is_none());
        fs::write(
            directory
                .path()
                .join("source/package/skills/analysis/references/guide.md"),
            "New content, same version",
        )
        .unwrap();
        let report = check(&client, &installed.summary).await;
        assert!(!report.manual && report.available.is_none());
        package(&directory.path().join("source/package"), "1.0.1");
        let report = check(&client, &installed.summary).await;
        let plugin::updates::Candidate::Package(candidate) = report.available.unwrap() else {
            panic!("inspected package expected");
        };
        assert_eq!(candidate.summary.version.as_deref(), Some("1.0.1"));
        assert_ne!(candidate.summary.digest, installed.summary.digest);
        assert_eq!(candidate.origin, installed.origin);
        assert_eq!(packages(&node), stored);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            ),
            installed
        );
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await,
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::CheckPluginUpdate {
                    name: "example".into(),
                    expected_revision: 1,
                    force: true,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let report = check(&client, &disabled.summary).await;
        assert!(report.available.is_some());
        let updated = info(execute(&client, install(worktree, 2)).await);
        assert_eq!(updated.summary.digest, candidate.summary.digest);
        assert!(!updated.summary.enabled);
        assert!(check(&client, &updated.summary).await.available.is_none());
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: 3,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(client.prepare(Command::CheckPluginUpdate {
                    name: "example".into(),
                    expected_revision: 3,
                    force: true,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_changed_worktree_content() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let installed = info(execute(&client, install(worktree, 0)).await);
        let path = directory
            .path()
            .join("source/package/skills/analysis/references/guide.md");
        package(&directory.path().join("source/package"), "1.0.1");
        fs::write(&path, "Reviewed update").unwrap();
        let Some(plugin::updates::Candidate::Package(candidate)) =
            check(&client, &installed.summary).await.available
        else {
            panic!("worktree update expected");
        };
        let request = client.prepare(Command::InstallPlugin {
            worktree,
            path: "package".into(),
            name: installed.summary.name.clone(),
            expected_revision: installed.summary.revision,
            expected_digest: Some(candidate.summary.digest),
        });
        fs::write(&path, "Changed after review").unwrap();
        let error = client.execute(request.clone()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert_eq!(client.execute(request).await.unwrap_err(), error);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: installed.summary.name.clone(),
                    },
                )
                .await,
            ),
            installed
        );

        let Some(plugin::updates::Candidate::Package(candidate)) =
            check(&client, &installed.summary).await.available
        else {
            panic!("current worktree update expected");
        };
        let updated = info(
            execute(
                &client,
                Command::InstallPlugin {
                    worktree,
                    path: "package".into(),
                    name: installed.summary.name.clone(),
                    expected_revision: installed.summary.revision,
                    expected_digest: Some(candidate.summary.digest.clone()),
                },
            )
            .await,
        );
        assert_eq!(updated.summary.digest, candidate.summary.digest);
        assert_eq!(updated.summary.revision, installed.summary.revision + 1);
        assert_eq!(updated.summary.enabled, installed.summary.enabled);
        assert_eq!(
            updated.summary.settings_revision,
            installed.summary.settings_revision
        );
        assert_eq!(
            fs::read_to_string(
                node.profile()
                    .join("plugins/packages")
                    .join(&updated.summary.digest)
                    .join("skills/analysis/references/guide.md"),
            )
            .unwrap(),
            "Changed after review"
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn unavailable_official_source() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        let stored = packages(&node);
        for name in ["files", "progress", "ssh", "databases"] {
            let current = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
            let error = client
                .execute(client.prepare(Command::CheckPluginUpdate {
                    force: true,
                    name: name.into(),
                    expected_revision: current.summary.revision,
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert_eq!(
                info(execute(&client, Command::ReadPlugin { name: name.into() }).await),
                current
            );
        }
        assert_eq!(packages(&node), stored);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn repository_checks_pin_without_installing() {
    for remote in [false, true] {
        let git = source::Git::new();
        let (_directory, node, controller, client, _) = fixture(remote).await;
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
            panic!("plugin source expected");
        };
        assert_eq!(selected.source.commit, git.first);
        let installed = info(
            execute(
                &client,
                Command::InstallPluginSource {
                    source: selected.source.clone(),
                    path: selected.path.clone(),
                    name: selected.info.summary.name.clone(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        let Output::SkillDiscovery(discovery) = execute(
            &client,
            Command::DiscoverSkills {
                source: git.source(),
            },
        )
        .await
        else {
            panic!("skill source expected");
        };
        let skill = discovery
            .skills
            .iter()
            .find(|skill| skill.skill.name == "analysis")
            .unwrap();
        let installed_skill = info(execute(&client, source::install(&discovery, skill, 0)).await);
        assert!(check(&client, &installed.summary).await.available.is_none());
        assert!(
            check(&client, &installed_skill.summary)
                .await
                .available
                .is_none()
        );
        let stored = packages(&node);
        let commit = git.update();
        let candidates = [
            check(&client, &installed.summary).await.available.unwrap(),
            check(&client, &installed_skill.summary)
                .await
                .available
                .unwrap(),
        ];
        assert_eq!(packages(&node), stored);
        for (current, candidate) in [&installed, &installed_skill].into_iter().zip(candidates) {
            let plugin::updates::Candidate::Package(candidate) = candidate else {
                panic!("inspected package expected");
            };
            assert_ne!(candidate.summary.digest, current.summary.digest);
            let Some(plugin::Origin::Online { source, path }) = candidate.origin else {
                panic!("online source expected");
            };
            assert_eq!(source.commit, commit);
            assert_eq!(source.git_ref, "release/v1");
            let command = if candidate.skill.is_some() {
                Command::InstallSkill {
                    source,
                    path,
                    name: current.summary.name.clone(),
                    expected_revision: 1,
                }
            } else {
                Command::InstallPluginSource {
                    source,
                    path,
                    name: current.summary.name.clone(),
                    expected_revision: 1,
                }
            };
            let updated = info(execute(&client, command).await);
            assert_eq!(updated.summary.digest, candidate.summary.digest);
            assert!(check(&client, &updated.summary).await.available.is_none());
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
