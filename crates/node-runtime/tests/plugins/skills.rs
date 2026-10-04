use super::*;
#[path = "../support/skills.rs"]
mod github;
use std::sync::atomic::Ordering;
#[path = "skills/git.rs"]
mod git;

async fn discover(client: &Client, source: plugin::skills::Source) -> plugin::skills::Discovery {
    let pending = client
        .dispatch(client.prepare(Command::DiscoverSkills { source }))
        .await
        .unwrap();
    assert!(!pending.receipt.durable);
    let Output::SkillDiscovery(discovery) = pending.completion.await.unwrap().unwrap() else {
        panic!("skill discovery expected")
    };
    discovery
}

#[tokio::test]
async fn manages_pinned_versions() {
    for remote in [false, true] {
        let github = github::Github::start().await;
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_with_skill_source(directory.path().join("node"), &github.endpoint)
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
        let packages = || {
            fs::read_dir(node.profile().join("plugins/packages"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let stored = packages();
        let discovery = discover(&client, github::source()).await;
        assert_eq!(
            discovery.source.repository,
            "https://github.com/owner/repo.git"
        );
        assert_eq!(discovery.source.commit, github::FIRST);
        assert_eq!(discovery.skills.len(), 3);
        let analysis = discovery
            .skills
            .iter()
            .find(|candidate| candidate.skill.name == "analysis")
            .unwrap();
        let writing = discovery
            .skills
            .iter()
            .find(|candidate| candidate.skill.name == "writing")
            .unwrap();
        assert_ne!(analysis.name, writing.name);
        assert!(inventory(&client).await.is_empty());
        assert_eq!(packages(), stored);
        github.updated.store(true, Ordering::SeqCst);
        let request = client.prepare(github::install(&discovery, analysis, 0));
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let first = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break info(result.unwrap());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            first.skills.as_slice(),
            std::slice::from_ref(&analysis.skill)
        );
        assert_eq!(
            first.origin,
            Some(plugin::Origin::Online {
                source: discovery.source.clone(),
                path: analysis.path.clone(),
            })
        );
        assert_eq!(first.skill.as_ref().unwrap().source, discovery.source);
        assert_eq!(first.skill.as_ref().unwrap().path, analysis.path);
        assert_eq!(first.summary.version.as_deref(), Some(github::FIRST));
        let package = node
            .profile()
            .join("plugins/packages")
            .join(&first.summary.digest);
        for (path, expected) in [
            ("skills/analysis/references/guide.md", "Guide first 中文 🙂"),
            ("skills/analysis/assets/example.txt", "Asset content"),
            ("SOURCE-LICENSE", "Fixture license notice"),
        ] {
            assert_eq!(fs::read_to_string(package.join(path)).unwrap(), expected);
        }
        assert_eq!(
            fs::read_to_string(package.join("skills/analysis/SKILL.md")).unwrap(),
            github::body("first")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                fs::metadata(package.join("skills/analysis/scripts/check.sh"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0
            );
        }
        let fetches = github.requests.lock().unwrap().len();
        assert_eq!(info(client.execute(request.clone()).await.unwrap()), first);
        assert_eq!(github.requests.lock().unwrap().len(), fetches);
        let other = info(execute(&client, github::install(&discovery, writing, 0)).await);
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: analysis.name.clone(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await,
        );
        assert!(!disabled.summary.enabled);
        assert!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: other.summary.name.clone()
                    }
                )
                .await
            )
            .summary
            .enabled
        );
        let next = discover(
            &client,
            plugin::skills::Source {
                repository: "https://github.com/owner/repo/tree/release/v1/skills/analysis".into(),
                git_ref: Some("release/v1".into()),
                path: None,
            },
        )
        .await;
        assert_eq!(next.source.commit, github::SECOND);
        assert!(
            github.requests.lock().unwrap().iter().any(|request| {
                request.starts_with("GET /repos/owner/repo/commits/release%2Fv1 ")
            })
        );
        assert_eq!(next.skills.len(), 1);
        assert_eq!(next.skills[0].name, analysis.name);
        let updated = info(execute(&client, github::install(&next, &next.skills[0], 2)).await);
        assert_eq!(
            updated.origin,
            Some(plugin::Origin::Online {
                source: next.source.clone(),
                path: next.skills[0].path.clone(),
            })
        );
        assert_ne!(updated.summary.digest, first.summary.digest);
        assert!(!updated.summary.enabled);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPluginVersion {
                        package: updated.summary.reference()
                    }
                )
                .await
            )
            .skill,
            updated.skill
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: writing.name.clone(),
                expected_revision: 1,
            },
        )
        .await;
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
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: analysis.name.clone()
                    }
                )
                .await
            ),
            updated
        );
        assert_eq!(info(client.execute(request).await.unwrap()), first);
        let enabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: analysis.name.clone(),
                    expected_revision: 3,
                    enabled: true,
                },
            )
            .await,
        );
        assert!(enabled.summary.enabled);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPluginVersion {
                        package: updated.summary.reference()
                    }
                )
                .await
            )
            .skill,
            updated.skill
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: analysis.name.clone(),
                expected_revision: 4,
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
async fn rejects_invalid_sources() {
    let github = github::Github::start().await;
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start_with_skill_source(directory.path().join("node"), &github.endpoint)
        .await
        .unwrap();
    let client = Client::new(node.local());
    let discovery = discover(&client, github::source()).await;
    let original = &discovery.skills[0];
    let fetches = github.requests.lock().unwrap().len();
    let mut invalid_commit = discovery.source.clone();
    invalid_commit.commit = "main".into();
    for command in [
        Command::InstallSkill {
            source: invalid_commit,
            path: original.path.clone(),
            name: original.name.clone(),
            expected_revision: 0,
        },
        Command::InstallSkill {
            source: discovery.source.clone(),
            path: "../escape".into(),
            name: original.name.clone(),
            expected_revision: 0,
        },
        Command::InstallSkill {
            source: discovery.source.clone(),
            path: original.path.clone(),
            name: "skill-forged".into(),
            expected_revision: 0,
        },
        Command::DiscoverSkills {
            source: plugin::skills::Source {
                repository: "https://github.com/owner/repo/tree/main/%2e%2e/private".into(),
                git_ref: None,
                path: None,
            },
        },
        Command::DiscoverSkills {
            source: plugin::skills::Source {
                repository: "owner/repo".into(),
                git_ref: Some("main~1".into()),
                path: None,
            },
        },
    ] {
        assert_eq!(
            client
                .execute(client.prepare(command))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(github.requests.lock().unwrap().len(), fetches);
    let unsafe_skill = discovery
        .skills
        .iter()
        .find(|candidate| candidate.skill.name == "unsafe")
        .unwrap();
    let failure = client
        .execute(client.prepare(github::install(&discovery, unsafe_skill, 0)))
        .await
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::InvalidRequest);
    assert!(failure.message.contains("symbolic links"));
    assert!(inventory(&client).await.is_empty());
    node.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "requires public GitHub access and SAILRY_SKILLS_SMOKE_REPOSITORY"]
async fn public_github() {
    let repository = std::env::var("SAILRY_SKILLS_SMOKE_REPOSITORY")
        .expect("set a small public skill repository or tree URL");
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let discovery = discover(
        &remote,
        plugin::skills::Source {
            repository,
            git_ref: std::env::var("SAILRY_SKILLS_SMOKE_REF").ok(),
            path: std::env::var("SAILRY_SKILLS_SMOKE_PATH").ok(),
        },
    )
    .await;
    let candidate = discovery
        .skills
        .first()
        .expect("repository contains a standard skill");
    let installed = info(execute(&remote, github::install(&discovery, candidate, 0)).await);
    assert_eq!(
        installed.skill.as_ref().unwrap().source.commit,
        discovery.source.commit
    );
    let local = Client::new(node.local());
    assert_eq!(
        info(
            execute(
                &local,
                Command::ReadPlugin {
                    name: candidate.name.clone()
                }
            )
            .await
        ),
        installed
    );
    eprintln!(
        "installed {} at {} ({})",
        candidate.path, discovery.source.commit, installed.summary.digest
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
