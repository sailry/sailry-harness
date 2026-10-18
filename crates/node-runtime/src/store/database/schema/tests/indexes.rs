use super::*;
use crate::store::plugins::storage::search;
use sailry_protocol::plugin::storage::Index;

type Record = (String, String, i64, Vec<u8>, Vec<u8>);

fn seed(db: &Connection, name: &str) -> String {
    let index = Index {
        fields: ["AI向量数据库".into(), "我们使用向量数据库".into()],
        tags: vec!["active".into()],
        order: 7,
        source: None,
    };
    db.execute(
        "INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES(?1,'entry',9,?2,?3)",
        rusqlite::params![
            name,
            br#"{"original":"untouched"}"#.as_slice(),
            serde_json::to_vec(&index).unwrap()
        ],
    )
    .unwrap();
    let definition = search::definitions(db)
        .unwrap()
        .into_iter()
        .find(|sql| sql.contains(&blake3::hash(name.as_bytes()).to_hex().to_string()))
        .unwrap()
        .replace(",cjk0,cjk1", "")
        .replace("unicode61 remove_diacritics 2", "ascii");
    db.execute_batch(&definition).unwrap();
    let table = format!("plugin_search_{}", blake3::hash(name.as_bytes()).to_hex());
    db.execute(
        &format!("INSERT INTO {table}(rowid,field0,field1,key,tags,ordinal) SELECT rowid,?2,?3,key,?4,?5 FROM plugin_values WHERE name=?1"),
        rusqlite::params![name, index.fields[0], index.fields[1], serde_json::to_string(&index.tags).unwrap(), index.order],
    )
    .unwrap();
    table
}

