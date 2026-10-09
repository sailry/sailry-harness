use super::*;
use rusqlite::Connection;

#[tokio::test]
async fn retains_unicode_phrases_and_scope() {
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
        let (directory, node, controller, client, worktree) = fixture(remote).await;
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

fn records(db: &Connection, name: &str) -> Vec<(String, i64, Vec<u8>, Vec<u8>)> {
    db.prepare("SELECT key,revision,value,index_data FROM plugin_values WHERE name=?1 ORDER BY key")
        .unwrap()
        .query_map([name], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn previous(db: &Connection, name: &str) -> String {
    let table = format!("plugin_search_{}", blake3::hash(name.as_bytes()).to_hex());
    let definition = db
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name=?1",
            [&table],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .replace(",cjk0,cjk1", "")
        .replace("unicode61 remove_diacritics 2", "ascii");
    db.execute_batch(&format!("DROP TABLE {table}; {definition}"))
        .unwrap();
    db.execute(&format!("INSERT INTO {table}(rowid,field0,field1,key,tags,ordinal) SELECT rowid,json_extract(index_data,'$.fields[0]'),json_extract(index_data,'$.fields[1]'),key,json_extract(index_data,'$.tags'),json_extract(index_data,'$.order') FROM plugin_values WHERE name=?1 AND index_data IS NOT NULL"), [name]).unwrap();
    definition
}

#[tokio::test]
async fn rebuilds_at_restart_without_source_changes() {
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
        let path = profile.join("storage/node.sqlite3");
        drop(client);
        node.shutdown().await.unwrap();
        let db = Connection::open(&path).unwrap();
        let original = records(&db, &context.package.name);
        let definition = previous(&db, &context.package.name);
        db.close().unwrap();
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
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
        let db =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(records(&db, &context.package.name), original);
        let backups = profile.join("storage/backups");
        let files = fs::read_dir(&backups)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(files.len(), 1);
        let saved = Connection::open_with_flags(
            files[0].path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(records(&saved, &context.package.name), original);
        let table = format!(
            "plugin_search_{}",
            blake3::hash(context.package.name.as_bytes()).to_hex()
        );
        assert_eq!(
            saved
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name=?1",
                    [&table],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            definition
        );
        controller.close().await.unwrap();
    }
}
