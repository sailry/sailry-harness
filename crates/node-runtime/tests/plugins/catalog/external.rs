use super::super::registry::fixture as registry;
use super::*;
use plugin::skillcatalog::{InstallSource, QueryScope, Source};

async fn listing(
    client: &Client,
    source: Source,
    query: &str,
    page: u32,
    force: bool,
) -> plugin::skillcatalog::Page {
    let pending = client
        .dispatch(client.prepare(Command::SearchSkillCatalog {
            source,
            query: query.into(),
            page,
            force,
        }))
        .await
        .unwrap();
    assert!(!pending.receipt.durable);
    let Output::SkillCatalog(page) = pending.completion.await.unwrap().unwrap() else {
        panic!("skill catalog expected")
    };
    page
}

fn retained(node: &Node) -> i64 {
    rusqlite::Connection::open_with_flags(
        node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
    .unwrap()
}

#[tokio::test]
async fn browses_sources_and_installs_selected_skills() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let registry = registry::Registry::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        node.set_skill_registry_fixture(&registry.endpoint).unwrap();
        let before = retained(&node);
        let first = listing(&client, Source::SkillsSh, "", 1, false).await;
        assert_eq!(first.query_scope, QueryScope::Catalog);
        assert_eq!(first.next, Some(2));
        assert_eq!(first.total, Some(400));
        assert_eq!(first.entries[0].id, "example/foreign/review");
        assert!(!first.entries[0].bundled);
        assert!(first.entries.iter().all(|entry| entry.icon.is_none()));
        assert_eq!(first.entries.len(), 2);
        assert_eq!(first.entries[1].origin, "open.feishu.cn");
        assert_eq!(first.entries[1].source, None);
        assert_eq!(
            first.entries[1].url.as_deref(),
            Some("https://skills.sh/open.feishu.cn/lark-doc")
        );
        let requests = catalog.requests.lock().unwrap().len();
        assert_eq!(
            listing(&client, Source::SkillsSh, "", 1, false).await,
            first
        );
        assert_eq!(catalog.requests.lock().unwrap().len(), requests);
        let second = listing(&client, Source::SkillsSh, "", 2, false).await;
        assert_eq!(second.entries[0].name, "analysis");
        assert_eq!(second.next, None);
        let filtered = listing(&client, Source::SkillsSh, "z", 1, false).await;
        assert_eq!(filtered.query_scope, QueryScope::Page);
        assert_eq!(filtered.total, None);
        assert_eq!(filtered.next, Some(2));
        assert!(filtered.entries.is_empty());
        let search = listing(&client, Source::SkillsSh, "review & analysis", 1, false).await;
        assert_eq!(search.query_scope, QueryScope::Catalog);
        assert_eq!(search.next, None);
        assert_eq!(search.total, None);
        assert_eq!(search.entries, first.entries);
        assert_eq!(
            client
                .execute(client.prepare(Command::SearchSkillCatalog {
                    source: Source::SkillsSh,
                    query: "review & analysis".into(),
                    page: 2,
                    force: false
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let requests = catalog.requests.lock().unwrap().clone();
        assert!(requests.contains(&"/skills-sh/api/skills/all-time/0".into()));
        assert!(requests.contains(&"/skills-sh/api/skills/all-time/1".into()));
        assert!(
            requests
                .iter()
                .any(|request| request == "/skills-sh/api/search?q=review+%26+analysis&limit=100")
        );
        catalog.status.store(503, Ordering::SeqCst);
        let unavailable = listing(&client, Source::SkillsSh, "review & analysis", 1, true).await;
        assert!(unavailable.unavailable);
        assert_eq!(unavailable.entries, search.entries);
        let requests = catalog.requests.lock().unwrap().len();
        assert_eq!(
            listing(&client, Source::SkillsSh, "review & analysis", 1, false).await,
            unavailable
        );
        assert_eq!(catalog.requests.lock().unwrap().len(), requests);
        catalog.status.store(200, Ordering::SeqCst);
        let hub = listing(&client, Source::Skillhub, "", 1, false).await;
        assert_eq!(hub.query_scope, QueryScope::Catalog);
        assert_eq!(hub.next, Some(2));
        assert_eq!(hub.total, Some(25));
        assert_eq!(hub.entries[0].description("zh-CN"), Some("分析示例"));
        assert_eq!(
            hub.entries[0].icon.as_deref(),
            Some("https://images.example.test/analysis.png")
        );
        let next = listing(&client, Source::Skillhub, "", 2, false).await;
        assert_eq!(next.next, None);
        assert_eq!(next.entries[0].icon, None);
        assert_ne!(next.entries[0].display_name, hub.entries[0].display_name);
        let found = listing(&client, Source::Skillhub, "analysis & review", 1, false).await;
        assert_eq!(found.entries, hub.entries);
        assert_eq!(retained(&node), before);
        let Some(InstallSource::Git(source)) = first.entries[0].source.clone() else {
            panic!("Git source expected")
        };
        assert_eq!(
            source.repository,
            "https://skills.sh/example/foreign/review"
        );
        let Output::SkillDiscovery(discovered) =
            execute(&client, Command::DiscoverSkills { source }).await
        else {
            panic!("skill discovery expected")
        };
        assert_eq!(discovered.source.commit, FIRST);
        assert_eq!(discovered.skills.len(), 1);
        assert_eq!(discovered.skills[0].path, "skills/review");
        let candidate = &discovered.skills[0];
        let installed = info(
            execute(
                &client,
                Command::InstallSkill {
                    source: discovered.source.clone(),
                    path: candidate.path.clone(),
                    name: candidate.name.clone(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        assert_eq!(installed.skills[0].name, "review");
        assert!(matches!(
            installed.skill,
            Some(plugin::skills::Provenance::Git(_))
        ));
        let Some(InstallSource::Registry(source)) = hub.entries[0].source.clone() else {
            panic!("registry source expected")
        };
        assert_eq!(source.namespace.as_deref(), Some("example"));
        assert_eq!(source.version.as_deref(), Some("1.0.0"));
        let Output::RegistrySkillDiscovery(discovered) =
            execute(&client, Command::DiscoverRegistrySkill { source }).await
        else {
            panic!("registry discovery expected")
        };
        let installed = info(
            execute(
                &client,
                Command::InstallRegistrySkill {
                    source: discovered.source,
                    name: discovered.skills[0].name.clone(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        assert_eq!(installed.skills[0].name, "analysis");
        assert!(matches!(
            installed.skill,
            Some(plugin::skills::Provenance::Registry { .. })
        ));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn inspects_foreign_packages_without_running_hooks() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let before = retained(&node);
        let inventory = execute(&client, Command::ListPlugins).await;
        let pending = client
            .dispatch(client.prepare(Command::ReadCatalogPluginInfo {
                source: plugin::catalog::Source::ThirdParty,
                id: "foreign".into(),
                bundled: false,
            }))
            .await
            .unwrap();
        assert!(!pending.receipt.durable);
        let Output::CatalogDetails(plugin::catalog::Details::Skills(details)) =
            pending.completion.await.unwrap().unwrap()
        else {
            panic!("foreign skills expected")
        };
        assert_eq!(details.metadata.name, "foreign");
        assert_eq!(details.metadata.version.as_deref(), Some("1.2.3"));
        assert_eq!(details.source.commit, FIRST);
        assert_eq!(details.unsupported, [plugin::catalog::Format::Codex]);
        assert_eq!(details.skills.len(), 2);
        assert_eq!(execute(&client, Command::ListPlugins).await, inventory);
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::ThirdParty,
                    id: "invalid".into(),
                    bundled: false
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(retained(&node), before);
        let candidate = &details.skills[0];
        let installed = info(
            execute(
                &client,
                Command::InstallSkill {
                    source: details.source.clone(),
                    path: candidate.path.clone(),
                    name: candidate.name.clone(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        assert_eq!(installed.skills.len(), 1);
        assert!(installed.extension.is_none());
        assert!(installed.mcp.is_empty());
        assert_eq!(installed.skills[0].name, candidate.skill.name);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn document(
    client: &Client,
    source: plugin::skills::DocumentSource,
) -> plugin::skills::Document {
    let request = client.prepare(Command::ReadSkillDocument { source });
    assert!(!request.command.durable());
    let pending = client.dispatch(request).await.unwrap();
    assert!(!pending.receipt.durable);
    let Output::SkillDocument(document) = pending.completion.await.unwrap().unwrap() else {
        panic!("skill document expected")
    };
    document
}

#[tokio::test]
async fn reads_immutable_documents_without_installation() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let registry = registry::Registry::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        node.set_skill_registry_fixture(&registry.endpoint).unwrap();
        let before = retained(&node);
        let inventory = execute(&client, Command::ListPlugins).await;
        let Output::SkillDiscovery(discovery) = execute(
            &client,
            Command::DiscoverSkills {
                source: plugin::skills::Source {
                    repository: "https://skills.sh/example/foreign/review".into(),
                    git_ref: None,
                    path: None,
                },
            },
        )
        .await
        else {
            panic!("skill discovery expected")
        };
        let source = plugin::skills::DocumentSource::Git {
            source: discovery.source.clone(),
            path: discovery.skills[0].path.clone(),
        };
        let read = document(&client, source).await;
        assert_eq!(read.skill, discovery.skills[0].skill);
        assert_eq!(read.content, "Review supplied changes\n");
        assert!(
            catalog
                .requests
                .lock()
                .unwrap()
                .iter()
                .rev()
                .take(1)
                .all(|path| path == &format!("/example/foreign/zip/{FIRST}"))
        );
        let count = catalog.requests.lock().unwrap().len();
        let error = client
            .execute(client.prepare(Command::ReadSkillDocument {
                source: plugin::skills::DocumentSource::Git {
                    source: discovery.source,
                    path: "../outside".into(),
                },
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(catalog.requests.lock().unwrap().len(), count);
        let Output::RegistrySkillDiscovery(discovery) = execute(
            &client,
            Command::DiscoverRegistrySkill {
                source: plugin::skills::RegistrySource {
                    namespace: Some("example".into()),
                    slug: "analysis".into(),
                    version: Some("1.0.0".into()),
                },
            },
        )
        .await
        else {
            panic!("registry discovery expected")
        };
        let read = document(
            &client,
            plugin::skills::DocumentSource::Registry(discovery.source.clone()),
        )
        .await;
        assert_eq!(read.skill, discovery.skills[0].skill);
        assert_eq!(read.content, "Revision 1.0.0\n");
        let mut changed = discovery.source;
        changed.archive_sha256 = "0".repeat(64);
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadSkillDocument {
                    source: plugin::skills::DocumentSource::Registry(changed)
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        catalog.status.store(503, Ordering::SeqCst);
        let bundled = listing(&client, Source::Official, "", 1, true).await;
        assert!(bundled.unavailable);
        let entry = bundled.entries.iter().find(|entry| entry.bundled).unwrap();
        let read = document(
            &client,
            plugin::skills::DocumentSource::Bundled {
                package: entry.package.clone().unwrap(),
                skill: entry.name.clone(),
            },
        )
        .await;
        assert_eq!(read.skill.name, entry.name);
        assert_eq!(
            Some(read.skill.description.as_str()),
            entry.description.as_deref()
        );
        assert!(!read.content.starts_with("---"));
        assert!(!read.content.trim().is_empty());
        assert_eq!(execute(&client, Command::ListPlugins).await, inventory);
        assert_eq!(retained(&node), before);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
