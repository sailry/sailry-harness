use super::enforcement::Policy;
use super::{Environment, policy, runtime};
use sailry_protocol::Fault;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(super) type Links = BTreeMap<PathBuf, PathBuf>;

pub(super) fn configure(
    policy: &mut Policy,
    variables: &mut policy::Variables,
    environment: &Environment,
    home: Option<&Path>,
    profile: Option<&Path>,
    anchor: &cap_std::fs::Dir,
    inputs: Option<&Path>,
) -> Result<Links, Fault> {
    let mut links = paths(policy, variables, environment.path(), home, profile)?;
    if let Some(home) = home {
        managers(policy, variables, anchor, home, profile, &mut links)?;
        #[cfg(target_os = "macos")]
        read(
            policy,
            &home.join("Library/Developer/Toolchains"),
            Some(home),
            profile,
            &mut links,
        );
    }
    if let Some(inputs) = inputs {
        executable(
            policy,
            inputs,
            environment.path(),
            home,
            profile,
            &mut BTreeSet::new(),
            &mut links,
        );
    }
    Ok(links)
}

pub(super) fn paths(
    policy: &mut Policy,
    variables: &mut policy::Variables,
    path: Option<&OsStr>,
    home: Option<&Path>,
    profile: Option<&Path>,
) -> Result<Links, Fault> {
    let mut visited = BTreeSet::new();
    let mut links = BTreeMap::new();
    variables.remove(OsStr::new("PATH"));
    if let Some(path) = path {
        let mut paths = Vec::new();
        for entry in std::env::split_paths(path) {
            let Some(directory) =
                read(policy, &entry, home, profile, &mut links).filter(|path| path.is_dir())
            else {
                paths.push(entry);
                continue;
            };
            paths.push(directory.clone());
            if !system(&directory) {
                visited.insert(directory.clone());
                libraries(policy, &directory, home, profile, &mut links);
                for entry in std::fs::read_dir(&directory)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                {
                    executable(
                        policy,
                        &entry.path(),
                        Some(path),
                        home,
                        profile,
                        &mut visited,
                        &mut links,
                    );
                }
            }
        }
        // Canonical PATH directories also remain available in Linux's private mount namespace.
        variables.insert(
            "PATH".into(),
            std::env::join_paths(paths).map_err(runtime::failure)?,
        );
    }
    Ok(links)
}

fn system(path: &Path) -> bool {
    ["/usr", "/bin", "/sbin", "/System", "/opt/homebrew"]
        .iter()
        .any(|root| path.starts_with(root))
}

fn executable(
    policy: &mut Policy,
    path: &Path,
    search: Option<&OsStr>,
    home: Option<&Path>,
    profile: Option<&Path>,
    visited: &mut BTreeSet<PathBuf>,
    links: &mut Links,
) {
    let Some(resolved) = policy::readable(path, home, profile) else {
        return;
    };
    if visited.contains(&resolved)
        || !resolved
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    {
        return;
    }
    #[cfg(target_os = "linux")]
    super::linux::links(path, links);
    let path = resolved;
    visited.insert(path.clone());
    read(policy, &path, home, profile, links);
    if let Some(directory) = path.parent()
        && visited.insert(directory.to_owned())
    {
        libraries(policy, directory, home, profile, links);
    }
    for ancestor in path.ancestors().skip(1) {
        if ancestor.file_name() == Some(OsStr::new("node_modules")) {
            read(policy, ancestor, home, profile, links);
            break;
        }
        if ancestor.join("pyvenv.cfg").is_file() {
            for relative in ["bin", "lib", "lib64", "pyvenv.cfg"] {
                read(policy, &ancestor.join(relative), home, profile, links);
            }
            break;
        }
        if home == Some(ancestor) || ancestor.parent().is_none() {
            break;
        }
    }
    let mut prefix = Vec::new();
    if std::fs::File::open(&path)
        .and_then(|file| file.take(4096).read_to_end(&mut prefix))
        .is_err()
    {
        return;
    }
    let Some(line) = prefix
        .strip_prefix(b"#!")
        .and_then(|line| line.split(|byte| *byte == b'\n').next())
        .and_then(|line| std::str::from_utf8(line).ok())
    else {
        return;
    };
    let mut words = line.split_ascii_whitespace();
    let Some(interpreter) = words.next() else {
        return;
    };
    let interpreter = if Path::new(interpreter).file_name() == Some(OsStr::new("env")) {
        let Some(command) = words.find(|word| !word.starts_with('-') && !word.contains('=')) else {
            return;
        };
        which::which_in(command, search, path.parent().unwrap_or(Path::new("/"))).ok()
    } else {
        Some(PathBuf::from(interpreter))
    };
    if let Some(interpreter) = interpreter {
        executable(policy, &interpreter, search, home, profile, visited, links);
    }
}

