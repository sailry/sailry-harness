use super::*;
use crate::store::database::Database;

mod indexes;

mod comparison {
    use super::*;

    const CURRENT: &str = "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT DEFAULT 'Kept'); CREATE INDEX records_value ON records(value);";

    #[test]
    fn accepts_matching_definitions() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("create table records (id integer primary key, /* value */ value text default 'Kept'); create index records_value on records (value);").unwrap();
        verify(&connection, Path::new("fixture"), CURRENT).unwrap();
    }

    #[test]
    fn accepts_typed_hook_defaults() {
        let current = "CREATE TABLE agent_runs(hooks BLOB NOT NULL)";
        for value in [
            br#"{"revision":0,"enabled":false,"before_tool":null,"after_tool":null,"turn_end":null}"#.to_vec(),
            serde_json::to_vec(&sailry_protocol::hooks::Settings::default()).unwrap(),
        ] {
            let connection = Connection::open_in_memory().unwrap();
            let encoded = value
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>();
            connection
                .execute_batch(&format!(
                    "CREATE TABLE agent_runs(hooks BLOB NOT NULL DEFAULT X'{encoded}')"
                ))
                .unwrap();
            let before = objects(&connection).unwrap();
            verify(&connection, Path::new("fixture"), current).unwrap();
            assert_eq!(objects(&connection).unwrap(), before);
        }
    }

    #[test]
    fn rejects_missing_objects() {
        for sql in [
            "",
            "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT DEFAULT 'Kept')",
        ] {
            let connection = Connection::open_in_memory().unwrap();
            connection.execute_batch(sql).unwrap();
            let before = objects(&connection).unwrap();
            assert!(verify(&connection, Path::new("fixture"), CURRENT).is_err());
            assert_eq!(objects(&connection).unwrap(), before);
        }
    }

    #[test]
    fn rejects_column_changes() {
        for columns in [
            "id INTEGER PRIMARY KEY",
            "id INTEGER PRIMARY KEY, value TEXT DEFAULT 'Kept', unused TEXT",
            "id INTEGER PRIMARY KEY, value INTEGER DEFAULT 'Kept'",
            "id INTEGER PRIMARY KEY, value TEXT NOT NULL DEFAULT 'Kept'",
            "id INTEGER PRIMARY KEY, value TEXT DEFAULT 'Changed'",
        ] {
            let connection = Connection::open_in_memory().unwrap();
            connection
                .execute_batch(&format!("CREATE TABLE records({columns})"))
                .unwrap();
            let before = objects(&connection).unwrap();
            assert!(verify(&connection, Path::new("fixture"), CURRENT).is_err());
            assert_eq!(objects(&connection).unwrap(), before);
        }
    }

    #[test]
    fn rejects_constraint_changes() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT DEFAULT 'Kept', UNIQUE(value)); CREATE INDEX records_value ON records(value);").unwrap();
        let before = objects(&connection).unwrap();
        assert!(verify(&connection, Path::new("fixture"), CURRENT).is_err());
        assert_eq!(objects(&connection).unwrap(), before);
    }

    #[test]
    fn rejects_table_options() {
        for options in ["STRICT", "WITHOUT ROWID"] {
            let connection = Connection::open_in_memory().unwrap();
            connection.execute_batch(&format!("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT DEFAULT 'Kept') {options}; CREATE INDEX records_value ON records(value);")).unwrap();
            let before = objects(&connection).unwrap();
            assert!(verify(&connection, Path::new("fixture"), CURRENT).is_err());
            assert_eq!(objects(&connection).unwrap(), before);
        }
    }

    #[test]
    fn rejects_index_changes() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT DEFAULT 'Kept'); CREATE INDEX records_value ON records(id);").unwrap();
        let before = objects(&connection).unwrap();
        assert!(verify(&connection, Path::new("fixture"), CURRENT).is_err());
        assert_eq!(objects(&connection).unwrap(), before);
    }

    #[test]
    fn rejects_unexpected_objects() {
        for sql in [
            "CREATE TABLE retained(value TEXT)",
            "CREATE TABLE sqlitex_retained(value TEXT)",
            "CREATE INDEX retained ON records(id)",
            "CREATE VIEW retained AS SELECT value FROM records",
            "CREATE TRIGGER retained AFTER INSERT ON records BEGIN DELETE FROM records; END",
        ] {
            let connection = Connection::open_in_memory().unwrap();
            connection.execute_batch(CURRENT).unwrap();
            connection.execute_batch(sql).unwrap();
            let before = objects(&connection).unwrap();
            assert!(verify(&connection, Path::new("fixture"), CURRENT).is_err());
            assert_eq!(objects(&connection).unwrap(), before);
        }
    }

    #[test]
    fn rejects_trigger_matching_index_name() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(CURRENT).unwrap();
        connection.execute_batch("DROP INDEX records_value; CREATE TRIGGER records_value AFTER INSERT ON records BEGIN DELETE FROM records; END; CREATE INDEX records_value ON records(value);").unwrap();
        let before = objects(&connection).unwrap();
        assert_eq!(before.len(), 3);
        let error = verify(&connection, Path::new("fixture"), CURRENT).unwrap_err();
        assert!(error.to_string().contains("records_value\" (trigger)"));
        assert_eq!(objects(&connection).unwrap(), before);
    }
}

