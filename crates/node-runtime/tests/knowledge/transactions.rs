use super::*;
use serde_json::json;

fn count(db: &rusqlite::Connection) -> u64 {
    db.prepare("SELECT body FROM events ORDER BY cursor")
        .unwrap()
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|row| serde_json::from_slice::<Event>(&row.unwrap()).unwrap())
        .filter(|event| matches!(event, Event::KnowledgeCollectionsChanged))
        .count() as u64
}

async fn declared(client: &Client, fixture: &Fixture, name: &str) -> Context {
    let package = fixture.root.join(name);
    fs::create_dir(&package).unwrap();
    fs::write(
        package.join("plugin.json"),
        json!({
            "$schema": plugin::schema::AGENT_PLUGINS_ID,
            "name":name,"version":"1.0.0","description":"Semantic storage fixture",
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":[Action::ReadStorage,Action::WriteStorage],
                "storage":{"collections":[
                    {"scope":"node","prefix":"notes/","schema":true,"index":{"semantic":true}},
                    {"scope":"node","key":"body/raw","schema":true}
                ]}
            }}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(info) = execute(
        client,
        Command::InstallPlugin {
            worktree: fixture.worktree,
            path: name.into(),
            name: name.into(),
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    Context {
        package: info.summary.reference(),
        worktree: Some(fixture.worktree),
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    }
}

fn metadata() -> plugin::storage::Index {
    plugin::storage::Index {
        fields: ["Original metadata".into(), "Literal keyword".into()],
        tags: vec!["active".into()],
        order: 0,
        source: Some(plugin::storage::Source {
            key: "body/raw".into(),
            pointer: "/text".into(),
        }),
    }
}

fn recall() -> plugin::storage::Search {
    plugin::storage::Search {
        terms: vec!["Original".into()],
        query: Some("Raw original text".into()),
        mode: Mode::Hybrid,
        weights: [3, 1],
        all: vec![],
        any: vec![],
        offset: 0,
        limit: 5,
    }
}

#[tokio::test]
async fn preserves_replay_and_atomic_index_rollback() {
    let fixture = Fixture::start().await;
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let name = format!("semantic-{index}");
        let context = declared(&client, &fixture, &name).await;
        run(
            &client,
            &context,
            Command::WritePluginValue {
                key: "body/raw".into(),
                value: json!({"text":"Raw original text 向量"}),
                expected_revision: 0,
            },
        )
        .await;
        let request = client
            .prepare(Command::WriteIndexedPluginValue {
                key: "notes/one".into(),
                value: json!({"title":"Original metadata"}),
                index: metadata(),
                expected_revision: 0,
            })
            .with_plugin(context.clone());
        let output = client.execute(request.clone()).await.unwrap();
        let first = list(&client)
            .await
            .into_iter()
            .find(|entry| matches!(&entry.owner,Owner::Plugin {name:owner} if owner==&name))
            .unwrap();
        found(&client, None, "向量", first.id).await;
        let Output::PluginSearch(page) =
            run(&client, &context, Command::SearchPluginValues(recall())).await
        else {
            panic!("search page expected")
        };
        assert_eq!(page.mode, Mode::Keyword);
        assert!(!page.indexing);
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].key, "notes/one");
        assert_eq!(page.entries[0].value, json!({"title":"Original metadata"}));
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ListKnowledgeCollections)
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let paused = super::saved(
            execute(
                &client,
                Command::PauseKnowledgeCollection {
                    id: first.id,
                    expected_revision: first.revision,
                    paused: true,
                },
            )
            .await,
        );
        let database =
            rusqlite::Connection::open(fixture.temp.path().join("node/storage/node.sqlite3"))
                .unwrap();
        let before = count(&database);
        assert_eq!(client.execute(request).await.unwrap(), output);
        let after = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == first.id)
            .unwrap();
        assert_eq!(after.revision, paused.revision);
        assert_eq!(after.progress.generation, paused.progress.generation);
        assert_eq!(after.rebuild, paused.rebuild);
        assert_eq!(after, paused);
        assert_eq!(paused.progress.status, Status::Paused);
        assert_eq!(paused.progress.sources, 1);
        assert_eq!(paused.progress.chunks, 1);
        assert_eq!(count(&database), before);
        let mutation = client
            .prepare(Command::WritePluginValue {
                key: "body/raw".into(),
                value: json!({"text":"Updated raw original text 向量"}),
                expected_revision: 1,
            })
            .with_plugin(context.clone());
        let updated = client.execute(mutation.clone()).await.unwrap();
        let changed = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == first.id)
            .unwrap();
        assert_eq!(changed.revision, paused.revision + 1);
        assert_eq!(changed.progress.generation, paused.progress.generation + 1);
        let baseline = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let current = list(&client)
                    .await
                    .into_iter()
                    .find(|entry| entry.id == first.id)
                    .unwrap();
                if current.revision == changed.revision
                    && current.progress.generation == changed.progress.generation
                    && current.paused
                    && current.progress.status == Status::Paused
                {
                    return current;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("paused source mutation reached its authoritative progress state");
        assert_eq!(baseline.progress.sources, paused.progress.sources);
        assert_eq!(baseline.progress.chunks, paused.progress.chunks);
        assert_eq!(baseline.rebuild, paused.rebuild);
        // The committed invalidation and the worker's Pending-to-Paused transition each publish once.
        assert_eq!(count(&database), before + 2);
        let before = count(&database);
        assert_eq!(client.execute(mutation).await.unwrap(), updated);
        let replayed = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == first.id)
            .unwrap();
        assert_eq!(replayed, baseline);
        assert_eq!(count(&database), before);
        let Output::PluginValue(raw) = run(
            &client,
            &context,
            Command::ReadPluginValue {
                key: "body/raw".into(),
            },
        )
        .await
        else {
            panic!("raw entry expected")
        };
        assert_eq!(raw.revision, 2);
        assert_eq!(raw.value, json!({"text":"Updated raw original text 向量"}));
        let rollback = client
            .prepare(Command::PluginTransaction {
                operations: vec![
                    plugin::transaction::Operation::Index {
                        key: "notes/rolled-back".into(),
                        value: json!({"title":"Rolled back"}),
                        index: metadata(),
                        expected_revision: 0,
                    },
                    plugin::transaction::Operation::Remove {
                        key: "notes/one".into(),
                        expected_revision: 0,
                    },
                ],
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(rollback.clone()).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client.execute(rollback).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let Output::PluginValue(absent) = run(
            &client,
            &context,
            Command::ReadPluginValue {
                key: "notes/rolled-back".into(),
            },
        )
        .await
        else {
            panic!("entry expected")
        };
        assert!(!absent.present);
        let Output::PluginSearch(retained) =
            run(&client, &context, Command::SearchPluginValues(recall())).await
        else {
            panic!("search page expected")
        };
        assert_eq!(retained.entries.len(), 1);
        assert_eq!(retained.entries[0].key, "notes/one");
        let after = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == first.id)
            .unwrap();
        assert_eq!(after.revision, changed.revision);
        assert_eq!(after.progress.generation, changed.progress.generation);
        assert_eq!(after, baseline);
        assert_eq!(count(&database), before);
    }
    fixture.close().await;
}
