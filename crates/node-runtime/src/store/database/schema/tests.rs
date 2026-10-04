use super::*;
use crate::store::database::Database;

mod additions {
    use super::*;

    #[test]
    fn uses_the_latest_ddl_and_preserves_values() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO records VALUES(1,'kept'); CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('retained')").unwrap();
        let ddl = "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT, note TEXT, enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1))); CREATE TABLE added(id INTEGER PRIMARY KEY, value TEXT); CREATE INDEX records_value ON records(value);";
        let plan = super::super::additions(&connection, Path::new("fixture"), ddl).unwrap();
        assert_eq!(plan.statements.len(), 4);
        plan.apply(&connection).unwrap();
        let values: (String, Option<String>, i64) = connection
            .query_row("SELECT value,note,enabled FROM records", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(values, ("kept".into(), None, 1));
        assert_eq!(
            connection
                .query_row("SELECT value FROM retained", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "retained"
        );
        assert!(
            super::super::additions(&connection, Path::new("fixture"), ddl)
                .unwrap()
                .statements
                .is_empty()
        );
    }

    #[test]
    fn keeps_unused_columns() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT, unused TEXT); INSERT INTO records VALUES(1,'kept','original')").unwrap();
        let plan = super::super::additions(
            &connection,
            Path::new("fixture"),
            "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT)",
        )
        .unwrap();
        assert!(plan.statements.is_empty());
        assert_eq!(
            connection
                .query_row("SELECT unused FROM records", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "original"
        );
    }

    #[test]
    fn distinguishes_defaults_from_constraints() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE records(id INTEGER PRIMARY KEY); INSERT INTO records VALUES(1)",
            )
            .unwrap();
        let ddl = "CREATE TABLE records(id INTEGER PRIMARY KEY, unique_note TEXT NOT NULL DEFAULT 'UNIQUE')";
        let plan = super::super::additions(&connection, Path::new("fixture"), ddl).unwrap();
        plan.apply(&connection).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT unique_note FROM records", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "UNIQUE"
        );
        for default in ["NULL", "(NULL)", "((NULL))"] {
            let connection = Connection::open_in_memory().unwrap();
            connection
                .execute_batch(&format!("CREATE TABLE records(id INTEGER PRIMARY KEY, unused TEXT NOT NULL DEFAULT {default}); INSERT INTO records VALUES(1,'kept')"))
                .unwrap();
            assert!(
                super::super::additions(
                    &connection,
                    Path::new("fixture"),
                    "CREATE TABLE records(id INTEGER PRIMARY KEY)"
                )
                .is_err()
            );
            assert_eq!(
                connection
                    .query_row("SELECT unused FROM records", [], |row| row
                        .get::<_, String>(0))
                    .unwrap(),
                "kept"
            );
        }
    }

    #[test]
    fn rebuilds_changed_indexes() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT); CREATE INDEX records_value ON records(id); INSERT INTO records VALUES(1,'kept')").unwrap();
        let ddl = "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT); CREATE INDEX records_value ON records(value);";
        let plan = super::super::additions(&connection, Path::new("fixture"), ddl).unwrap();
        plan.apply(&connection).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT name FROM pragma_index_info('records_value')",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "value"
        );
        assert!(
            super::super::additions(&connection, Path::new("fixture"), ddl)
                .unwrap()
                .statements
                .is_empty()
        );
    }

    #[test]
    fn rejects_ambiguous_changes_before_writing() {
        for ddl in [
            "CREATE TABLE records(id INTEGER PRIMARY KEY, value INTEGER)",
            "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT, required TEXT NOT NULL)",
            "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT, special TEXT UNIQUE)",
            "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT, PRIMARY KEY(value))",
        ] {
            let connection = Connection::open_in_memory().unwrap();
            connection.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO records VALUES(1,'00123')").unwrap();
            assert!(super::super::additions(&connection, Path::new("fixture"), ddl).is_err());
            assert_eq!(
                connection
                    .query_row("SELECT value FROM records", [], |row| row
                        .get::<_, String>(0))
                    .unwrap(),
                "00123"
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT count(*) FROM pragma_table_info('records')",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                2
            );
        }
    }

    #[test]
    fn rolls_back_failed_index_changes() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO records VALUES(1,'same'),(2,'same'); CREATE INDEX records_value ON records(value);").unwrap();
        let ddl = "CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT, note TEXT); CREATE UNIQUE INDEX records_value ON records(value)";
        let plan = super::super::additions(&connection, Path::new("fixture"), ddl).unwrap();
        {
            let transaction = connection.transaction().unwrap();
            assert!(plan.apply(&transaction).is_err());
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('records')",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        assert!(!connection.query_row("SELECT \"unique\" FROM pragma_index_list('records') WHERE name='records_value'", [], |row| row.get::<_, bool>(0)).unwrap());
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM records", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
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
        for (_, (kind, sql)) in objects(&connection).unwrap() {
            if kind == "table" {
                assert!(!Table::parse(&sql).unwrap().columns.is_empty());
            }
        }
        assert!(
            super::super::additions(&connection, Path::new("fixture"), DDL)
                .unwrap()
                .statements
                .is_empty()
        );
    }
}

