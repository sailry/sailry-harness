use sailry_protocol::Fault;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

pub(super) fn complete(
    args: &mut Vec<OsString>,
    program: &OsStr,
    aliases: &BTreeMap<PathBuf, PathBuf>,
    protected: &[PathBuf],
) -> Result<(), Fault> {
    let mut mounts = vec![
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
    ];
    for alias in ["/bin", "/sbin", "/lib", "/lib64"] {
        if let Ok(path) = std::fs::canonicalize(alias)
            && path != Path::new(alias)
        {
            mounts.extend(["--ro-bind".into(), path.into_os_string(), alias.into()]);
        }
    }
    let mut aliases = aliases.clone();
    links(Path::new(program), &mut aliases);
    for (path, target) in aliases {
        mounts.extend([
            "--symlink".into(),
            target.into_os_string(),
            path.into_os_string(),
        ]);
    }
    // These mounts must follow every writable workspace policy mount.
    // Missing future .git entries cannot be mounted without changing the host.
    let paths: BTreeSet<_> = protected
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    for path in paths {
        mounts.extend([
            "--ro-bind".into(),
            path.as_os_str().to_owned(),
            path.into_os_string(),
        ]);
    }
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| super::runtime::failure("sandbox wrapper has no command separator"))?;
    args.splice(separator..separator, mounts);
    Ok(())
}

pub(super) fn links(path: &Path, aliases: &mut BTreeMap<PathBuf, PathBuf>) {
    if !path.is_absolute() {
        return;
    }
    let mut path = path.to_owned();
    let mut visited = BTreeSet::new();
    while visited.insert(path.clone()) {
        let mut prefix = PathBuf::new();
        let mut components = path.components();
        let mut resolved = None;
        while let Some(component) = components.next() {
            if component == std::path::Component::ParentDir {
                prefix.pop();
                continue;
            }
            prefix.push(component);
            if let Ok(target) = std::fs::read_link(&prefix) {
                if !["/bin", "/sbin", "/lib", "/lib64"]
                    .iter()
                    .any(|alias| prefix == Path::new(alias))
                {
                    aliases.insert(prefix.clone(), target.clone());
                }
                resolved = Some(
                    if target.is_absolute() {
                        target
                    } else {
                        prefix.parent().expect("a link has a parent").join(target)
                    }
                    .join(components.collect::<PathBuf>()),
                );
                break;
            }
        }
        let Some(next) = resolved else {
            break;
        };
        path = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_protected_paths_after_workspace() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let metadata = root.join(".git");
        std::fs::create_dir(&metadata).unwrap();
        let worktrees = crate::workspaces::worktrees(&root);
        std::fs::create_dir_all(&worktrees).unwrap();
        let mut args = vec![
            "--bind".into(),
            root.as_os_str().to_owned(),
            root.as_os_str().to_owned(),
            "--".into(),
            "/bin/sh".into(),
        ];
        complete(
            &mut args,
            OsStr::new("/bin/sh"),
            &BTreeMap::new(),
            &[metadata.clone(), worktrees.clone()],
        )
        .unwrap();
        let separator = args.iter().position(|argument| argument == "--").unwrap();
        assert_eq!(args[0], "--bind");
        assert_eq!(
            &args[separator - 3..separator],
            &[
                OsString::from("--ro-bind"),
                worktrees.as_os_str().to_owned(),
                worktrees.as_os_str().to_owned()
            ]
        );
        assert_eq!(
            &args[separator - 6..separator - 3],
            &[
                OsString::from("--ro-bind"),
                metadata.as_os_str().to_owned(),
                metadata.as_os_str().to_owned()
            ]
        );
        assert_eq!(args[separator + 1], "/bin/sh");
        assert!(!args.iter().any(|argument| argument == "--tmpfs"));
    }
}
