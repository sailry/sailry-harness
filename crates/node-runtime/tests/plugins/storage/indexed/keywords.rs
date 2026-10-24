use super::*;
use turso::Connection;

#[tokio::test]
async fn retains_unicode_phrases_and_scope() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source");
        let context = actions::install_actions(
            &client,
            &source.join("package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        for (key, title, body) in [
            ("raw", "我们使用向量数据库", ""),
            ("single", "向", ""),
            ("mixed", "AI向量数据库", ""),
            ("latin", "Café C++ C#", ""),
            ("kana", "すし屋", ""),
            ("hangul", "한국어", ""),
        ] {
            indexed(&client, &context, key, index(title, body, &["raw"], 1), 0).await;
        }
        for (term, expected) in [
            ("我们", vec!["raw"]),
            ("量数", vec!["mixed", "raw"]),
            ("库", vec!["mixed", "raw"]),
            ("AI向量", vec!["mixed"]),
            ("AI", vec!["mixed"]),
            ("CAFE", vec!["latin"]),
            ("c++", vec!["latin"]),
            ("c#", vec!["latin"]),
            ("し屋", vec!["kana"]),
            ("국어", vec!["hangul"]),
        ] {
            let page = search(&client, &context, query(&[term])).await;
            let mut actual = keys(&page);
            actual.sort_unstable();
            assert_eq!(actual, expected, "{term}, remote={remote}");
        }
        assert!(keys(&search(&client, &context, query(&["向"])).await).contains(&"single"));
        assert!(
            search(&client, &context, query(&["向\" OR Café"]))
                .await
                .entries
                .is_empty()
        );
        indexed(
            &client,
            &context,
            "reversed",
            index("向量 AI", "", &["raw"], 1),
            0,
        )
        .await;
        assert_eq!(
            keys(&search(&client, &context, query(&["AI向量"])).await),
            ["mixed"]
        );
        let other = install_named(
            &client,
            &source,
            worktree,
            "private-corpus",
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        indexed(&client, &other, "private", index("向量", "", &[], 100), 0).await;
        assert!(!keys(&search(&client, &context, query(&["向量"])).await).contains(&"private"));
        assert_eq!(
            keys(&search(&client, &other, query(&["向量"])).await),
            ["private"]
        );
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_weights_filters_and_memory_tokens() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        for (key, metadata) in [
            (
                "title",
                index("向 量 向量", "", &["active", "project-a"], 1),
            ),
            ("body", index("", "向 量 向量", &["active", "project-b"], 2)),
            ("archive", index("向量", "", &["archive"], 99)),
        ] {
            indexed(&client, &context, key, metadata, 0).await;
        }
        let mut input = query(&["向量"]);
        input.all = vec!["active".into()];
        assert_eq!(
            keys(&search(&client, &context, input.clone()).await),
            ["title", "body"]
        );
        input.weights = [1, 3];
        assert_eq!(
            keys(&search(&client, &context, input.clone()).await),
            ["body", "title"]
        );
        input.any = vec!["project-a".into()];
        assert_eq!(
            keys(&search(&client, &context, input.clone()).await),
            ["title"]
        );
        input.any.clear();
        input.terms.clear();
        assert_eq!(
            keys(&search(&client, &context, input).await),
            ["body", "title"]
        );
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn records(db: &Connection, name: &str) -> Vec<(String, i64, Vec<u8>, Vec<u8>)> {
    let mut rows = db
        .query(
            "SELECT key,revision,value,index_data FROM plugin_values WHERE name=?1 ORDER BY key",
            [name],
        )
        .await
        .unwrap();
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        records.push((
            row.get(0).unwrap(),
            row.get(1).unwrap(),
            row.get(2).unwrap(),
            row.get(3).unwrap(),
        ));
    }
    records
}

async fn previous(db: &Connection, name: &str) -> String {
    let corpus = format!("plugin_search_{}", blake3::hash(name.as_bytes()).to_hex());
    let index = format!("{corpus}_fts");
    let current: String = db
        .prepare("SELECT sql FROM sqlite_schema WHERE name=?1")
        .await
        .unwrap()
        .query_row([index.as_str()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    // An older native derived index lacks the CJK fields; source records stay exact.
    let definition = format!(
        "CREATE INDEX {index} ON {corpus} USING fts (field0, field1) WITH (tokenizer = 'whitespace')"
    );
    assert_ne!(definition, current);
    db.execute_batch(format!("DROP INDEX {index}; {definition}"))
        .await
        .unwrap();
    definition
}

#[tokio::test]
async fn rebuilds_at_restart_without_source_changes() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
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
            "raw",
            index("AI向量数据库", "我们使用向量数据库", &["active"], 7),
            0,
        )
        .await;
        indexed(
            &client,
            &context,
            "single",
            index("库", "", &["active"], 8),
            0,
        )
        .await;
        let profile = node.profile().to_path_buf();
        let path = profile.join("storage/node.turso");
        drop(client);
        node.shutdown().await.unwrap();
        let (db_handle, db) = crate::plugins::database_fixture::open(&path, false).await;
        let original = records(&db, &context.package.name).await;
        let definition = previous(&db, &context.package.name).await;
        crate::plugins::database_fixture::close(db_handle, db).await;
        let node = Node::start_empty_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        for term in ["向量", "量数", "AI向量"] {
            assert_eq!(
                keys(&search(&client, &context, query(&[term])).await),
                ["raw"]
            );
        }
        let mut tail = keys(&search(&client, &context, query(&["库"])).await)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        tail.sort();
        assert_eq!(tail, ["raw", "single"]);
        assert_eq!(read(&client, &context, "raw").await.revision, 1);
        assert_eq!(
            read(&client, &context, "raw").await.value,
            json!({"key":"raw"})
        );
        drop(client);
        node.shutdown().await.unwrap();
        let (db_handle, db) = crate::plugins::database_fixture::open(&path, true).await;
        assert_eq!(records(&db, &context.package.name).await, original);
        let backups = profile.join("storage/backups");
        let files = fs::read_dir(&backups)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(files.len(), 1);
        let (saved_handle, saved) =
            crate::plugins::database_fixture::open(&(files[0].path().join("node.turso")), true)
                .await;
        assert_eq!(records(&saved, &context.package.name).await, original);
        let table = format!(
            "plugin_search_{}_fts",
            blake3::hash(context.package.name.as_bytes()).to_hex()
        );
        assert_eq!(
            (async {
                let mut statement = saved
                    .prepare("SELECT sql FROM sqlite_master WHERE name=?1")
                    .await?;
                let row = statement.query_row([table.as_str()]).await?;
                let mapped: turso::Result<_> = row.get::<String>(0);
                mapped
            })
            .await
            .unwrap(),
            definition
        );
        crate::plugins::database_fixture::close(db_handle, db).await;
        crate::plugins::database_fixture::close(saved_handle, saved).await;
        controller.close().await.unwrap();
    }
}