mod startup {
    use super::*;

    #[test]
    fn rejects_removed_storage_without_writing() {
        for table in ["credentials", "plugin_credentials"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("database.sqlite3");
            let node = NodeId([64; 32]);
            let database = Database::open(&path, node, None).unwrap();
            database
                .connection
                .execute_batch(&format!("CREATE TABLE {table}(value TEXT); INSERT INTO {table} VALUES('kept'); DROP TABLE model_catalog_status"))
                .unwrap();
            database.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            let error = Database::open(&path, node, None).err().unwrap();
            assert!(
                error
                    .to_string()
                    .contains("incompatible development database")
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(snapshots(directory.path()).is_empty());
            let connection = Connection::open(&path).unwrap();
            assert_eq!(
                connection
                    .query_row(&format!("SELECT value FROM {table}"), [], |row| row
                        .get::<_, String>(0))
                    .unwrap(),
                "kept"
            );
            assert!(
                connection
                    .prepare("SELECT body FROM model_catalog_status")
                    .is_err()
            );
        }
    }

    #[test]
    fn synchronizes_once_and_keeps_a_private_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let node = NodeId([63; 32]);
        let database = Database::open(&path, node, None).unwrap();
        // Optional metadata can be added; historical initialization markers must stay intact.
        database.connection.execute_batch("UPDATE node SET plugin_defaults=1; ALTER TABLE agent_runs DROP COLUMN provider; DROP INDEX dispatch_handlers_source; DROP TABLE model_catalog_status").unwrap();
        database.close().unwrap();
        let database = Database::open(&path, node, None).unwrap();
        assert_eq!(
            database
                .connection
                .query_row("SELECT plugin_defaults FROM node", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .connection
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('agent_runs') WHERE name='provider'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert!(
            database
                .connection
                .prepare("SELECT body FROM model_catalog_status")
                .is_ok()
        );
        assert!(database.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='dispatch_handlers_source')", [], |row| row.get::<_, bool>(0)).unwrap());
        database.close().unwrap();
        let snapshots = snapshots(directory.path());
        assert_eq!(snapshots.len(), 1);
        let snapshot = Connection::open(&snapshots[0]).unwrap();
        assert_eq!(
            snapshot
                .query_row("SELECT identity FROM node", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            node.0
        );
        assert_eq!(
            snapshot
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('agent_runs') WHERE name='provider'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert!(
            !snapshot
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='model_catalog_status')",
                    [],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap()
        );
        drop(snapshot);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&snapshots[0])
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        Database::open(&path, node, None).unwrap().close().unwrap();
        assert_eq!(self::snapshots(directory.path()), snapshots);
    }

    #[test]
    fn snapshot_includes_committed_wal_records() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE records(value TEXT); INSERT INTO records VALUES('committed')").unwrap();
        let plan = Plan {
            fresh: false,
            statements: vec!["CREATE TABLE added(value TEXT)".into()],
        };
        let snapshot = plan.backup(&connection, &path).unwrap().unwrap();
        let copy = Connection::open(snapshot).unwrap();
        assert_eq!(
            copy.query_row("SELECT value FROM records", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "committed"
        );
        assert!(
            !copy
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='added')",
                    [],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap()
        );
    }

    #[test]
    fn failed_preflight_does_not_mutate_the_profile() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let node = NodeId([64; 32]);
        let database = Database::open(&path, node, None).unwrap();
        database.connection.execute_batch("DROP TABLE model_catalog_status; CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('kept');").unwrap();
        database
            .connection
            .execute(
                "INSERT INTO events(body) VALUES(?1)",
                [b"invalid".as_slice()],
            )
            .unwrap();
        database.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(Database::open(&path, node, None).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(snapshots(directory.path()).is_empty());
        let connection = Connection::open(&path).unwrap();
        assert!(
            !connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='model_catalog_status')",
                    [],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap()
        );
        assert_eq!(
            connection
                .query_row("SELECT value FROM retained", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "kept"
        );
    }

    fn snapshots(directory: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("schema-backup-")
            })
            .collect()
    }
}
