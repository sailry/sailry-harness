use super::*;
use crate::store::database::{connect, engine};

#[tokio::test]
async fn visits_each_owned_session_once() {
    let directory = tempfile::tempdir().unwrap();
    let database = engine(&directory.path().join("graph.turso")).await.unwrap();
    let connection = connect(&database).await.unwrap();
    connection
        .execute_batch(
            "CREATE TABLE turns(id TEXT PRIMARY KEY,session TEXT NOT NULL); \
         CREATE TABLE agent_delegations(turn TEXT NOT NULL,parent_turn TEXT NOT NULL)",
        )
        .await
        .unwrap();
    let root = SessionId::new();
    let child = SessionId::new();
    let grandchild = SessionId::new();
    let unrelated = SessionId::new();
    for (turn, session) in [
        ("root", root),
        ("child", child),
        ("duplicate", child),
        ("grandchild", grandchild),
        ("cycle", root),
        ("unrelated", unrelated),
    ] {
        connection
            .execute(
                "INSERT INTO turns VALUES(?1,?2)",
                (turn, session.to_string()),
            )
            .await
            .unwrap();
    }
    for (turn, parent) in [
        ("child", "root"),
        ("duplicate", "root"),
        ("grandchild", "child"),
        ("cycle", "grandchild"),
        ("missing", "root"),
    ] {
        connection
            .execute(
                "INSERT INTO agent_delegations VALUES(?1,?2)",
                (turn, parent),
            )
            .await
            .unwrap();
    }
    let ids = subtree(&connection, root).await.unwrap();
    assert_eq!(ids, vec![root, child, grandchild]);
    assert!(!ids.contains(&unrelated));
}

#[tokio::test]
async fn uses_the_callers_transaction_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let database = engine(&directory.path().join("graph.turso")).await.unwrap();
    let writer = connect(&database).await.unwrap();
    let reader = connect(&database).await.unwrap();
    writer
        .execute_batch(
            "CREATE TABLE turns(id TEXT PRIMARY KEY,session TEXT NOT NULL); \
         CREATE TABLE agent_delegations(turn TEXT NOT NULL,parent_turn TEXT NOT NULL)",
        )
        .await
        .unwrap();
    let root = SessionId::new();
    let child = SessionId::new();
    writer
        .execute("INSERT INTO turns VALUES('root',?1)", [root.to_string()])
        .await
        .unwrap();
    reader.execute("BEGIN DEFERRED", ()).await.unwrap();
    assert_eq!(subtree(&reader, root).await.unwrap(), vec![root]);
    writer.execute("BEGIN CONCURRENT", ()).await.unwrap();
    writer
        .execute("INSERT INTO turns VALUES('child',?1)", [child.to_string()])
        .await
        .unwrap();
    writer
        .execute("INSERT INTO agent_delegations VALUES('child','root')", ())
        .await
        .unwrap();
    writer.execute("COMMIT", ()).await.unwrap();
    assert_eq!(subtree(&reader, root).await.unwrap(), vec![root]);
    reader.execute("COMMIT", ()).await.unwrap();
    assert_eq!(subtree(&reader, root).await.unwrap(), vec![root, child]);
}
