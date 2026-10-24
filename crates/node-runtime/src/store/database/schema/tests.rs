use super::*;

mod definitions;
mod indexes;
mod references;
mod search;

async fn fixture(ddl: &str) -> (tempfile::TempDir, turso::Database, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let database = super::super::engine(&directory.path().join("node.turso"))
        .await
        .unwrap();
    let connection = super::super::connect(&database).await.unwrap();
    connection.execute_batch(ddl).await.unwrap();
    (directory, database, connection)
}

const CURRENT: &str = "CREATE TABLE records(id INTEGER PRIMARY KEY,value TEXT DEFAULT 'Kept'); CREATE INDEX records_value ON records(value)";

#[tokio::test]
async fn accepts_matching_definitions() {
    let (_directory, _database, connection) = fixture("create table records(id integer primary key, /* value */ value text default 'Kept'); create index records_value on records(value)").await;
    let expected = expected(CURRENT, &[]).await.unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
}

#[tokio::test]
async fn rejects_changed_columns_and_constraints() {
    let expected = expected(CURRENT, &[]).await.unwrap();
    for columns in [
        "id INTEGER PRIMARY KEY",
        "id INTEGER PRIMARY KEY,value TEXT DEFAULT 'Changed'",
        "id INTEGER PRIMARY KEY,value INTEGER DEFAULT 'Kept'",
        "id INTEGER PRIMARY KEY,value TEXT DEFAULT 'Kept',extra TEXT",
        "id INTEGER PRIMARY KEY,value TEXT DEFAULT 'Kept',UNIQUE(value)",
    ] {
        let index_column = if columns.contains("value") {
            "value"
        } else {
            "id"
        };
        let (_directory, _database, connection) = fixture(&format!(
            "CREATE TABLE records({columns}); CREATE INDEX records_value ON records({index_column})"
        ))
        .await;
        let before = objects(&connection).await.unwrap();
        assert!(
            verify_objects(&connection, Path::new("fixture"), &expected)
                .await
                .is_err()
        );
        assert_eq!(objects(&connection).await.unwrap(), before);
    }
}

#[tokio::test]
async fn reports_altered_fields_before_constraints() {
    for (ddl, alteration, detail) in [
        (
            "CREATE TABLE records(id TEXT PRIMARY KEY REFERENCES records(id),value BLOB)",
            "ALTER TABLE records ADD COLUMN unused BLOB",
            "records.unused",
        ),
        (
            "CREATE TABLE records(name TEXT NOT NULL,key TEXT NOT NULL,index_data BLOB,PRIMARY KEY(name,key))",
            "ALTER TABLE records DROP COLUMN index_data; ALTER TABLE records ADD COLUMN index_data TEXT",
            "records.index_data",
        ),
    ] {
        let (_directory, _database, connection) = fixture(ddl).await;
        let expected = objects(&connection).await.unwrap();
        connection.execute_batch(alteration).await.unwrap();
        let before = objects(&connection).await.unwrap();
        let error = plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(detail), "{error}");
        assert!(error.contains("existing data was not changed"), "{error}");
        assert_eq!(objects(&connection).await.unwrap(), before);
    }
}

#[tokio::test]
async fn rejects_constraint_changes_without_writes() {
    let expected = expected(CURRENT, &[]).await.unwrap();
    let (_directory, _database, connection) = fixture(
        "CREATE TABLE records(id INTEGER PRIMARY KEY,value TEXT DEFAULT 'Kept',UNIQUE(value)); CREATE INDEX records_value ON records(value)",
    )
    .await;
    let before = objects(&connection).await.unwrap();
    let error = plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("current constraints"), "{error}");
    assert!(error.contains("existing data was not changed"), "{error}");
    assert_eq!(objects(&connection).await.unwrap(), before);
}

#[tokio::test]
async fn rejects_unowned_objects() {
    let expected = expected(CURRENT, &[]).await.unwrap();
    for ddl in [
        "CREATE TABLE unowned(value TEXT)",
        "CREATE INDEX unowned ON records(id)",
        "CREATE VIEW unowned AS SELECT value FROM records",
    ] {
        let (_directory, _database, connection) = fixture(CURRENT).await;
        connection.execute_batch(ddl).await.unwrap();
        let before = objects(&connection).await.unwrap();
        assert!(
            verify_objects(&connection, Path::new("fixture"), &expected)
                .await
                .is_err()
        );
        assert_eq!(objects(&connection).await.unwrap(), before);
    }
}

#[tokio::test]
async fn adds_current_defaults_in_one_transaction() {
    let (_directory, _database, connection) = fixture(
        "CREATE TABLE agent_runs(id INTEGER PRIMARY KEY); INSERT INTO agent_runs VALUES(1)",
    )
    .await;
    let current = "CREATE TABLE agent_runs(id INTEGER PRIMARY KEY,hooks BLOB NOT NULL)";
    let expected = expected(current, &[]).await.unwrap();
    let plan = plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
        .await
        .unwrap();
    assert!(!plan.is_empty());
    connection.execute("BEGIN", ()).await.unwrap();
    plan.apply(&connection).await.unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
    let hooks = connection
        .prepare("SELECT hooks FROM agent_runs WHERE id=1")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<Vec<u8>>(0)
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<sailry_protocol::hooks::Settings>(&hooks).unwrap(),
        sailry_protocol::hooks::Settings::default()
    );
    connection.execute("ROLLBACK", ()).await.unwrap();
    assert_eq!(
        connection
            .prepare("SELECT count(*) FROM pragma_table_info('agent_runs') WHERE name='hooks'")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn refuses_missing_required_defaults() {
    let (_directory, _database, connection) =
        fixture("CREATE TABLE records(id INTEGER PRIMARY KEY); INSERT INTO records VALUES(1)")
            .await;
    let expected = expected(
        "CREATE TABLE records(id INTEGER PRIMARY KEY,value TEXT NOT NULL)",
        &[],
    )
    .await
    .unwrap();
    assert!(
        plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
            .await
            .unwrap_err()
            .to_string()
            .contains("requires a default")
    );
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
        1
    );
}

#[test]
fn rejects_unowned_native_metadata() {
    let expected = Objects::new();
    let mut actual = Objects::from([(
        ("table".into(), "__turso_internal_foreign".into()),
        "CREATE TABLE __turso_internal_foreign(value BLOB)".into(),
    )]);
    check_backing(&mut actual, &expected, Path::new("fixture")).unwrap();
    assert!(actual.contains_key(&("table".into(), "__turso_internal_foreign".into())));
}

#[tokio::test]
async fn keeps_contract_versions_at_one() {
    let (_directory, _database, connection) = fixture(DDL).await;
    connection
        .pragma_update("application_id", super::super::APPLICATION_ID)
        .await
        .unwrap();
    connection.pragma_update("user_version", 1).await.unwrap();
    connection
        .execute(
            "INSERT INTO node(singleton,identity) VALUES(1,?1)",
            [NodeId([1; 32]).0.to_vec()],
        )
        .await
        .unwrap();
    assert!(!ownership(&connection, Path::new("fixture")).await.unwrap());
    identity(&connection, Path::new("fixture"), NodeId([1; 32]))
        .await
        .unwrap();
    for (pragma, value) in [("user_version", 2), ("application_id", 1)] {
        connection.pragma_update(pragma, value).await.unwrap();
        assert!(ownership(&connection, Path::new("fixture")).await.is_err());
    }
}
