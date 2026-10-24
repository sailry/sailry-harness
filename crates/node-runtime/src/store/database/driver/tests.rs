use super::*;

async fn integer(connection: &Connection, sql: &str) -> turso::Result<i64> {
    connection.prepare(sql).await?.query_row(()).await?.get(0)
}

async fn handle(path: &Path) -> Result<Handle, Error> {
    Ok(Handle::new(path.to_owned(), open(path).await?))
}

#[tokio::test]
async fn configures_each_connection() -> Result<(), Error> {
    let directory = tempfile::tempdir()?;
    let handle = handle(&directory.path().join("node.turso")).await?;
    {
        let writer = handle.lease(false).await?;
        assert_eq!(integer(&writer, "PRAGMA synchronous").await?, 2);
        assert_eq!(integer(&writer, "PRAGMA foreign_keys").await?, 1);
        assert_eq!(integer(&writer, "PRAGMA query_only").await?, 0);
        writer
            .execute("CREATE TABLE items(value INTEGER)", ())
            .await?;
    }
    for _ in 0..2 {
        let reader = handle.lease(true).await?;
        assert_eq!(integer(&reader, "PRAGMA synchronous").await?, 2);
        assert_eq!(integer(&reader, "PRAGMA foreign_keys").await?, 1);
        assert_eq!(integer(&reader, "PRAGMA query_only").await?, 1);
        assert!(
            reader
                .execute("INSERT INTO items VALUES(1)", ())
                .await
                .is_err()
        );
    }
    drop(handle.pause().await?);
    Ok(())
}

#[tokio::test]
async fn preserves_read_snapshots() -> Result<(), Error> {
    let directory = tempfile::tempdir()?;
    let handle = handle(&directory.path().join("node.turso")).await?;
    {
        let writer = handle.lease(false).await?;
        writer.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY,value INTEGER NOT NULL); INSERT INTO items VALUES(1,10)").await?;
        let reader = handle.lease(true).await?;
        let observer = handle.lease(true).await?;
        reader.execute("BEGIN DEFERRED", ()).await?;
        assert_eq!(integer(&reader, "SELECT value FROM items").await?, 10);
        writer.execute("BEGIN CONCURRENT", ()).await?;
        writer.execute("UPDATE items SET value=20", ()).await?;
        assert_eq!(integer(&writer, "SELECT value FROM items").await?, 20);
        assert_eq!(integer(&observer, "SELECT value FROM items").await?, 10);
        writer.execute("COMMIT", ()).await?;
        assert_eq!(integer(&reader, "SELECT value FROM items").await?, 10);
        assert_eq!(integer(&observer, "SELECT value FROM items").await?, 20);
        reader.execute("COMMIT", ()).await?;
        assert_eq!(integer(&reader, "SELECT value FROM items").await?, 20);
    }
    drop(handle.pause().await?);
    Ok(())
}

#[tokio::test]
async fn maintenance_drains_before_reopening() -> Result<(), Error> {
    let directory = tempfile::tempdir()?;
    let handle = handle(&directory.path().join("node.turso")).await?;
    let connection = handle.lease(false).await?;
    connection
        .execute("CREATE TABLE items(value INTEGER)", ())
        .await?;
    connection
        .execute("INSERT INTO items VALUES(7)", ())
        .await?;
    let mut closing = Box::pin(handle.pause());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut closing)
            .await
            .is_err()
    );
    drop(connection);
    let closed = tokio::time::timeout(Duration::from_secs(2), closing)
        .await
        .unwrap()?;
    let mut reading = Box::pin(handle.lease(true));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut reading)
            .await
            .is_err()
    );
    closed.reopen().await?;
    let connection = tokio::time::timeout(Duration::from_secs(2), reading)
        .await
        .unwrap()?;
    assert_eq!(integer(&connection, "SELECT value FROM items").await?, 7);
    drop(connection);
    drop(handle.pause().await?);
    Ok(())
}

#[tokio::test]
async fn cancelled_maintenance_preserves_the_handle() -> Result<(), Error> {
    let directory = tempfile::tempdir()?;
    let handle = handle(&directory.path().join("node.turso")).await?;
    let connection = handle.lease(false).await?;
    connection
        .execute("CREATE TABLE items(value INTEGER)", ())
        .await?;
    let mut closing = Box::pin(handle.pause());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut closing)
            .await
            .is_err()
    );
    drop(closing);
    drop(connection);
    let reader = handle.lease(true).await?;
    assert_eq!(integer(&reader, "SELECT count(*) FROM items").await?, 0);
    drop(reader);
    drop(handle.pause().await?);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_linked_sidecars_before_creation() -> Result<(), Error> {
    for hard in [false, true] {
        for index in 1..4 {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("node.turso");
            let preserved = directory.path().join("preserved");
            std::fs::write(&preserved, b"preserved file")?;
            let sidecar = &crate::profile::database_files(&path)[index];
            if hard {
                std::fs::hard_link(&preserved, sidecar)?;
            } else {
                std::os::unix::fs::symlink(&preserved, sidecar)?;
            }
            assert!(matches!(
                open(&path).await,
                Err(Error::InvalidProfile { .. })
            ));
            assert!(!path.exists());
            assert_eq!(std::fs::read(&preserved)?, b"preserved file");
        }
    }
    Ok(())
}
