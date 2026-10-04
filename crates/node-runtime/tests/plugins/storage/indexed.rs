use super::*;
use sailry_protocol::plugin::{
    storage::{Index, Search, SearchPage},
    transaction::Operation,
};

fn index(title: &str, body: &str, tags: &[&str], order: i64) -> Index {
    Index {
        fields: [title.into(), body.into()],
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
        order,
    }
}

fn query(terms: &[&str]) -> Search {
    Search {
        terms: terms.iter().map(|term| (*term).into()).collect(),
        weights: [3, 1],
        all: Vec::new(),
        any: Vec::new(),
        offset: 0,
        limit: 100,
    }
}

async fn search(client: &Client, context: &Context, query: Search) -> SearchPage {
    let Output::PluginSearch(page) = run(client, context, Command::SearchPluginValues(query)).await
    else {
        panic!("plugin search page expected")
    };
    page
}

async fn indexed(
    client: &Client,
    context: &Context,
    key: &str,
    index: Index,
    revision: u64,
) -> Entry {
    entry(
        run(
            client,
            context,
            Command::WriteIndexedPluginValue {
                key: key.into(),
                value: json!({"key":key}),
                index,
                expected_revision: revision,
            },
        )
        .await,
    )
}

fn keys(page: &SearchPage) -> Vec<&str> {
    page.entries
        .iter()
        .map(|entry| entry.key.as_str())
        .collect()
}

