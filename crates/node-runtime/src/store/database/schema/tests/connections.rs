use super::*;
use rusqlite::params;
use sailry_protocol::RequestId;
use serde_json::{Value, json};

fn record(id: RequestId, connecting: bool) -> Value {
    json!({
        "id":id,"plugin":"fixture","connector":"account","revision":2,"auth_revision":1,
        "method":"secret","enabled":true,"fields":{"label":"Original"},
        "secrets":{"token":"private-original"},
        "grant":{"access":"private-access","refresh":"private-refresh","expires_at_ms":4_102_444_800_000u64,"extra":{}},
        "grant_revision":1,"identity":null,"bound":null,
        "state":if connecting {"connecting"} else {"connected"},"checked_at_ms":10
    })
}

fn insert(path: &Path, id: RequestId, value: &Value) -> Vec<u8> {
    let body = serde_json::to_vec_pretty(value).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute("INSERT INTO plugin_connections(id,plugin,connector,revision,body) VALUES(?1,'fixture','account',2,?2)",params![id.to_string(),body]).unwrap();
    db.close().unwrap();
    body
}

fn stored(db: &Connection, id: RequestId) -> Vec<u8> {
    db.query_row(
        "SELECT body FROM plugin_connections WHERE id=?1",
        [id.to_string()],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn defaults_and_recovers_without_rewriting_history() {
    for connecting in [false, true] {
        let (directory, path, node) = startup::fixture("");
        let id = RequestId::new();
        let original = record(id, connecting);
        let raw = insert(&path, id, &original);
        let db = startup::upgraded(directory.path(), &path, node);
        let mut expected = original;
        expected["login"] = Value::Null;
        if connecting {
            expected["state"] = json!("not_connected");
        }
        let actual: Value = serde_json::from_slice(&stored(&db, id)).unwrap();
        // Existing fields and fence revisions are unchanged, including grants
        // whose old interrupted state cannot justify restoring Connected.
        assert_eq!(actual, expected);
        db.close().unwrap();
        let backups = directory.path().join("backups");
        let files = startup::entries(&backups);
        assert_eq!(files.len(), 1);
        let previous = Connection::open_with_flags(
            backups.join(&files[0]),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(stored(&previous, id), raw);
        previous.close().unwrap();
    }
}

#[test]
fn discards_invalid_baselines_without_changing_accounts() {
    for nested in [false, true] {
        let (directory, path, node) = startup::fixture("");
        let id = RequestId::new();
        let mut previous = record(id, false);
        previous["revision"] = json!(1);
        previous["login"] = Value::Null;
        if nested {
            previous["login"] = json!({"attempt":RequestId::new(),"revision":1,"auth_revision":1,"grant_revision":1,"previous":null});
        } else {
            previous["id"] = json!(RequestId::new());
        }
        let mut value = record(id, true);
        value["login"] = json!({"attempt":RequestId::new(),"revision":2,"auth_revision":1,"grant_revision":1,"previous":previous});
        let raw = insert(&path, id, &value);
        let db = startup::upgraded(directory.path(), &path, node);
        value["login"] = Value::Null;
        value["state"] = json!("not_connected");
        assert_eq!(
            serde_json::from_slice::<Value>(&stored(&db, id)).unwrap(),
            value
        );
        db.close().unwrap();
        let backups = directory.path().join("backups");
        let files = startup::entries(&backups);
        assert_eq!(files.len(), 1);
        let previous = Connection::open_with_flags(
            backups.join(&files[0]),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(stored(&previous, id), raw);
        previous.close().unwrap();
    }
}

#[test]
fn rejects_invalid_accounts_before_backup_or_writes() {
    let (directory, path, node) = startup::fixture("");
    let id = RequestId::new();
    let mut value = record(id, true);
    value["auth_revision"] = json!(0);
    value["login"] = json!({"attempt":RequestId::new(),"revision":2,"auth_revision":1,"grant_revision":1,"previous":null});
    let raw = insert(&path, id, &value);
    let before = std::fs::read(&path).unwrap();
    let files = startup::entries(directory.path());
    let error = Database::open(&path, node, None).err().unwrap().to_string();
    assert!(error.contains("invalid stored connection"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(startup::entries(directory.path()), files);
    let db =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(stored(&db, id), raw);
    assert_eq!(
        db.query_row("SELECT status FROM requests", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "admitted"
    );
}
