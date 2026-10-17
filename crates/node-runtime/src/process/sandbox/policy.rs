use super::{Environment, runtime};
use adk_sandbox::sandbox::{AccessMode, AllowedPath, SandboxPolicy, SandboxPolicyBuilder};
use cap_fs_ext::DirExt;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) type Variables = BTreeMap<OsString, OsString>;

pub(super) fn create(
    root: &Path,
    profile: Option<&Path>,
    scratch: &tempfile::TempDir,
    state: &Path,
    environment: &Environment,
    inputs: Option<&Path>,
) -> Result<(SandboxPolicy, Variables, super::toolchains::Links), Fault> {
    let root = root.canonicalize().map_err(runtime::failure)?;
    let scratch = scratch.path().canonicalize().map_err(runtime::failure)?;
    let home = environment.home().and_then(|path| path.canonicalize().ok());
    let profile = profile
        .map(Path::canonicalize)
        .transpose()
        .map_err(runtime::failure)?;
    validate(&root, home.as_deref(), profile.as_deref())?;
    let state = state.canonicalize().map_err(runtime::failure)?;
    let anchor = crate::files::path::root(&state)?;
    let mut policy = SandboxPolicyBuilder::new()
        .allow_process_spawn()
        .allow_read_write(&root)
        .allow_read_write(&scratch)
        .build();
    if let Some(inputs) = inputs {
        policy.allowed_paths.push(AllowedPath {
            path: inputs.canonicalize().map_err(runtime::failure)?,
            mode: AccessMode::ReadOnly,
        });
    }
    let mut variables = Variables::new();
    for (name, relative) in [
        ("HOME", "home"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("CARGO_HOME", "cargo"),
        ("npm_config_cache", "npm"),
        ("PIP_CACHE_DIR", "pip"),
        ("PYTHONPYCACHEPREFIX", "pycache"),
    ] {
        let path = state.join(relative);
        directories(&anchor, Path::new(relative))?;
        variables.insert(name.into(), path.into_os_string());
    }
    for name in ["TMPDIR", "TMP", "TEMP"] {
        variables.insert(name.into(), scratch.clone().into_os_string());
    }
    let npm_config = state.join("npmrc");
    anchor
        .open_with(
            "npmrc",
            cap_std::fs::OpenOptions::new().write(true).create_new(true),
        )
        .map_err(runtime::failure)?;
    variables.insert("npm_config_userconfig".into(), npm_config.into_os_string());
    variables.insert("HISTFILE".into(), state.join("history").into_os_string());
    variables.insert("BASH_ENV".into(), "/dev/null".into());
    variables.insert("ENV".into(), "/dev/null".into());
    #[cfg(target_os = "macos")]
    variables.insert("xcrun_db".into(), state.join("xcrun_db").into_os_string());
    if let Some(home) = &home {
        for relative in [".cargo/bin", ".rustup/toolchains", ".rustup/settings.toml"] {
            read(
                &mut policy,
                &home.join(relative),
                Some(home),
                profile.as_deref(),
            );
        }
        if home.join(".rustup/settings.toml").is_file() {
            variables.insert("RUSTUP_HOME".into(), home.join(".rustup").into_os_string());
        }
        for relative in [
            "registry/src",
            "registry/cache",
            "registry/index",
            "git/checkouts",
            "git/db",
        ] {
            cache(
                &mut policy,
                &home.join(".cargo").join(relative),
                &anchor,
                Path::new("cargo").join(relative).as_path(),
                home,
                profile.as_deref(),
            )?;
        }
        for relative in ["content-v2", "index-v5"] {
            cache(
                &mut policy,
                &home.join(".npm/_cacache").join(relative),
                &anchor,
                Path::new("npm/_cacache").join(relative).as_path(),
                home,
                profile.as_deref(),
            )?;
        }
    }
    let links = super::toolchains::configure(
        &mut policy,
        &mut variables,
        environment,
        home.as_deref(),
        profile.as_deref(),
        &anchor,
        inputs,
    )?;
    #[cfg(target_os = "macos")]
    {
        let selected = environment
            .developer()
            .unwrap_or_else(|| Path::new("/Applications/Xcode.app/Contents/Developer"));
        if let Some(developer) = read(&mut policy, selected, home.as_deref(), profile.as_deref())
            && developer.file_name() == Some(std::ffi::OsStr::new("Developer"))
            && let Some(contents) = developer.parent()
        {
            for relative in [
                "SharedFrameworks",
                "OtherFrameworks",
                "Frameworks",
                "SystemFrameworks",
            ] {
                read(
                    &mut policy,
                    &contents.join(relative),
                    home.as_deref(),
                    profile.as_deref(),
                );
            }
        }
        read(
            &mut policy,
            Path::new("/Library/Developer/CommandLineTools"),
            home.as_deref(),
            profile.as_deref(),
        );
    }
    #[cfg(target_os = "linux")]
    for path in [
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/etc/ld.so.cache",
        "/etc/localtime",
        "/etc/passwd",
        "/etc/group",
    ] {
        read(
            &mut policy,
            Path::new(path),
            home.as_deref(),
            profile.as_deref(),
        );
    }
    Ok((policy, variables, links))
}

fn validate(root: &Path, home: Option<&Path>, profile: Option<&Path>) -> Result<(), Fault> {
    if root.parent().is_none()
        || home.is_some_and(|home| home.starts_with(root))
        || profile.is_some_and(|profile| {
            profile.starts_with(root)
                || root.starts_with(profile) && !crate::workspaces::managed(root, profile)
        })
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "workspace overlaps protected data; request unrestricted access",
        ));
    }
    Ok(())
}

