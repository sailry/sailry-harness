use super::*;

async fn listing(client: &Client, query: &str, force: bool) -> plugin::skillcatalog::Page {
    let request = client.prepare(Command::SearchSkillCatalog {
        source: plugin::skillcatalog::Source::Official,
        page: 1,
        query: query.into(),
        force,
    });
    assert!(!request.command.durable());
    let pending = client.dispatch(request).await.unwrap();
    assert!(!pending.receipt.durable);
    let Output::SkillCatalog(page) = pending.completion.await.unwrap().unwrap() else {
        panic!("skill catalog expected");
    };
    page
}

#[tokio::test]
async fn shares_immutable_sources_and_preserves_offline_inventory() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let count = async || -> i64 {
            node.with_storage(|db| {
                Box::pin(async move {
                    db.prepare("SELECT count(*) FROM requests")
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .query_row(())
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .get(0)
                        .map_err(crate::plugins::database_fixture::fault)
                })
            })
            .await
            .unwrap()
        };
        let before = count().await;
        execute(&client, search("official-new")).await;
        let page = listing(&client, "fixture-skill", false).await;
        assert!(!page.unavailable);
        assert_eq!(page.entries.len(), 1);
        let entry = &page.entries[0];
        assert_eq!(entry.id, "official-new/fixture-skill");
        assert_eq!(entry.title("zh-CN"), "测试技能");
        assert!(!entry.bundled);
        let Some(plugin::skillcatalog::InstallSource::Git(source)) = &entry.source else {
            panic!("Git source expected")
        };
        assert_eq!(source.git_ref.as_deref(), Some(FIRST));
        assert_eq!(
            source.path.as_deref(),
            Some("official-new/skills/fixture-skill")
        );
        assert_eq!(*catalog.requests.lock().unwrap(), catalog.index_paths());

        catalog.updated.store(true, Ordering::SeqCst);
        catalog.invalid.store(true, Ordering::SeqCst);
        assert_eq!(
            client
                .execute(client.prepare(Command::SearchSkillCatalog {
                    source: plugin::skillcatalog::Source::Official,
                    page: 1,
                    query: String::new(),
                    force: true,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(listing(&client, "fixture-skill", false).await, page);
        catalog.invalid.store(false, Ordering::SeqCst);
        catalog.legacy.store(true, Ordering::SeqCst);
        let legacy = listing(&client, "", true).await;
        assert!(!legacy.unavailable);
        assert!(!legacy.entries.is_empty());
        assert!(
            legacy
                .entries
                .iter()
                .all(|entry| entry.bundled && matches!(&entry.source, Some(plugin::skillcatalog::InstallSource::Git(source)) if source.git_ref.as_deref() == Some("main")))
        );
        assert!(legacy.entries.iter().all(
            |entry| entry.id == format!("{}/{}", entry.package.as_deref().unwrap(), entry.name)
        ));
        catalog.status.store(503, Ordering::SeqCst);
        let offline = listing(&client, "", true).await;
        assert!(offline.unavailable);
        assert_eq!(offline.entries, legacy.entries);
        assert_eq!(count().await, before);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