mod corpus_indexes {
    use super::*;
    use crate::store::plugins::storage::search;

    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(DDL).unwrap();
        db.execute(
            "INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES('example','entry',1,?1,?2)",
            rusqlite::params![
                b"true".as_slice(),
                br#"{"fields":["title","body"],"tags":[],"order":1}"#.as_slice()
            ],
        )
        .unwrap();
        db
    }

    fn create(db: &Connection) {
        for definition in search::definitions(db).unwrap() {
            db.execute_batch(&definition).unwrap();
        }
    }

    fn unchanged(db: &Connection, accepts: bool) {
        let before = objects(db).unwrap();
        let changes = db.total_changes();
        assert_eq!(verify(db, Path::new("fixture"), DDL).is_ok(), accepts);
        assert_eq!(objects(db).unwrap(), before);
        assert_eq!(db.total_changes(), changes);
    }

    #[test]
    fn accepts_registered_and_empty_indexes() {
        let db = fixture();
        create(&db);
        unchanged(&db, true);
        db.execute("UPDATE plugin_values SET value=NULL,index_data=NULL", [])
            .unwrap();
        unchanged(&db, true);
    }

    #[test]
    fn rejects_missing_index() {
        unchanged(&fixture(), false);
    }

    #[test]
    fn rejects_orphaned_namespaces() {
        let db = fixture();
        create(&db);
        db.execute("DELETE FROM plugin_values", []).unwrap();
        unchanged(&db, false);
    }

    #[test]
    fn rejects_changed_definitions() {
        let db = fixture();
        let definition = search::definitions(&db)
            .unwrap()
            .remove(0)
            .replace("field1", "other");
        db.execute_batch(&definition).unwrap();
        unchanged(&db, false);
    }

    #[test]
    fn rejects_missing_shadow_tables() {
        let db = fixture();
        create(&db);
        let name = format!("plugin_search_{}_config", blake3::hash(b"example").to_hex());
        db.execute_batch(&format!("DROP TABLE {name}")).unwrap();
        unchanged(&db, false);
    }
}

mod declarations {
    use super::*;

    #[test]
    fn handles_commas_comments_and_literals() {
        let definition = Table::parse("CREATE TABLE records(id INTEGER PRIMARY KEY, -- current key\n value TEXT DEFAULT 'a,b--c' CHECK(value IN ('a,b--c','it''s fine')), note TEXT /* unused, */ DEFAULT '/*literal*/', UNIQUE(value,note))").unwrap();
        assert_eq!(definition.columns.len(), 3);
        assert_eq!(definition.constraints.len(), 1);
        assert!(definition.columns["value"].contains("'a,b--c'"));
        assert_eq!(
            normalize("value text DEFAULT 'Keep Case'"),
            "VALUE TEXT DEFAULT'Keep Case'"
        );
        assert_ne!(normalize("value INT EGER"), normalize("value INTEGER"));
        assert_ne!(
            normalize("value TEXT DEFAULT 'A'"),
            normalize("value TEXT DEFAULT 'a'")
        );
    }

    #[test]
    fn inspects_all_current_tables() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(DDL).unwrap();
        for ((kind, _), sql) in objects(&connection).unwrap() {
            if kind == "table" {
                assert!(!Table::parse(&sql).unwrap().columns.is_empty());
            }
        }
        verify(&connection, Path::new("fixture"), DDL).unwrap();
    }
}

