use super::*;
#[path = "../support/skillhub.rs"]
mod registry;

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

#[tokio::test]
async fn installs_and_updates_verified_packages() {
    for remote in [false, true] {
        let registry = registry::Registry::start().await;
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
        assert_eq!(inventory(&client).await[0], updated.summary);
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
    }
}
