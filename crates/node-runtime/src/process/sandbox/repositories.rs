use super::{policy, runtime};
use adk_sandbox::sandbox::SandboxPolicy;
use cap_fs_ext::{DirExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sailry_protocol::{ErrorCode, Fault};
use std::{
    collections::BTreeSet,
    io::Read as _,
    path::{Path, PathBuf},
};

pub(super) fn capture(
    root: &Path,
    policy: &mut SandboxPolicy,
    home: Option<&Path>,
    profile: Option<&Path>,
) -> Result<Vec<PathBuf>, Fault> {
    let root = root.canonicalize().map_err(runtime::failure)?;
    let mut capture = Capture {
        root: &root,
        policy,
        home,
        profile,
        paths: BTreeSet::new(),
    };
    for ancestor in root.ancestors() {
        let directory = crate::files::path::root(ancestor)?;
        if capture.inspect(&directory, ancestor)? {
            break;
        }
    }
    let control = crate::files::Control::new(tokio_util::sync::CancellationToken::new());
    let report = crate::files::walk::directories(&root, &control, &mut |directory, relative| {
        capture.inspect(directory, &root.join(relative)).map(|_| ())
    })?;
    if report.partial {
        return Err(Fault::new(
            ErrorCode::Busy,
            "Git metadata discovery was incomplete; use a narrower workspace or unrestricted access",
        ));
    }
    Ok(capture.paths.into_iter().collect())
}

struct Capture<'a> {
    root: &'a Path,
    policy: &'a mut SandboxPolicy,
    home: Option<&'a Path>,
    profile: Option<&'a Path>,
    paths: BTreeSet<PathBuf>,
}

impl Capture<'_> {
    fn inspect(&mut self, directory: &Dir, path: &Path) -> Result<bool, Fault> {
        let metadata = match directory.symlink_metadata(".git") {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(runtime::failure(error)),
        };
        let entry = path.join(".git");
        if metadata.is_file() {
            self.protect(&entry);
            if let Some(text) = text(directory, ".git")?
                && let Some(target) = text.trim().strip_prefix("gitdir: ")
            {
                self.target(&path.join(target))?;
            }
        } else if metadata.is_dir() {
            self.protect(&entry);
            let metadata = directory
                .open_dir_nofollow(".git")
                .map_err(runtime::failure)?;
            self.common(&metadata, &entry)?;
        } else if metadata.is_symlink() {
            // The link is never traversed by the workspace scan. Its existing
            // target is protected separately; Linux mounts cannot freeze the link.
            self.paths.insert(entry.clone());
            self.target(&entry)?;
        }
        Ok(true)
    }

    fn protect(&mut self, path: &Path) {
        if path.starts_with(self.root) {
            self.paths.insert(path.to_owned());
        } else if let Some(path) = policy::read(self.policy, path, self.home, self.profile) {
            self.paths.insert(path);
        }
    }

    fn target(&mut self, path: &Path) -> Result<(), Fault> {
        let Ok(path) = path.canonicalize() else {
            return Ok(());
        };
        if self.root.starts_with(&path) || self.paths.contains(&path) {
            return Ok(());
        }
        if !path.starts_with(self.root)
            && policy::readable(&path, self.home, self.profile).is_none()
        {
            return Ok(());
        }
        let directory = match crate::files::path::root(&path) {
            Ok(directory) => directory,
            Err(_) => return Ok(()),
        };
        // A Git pointer must not turn an arbitrary external directory into a
        // read grant. Both regular and linked repositories have a regular HEAD.
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
            return Ok(());
        }
        self.protect(&path);
        self.common(&directory, &path)
    }

    fn common(&mut self, directory: &Dir, path: &Path) -> Result<(), Fault> {
        if let Some(text) = text(directory, "commondir")? {
            let target = text.trim();
            if !target.is_empty() {
                self.target(&path.join(target))?;
            }
        }
        Ok(())
    }
}

fn text(directory: &Dir, name: &str) -> Result<Option<String>, Fault> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(runtime::failure(error)),
    }
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
    Ok(String::from_utf8(bytes).ok())
}