mod startup {
    use super::*;
    use sailry_protocol::RequestId;
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn adds_missing_tables() {
        for table in [
            "agent_continuations",
            "plugin_conversation_records",
            "plugin_conversation_history",
            "plugin_conversation_values",
            "plugin_values",
            "dispatch_jobs",
            "model_catalog_status",
            "sandbox_settings",
            "search_settings",
        ] {
            let (directory, path, node) = fixture(&format!("DROP TABLE {table}"));
            let connection = upgraded(directory.path(), &path, node);
            assert!(
                connection
                    .prepare(&format!("SELECT * FROM {table}"))
                    .is_ok()
            );
        }
    }

    #[test]
    fn adds_missing_columns() {
        for (table, column) in [
            ("agent_runs", "provider"),
            ("agent_runs", "hooks"),
            ("plugin_values", "index_data"),
            ("sessions", "state"),
            ("agent_approvals", "prepared"),
        ] {
            let (directory, path, node) =
                fixture(&format!("ALTER TABLE {table} DROP COLUMN {column}"));
            let connection = upgraded(directory.path(), &path, node);
            assert!(
                connection
                    .prepare(&format!("SELECT {column} FROM {table}"))
                    .is_ok()
            );
        }
    }

    #[test]
    fn preserves_typed_hook_default_in_backup() {
        let value = br#"{"revision":0,"enabled":false,"before_tool":null,"after_tool":null,"turn_end":null}"#;
        let encoded = value
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        let (directory, path, node) = fixture(&format!(
            "ALTER TABLE agent_runs DROP COLUMN hooks; ALTER TABLE agent_runs ADD COLUMN hooks BLOB NOT NULL DEFAULT X'{encoded}'; DROP TABLE code_settings"
        ));
        let original = objects(&Connection::open(&path).unwrap()).unwrap();
        let connection = upgraded(directory.path(), &path, node);
        assert_eq!(
            objects(&connection)
                .unwrap()
                .get(&("table".into(), "agent_runs".into())),
            original.get(&("table".into(), "agent_runs".into()))
        );
        let backups = directory.path().join("backups");
        let previous = Connection::open_with_flags(
            backups.join(&entries(&backups)[0]),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(objects(&previous).unwrap(), original);
    }

    #[test]
    fn rejects_changed_hook_defaults_without_writing() {
        let default = br#"{"revision":0,"enabled":false,"before_tool":null,"after_tool":null,"turn_end":null}"#;
        for (declaration, value) in [
            (
                "hooks BLOB NOT NULL",
                br#"{"revision":0,"enabled":true,"before_tool":null,"after_tool":null,"turn_end":null}"#.as_slice(),
            ),
            ("hooks BLOB NOT NULL", b"{".as_slice()),
            (
                "hooks BLOB NOT NULL",
                br#"{"revision":0,"enabled":false,"before_tool":null,"after_tool":null,"turn_end":null,"foreign":false}"#.as_slice(),
            ),
            ("hooks TEXT NOT NULL", default.as_slice()),
            ("hooks BLOB", default.as_slice()),
            (
                "hooks BLOB NOT NULL CHECK(json_valid(hooks))",
                default.as_slice(),
            ),
        ] {
            let encoded = value
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>();
            let (directory, path, node) = fixture(&format!(
                "ALTER TABLE agent_runs DROP COLUMN hooks; ALTER TABLE agent_runs ADD COLUMN {declaration} DEFAULT X'{encoded}'; DROP TABLE code_settings"
            ));
            rejected(directory.path(), &path, node, "agent_runs.hooks");
        }
    }

    #[test]
    fn index_changes_preserve_profile() {
        let (directory, path, node) = fixture(
            "DROP INDEX dispatch_handlers_source; CREATE INDEX dispatch_handlers_source ON dispatch_handlers(topic,source)",
        );
        rejected(directory.path(), &path, node, "dispatch_handlers_source");
    }

    #[test]
    fn adds_missing_indexes() {
        let (directory, path, node) = fixture("DROP INDEX dispatch_handlers_source");
        let connection = upgraded(directory.path(), &path, node);
        assert!(
            objects(&connection)
                .unwrap()
                .contains_key(&("index".into(), "dispatch_handlers_source".into()))
        );
    }

    #[test]
    fn malformed_json_preserves_diagnostics_and_profile() {
        for source in ["plugins.body", "requests.result"] {
            let (directory, path, node) = fixture("");
            let connection = Connection::open(&path).unwrap();
            let malformed = br#"{"token":"diagnostic-secret-marker""#;
            if source == "plugins.body" {
                connection.execute("INSERT INTO plugins(name,revision,installed,body) VALUES('fixture',1,1,?1)", [malformed.as_slice()]).unwrap();
            } else {
                connection
                    .execute(
                        "UPDATE requests SET result=?1",
                        [std::str::from_utf8(malformed).unwrap()],
                    )
                    .unwrap();
            }
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            let files = entries(directory.path());
            let error = Database::open(&path, node, None).err().unwrap().to_string();
            assert!(
                error.contains(&format!("invalid stored JSON in {source}, record 1")),
                "{error}"
            );
            assert!(error.contains("line 1"), "{error}");
            assert!(error.contains("existing data was not changed"), "{error}");
            assert!(!error.contains("diagnostic-secret-marker"), "{error}");
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(entries(directory.path()), files);
            let connection =
                Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .unwrap();
            assert_eq!(
                connection
                    .query_row("SELECT status FROM requests", [], |row| row
                        .get::<_, String>(0))
                    .unwrap(),
                "admitted"
            );
        }
    }

    #[test]
    fn unexpected_storage_preserves_profile() {
        for (sql, name) in [
            (
                "CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('kept')",
                "retained",
            ),
            (
                "CREATE TABLE sqlitex_retained(value TEXT); INSERT INTO sqlitex_retained VALUES('kept')",
                "sqlitex_retained",
            ),
            (
                "ALTER TABLE agent_runs ADD COLUMN unused BLOB",
                "agent_runs.unused",
            ),
            (
                "CREATE VIEW retained AS SELECT id FROM sessions",
                "retained",
            ),
            (
                "CREATE TRIGGER retained AFTER INSERT ON events BEGIN DELETE FROM events; END",
                "retained",
            ),
        ] {
            let (directory, path, node) = fixture(sql);
            rejected(directory.path(), &path, node, name);
        }
    }

    #[test]
    fn unowned_storage_is_not_fresh() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE sqlitex_retained(value TEXT); INSERT INTO sqlitex_retained VALUES('kept')").unwrap();
        connection.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        let files = entries(directory.path());
        let error = Database::open(&path, NodeId([65; 32]), None)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("application_id is 0x0"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(entries(directory.path()), files);
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT value FROM sqlitex_retained", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "kept"
        );
    }

