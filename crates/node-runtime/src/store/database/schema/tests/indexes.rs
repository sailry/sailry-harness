use super::*;

#[tokio::test]
async fn accepts_empty_and_materialized_native_backing() {
    let ddl = "CREATE TABLE corpus(id INTEGER PRIMARY KEY,body TEXT NOT NULL); CREATE INDEX corpus_text ON corpus USING fts(body)";
    let (_directory, _database, connection) = fixture(ddl).await;
    let expected = expected(ddl, &[]).await.unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
    connection
        .execute("INSERT INTO corpus VALUES(1,'alpha')", ())
        .await
        .unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
    connection.execute("DELETE FROM corpus", ()).await.unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
}

#[tokio::test]
async fn restores_missing_indexes_transactionally() {
    let (_directory, _database, connection) = fixture(CURRENT).await;
    let expected = expected(CURRENT, &[]).await.unwrap();
    connection
        .execute("DROP INDEX records_value", ())
        .await
        .unwrap();
    let plan = plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
        .await
        .unwrap();
    connection.execute("BEGIN", ()).await.unwrap();
    plan.apply(&connection).await.unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
    connection.execute("ROLLBACK", ()).await.unwrap();
    assert!(
        verify_objects(&connection, Path::new("fixture"), &expected)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn recognizes_declared_native_refresh() {
    let ddl = format!("{CURRENT}; CREATE TABLE other(id INTEGER PRIMARY KEY,body TEXT NOT NULL)");
    let definitions = vec![
        "CREATE TABLE corpus(id INTEGER PRIMARY KEY,body TEXT NOT NULL,extra TEXT NOT NULL)"
            .to_owned(),
        "CREATE INDEX corpus_text ON corpus USING fts(body,extra) WITH(tokenizer='whitespace')"
            .to_owned(),
    ];
    let expected = expected(&ddl, &definitions).await.unwrap();
    for (index, change, declared, accepted) in [
        (
            "CREATE INDEX corpus_text ON corpus USING fts(body)",
            "",
            true,
            true,
        ),
        (
            "CREATE INDEX corpus_text ON corpus USING fts(body)",
            "",
            false,
            false,
        ),
        (
            "CREATE INDEX corpus_text ON other USING fts(body)",
            "",
            true,
            false,
        ),
        ("CREATE INDEX corpus_text ON corpus(body)", "", true, false),
        (
            "CREATE INDEX corpus_text ON corpus USING fts(body)",
            "DROP INDEX records_value; CREATE INDEX records_value ON records(id)",
            true,
            false,
        ),
        (
            "CREATE INDEX corpus_text ON corpus USING fts(body)",
            "CREATE INDEX unowned_text ON corpus USING fts(body)",
            true,
            false,
        ),
    ] {
        let (_directory, _database, connection) =
            fixture(&format!("{ddl}; {}; {index}", definitions[0])).await;
        if !change.is_empty() {
            connection.execute_batch(change).await.unwrap();
        }
        let before = objects(&connection).await.unwrap();
        let plan = plan::Plan::prepare(
            &connection,
            Path::new("fixture"),
            &expected,
            if declared { &definitions } else { &[] },
        )
        .await;
        if accepted {
            assert!(!plan.unwrap().is_empty());
        } else {
            assert!(plan.is_err(), "{index}; {change}");
        }
        assert_eq!(objects(&connection).await.unwrap(), before);
    }
}