#[tokio::test]
async fn invalidates_without_values() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let observer = Client::new(if remote {
            node.local()
        } else {
            controller.handle().remote(node.link().address())
        });
        let mut updates = observer.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        projection.apply(1, updates.next().await.unwrap()).unwrap();
        let before = projection.snapshot().unwrap().clone();
        let original = request(
            &client,
            &context,
            Command::WriteIndexedPluginValue {
                key: "private-key".into(),
                value: json!({"private-body":"Saved 中文 🙂 content"}),
                index: index("private-token", "", &["private-tag"], 1),
                expected_revision: 0,
            },
        );
        let saved = client.execute(original.clone()).await.unwrap();
        let update = tokio::time::timeout(Duration::from_secs(5), updates.next())
            .await
            .unwrap()
            .unwrap();
        let Update::Event(envelope) = &update else {
            panic!("invalidation expected")
        };
        assert_eq!(
            envelope.event,
            Event::PluginValuesChanged {
                name: context.package.name.clone()
            }
        );
        let visible = serde_json::to_string(&update).unwrap();
        for private in [
            "private-key",
            "private-body",
            "private-token",
            "private-tag",
            "Saved 中文",
        ] {
            assert!(!visible.contains(private));
        }
        projection.apply(1, update).unwrap();
        let mut expected = before;
        expected.cursor += 1;
        assert_eq!(projection.snapshot().unwrap(), &expected);
        assert_eq!(
            read(&observer, &context, "private-key").await.value,
            json!({"private-body":"Saved 中文 🙂 content"})
        );
        assert_eq!(
            keys(&search(&observer, &context, query(&["private-token"])).await),
            ["private-key"]
        );
        assert_eq!(client.execute(original).await.unwrap(), saved);
        let rollback = request(
            &client,
            &context,
            Command::PluginTransaction {
                operations: vec![
                    Operation::Index {
                        key: "rolled-back".into(),
                        value: json!(true),
                        index: index("private-token", "", &[], 2),
                        expected_revision: 0,
                    },
                    Operation::Remove {
                        key: "private-key".into(),
                        expected_revision: 0,
                    },
                ],
            },
        );
        assert_eq!(
            client.execute(rollback).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert!(!read(&observer, &context, "rolled-back").await.present);
        run(
            &client,
            &context,
            Command::RemovePluginValue {
                key: "never-existed".into(),
                expected_revision: 0,
            },
        )
        .await;
        let Output::Snapshot(actual) = execute(&observer, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(actual, expected);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), updates.next())
                .await
                .is_err()
        );
        drop(updates);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn scopes_corpus_filters_and_literal_terms() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let source = directory.path().join("source");
        let context = actions::install_actions(
            &client,
            &source.join("package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let other = install_named(
            &client,
            &source,
            worktree,
            "private-catalog",
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let empty = request(&client, &context, Command::SearchPluginValues(query(&[])));
        let admission = client.dispatch(empty).await.unwrap();
        assert!(!admission.receipt.durable);
        assert!(
            matches!(admission.completion.await.unwrap().unwrap(),Output::PluginSearch(page) if page.entries.is_empty())
        );
        let corpus = [
            ("title", index("rust rust", "", &["active", "global"], 1)),
            ("body", index("", "rust", &["active", "project-a"], 2)),
            (
                "archive",
                index(
                    "rust",
                    "rust rust rust rust rust rust",
                    &["archive", "global"],
                    99,
                ),
            ),
            (
                "foreign",
                index("rust", "rust rust", &["active", "project-b"], 100),
            ),
            (
                "unicode",
                index("北 京 北京 c++ c#", "", &["active", "global"], 3),
            ),
        ];
        let reference = rusqlite::Connection::open_in_memory().unwrap();
        reference.execute_batch("CREATE VIRTUAL TABLE corpus USING fts5(field0,field1,key UNINDEXED,tags UNINDEXED,ordinal UNINDEXED,tokenize='ascii tokenchars ''+#''')").unwrap();
        for (key, metadata) in &corpus {
            indexed(&client, &context, key, metadata.clone(), 0).await;
            reference
                .execute(
                    "INSERT INTO corpus VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![
                        metadata.fields[0],
                        metadata.fields[1],
                        key,
                        serde_json::to_string(&metadata.tags).unwrap(),
                        metadata.order
                    ],
                )
                .unwrap();
        }
        indexed(
            &client,
            &other,
            "title",
            index("unrelated", "", &["active", "global"], 1000),
            0,
        )
        .await;
        assert!(
            search(&client, &other, query(&["rust"]))
                .await
                .entries
                .is_empty()
        );
        let mut filtered = query(&["rust"]);
        filtered.all = vec!["active".into()];
        filtered.any = vec!["global".into(), "project-a".into()];
        let expected:Vec<String>=reference.prepare("SELECT key FROM corpus WHERE corpus MATCH '\"rust\"' AND EXISTS(SELECT 1 FROM json_each(tags) WHERE value='active') AND EXISTS(SELECT 1 FROM json_each(tags) WHERE value IN ('global','project-a')) ORDER BY bm25(corpus,3,1),ordinal DESC,key ASC").unwrap().query_map([],|row|row.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(expected.len(), 2);
        assert_eq!(
            search(&client, &context, filtered.clone())
                .await
                .entries
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>(),
            expected
        );
        filtered.limit = 1;
        let first = search(&client, &context, filtered.clone()).await;
        assert_eq!(first.entries[0].key, expected[0]);
        assert_eq!(first.next, Some(1));
        assert!(first.now_ms > 0);
        filtered.offset = first.next.unwrap();
        let second = search(&client, &context, filtered).await;
        assert_eq!(second.entries[0].key, expected[1]);
        assert_eq!(second.next, None);
        assert_eq!(
            keys(&search(&client, &context, query(&["c++", "c#"])).await),
            ["unicode"]
        );
        assert_eq!(
            keys(&search(&client, &context, query(&["北京"])).await),
            ["unicode"]
        );
        assert!(
            search(&client, &context, query(&["rust OR unrelated"]))
                .await
                .entries
                .is_empty()
        );
        let mut chronological = query(&[]);
        chronological.all = vec!["active".into(), "global".into()];
        assert_eq!(
            keys(&search(&client, &context, chronological).await),
            ["unicode", "title"]
        );
        let mut stale = context.clone();
        stale.package.settings_revision += 1;
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &stale,
                    Command::SearchPluginValues(query(&[]))
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let mut forged = context.clone();
        forged.package.name = "private-catalog".into();
        assert!(
            client
                .execute(request(
                    &client,
                    &forged,
                    Command::SearchPluginValues(query(&[]))
                ))
                .await
                .is_err()
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn updates_indexes_atomically_and_recovers_receipts() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        indexed(
            &client,
            &context,
            "indexed",
            index("before", "", &["visible"], 1),
            0,
        )
        .await;
        indexed(
            &client,
            &context,
            "indexed",
            index("after", "", &["visible"], 2),
            1,
        )
        .await;
        assert!(
            search(&client, &context, query(&["before"]))
                .await
                .entries
                .is_empty()
        );
        assert_eq!(
            keys(&search(&client, &context, query(&["after"])).await),
            ["indexed"]
        );
        write(&client, &context, "indexed", json!("ordinary"), 2).await;
        assert!(
            search(&client, &context, query(&["after"]))
                .await
                .entries
                .is_empty()
        );
        indexed(
            &client,
            &context,
            "indexed",
            index("restore", "", &["visible"], 3),
            3,
        )
        .await;
        let failed = request(
            &client,
            &context,
            Command::PluginTransaction {
                operations: vec![
                    Operation::Index {
                        key: "rolled-back".into(),
                        value: json!(true),
                        index: index("restore", "", &["visible"], 99),
                        expected_revision: 0,
                    },
                    Operation::Remove {
                        key: "indexed".into(),
                        expected_revision: 0,
                    },
                ],
            },
        );
        assert_eq!(
            client.execute(failed.clone()).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client.execute(failed).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert!(!read(&client, &context, "rolled-back").await.present);
        assert_eq!(
            keys(&search(&client, &context, query(&["restore"])).await),
            ["indexed"]
        );
        let committed = request(
            &client,
            &context,
            Command::PluginTransaction {
                operations: vec![
                    Operation::Index {
                        key: "saved".into(),
                        value: json!(true),
                        index: index("durable", "", &["visible"], 4),
                        expected_revision: 0,
                    },
                    Operation::Remove {
                        key: "indexed".into(),
                        expected_revision: 4,
                    },
                ],
            },
        );
        let result = client.execute(committed.clone()).await.unwrap();
        assert_eq!(client.execute(committed.clone()).await.unwrap(), result);
        assert_eq!(
            keys(&search(&client, &context, query(&[])).await),
            ["saved"]
        );
        assert_eq!(read(&client, &context, "indexed").await.revision, 5);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(committed).await.unwrap(), result);
        let recovered = search(&client, &context, query(&["durable"])).await;
        assert_eq!(keys(&recovered), ["saved"]);
        assert_eq!(recovered.entries[0].revision, 1);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
