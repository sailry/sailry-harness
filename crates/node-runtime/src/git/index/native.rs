use super::*;
use cap_fs_ext::DirExt;
use cap_std::fs::{MetadataExt, OpenOptionsExt};
use git2::{Index, IndexEntry, IndexTime, Oid};
use std::{
    io::Write,
    time::{Duration, Instant},
};

struct Publication {
    lock: Lock,
    candidate: String,
    candidate_owned: bool,
}

impl Drop for Publication {
    fn drop(&mut self) {
        if self.candidate_owned {
            let _ = self.lock.dir.remove_file(&self.candidate);
        }
    }
}

pub(super) fn update(
    root: &Path,
    paths: &[String],
    operation: GitIndexChange,
    expected: &str,
    expected_head: Option<&str>,
    id: RequestId,
) -> Result<GitIndex, Fault> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let retained = path::root(root)?;
    let repository = super::super::open(root)?;
    let location = location(&repository)?;
    let parent = location.parent().unwrap();
    let lock = Lock::acquire(&repository)?;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .follow(FollowSymlinks::No);
    let mut publication = Publication {
        lock,
        candidate: format!("sailry-index-{id}"),
        candidate_owned: false,
    };
    let original = read(&publication.lock.dir, "index", MAX_INDEX_BYTES)?;
    if digest(original.as_deref()) != expected {
        return Err(conflict());
    }
    let tree = match repository.head() {
        Ok(head) => {
            if head.target().map(|oid| oid.to_string()).as_deref() != expected_head {
                return Err(conflict());
            }
            Some(head.peel_to_tree().map_err(fault)?)
        }
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch && expected_head.is_none() => {
            None
        }
        Err(error) => return Err(fault(error)),
    };
    let candidate = parent.join(&publication.candidate);
    // Reserve a unique path before libgit2 creates its own candidate lock.
    let mut file = publication
        .lock
        .dir
        .open_with(&publication.candidate, &options)
        .map_err(io_error)?;
    publication.candidate_owned = true;
    if let Some(original) = &original {
        file.write_all(original).map_err(io_error)?;
    }
    drop(file);
    let mut index = if original.is_some() {
        Index::open(&candidate).map_err(fault)?
    } else {
        // libgit2 accepts a nonexistent index, not a zero-length index file.
        publication
            .lock
            .dir
            .remove_file(&publication.candidate)
            .map_err(io_error)?;
        Index::open(&candidate).map_err(fault)?
    };
    repository.set_index(&mut index).map_err(fault)?;
    for value in paths {
        if Instant::now() >= deadline {
            return Err(Fault::new(
                ErrorCode::Busy,
                "Git index update deadline exceeded",
            ));
        }
        if index.has_conflicts() {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "resolve index conflicts before updating staged paths",
            ));
        }
        match operation {
            GitIndexChange::Stage => {
                stage(root, &repository, &mut index, value, deadline).map_err(|mut error| {
                    error.message = format!("{}: {}", value, error.message);
                    error
                })?;
            }
            GitIndexChange::Unstage => {
                match tree.as_ref().map(|tree| tree.get_path(Path::new(value))) {
                    Some(Ok(entry)) => index
                        .add(&entry_record(value, entry.filemode() as u32, entry.id()))
                        .map_err(fault)?,
                    Some(Err(error)) if error.code() != git2::ErrorCode::NotFound => {
                        return Err(fault(error));
                    }
                    _ => remove(&mut index, value)?,
                }
            }
        }
    }
    index.write().map_err(fault)?;
    let bytes = read(
        &publication.lock.dir,
        &publication.candidate,
        MAX_INDEX_BYTES,
    )?
    .ok_or_else(conflict)?;
    publication.lock.file.write_all(&bytes).map_err(io_error)?;
    publication.lock.file.sync_all().map_err(io_error)?;
    if Instant::now() >= deadline {
        return Err(Fault::new(
            ErrorCode::Busy,
            "Git index update deadline exceeded",
        ));
    }
    super::super::check_root(&retained, root)?;
    super::super::check_root(&publication.lock.dir, parent)?;
    if !publication.lock.owns()
        || digest(read(&publication.lock.dir, "index", MAX_INDEX_BYTES)?.as_deref()) != expected
    {
        return Err(conflict());
    }
    let current_head = repository
        .head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string());
    if current_head.as_deref() != expected_head {
        return Err(conflict());
    }
    let renamed = publication
        .lock
        .dir
        .rename("index.lock", &publication.lock.dir, "index");
    publication.lock.published = true;
    renamed.map_err(|_| {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "Git index publication is uncertain; inspect index and index.lock before retrying",
        )
    })?;
    let confirmed = (|| {
        publication
            .lock
            .dir
            .try_clone()
            .map_err(io_error)?
            .into_std_file()
            .sync_all()
            .map_err(io_error)?;
        super::super::check_root(&retained, root)?;
        super::super::check_root(&publication.lock.dir, parent)?;
        Ok::<_, Fault>(())
    })();
    if confirmed.is_err() {
        return Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "Git index publication could not be confirmed; inspect status before retrying",
        ));
    }
    Ok(GitIndex {
        revision: digest(Some(&bytes)),
    })
}

