use super::*;

#[tokio::test]
async fn groups_composite_keys_and_skips_nulls() {
    let (_directory, _database, connection) = fixture("CREATE TABLE parents(a INTEGER,b TEXT,PRIMARY KEY(a,b)); CREATE TABLE children(a INTEGER,b TEXT,FOREIGN KEY(a,b) REFERENCES parents(a,b)); INSERT INTO parents VALUES(1,'one'); INSERT INTO children VALUES(1,'one'),(NULL,'missing'),(2,NULL)").await;
    audit_references(&connection, "children").await.unwrap();
    connection
        .pragma_update("foreign_keys", "OFF")
        .await
        .unwrap();
    connection
        .execute("INSERT INTO children VALUES(1,'missing')", ())
        .await
        .unwrap();
    let error = audit_references(&connection, "children")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("invalid stored reference in children"),
        "{error}"
    );
    assert!(!error.contains("missing"), "{error}");
}

#[tokio::test]
async fn checks_cascading_relationships() {
    let (_directory, _database, connection) = fixture("CREATE TABLE parents(id INTEGER PRIMARY KEY); CREATE TABLE children(id INTEGER PRIMARY KEY,parent INTEGER NOT NULL REFERENCES parents(id) ON DELETE CASCADE); INSERT INTO parents VALUES(1); INSERT INTO children VALUES(1,1)").await;
    connection
        .execute("DELETE FROM parents WHERE id=1", ())
        .await
        .unwrap();
    audit_references(&connection, "children").await.unwrap();
    assert_eq!(
        connection
            .prepare("SELECT count(*) FROM children")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        0
    );
    connection
        .pragma_update("foreign_keys", "OFF")
        .await
        .unwrap();
    connection
        .execute("INSERT INTO children VALUES(2,2)", ())
        .await
        .unwrap();
    assert!(audit_references(&connection, "children").await.is_err());
}

#[tokio::test]
async fn covers_every_owned_parent() {
    let (_directory, _database, connection) = fixture(DDL).await;
    references(&connection).await.unwrap();
    let names = definition::table_names(DDL).unwrap();
    assert!(names.iter().any(|name| name == "turns"));
    assert!(names.iter().any(|name| name == "agent_pending"));
    connection
        .pragma_update("foreign_keys", "OFF")
        .await
        .unwrap();
    connection.execute("INSERT INTO agent_pending(turn,position,revision,request,caller) VALUES('turn',1,1,'request',X'01')", ()).await.unwrap();
    let error = references(&connection).await.unwrap_err().to_string();
    assert!(error.contains("agent_pending"), "{error}");
}
