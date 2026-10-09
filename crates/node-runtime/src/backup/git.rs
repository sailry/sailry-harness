//! Linked checkouts are archived with independent Git metadata, without source writes.
use super::{MAX_BYTES, MAX_FILES, MAX_MANIFEST, cancelled, io};
use crate::files;
use git2::{Config, ConfigLevel, Oid, Repository, RepositoryInitOptions, RepositoryOpenFlags};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Object(Oid),
    Symbolic(String),
}

#[derive(PartialEq, Eq)]
struct State {
    head: Target,
    references: BTreeMap<String, Target>,
    configuration: Vec<(String, String)>,
    index: Option<(u64, [u8; 32])>,
}

/// The returned directory is the replacement `.git` tree, not a checkout.
/// Its files still pass through the archive's bounded, no-follow inventory.
pub(super) fn prepare(
    source: &Path,
    temporary: &Path,
    cancel: &CancellationToken,
) -> Result<Option<tempfile::TempDir>, Fault> {
    cancelled(cancel)?;
    let root = files::path::root(source)?;
    match root.symlink_metadata(".git") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error)),
        Ok(metadata) if metadata.is_dir() => return Ok(None),
        Ok(metadata) if metadata.is_file() => (),
        Ok(_) => return Err(Fault::invalid("unsupported backup Git metadata entry")),
    }
    let repository = Repository::open_ext(
        source,
        RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&OsStr>(),
    )
    .map_err(io)?;
    if !repository.is_worktree() {
        return Ok(None);
    }
    let workdir = repository
        .workdir()
        .ok_or_else(|| Fault::invalid("backup requires a non-bare checkout"))?
        .canonicalize()
        .map_err(io)?;
    if workdir != source {
        return Err(Fault::invalid("backup Git checkout root does not match"));
    }
    let before = State::read(&repository, cancel)?;
    let directory = tempfile::Builder::new()
        .prefix("backup-git-")
        .tempdir_in(temporary)
        .map_err(io)?;
    let destination = Repository::init_opts(
        directory.path(),
        RepositoryInitOptions::new()
            .bare(true)
            .external_template(false),
    )
    .map_err(io)?;
    objects(&repository, &destination, cancel)?;
    for (name, target) in &before.references {
        reference(&destination, name, target)?;
    }
    reference(&destination, "HEAD", &before.head)?;
    copy_index(&repository, directory.path(), &before.index, cancel)?;
    metadata(&repository, directory.path(), cancel)?;
    configuration(directory.path(), &before.configuration, cancel)?;
    cancelled(cancel)?;
    if State::read(&repository, cancel)? != before {
        return Err(changed());
    }
    Ok(Some(directory))
}

impl State {
    fn read(repository: &Repository, cancel: &CancellationToken) -> Result<Self, Fault> {
        let head = target(&repository.find_reference("HEAD").map_err(io)?)?;
        let mut references = BTreeMap::new();
        let mut size = 0usize;
        for reference in repository.references().map_err(io)? {
            cancelled(cancel)?;
            let reference = reference.map_err(io)?;
            let name = reference.name().map_err(io)?.to_owned();
            let target = target(&reference)?;
            size = size
                .saturating_add(name.len())
                .saturating_add(match &target {
                    Target::Object(_) => 40,
                    Target::Symbolic(name) => name.len(),
                });
            if size > MAX_MANIFEST {
                return Err(Fault::invalid("backup Git references exceed the limit"));
            }
            references.insert(name, target);
        }
        let configuration = settings(repository, cancel)?;
        let index = index(repository, cancel)?;
        Ok(Self {
            head,
            references,
            configuration,
            index,
        })
    }
}

fn target(reference: &git2::Reference<'_>) -> Result<Target, Fault> {
    if let Some(object) = reference.target() {
        return Ok(Target::Object(object));
    }
    reference
        .symbolic_target()
        .map_err(io)?
        .map(|name| Target::Symbolic(name.to_owned()))
        .ok_or_else(|| Fault::invalid("unsupported backup Git reference"))
}

fn reference(repository: &Repository, name: &str, target: &Target) -> Result<(), Fault> {
    match target {
        Target::Object(object) => repository.reference(name, *object, true, "Profile backup"),
        Target::Symbolic(target) => {
            repository.reference_symbolic(name, target, true, "Profile backup")
        }
    }
    .map_err(io)?;
    Ok(())
}

