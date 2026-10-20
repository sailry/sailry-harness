use super::*;
#[path = "../support/skillhub.rs"]
pub(super) mod fixture;

fn source() -> plugin::skills::RegistrySource {
    plugin::skills::RegistrySource {
        namespace: None,
        slug: "analysis".into(),
        version: None,
    }
}

async fn discover(client: &Client) -> plugin::skills::RegistryDiscovery {
    let pending = client
        .dispatch(client.prepare(Command::DiscoverRegistrySkill { source: source() }))
        .await
        .unwrap();
    assert!(!pending.receipt.durable);
    let Output::RegistrySkillDiscovery(discovery) = pending.completion.await.unwrap().unwrap()
    else {
        panic!("registry discovery expected");
    };
    discovery
}

async fn inventory(client: &Client) -> Vec<Summary> {
    let Output::Plugins(plugins) = execute(client, Command::ListPlugins).await else {
        panic!("plugin list expected");
    };
    plugins
}

#[tokio::test]
async fn installs_and_updates_verified_packages() {
    for remote in [false, true] {
        let registry = fixture::Registry::start().await;
        registry.state.lock().unwrap().legacy = true;
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_skill_registry_fixture(&registry.endpoint).unwrap();
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
        let discovery = discover(&client).await;
        assert_eq!(discovery.source.namespace, "example");
        assert_eq!(discovery.source.version, "1.0.0");
        assert_eq!(discovery.source.archive_sha256.len(), 64);
        assert!(inventory(&client).await.is_empty());
        let candidate = &discovery.skills[0];
        assert_eq!(candidate.skill.name, "analysis");
        assert_eq!(
            candidate.skill.display_name.as_deref(),
            Some("Analysis / Review")
        );
        let Output::SkillDocument(document) = execute(
            &client,
            Command::ReadSkillDocument {
                source: plugin::skills::DocumentSource::Registry(discovery.source.clone()),
            },
        )
        .await
        else {
            panic!("skill document expected");
        };
        assert_eq!(document.skill, candidate.skill);
        assert_eq!(document.content, "Revision 1.0.0\n");
        let command = Command::InstallRegistrySkill {
            source: discovery.source.clone(),
            name: candidate.name.clone(),
            expected_revision: 0,
        };
        let request = client.prepare(command);
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        let installed = info(pending.completion.await.unwrap().unwrap());
        assert_eq!(
            installed.skill,
            Some(plugin::skills::Provenance::Registry {
                registry: discovery.source.clone()
            })
        );
        assert_eq!(
            installed.origin,
            Some(plugin::Origin::Registry {
                source: discovery.source.clone()
            })
        );
        assert_eq!(installed.summary.version.as_deref(), Some("1.0.0"));
        let root = node
            .profile()
            .join("plugins/packages")
            .join(&installed.summary.digest);
        assert_eq!(
            fs::read_to_string(root.join("skills/analysis/references/guide.md")).unwrap(),
            "Fixture content"
        );
        assert_eq!(
            fs::read_to_string(root.join("SOURCE-SKILL.md")).unwrap(),
            fixture::document("1.0.0", true)
        );
        let projected = fs::read_to_string(root.join("skills/analysis/SKILL.md")).unwrap();
        assert!(projected.contains("\"name\":\"analysis\""));
        assert!(projected.contains("clawdbot"));
        assert_eq!(installed.skills, vec![candidate.skill.clone()]);
        assert_eq!(
            fs::read_to_string(root.join("SOURCE-LICENSE")).unwrap(),
            "Fixture source license"
        );
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Plugin(installed.clone())
        );
        let check = |revision| Command::CheckPluginUpdate {
            name: candidate.name.clone(),
            expected_revision: revision,
            force: false,
        };
        let Output::PluginUpdate(report) = execute(&client, check(1)).await else {
            panic!("update report expected");
        };
        assert!(report.available.is_none());
        registry.state.lock().unwrap().newer = true;
        let Output::PluginUpdate(report) = execute(&client, check(1)).await else {
            panic!("update report expected");
        };
        let Some(plugin::updates::Candidate::Package(available)) = report.available else {
            panic!("registry update expected");
        };
        let Some(plugin::Origin::Registry { source: resolved }) = available.origin else {
            panic!("registry origin expected");
        };
        assert_eq!(resolved.version, "1.1.0");
        let updated = info(
            execute(
                &client,
                Command::InstallRegistrySkill {
                    source: resolved,
                    name: candidate.name.clone(),
                    expected_revision: 1,
                },
            )
            .await,
        );
        assert_eq!(updated.summary.version.as_deref(), Some("1.1.0"));
        assert_eq!(updated.summary.revision, 2);
        registry.state.lock().unwrap().corrupt_archive = true;
        let failed = client
            .execute(client.prepare(Command::InstallRegistrySkill {
                source: discovery.source.clone(),
                name: candidate.name.clone(),
                expected_revision: 2,
            }))
            .await
            .unwrap_err();
        assert_eq!(failed.code, ErrorCode::InvalidRequest);
        assert_eq!(
            inventory(&client).await.as_slice(),
            std::slice::from_ref(&updated.summary)
        );
        registry.state.lock().unwrap().corrupt_archive = false;
        registry.state.lock().unwrap().unsigned = true;
        assert_eq!(
            client
                .execute(client.prepare(Command::DiscoverRegistrySkill { source: source() }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(
            registry
                .state
                .lock()
                .unwrap()
                .requests
                .iter()
                .all(|request| !request.contains("/files"))
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        registry.state.lock().unwrap().unsigned = false;
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_skill_registry_fixture(&registry.endpoint).unwrap();
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
        let restored = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: candidate.name.clone(),
                },
            )
            .await,
        );
        assert_eq!(restored, updated);
        let Output::SkillDocument(document) = execute(
            &client,
            Command::ReadSkillDocument {
                source: plugin::skills::DocumentSource::Registry(discovery.source.clone()),
            },
        )
        .await
        else {
            panic!("skill document expected");
        };
        assert_eq!(document.skill, candidate.skill);
        assert_eq!(document.content, "Revision 1.0.0\n");
        let root = node
            .profile()
            .join("plugins/packages")
            .join(&restored.summary.digest);
        assert_eq!(
            fs::read_to_string(root.join("SOURCE-SKILL.md")).unwrap(),
            fixture::document("1.1.0", true)
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn queues_reads_and_releases_cancelled_slots() {
    for remote in [false, true] {
        let registry = fixture::Registry::start().await;
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
        registry.state.lock().unwrap().hold = Some(gate.clone());
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_skill_registry_fixture(&registry.endpoint).unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = std::sync::Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let start = |version: Option<&str>| {
            let mut selected = source();
            selected.version = version.map(str::to_owned);
            let client = client.clone();
            tokio::spawn(async move {
                client
                    .execute(client.prepare(Command::DiscoverRegistrySkill { source: selected }))
                    .await
            })
        };
        let first = start(None);
        let second = start(Some("1.0.0"));
        tokio::time::timeout(Duration::from_secs(5), registry.arrivals.acquire_many(2))
            .await
            .unwrap()
            .unwrap()
            .forget();
        let mut cancelled = start(None);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), &mut cancelled)
                .await
                .is_err()
        );
        cancelled.abort();
        assert!(cancelled.await.unwrap_err().is_cancelled());
        registry.state.lock().unwrap().newer = true;
        let latest_gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
        registry.state.lock().unwrap().hold = Some(latest_gate.clone());
        let mut latest = start(None);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), &mut latest)
                .await
                .is_err()
        );
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(5), registry.arrivals.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        second.abort();
        assert!(second.await.unwrap_err().is_cancelled());
        latest_gate.add_permits(1);
        let Output::RegistrySkillDiscovery(discovery) =
            tokio::time::timeout(Duration::from_secs(5), latest)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
        else {
            panic!("registry discovery expected");
        };
        assert_eq!(discovery.source.version, "1.1.0");
        assert_eq!(
            registry
                .state
                .lock()
                .unwrap()
                .requests
                .iter()
                .filter(|path| path.starts_with("/api/v1/skills/analysis"))
                .count(),
            3
        );
        registry.state.lock().unwrap().hold = None;
        let Output::SkillDocument(document) = client
            .execute(client.prepare(Command::ReadSkillDocument {
                source: plugin::skills::DocumentSource::Registry(discovery.source),
            }))
            .await
            .unwrap()
        else {
            panic!("skill document expected");
        };
        assert!(document.content.contains("Revision 1.1.0"));
        registry.state.lock().unwrap().hold = None;
        registry.state.lock().unwrap().unsigned = true;
        let error = client
            .execute(client.prepare(Command::DiscoverRegistrySkill { source: source() }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(inventory(&client).await.is_empty());
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn shares_overlapping_inspections_without_installation() {
    for remote in [false, true] {
        let registry = fixture::Registry::start().await;
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
        registry.state.lock().unwrap().hold = Some(gate.clone());
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_skill_registry_fixture(&registry.endpoint).unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = std::sync::Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let first = tokio::spawn({
            let client = client.clone();
            async move { discover(&client).await }
        });
        tokio::time::timeout(Duration::from_secs(5), registry.arrivals.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        let mut second = tokio::spawn({
            let client = client.clone();
            async move { discover(&client).await }
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(150), &mut second)
                .await
                .is_err()
        );
        assert_eq!(registry.state.lock().unwrap().requests.len(), 1);
        gate.add_permits(1);
        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert_eq!(first, second);
        let requests = registry.state.lock().unwrap().requests.clone();
        assert_eq!(
            requests
                .iter()
                .filter(|path| path.starts_with("/api/v1/skills/"))
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|path| path.starts_with("/archive"))
                .count(),
            1
        );
        assert!(inventory(&client).await.is_empty());
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
