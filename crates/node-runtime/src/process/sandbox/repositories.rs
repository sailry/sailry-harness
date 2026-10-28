use super::enforcement::Policy;
use super::{policy, runtime};
use cap_fs_ext::{DirExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use ignore::gitignore::Gitignore;
use sailry_protocol::Fault;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read as _,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

mod cache;
use cache::{Snapshot, Witness};

pub(super) fn capture(
    root: &Path,
    policy: &mut Policy,
    home: Option<&Path>,
    profile: Option<&Path>,
) -> Result<Vec<PathBuf>, Fault> {
    let root = root.canonicalize().map_err(runtime::failure)?;
    Ok(cache::Cache::global()
        .discover(&root)?
        .into_iter()
        .filter_map(|path| {
            if path.starts_with(&root) {
                Some(path)
            } else {
                // Reapply the current credential boundaries to every cached path.
                policy::read(policy, &path, home, profile)
            }
        })
        .collect())
}

fn discover(root: &Path) -> Result<Snapshot, Fault> {
    let mut capture = Capture {
        root,
        snapshot: Snapshot {
            stable: true,
            ..Default::default()
        },
        layers: Vec::new(),
        control: crate::files::Control::new(tokio_util::sync::CancellationToken::new()),
        report: crate::files::walk::Report::default(),
        skipped: 0,
    };
    for ancestor in root.ancestors() {
        let directory = crate::files::path::root(ancestor)?;
        if capture.inspect(&directory, ancestor) {
            break;
        }
    }
    let directory = crate::files::path::root(root)?;
    capture.directory(&directory, root, Path::new(""), 0);
    let skipped = capture.skipped + capture.report.skipped;
    if skipped > 0 {
        eprintln!(
            "Git metadata discovery omitted {skipped} unreadable or unsupported entries in {}; discovered metadata remains protected",
            root.display()
        );
    }
    Ok(capture.snapshot)
}

struct Capture<'a> {
    root: &'a Path,
    snapshot: Snapshot,
    layers: Vec<Gitignore>,
    control: crate::files::Control,
    report: crate::files::walk::Report,
    skipped: usize,
}

impl Capture<'_> {
    fn witness(&mut self, directory: &Dir, parent: &Path, name: &str) {
        self.snapshot.witnesses.insert(
            parent.join(name),
            Witness::read(directory.symlink_metadata(name)),
        );
    }

    fn directory(&mut self, directory: &Dir, path: &Path, relative: &Path, depth: usize) {
        self.snapshot
            .witnesses
            .insert(path.into(), Witness::read(directory.dir_metadata()));
        self.inspect(directory, path);
        self.witness(directory, path, ".gitignore");
        let inherited = self.layers.len();
        if let Some(layer) = crate::files::walk::ignores::load_path(
            directory,
            relative,
            &self.control,
            &mut self.report,
        ) {
            self.layers.push(layer);
        }
        let Ok(entries) = directory.entries() else {
            self.skipped += 1;
            self.layers.truncate(inherited);
            return;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                self.skipped += 1;
                self.snapshot.stable = false;
                continue;
            };
            let name = entry.file_name();
            if name.eq_ignore_ascii_case(".git") {
                continue;
            }
            let metadata = match directory.symlink_metadata(&name) {
                Ok(metadata) => metadata,
                Err(_) => {
                    self.skipped += 1;
                    self.snapshot.stable = false;
                    continue;
                }
            };
            if !metadata.is_dir() || metadata.is_symlink() {
                continue;
            }
            let relative = relative.join(&name);
            if self.layers.iter().rev().find_map(|layer| {
                let found = layer.matched(&relative, true);
                (!found.is_none()).then(|| found.is_ignore())
            }) == Some(true)
            {
                continue;
            }
            let child = path.join(&name);
            self.snapshot
                .witnesses
                .insert(child.clone(), Witness::read(Ok(metadata)));
            if depth >= 64 {
                self.skipped += 1;
                continue;
            }
            match directory.open_dir_nofollow(&name) {
                Ok(directory) => self.directory(&directory, &child, &relative, depth + 1),
                Err(_) => self.skipped += 1,
            }
        }
        self.layers.truncate(inherited);
    }

    fn inspect(&mut self, directory: &Dir, path: &Path) -> bool {
        self.witness(directory, path, ".git");
        let metadata = match directory.symlink_metadata(".git") {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
            Err(_) => {
                self.skipped += 1;
                return false;
            }
        };
        let entry = path.join(".git");
        self.snapshot.paths.insert(entry.clone());
        if metadata.is_file() {
            match text(directory, ".git") {
                Ok(Some(text)) => {
                    if let Some(target) = text.trim_ascii().strip_prefix(b"gitdir: ") {
                        self.target(&path.join(std::ffi::OsStr::from_bytes(target)));
                    }
                }
                Ok(None) => {}
                Err(_) => self.skipped += 1,
            }
        } else if metadata.is_dir() {
            match directory.open_dir_nofollow(".git") {
                Ok(directory) => self.common(&directory, &entry),
                Err(_) => self.skipped += 1,
            }
        } else if metadata.is_symlink() {
            // Protect its existing target separately; Linux cannot freeze the link.
            self.target(&entry);
        }
        true
    }

    fn target(&mut self, path: &Path) {
        let resolved = path.canonicalize().ok();
        self.snapshot.targets.insert(path.into(), resolved.clone());
        let Some(path) = resolved else {
            return;
        };
        if self.root.starts_with(&path) || self.snapshot.paths.contains(&path) {
            return;
        }
        let directory = match crate::files::path::root(&path) {
            Ok(directory) => directory,
            Err(_) => {
                self.skipped += 1;
                return;
            }
        };
        for name in ["HEAD", "objects", "commondir"] {
            self.witness(&directory, &path, name);
        }
        // A Git pointer never grants read access to an arbitrary external directory.
        if !directory
            .symlink_metadata("HEAD")
            .is_ok_and(|head| head.is_file())
            || !(directory
                .symlink_metadata("objects")
                .is_ok_and(|objects| objects.is_dir())
                || directory
                    .symlink_metadata("commondir")
                    .is_ok_and(|common| common.is_file()))
        {
            return;
        }
        self.snapshot.paths.insert(path.clone());
        self.common(&directory, &path);
    }

    fn common(&mut self, directory: &Dir, path: &Path) {
        self.witness(directory, path, "commondir");
        match text(directory, "commondir") {
            Ok(Some(text)) if !text.trim_ascii().is_empty() => {
                self.target(&path.join(std::ffi::OsStr::from_bytes(text.trim_ascii())))
            }
            Ok(_) => {}
            Err(_) => self.skipped += 1,
        }
    }
}

fn text(directory: &Dir, name: &str) -> Result<Option<Vec<u8>>, Fault> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(runtime::failure(error)),
    };
    let mut options = OpenOptions::new();
    options.read(true).follow(cap_fs_ext::FollowSymlinks::No);
    let file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(runtime::failure(error)),
    };
    if !file.metadata().map_err(runtime::failure)?.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(runtime::failure)?;
    if bytes.len() > 4096 || bytes.contains(&0) {
        return Ok(None);
    }
    Ok(Some(bytes))
}