fn stage(
    root: &Path,
    repository: &Repository,
    index: &mut Index,
    value: &str,
    deadline: Instant,
) -> Result<(), Fault> {
    let parts = path::components(value, false)?;
    let dir = match path::descend(path::root(root)?, &parts[..parts.len() - 1]) {
        Ok(dir) => dir,
        Err(error) if error.code == ErrorCode::NotFound => return remove(index, value),
        Err(error) => return Err(error),
    };
    let name = parts[parts.len() - 1];
    let metadata = match dir.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return remove(index, value),
        Err(error) => return Err(io_error(error)),
    };
    if repository
        .status_should_ignore(Path::new(value))
        .map_err(fault)?
        && index.get_path(Path::new(value), 0).is_none()
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "ignored files cannot be staged by this operation",
        ));
    }
    let (oid, mode) = if metadata.is_symlink() {
        use std::os::unix::ffi::OsStrExt;
        (
            repository
                .blob(
                    dir.read_link_contents(name)
                        .map_err(io_error)?
                        .as_os_str()
                        .as_bytes(),
                )
                .map_err(fault)?,
            0o120000,
        )
    } else {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .custom_flags(libc::O_NONBLOCK);
        let mut file = dir.open_with(name, &options).map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "expected a regular Git resource",
            ));
        }
        let length = usize::try_from(metadata.len()).map_err(|_| {
            Fault::new(
                ErrorCode::InvalidRequest,
                "file size exceeds platform capacity",
            )
        })?;
        let odb = repository.odb().map_err(fault)?;
        let mut writer = odb.writer(length, git2::ObjectType::Blob).map_err(fault)?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if Instant::now() >= deadline {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "Git index update deadline exceeded",
                ));
            }
            let count = file.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            writer.write_all(&buffer[..count]).map_err(io_error)?;
        }
        (
            writer.finalize().map_err(fault)?,
            if metadata.mode() & 0o111 != 0 {
                0o100755
            } else {
                0o100644
            },
        )
    };
    index.add(&entry_record(value, mode, oid)).map_err(fault)?;
    sync_blob(repository, oid)
}

fn sync_blob(repository: &Repository, oid: Oid) -> Result<(), Fault> {
    // libgit2's default fsyncObjectFiles setting is false. Persist a newly
    // written loose object and its directory before publishing its index entry.
    let hex = oid.to_string();
    let objects = path::root(&repository.commondir().join("objects"))?;
    let directory = match objects.open_dir_nofollow(&hex[..2]) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()), // Existing packed object.
        Err(error) => return Err(io_error(error)),
    };
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    match directory.open_with(&hex[2..], &options) {
        Ok(file) => {
            if !file.metadata().map_err(io_error)?.is_file() {
                return Err(conflict());
            }
            file.sync_all().map_err(io_error)?;
            directory.into_std_file().sync_all().map_err(io_error)?;
            objects.into_std_file().sync_all().map_err(io_error)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn entry_record(value: &str, mode: u32, id: Oid) -> IndexEntry {
    IndexEntry {
        ctime: IndexTime::new(0, 0),
        mtime: IndexTime::new(0, 0),
        dev: 0,
        ino: 0,
        mode,
        uid: 0,
        gid: 0,
        file_size: 0,
        id,
        flags: 0,
        flags_extended: 0,
        path: value.as_bytes().to_vec(),
    }
}

fn remove(index: &mut Index, value: &str) -> Result<(), Fault> {
    match index.remove_path(Path::new(value)) {
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(()),
        result => result.map_err(fault),
    }
}

fn conflict() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "Git HEAD or index changed; inspect status before retrying",
    )
}