fn libraries(
    policy: &mut Policy,
    directory: &Path,
    home: Option<&Path>,
    profile: Option<&Path>,
    links: &mut Links,
) {
    if directory.file_name() != Some(OsStr::new("bin")) {
        return;
    }
    let Some(prefix) = directory.parent() else {
        return;
    };
    let managed = home.is_some_and(|home| {
        [
            ".nvm/versions/node",
            ".pyenv/versions",
            ".asdf/installs",
            ".local/share/mise/installs",
            ".local/share/uv/python",
            ".local/share/uv/tools",
            ".local/share/pipx/venvs",
            ".local/pipx/venvs",
            "Library/FlyEnv/env",
        ]
        .iter()
        .any(|root| prefix.starts_with(home.join(root)))
    }) || prefix.join("pyvenv.cfg").is_file();
    for relative in ["lib", "lib64", "share"] {
        let directory = prefix.join(relative);
        if managed {
            read(policy, &directory, home, profile, links);
        } else {
            // Common executable prefixes also hold unrelated app data and credentials.
            for entry in std::fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
            {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name
                    .strip_prefix("python")
                    .or_else(|| name.strip_prefix("libpython"))
                    .is_some_and(|suffix| suffix.starts_with(|value: char| value.is_ascii_digit()))
                {
                    read(policy, &entry.path(), home, profile, links);
                }
            }
        }
    }
}

fn managers(
    policy: &mut Policy,
    variables: &mut policy::Variables,
    anchor: &cap_std::fs::Dir,
    home: &Path,
    profile: Option<&Path>,
    links: &mut Links,
) -> Result<(), Fault> {
    for (relative, variable, parts) in [
        (
            ".pyenv",
            "PYENV_ROOT",
            &["bin", "shims", "libexec", "plugins", "versions", "version"][..],
        ),
        (
            ".asdf",
            "ASDF_DATA_DIR",
            &["bin", "shims", "lib", "libexec", "plugins", "installs"][..],
        ),
        (".volta", "VOLTA_HOME", &["bin", "tools"][..]),
        (
            ".local/share/mise",
            "MISE_DATA_DIR",
            &["shims", "installs", "plugins"][..],
        ),
    ] {
        let Some(root) = policy::readable(&home.join(relative), Some(home), profile)
            .filter(|root| root.is_dir())
        else {
            continue;
        };
        for part in parts {
            read(policy, &root.join(part), Some(home), profile, links);
        }
        variables.insert(variable.into(), root.clone().into_os_string());
        if relative == ".asdf" {
            variables.insert("ASDF_DIR".into(), root.clone().into_os_string());
        }
        if matches!(relative, ".pyenv" | ".asdf" | ".volta") {
            link(anchor, &root, &Path::new("home").join(relative))?;
        }
    }
    for (source, target) in [
        (".tool-versions", "home/.tool-versions"),
        (".python-version", "home/.python-version"),
        (".config/mise/config.toml", "config/mise/config.toml"),
    ] {
        if let Some(source) = read(policy, &home.join(source), Some(home), profile, links)
            .filter(|path| path.is_file())
        {
            link(anchor, &source, Path::new(target))?;
        }
    }
    Ok(())
}

fn read(
    policy: &mut Policy,
    path: &Path,
    home: Option<&Path>,
    profile: Option<&Path>,
    links: &mut Links,
) -> Option<PathBuf> {
    let resolved = policy::read(policy, path, home, profile)?;
    #[cfg(target_os = "linux")]
    super::linux::links(path, links);
    #[cfg(target_os = "macos")]
    let _ = links;
    Some(resolved)
}

fn link(anchor: &cap_std::fs::Dir, source: &Path, target: &Path) -> Result<(), Fault> {
    let parent = policy::directories(anchor, target.parent().expect("tool metadata has a parent"))?;
    parent
        .symlink_contents(
            source,
            target.file_name().expect("tool metadata has a name"),
        )
        .map_err(runtime::failure)
}

#[cfg(test)]
mod tests;
