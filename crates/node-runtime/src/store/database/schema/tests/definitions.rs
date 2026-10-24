use super::*;

const CURRENT: &str = "CREATE TABLE parents(id INTEGER PRIMARY KEY,other INTEGER UNIQUE,UNIQUE(id,other)); CREATE TABLE records(id INTEGER PRIMARY KEY,parent INTEGER REFERENCES parents(id) ON DELETE CASCADE,value TEXT NOT NULL DEFAULT 'Kept' CHECK(length(value)>0),extra BLOB)";

#[tokio::test]
async fn preserves_altered_foreign_keys() {
    let (_directory, _database, connection) = fixture(CURRENT).await;
    connection
        .execute_batch("INSERT INTO parents(id) VALUES(1); INSERT INTO records(id,parent) VALUES(1,1); ALTER TABLE records DROP COLUMN extra")
        .await
        .unwrap();
    let before = objects(&connection).await.unwrap();
    let expected = expected(CURRENT, &[]).await.unwrap();
    let plan = plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
        .await
        .unwrap();
    assert!(plan.adds("records", "extra"));
    assert!(!plan.adds("database_profiles", "password_target"));
    assert_eq!(objects(&connection).await.unwrap(), before);
    connection.execute("BEGIN", ()).await.unwrap();
    plan.apply(&connection).await.unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
    audit_references(&connection, "records").await.unwrap();
    connection
        .execute("DELETE FROM parents WHERE id=1", ())
        .await
        .unwrap();
    assert_eq!(
        connection
            .prepare("SELECT count(*) FROM records")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        0
    );
    connection.execute("ROLLBACK", ()).await.unwrap();
    assert_eq!(objects(&connection).await.unwrap(), before);
    assert_eq!(
        connection
            .prepare("SELECT parent,value FROM records WHERE id=1")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| Ok((row.get::<i64>(0)?, row.get::<String>(1)?)))
            .unwrap(),
        (1, "Kept".to_owned())
    );
}

#[tokio::test]
async fn rejects_changed_constraints_after_normalization() {
    let expected = expected(CURRENT, &[]).await.unwrap();
    for (original, changed) in [
        (" REFERENCES parents(id) ON DELETE CASCADE", ""),
        ("REFERENCES parents(id)", "REFERENCES parents(other)"),
        ("ON DELETE CASCADE", "ON DELETE SET NULL"),
        ("id INTEGER PRIMARY KEY,parent", "id INTEGER,parent"),
        ("DEFAULT 'Kept'", "DEFAULT 'Changed'"),
        ("length(value)>0", "length(value)>=0"),
        (
            ",extra BLOB)",
            ",extra BLOB,FOREIGN KEY(id,parent) REFERENCES parents(id,other))",
        ),
    ] {
        let (_directory, _database, connection) =
            fixture(&CURRENT.replace(original, changed)).await;
        let before = objects(&connection).await.unwrap();
        assert!(
            verify_objects(&connection, Path::new("fixture"), &expected)
                .await
                .is_err(),
            "{changed}"
        );
        assert!(
            plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
                .await
                .is_err(),
            "{changed}"
        );
        assert_eq!(objects(&connection).await.unwrap(), before);
    }
}
