use super::*;

#[test]
fn retains_wal_records_privately() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("node.sqlite3");
    let source = Connection::open(&path).unwrap();
    source.pragma_update(None, "journal_mode", "WAL").unwrap();
    source
        .execute_batch("CREATE TABLE records(value TEXT); INSERT INTO records VALUES('preserved')")
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let wal = std::fs::read(directory.path().join("node.sqlite3-wal")).unwrap();
    let saved = save(&source, &path).unwrap();
    let backup = Connection::open_with_flags(&saved, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        backup
            .query_row("SELECT value FROM records", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::read(directory.path().join("node.sqlite3-wal")).unwrap(),
        wal
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            saved.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            saved
                .parent()
                .unwrap()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn rejects_busy_snapshot_without_waiting() {
    let mut source = Connection::open_in_memory().unwrap();
    source
        .execute_batch("CREATE TABLE records(value TEXT)")
        .unwrap();
    let transaction = source.transaction().unwrap();
    transaction
        .execute("INSERT INTO records VALUES('uncommitted')", [])
        .unwrap();
    let mut destination = Connection::open_in_memory().unwrap();
    assert!(copy(&transaction, &mut destination).is_err());
    transaction.rollback().unwrap();
    assert_eq!(
        source
            .query_row("SELECT count(*) FROM records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn rehearses_privately_and_removes_sidecars() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().canonicalize().unwrap().join("private");
    crate::profile::private_directory(&parent).unwrap();
    let path = parent.join("node.sqlite3");
    let source = Connection::open(&path).unwrap();
    source.pragma_update(None, "journal_mode", "WAL").unwrap();
    source
        .execute_batch("CREATE TABLE records(value TEXT); INSERT INTO records VALUES('preserved')")
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let wal = std::fs::read(path.with_extension("sqlite3-wal")).unwrap();
    let mut snapshot = preview(&source, &path).unwrap();
    let root = snapshot._directory.path().to_path_buf();
    let stored: String = snapshot
        .connection
        .query_row("PRAGMA database_list", [], |row| row.get(2))
        .unwrap();
    assert_eq!(Path::new(&stored), snapshot._file.path());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(root.metadata().unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(
            snapshot
                ._file
                .as_file()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    snapshot
        .connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let transaction = snapshot.connection.transaction().unwrap();
    transaction
        .execute("UPDATE records SET value='rehearsal'", [])
        .unwrap();
    transaction.rollback().unwrap();
    assert_eq!(
        source
            .query_row("SELECT value FROM records", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::read(path.with_extension("sqlite3-wal")).unwrap(),
        wal
    );
    assert!(!parent.join("backups").exists());
    drop(snapshot);
    assert!(!root.exists());
}
