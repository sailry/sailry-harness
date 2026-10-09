use crate::Error;
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::path::{Path, PathBuf};
#[cfg(test)]
mod tests;

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
