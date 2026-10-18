use crate::Error;
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::path::{Path, PathBuf};
#[cfg(test)]
mod tests;

/// Own the file and any SQLite sidecars until the preview connection closes.
pub(super) struct Preview {
    pub connection: Connection,
    _file: tempfile::NamedTempFile,
    _directory: tempfile::TempDir,
}

pub(super) fn preview(source: &Connection, path: &Path) -> Result<Preview, Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Worker("database directory is unavailable".into()))?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".upgrade-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir_in(parent)?;
    crate::profile::private_directory(directory.path())?;
    let file = tempfile::Builder::new()
        .prefix("snapshot-")
        .suffix(".sqlite3")
        .tempfile_in(directory.path())?;
    let mut connection = Connection::open_with_flags(
        file.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    copy(source, &mut connection)?;
    Ok(Preview {
        connection,
        _file: file,
        _directory: directory,
    })
}

pub(super) fn copy(source: &Connection, destination: &mut Connection) -> Result<(), Error> {
    let backup = Backup::new(source, destination)?;
    loop {
        match backup.step(128)? {
            StepResult::Done => return Ok(()),
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => {
                return Err(Error::Worker(
                    "database snapshot is busy; existing data was not changed".into(),
                ));
            }
            _ => {
                return Err(Error::Worker(
                    "database snapshot did not complete; existing data was not changed".into(),
                ));
            }
        }
    }
}

pub(super) fn save(source: &Connection, path: &Path) -> Result<PathBuf, Error> {
    let directory = path
        .parent()
        .ok_or_else(|| Error::Worker("database directory is unavailable".into()))?
        .join("backups");
    crate::profile::private_directory(&directory)?;
    let temporary = tempfile::NamedTempFile::new_in(&directory)?;
    let mut destination = Connection::open_with_flags(
        temporary.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    copy(source, &mut destination)?;
    destination
        .close()
        .map_err(|(_, error)| Error::Database(error))?;
    temporary.as_file().sync_all()?;
    let target = directory.join(format!(
        "node-before-upgrade-{}.sqlite3",
        uuid::Uuid::new_v4()
    ));
    temporary
        .persist_noclobber(&target)
        .map_err(|error| Error::Io(error.error))?;
    Ok(target)
}