    pub(super) fn fixture(sql: &str) -> (tempfile::TempDir, PathBuf, NodeId) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let node = NodeId([63; 32]);
        let database = Database::open(&path, node, None).unwrap();
        database
            .connection
            .execute(
                "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                rusqlite::params![
                    &node.0[..],
                    RequestId::new().to_string(),
                    b"kept".as_slice()
                ],
            )
            .unwrap();
        database.connection.execute_batch(sql).unwrap();
        database.close().unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        connection.close().unwrap();
        (directory, path, node)
    }

    pub(super) fn rejected(
        directory: &Path,
        path: &Path,
        node: NodeId,
        detail: &str,
    ) -> Connection {
        let before = std::fs::read(path).unwrap();
        let files = entries(directory);
        let error = Database::open(path, node, None).err().unwrap().to_string();
        assert!(
            error.contains("incompatible development database"),
            "{error}"
        );
        assert!(error.contains(detail), "{error}");
        assert!(error.contains("existing data was not changed"), "{error}");
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert_eq!(entries(directory), files);
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                .unwrap(),
            "delete"
        );
        assert_eq!(
            connection
                .query_row("SELECT status FROM requests", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "admitted"
        );
        assert_eq!(
            connection
                .query_row("SELECT identity FROM node", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            node.0
        );
        connection
    }

    pub(super) fn entries(directory: &Path) -> Vec<OsString> {
        let mut entries = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    pub(super) fn upgraded(directory: &Path, path: &Path, node: NodeId) -> Connection {
        let database = Database::open(path, node, None).unwrap();
        database.close().unwrap();
        let backups = directory.join("backups");
        let files = entries(&backups);
        assert_eq!(files.len(), 1);
        let previous = Connection::open_with_flags(
            backups.join(&files[0]),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            previous
                .query_row("SELECT status FROM requests", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "admitted"
        );
        assert_eq!(
            previous
                .query_row("SELECT body FROM requests", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            b"kept"
        );
        drop(previous);
        let database = Database::open(path, node, None).unwrap();
        database.close().unwrap();
        assert_eq!(entries(&backups), files);
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT identity FROM node", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            node.0
        );
        assert_eq!(
            connection
                .query_row("SELECT status FROM requests", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "unknown"
        );
        assert_eq!(
            connection
                .query_row("SELECT body FROM requests", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            b"kept"
        );
        connection
    }
}
