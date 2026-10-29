use super::*;
use std::path::{Path, PathBuf};

async fn older(partial: bool) -> Fixture {
    let mut fixture = Fixture::new().await;
    let mut event = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::ToolRequest(request(
            "called-tool",
            "called_tool",
            serde_json::json!({}),
        ))],
    );
    if partial {
        event.metadata.context = Some(records::Context {
            loaded_tools: Some(["discovered_tool".into()].into()),
            ..Default::default()
        });
    }
    fixture
        .database
        .connection
        .execute("UPDATE node SET tool_defaults=0 WHERE singleton=1", ())
        .await
        .unwrap();
    fixture.append([event]).await;
    if partial {
        fixture
            .database
            .connection
            .execute(
                "UPDATE agent_state SET body=?1 WHERE owner=?2 AND key=?3",
                params![
                    encode(&serde_json::json!(["discovered_tool"])).unwrap(),
                    fixture.session.to_string(),
                    LOADED_TOOLS
                ],
            )
            .await
            .unwrap();
    } else {
        fixture
            .database
            .connection
            .execute(
                "DELETE FROM agent_state WHERE owner=?1 AND key=?2",
                params![fixture.session.to_string(), LOADED_TOOLS],
            )
            .await
            .unwrap();
    }
    assert_eq!(
        missing_tools(&fixture.database.connection).await.unwrap()[&fixture.session],
        BTreeSet::from(["called_tool".into()])
    );
    fixture
}

#[tokio::test]
async fn missing_tools_deduplicate_per_session() {
    let mut fixture = older(true).await;
    let event = message(
        "assistant",
        Role::Assistant,
        ["called_tool", "called_tool", "second_tool"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                BlockKind::ToolRequest(request(
                    &format!("called-{index}"),
                    name,
                    serde_json::json!({}),
                ))
            })
            .collect(),
    );
    let turn = fixture.append([event]).await;
    let fork = fixture.fork(fixture.session, turn).await;
    assert_eq!(
        state(&fixture.database.connection, fork.id).await.unwrap()[LOADED_TOOLS],
        serde_json::json!(["discovered_tool", "called_tool", "second_tool"])
    );
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_state SET body=?1 WHERE owner=?2 AND key=?3",
            params![
                encode(&serde_json::json!(["discovered_tool", "second_tool"])).unwrap(),
                fixture.session.to_string(),
                LOADED_TOOLS
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        missing_tools(&fixture.database.connection).await.unwrap(),
        BTreeMap::from([(fixture.session, BTreeSet::from(["called_tool".into()]))])
    );
}

fn backups(path: &Path) -> Vec<PathBuf> {
    let directory = path.parent().unwrap().join("backups");
    if !directory.exists() {
        return Vec::new();
    }
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect()
}

#[tokio::test]
async fn fills_called_names_with_backup() {
    for partial in [false, true] {
        let mut fixture = older(partial).await;
        let before = state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .get(LOADED_TOOLS)
            .cloned();
        let mut history = fixture.read(None, 20).await;
        // The admitted user input is still pending; restart pauses it once.
        assert!(!history.page.queue.paused);
        history.page.queue.paused = true;
        history.page.queue.revision += 1;
        let path = fixture.database.path.clone();
        let node = fixture.database.node;
        fixture.database.close().await.unwrap();
        fixture.database = Database::open(&path, node, None).await.unwrap();
        let expected = if partial {
            serde_json::json!(["discovered_tool", "called_tool"])
        } else {
            serde_json::json!(["called_tool"])
        };
        assert_eq!(
            state(&fixture.database.connection, fixture.session)
                .await
                .unwrap()[LOADED_TOOLS],
            expected
        );
        assert!(
            missing_tools(&fixture.database.connection)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(complete(&fixture.database.connection).await);
        assert_eq!(fixture.read(None, 20).await, history);
        let saved = backups(&path);
        assert_eq!(saved.len(), 1);
        let database = turso::Builder::new_local(saved[0].to_str().unwrap())
            .experimental_index_method(true)
            .read_only(true)
            .build()
            .await
            .unwrap();
        let connection = database.connect().unwrap();
        assert_eq!(
            state(&connection, fixture.session)
                .await
                .unwrap()
                .get(LOADED_TOOLS)
                .cloned(),
            before
        );
        assert!(!complete(&connection).await);
        drop(connection);
        drop(database);
        fixture.database.close().await.unwrap();
        fixture.database = Database::open(&path, node, None).await.unwrap();
        assert_eq!(backups(&path), saved);
        assert_eq!(
            state(&fixture.database.connection, fixture.session)
                .await
                .unwrap()[LOADED_TOOLS],
            expected
        );
        assert_eq!(fixture.read(None, 20).await, history);
        fixture.database.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_rehearsal() {
    let fixture = older(true).await;
    let path = fixture.database.path.clone();
    let node = fixture.database.node;
    let request = Request::new(node, Command::Snapshot);
    fixture.database.connection.execute(
        "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed','invalid')",
        params![&node.0[..], request.id.to_string(), encode(&request).unwrap()],
    ).await.unwrap();
    fixture.database.close().await.unwrap();
    assert!(Database::open(&path, node, None).await.is_err());
    assert!(backups(&path).is_empty());
    let database = crate::store::database::engine(&path).await.unwrap();
    let connection = crate::store::database::connect(&database).await.unwrap();
    assert_eq!(
        state(&connection, fixture.session).await.unwrap()[LOADED_TOOLS],
        serde_json::json!(["discovered_tool"])
    );
    assert_eq!(
        missing_tools(&connection).await.unwrap()[&fixture.session],
        BTreeSet::from(["called_tool".into()])
    );
    assert!(!complete(&connection).await);
}

async fn complete(connection: &Connection) -> bool {
    connection
        .prepare("SELECT tool_defaults FROM node WHERE singleton=1")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap()
}

#[tokio::test]
async fn fresh_defaults_need_no_backup() {
    let mut fixture = Fixture::new().await;
    assert!(complete(&fixture.database.connection).await);
    let path = fixture.database.path.clone();
    let node = fixture.database.node;
    fixture.database.close().await.unwrap();
    fixture.database = Database::open(&path, node, None).await.unwrap();
    assert!(complete(&fixture.database.connection).await);
    assert!(backups(&path).is_empty());
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn upgrades_missing_tool_default() {
    let mut fixture = Fixture::new().await;
    fixture
        .database
        .connection
        .execute("ALTER TABLE node DROP COLUMN tool_defaults", ())
        .await
        .unwrap();
    let path = fixture.database.path.clone();
    let node = fixture.database.node;
    fixture.database.close().await.unwrap();
    fixture.database = Database::open(&path, node, None).await.unwrap();
    assert!(complete(&fixture.database.connection).await);
    assert_eq!(backups(&path).len(), 1);
    fixture.database.close().await.unwrap();
}