pub(super) fn readable(
    path: &Path,
    home: Option<&Path>,
    profile: Option<&Path>,
) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    if path.parent().is_none()
        || home.is_some_and(|home| home.starts_with(&path))
        || profile.is_some_and(|profile| path.starts_with(profile) || profile.starts_with(&path))
    {
        return None;
    }
    if let Some(home) = home
        && [
            ".ssh",
            ".gnupg",
            ".aws",
            ".azure",
            ".kube",
            ".docker",
            ".config",
            ".gitconfig",
            ".git-credentials",
            ".npmrc",
            ".netrc",
            ".pypirc",
            ".cargo/credentials",
            ".cargo/credentials.toml",
        ]
        .iter()
        .any(|relative| path.starts_with(home.join(relative)))
        && path != home.join(".config/mise/config.toml")
    {
        return None;
    }
    Some(path)
}

pub(super) fn read(
    policy: &mut SandboxPolicy,
    path: &Path,
    home: Option<&Path>,
    profile: Option<&Path>,
) -> Option<PathBuf> {
    let path = readable(path, home, profile)?;
    if !policy
        .allowed_paths
        .iter()
        .any(|entry| path.starts_with(&entry.path))
    {
        policy.allowed_paths.push(AllowedPath {
            path: path.clone(),
            mode: AccessMode::ReadOnly,
        });
    }
    Some(path)
}

fn cache(
    policy: &mut SandboxPolicy,
    source: &Path,
    anchor: &cap_std::fs::Dir,
    target: &Path,
    home: &Path,
    profile: Option<&Path>,
) -> Result<(), Fault> {
    let Some(source) = read(policy, source, Some(home), profile) else {
        return Ok(());
    };
    let parent = directories(anchor, target.parent().expect("cache target has a parent"))?;
    parent
        .symlink_contents(source, target.file_name().expect("cache target has a name"))
        .map_err(runtime::failure)?;
    Ok(())
}

pub(super) fn directories(
    anchor: &cap_std::fs::Dir,
    path: &Path,
) -> Result<cap_std::fs::Dir, Fault> {
    let mut directory = anchor.try_clone().map_err(runtime::failure)?;
    for component in path.components() {
        let name = component.as_os_str();
        if let Err(error) = directory.create_dir(name)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(runtime::failure(error));
        }
        directory = directory
            .open_dir_nofollow(name)
            .map_err(runtime::failure)?;
    }
    Ok(directory)
}