fn records(db: &Connection) -> Vec<Record> {
    db.prepare("SELECT name,key,revision,value,index_data FROM plugin_values ORDER BY name,key")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn matches(db: &Connection, term: &str) -> Vec<String> {
    let table = format!("plugin_search_{}", blake3::hash(b"example").to_hex());
    db.prepare(&format!(
        "SELECT key FROM {table} WHERE {table} MATCH ?1 ORDER BY key"
    ))
    .unwrap()
    .query_map([crate::knowledge::text::query(term)], |row| row.get(0))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[test]
fn rebuilds_without_rewriting_records() {
    let (directory, path, node) = startup::fixture("");
    let db = Connection::open(&path).unwrap();
    let table = seed(&db, "example");
    let original = records(&db);
    let previous = objects(&db).unwrap();
    assert!(verify(&db, &path, DDL).is_err());
    db.close().unwrap();
    let db = startup::upgraded(directory.path(), &path, node);
    assert_eq!(records(&db), original);
    verify(&db, &path, DDL).unwrap();
    search::validate(&db).unwrap();
    for term in ["向量", "量数", "库", "AI向量"] {
        assert_eq!(matches(&db, term), ["entry"]);
    }
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let backups = directory.path().join("backups");
    let saved = Connection::open_with_flags(
        backups.join(&startup::entries(&backups)[0]),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(records(&saved), original);
    assert_eq!(objects(&saved).unwrap(), previous);
    assert!(objects(&saved).unwrap()[&("table".into(), table)].contains("ascii"));
}

#[test]
fn rejects_malformed_source_before_backup() {
    let (directory, path, node) = startup::fixture("");
    let db = Connection::open(&path).unwrap();
    seed(&db, "example");
    db.execute(
        "UPDATE plugin_values SET index_data=?1",
        [br#"{"fields":["private-index-marker""#.as_slice()],
    )
    .unwrap();
    db.close().unwrap();
    let before = std::fs::read(&path).unwrap();
    let files = startup::entries(directory.path());
    let error = Database::open(&path, node, None).err().unwrap().to_string();
    assert!(
        error.contains("invalid stored JSON in plugin_values.index_data, record 1"),
        "{error}"
    );
    assert!(error.contains("line 1"), "{error}");
    assert!(error.contains("existing data was not changed"), "{error}");
    assert!(!error.contains("private-index-marker"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(startup::entries(directory.path()), files);
}

#[test]
fn rejects_unowned_objects_during_rebuild() {
    for replace in [false, true] {
        let (directory, path, node) = startup::fixture("");
        let db = Connection::open(&path).unwrap();
        let table = seed(&db, "example");
        let detail = if replace {
            db.execute_batch(&format!("DROP TABLE {table}; CREATE TABLE {table}(value TEXT); INSERT INTO {table} VALUES('kept')")).unwrap();
            "not an owned FTS5 index"
        } else {
            db.execute_batch(
                "CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('kept')",
            )
            .unwrap();
            "retained"
        };
        db.close().unwrap();
        startup::rejected(directory.path(), &path, node, detail);
    }
}

#[test]
fn rolls_back_failed_upgrade_transaction() {
    let (directory, path, node) = startup::fixture("");
    let mut db = Connection::open(&path).unwrap();
    seed(&db, "example");
    seed(&db, "later");
    let schema = prepare(&db, &path, node).unwrap();
    assert!(matches!(&schema.0, State::Upgrade { .. }));
    db.execute(
        "UPDATE plugin_values SET index_data=?1 WHERE name='later'",
        [b"{".as_slice()],
    )
    .unwrap();
    let before = objects(&db).unwrap();
    let original = records(&db);
    {
        let transaction = db.transaction().unwrap();
        let error = schema.initialize(&transaction).unwrap_err().to_string();
        assert!(error.contains("plugin_values.index_data"), "{error}");
        assert_ne!(objects(&transaction).unwrap(), before);
        transaction.rollback().unwrap();
    }
    assert_eq!(objects(&db).unwrap(), before);
    assert_eq!(records(&db), original);
    assert_eq!(startup::entries(&directory.path().join("backups")).len(), 1);
}

#[test]
fn uses_session_lookup_and_preserves_records() {
    let (directory, path, node) = startup::fixture("DROP INDEX turns_session");
    let original = Connection::open(&path).unwrap();
    let plan = |db: &Connection| -> Vec<String> {
        db.prepare("EXPLAIN QUERY PLAN SELECT id FROM turns WHERE session=?1 ORDER BY rowid")
            .unwrap()
            .query_map(["session"], |row| row.get(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(
        plan(&original)
            .iter()
            .any(|line| line.contains("SCAN turns"))
    );
    let before = original
        .query_row("SELECT body FROM requests", [], |row| {
            row.get::<_, Vec<u8>>(0)
        })
        .unwrap();
    original.close().unwrap();
    let upgraded = startup::upgraded(directory.path(), &path, node);
    assert!(
        plan(&upgraded)
            .iter()
            .any(|line| line.contains("USING INDEX turns_session"))
    );
    assert_eq!(
        upgraded
            .query_row("SELECT body FROM requests", [], |row| row
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        before
    );
    let backups = directory.path().join("backups");
    let saved = Connection::open_with_flags(
        backups.join(&startup::entries(&backups)[0]),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert!(plan(&saved).iter().any(|line| line.contains("SCAN turns")));
    assert_eq!(
        saved
            .query_row("SELECT body FROM requests", [], |row| row
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        before
    );
    assert_eq!(
        upgraded
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn rolls_back_session_index_on_validation_failure() {
    let (directory, path, node) = startup::fixture("DROP INDEX turns_session");
    let mut db = Connection::open(&path).unwrap();
    let schema = prepare(&db, &path, node).unwrap();
    db.execute(
        "UPDATE requests SET status='completed', result=?1",
        [b"{".as_slice()],
    )
    .unwrap();
    let before = objects(&db).unwrap();
    {
        let transaction = db.transaction().unwrap();
        assert!(schema.initialize(&transaction).is_err());
        assert!(
            objects(&transaction)
                .unwrap()
                .contains_key(&("index".into(), "turns_session".into()))
        );
        transaction.rollback().unwrap();
    }
    assert_eq!(objects(&db).unwrap(), before);
    assert_eq!(
        db.query_row("SELECT result FROM requests", [], |row| row
            .get::<_, Vec<u8>>(0))
            .unwrap(),
        b"{"
    );
    assert_eq!(startup::entries(&directory.path().join("backups")).len(), 1);
}