fn settings(
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<Vec<(String, String)>, Fault> {
    let configuration = repository.config().map_err(io)?;
    let mut values = Vec::new();
    let mut size = 0usize;
    for level in [ConfigLevel::Local, ConfigLevel::Worktree] {
        let configuration = match configuration.open_level(level) {
            Ok(configuration) => configuration,
            Err(error) if error.code() == git2::ErrorCode::NotFound => continue,
            Err(error) => return Err(io(error)),
        };
        let mut entries = configuration.entries(None).map_err(io)?;
        while let Some(entry) = entries.next() {
            cancelled(cancel)?;
            let entry = entry.map_err(io)?;
            let name = entry.name().map_err(io)?;
            // Included values are flattened; path directives cannot retain external owners.
            if name == "include.path"
                || (name.starts_with("includeif.") && name.ends_with(".path"))
                || matches!(
                    name,
                    "core.worktree" | "core.bare" | "extensions.worktreeconfig"
                )
            {
                continue;
            }
            let value = if entry.has_value() {
                entry.value().map_err(io)?
            } else {
                "true"
            };
            size = size.saturating_add(name.len()).saturating_add(value.len());
            if size > MAX_MANIFEST {
                return Err(Fault::invalid("backup Git configuration exceeds the limit"));
            }
            values.push((name.to_owned(), value.to_owned()));
        }
    }
    Ok(values)
}

fn configuration(
    directory: &Path,
    values: &[(String, String)],
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let fragment = tempfile::NamedTempFile::new_in(directory).map_err(io)?;
    let mut output = std::fs::File::create(directory.join("config")).map_err(io)?;
    for (name, value) in values {
        cancelled(cancel)?;
        std::fs::write(fragment.path(), []).map_err(io)?;
        // libgit2 serializes names and escaping, including repeated or empty values.
        Config::open(fragment.path())
            .map_err(io)?
            .set_str(name, value)
            .map_err(io)?;
        output
            .write_all(&std::fs::read(fragment.path()).map_err(io)?)
            .map_err(io)?;
    }
    output
        .write_all(b"\n[core]\n\tbare = false\n\tworktree = ..\n")
        .map_err(io)?;
    output.sync_all().map_err(io)
}

fn index(
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<Option<(u64, [u8; 32])>, Fault> {
    let directory = files::path::root(repository.path())?;
    match directory.symlink_metadata("index") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error)),
        Ok(_) => (),
    }
    let mut input = files::open_regular(&directory, "index")?.into_std();
    let mut size = 0u64;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let count = input.read(&mut buffer).map_err(io)?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        if size > MAX_BYTES {
            return Err(Fault::invalid("backup Git index exceeds the limit"));
        }
        hash.update(&buffer[..count]);
    }
    Ok(Some((size, hash.finalize().into())))
}

fn copy_index(
    repository: &Repository,
    destination: &Path,
    expected: &Option<(u64, [u8; 32])>,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let Some((size, expected_hash)) = expected else {
        return Ok(());
    };
    let root = files::path::root(repository.path())?;
    let mut input = files::open_regular(&root, "index")?.into_std();
    let mut output = std::fs::File::create(destination.join("index")).map_err(io)?;
    let mut hash = Sha256::new();
    let mut copied = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let count = input.read(&mut buffer).map_err(io)?;
        if count == 0 {
            break;
        }
        copied = copied.saturating_add(count as u64);
        if copied > *size {
            return Err(changed());
        }
        hash.update(&buffer[..count]);
        output.write_all(&buffer[..count]).map_err(io)?;
    }
    if copied != *size || <[u8; 32]>::from(hash.finalize()) != *expected_hash {
        return Err(changed());
    }
    output.sync_all().map_err(io)
}

fn objects(
    source: &Repository,
    destination: &Repository,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let source_objects = source.odb().map_err(io)?;
    let mut builder = source.packbuilder().map_err(io)?;
    builder.set_threads(1);
    builder
        .set_progress_callback(|_, _, _| !cancel.is_cancelled())
        .map_err(io)?;
    let mut total = 0u64;
    let mut failure = None;
    let result = source_objects.foreach(|object| {
        let result = (|| {
            cancelled(cancel)?;
            let (size, _) = source_objects.read_header(*object).map_err(io)?;
            total = total.saturating_add(size as u64);
            if total > MAX_BYTES {
                return Err(Fault::invalid("backup Git objects exceed the limit"));
            }
            builder.insert_object(*object, None).map_err(io)
        })();
        if let Err(error) = result {
            failure = Some(error);
            return false;
        }
        true
    });
    if let Some(error) = failure {
        return Err(error);
    }
    result.map_err(io)?;
    if builder.object_count() == 0 {
        return Ok(());
    }
    let destination_objects = destination.odb().map_err(io)?;
    let mut writer = destination_objects.packwriter().map_err(io)?;
    writer.progress(|progress| {
        !cancel.is_cancelled() && progress.received_bytes() as u64 <= MAX_BYTES
    });
    let mut written = 0u64;
    let result = builder.foreach(|bytes| {
        let result = (|| {
            cancelled(cancel)?;
            written = written.saturating_add(bytes.len() as u64);
            if written > MAX_BYTES {
                return Err(Fault::invalid("backup Git pack exceeds the limit"));
            }
            writer.write_all(bytes).map_err(io)
        })();
        if let Err(error) = result {
            failure = Some(error);
            return false;
        }
        true
    });
    if let Some(error) = failure {
        return Err(error);
    }
    cancelled(cancel)?;
    result.map_err(io)?;
    writer.commit().map_err(io)?;
    cancelled(cancel)
}

