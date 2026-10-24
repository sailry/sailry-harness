//! Native handles and connection settings shared by the Node's two databases.
use crate::Error;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};
use turso::{Connection, Database};

#[cfg(test)]
mod tests;

/// One native database handle, shared without sharing transaction connections.
pub(crate) struct Handle {
    path: PathBuf,
    state: Arc<RwLock<Option<Database>>>,
}

/// Keep the database open until this connection and its transaction have ended.
pub(crate) struct Lease {
    connection: Connection,
    _open: OwnedRwLockReadGuard<Option<Database>>,
}

/// Maintenance may inspect a drained database before closing its native handle.
pub(crate) struct Exclusive {
    connection: Option<Connection>,
    path: PathBuf,
    state: OwnedRwLockWriteGuard<Option<Database>>,
}

/// The lifecycle owner must reopen after copying, or keep it closed for shutdown.
#[must_use]
pub(crate) struct Closed {
    path: PathBuf,
    state: OwnedRwLockWriteGuard<Option<Database>>,
}

impl Handle {
    pub(crate) fn new(path: PathBuf, database: Database) -> Self {
        Self {
            path,
            state: Arc::new(RwLock::new(Some(database))),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) async fn lease(&self, readonly: bool) -> Result<Lease, Error> {
        let guard = self.state.clone().read_owned().await;
        let database = guard.as_ref().ok_or(Error::Stopped)?;
        let connection = connect(database).await?;
        if readonly {
            connection.pragma_update("query_only", 1).await?;
        }
        Ok(Lease {
            connection,
            _open: guard,
        })
    }

    pub(crate) async fn exclusive(&self) -> Result<Exclusive, Error> {
        let state = self.state.clone().write_owned().await;
        let database = state.as_ref().ok_or(Error::Stopped)?;
        let connection = connect(database).await?;
        Ok(Exclusive {
            connection: Some(connection),
            path: self.path.clone(),
            state,
        })
    }

    pub(crate) async fn pause(&self) -> Result<Closed, Error> {
        self.exclusive().await?.close().await
    }
}

impl Lease {
    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

impl std::ops::Deref for Lease {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.connection
    }
}

impl std::ops::DerefMut for Lease {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

impl Exclusive {
    pub(crate) fn connection(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("maintenance owns its open connection")
    }

    pub(crate) async fn close(mut self) -> Result<Closed, Error> {
        checkpoint(self.connection()).await?;
        drop(self.connection.take());
        drop(self.state.take());
        Ok(Closed {
            path: self.path,
            state: self.state,
        })
    }
}

impl Closed {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) async fn reopen(mut self) -> Result<(), Error> {
        let database = open(&self.path).await?;
        *self.state = Some(database);
        Ok(())
    }
}

pub(crate) async fn open(path: &Path) -> Result<Database, Error> {
    let memory = path == Path::new(":memory:");
    let mut initialized = false;
    if !memory {
        #[cfg(windows)]
        crate::profile::windows::prepare_database(path)?;
        for (index, file) in crate::profile::database_files(path).into_iter().enumerate() {
            match file.symlink_metadata() {
                Ok(metadata) => {
                    if !metadata.is_file() || metadata.is_symlink() {
                        return Err(Error::InvalidProfile {
                            path: file,
                            reason: "database must be a regular file",
                        });
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if metadata.nlink() != 1 {
                            return Err(Error::InvalidProfile {
                                path: file,
                                reason: "database must not be hard-linked",
                            });
                        }
                    }
                    if index == 0 {
                        initialized = metadata.len() > 0;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    let name = path.to_str().ok_or_else(|| Error::InvalidProfile {
        path: path.to_owned(),
        reason: "database path must be valid UTF-8",
    })?;
    let database = turso::Builder::new_local(name)
        .experimental_index_method(true)
        .build()
        .await?;
    let connection = connect(&database).await?;
    if !initialized {
        connection.pragma_update("journal_mode", "'mvcc'").await?;
    }
    let mode: String = connection
        .prepare("PRAGMA journal_mode")
        .await?
        .query_row(())
        .await?
        .get(0)?;
    if mode != "mvcc" {
        return Err(Error::Worker("Node database is not in MVCC mode".into()));
    }
    #[cfg(windows)]
    if !memory {
        crate::profile::windows::validate_database(path)?;
    }
    Ok(database)
}

pub(crate) async fn connect(database: &Database) -> Result<Connection, Error> {
    let connection = database.connect()?;
    connection.pragma_update("synchronous", "FULL").await?;
    connection.pragma_update("foreign_keys", 1).await?;
    // Conflicts are retried only at the complete transaction boundary.
    connection.busy_timeout(Duration::ZERO)?;
    Ok(connection)
}

pub(crate) async fn checkpoint(connection: &Connection) -> Result<(), Error> {
    let mut rows = connection
        .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
        .await?;
    while rows.next().await?.is_some() {}
    Ok(())
}