fn metadata(
    source: &Repository,
    destination: &Path,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let common = files::path::root(source.commondir())?;
    let local = files::path::root(source.path())?;
    let mut size = 0u64;
    let mut count = 0usize;
    for (directory, name) in [
        (&common, "info/exclude"),
        (&common, "info/attributes"),
        (&common, "info/grafts"),
        (&common, "info/sparse-checkout"),
        (&common, "shallow"),
        (&local, "info/sparse-checkout"),
        (&local, "ORIG_HEAD"),
        (&local, "MERGE_HEAD"),
        (&local, "MERGE_MSG"),
        (&local, "MERGE_MODE"),
        (&local, "CHERRY_PICK_HEAD"),
        (&local, "REVERT_HEAD"),
        (&local, "AUTO_MERGE"),
        (&local, "SQUASH_MSG"),
        (&local, "rebase-merge"),
        (&local, "rebase-apply"),
        (&local, "sequencer"),
    ] {
        copy_metadata(directory, name, destination, &mut size, &mut count, cancel)?;
    }
    // A split index retains its adjacent shared index, without retaining gitdir links.
    for entry in local.entries().map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name();
        let Some(name) = name
            .to_str()
            .filter(|name| name.starts_with("sharedindex."))
        else {
            continue;
        };
        copy_metadata(&local, name, destination, &mut size, &mut count, cancel)?;
    }
    Ok(())
}

fn copy_metadata(
    root: &cap_std::fs::Dir,
    relative: &str,
    destination: &Path,
    size: &mut u64,
    count: &mut usize,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    use cap_fs_ext::DirExt;
    cancelled(cancel)?;
    let parts = files::path::components(relative, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let directory = match files::path::descend(root.try_clone().map_err(io)?, parents) {
        Ok(directory) => directory,
        Err(error) if error.code == ErrorCode::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let metadata = match directory.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io(error)),
    };
    if metadata.is_dir() {
        if parts.len() > 64 {
            return Err(Fault::invalid(
                "backup Git metadata nesting exceeds the limit",
            ));
        }
        let directory = directory.open_dir_nofollow(name).map_err(io)?;
        for entry in directory.entries().map_err(io)? {
            let entry = entry.map_err(io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Fault::invalid("backup Git names must be UTF-8"))?;
            files::path::components(&name, false)?;
            copy_metadata(
                root,
                &format!("{relative}/{name}"),
                destination,
                size,
                count,
                cancel,
            )?;
        }
        return Ok(());
    }
    let mut input = files::open_regular(&directory, name)?.into_std();
    let before = input.metadata().map_err(io)?;
    *size = size.saturating_add(before.len());
    *count = count.saturating_add(1);
    if *size > MAX_BYTES || *count > MAX_FILES {
        return Err(Fault::invalid("backup Git metadata exceeds the limit"));
    }
    let path = destination.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(io)?;
    let mut output = std::fs::File::create(path).map_err(io)?;
    let mut copied = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let bytes = input.read(&mut buffer).map_err(io)?;
        if bytes == 0 {
            break;
        }
        copied = copied.saturating_add(bytes as u64);
        if copied > before.len() {
            return Err(changed());
        }
        output.write_all(&buffer[..bytes]).map_err(io)?;
    }
    let after = input.metadata().map_err(io)?;
    if copied != before.len()
        || before.len() != after.len()
        || before.modified().map_err(io)? != after.modified().map_err(io)?
    {
        return Err(changed());
    }
    output.sync_all().map_err(io)
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "Git metadata changed while creating the backup",
    )
}

#[cfg(test)]
mod tests;
