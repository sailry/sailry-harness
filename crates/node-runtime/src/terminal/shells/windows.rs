use crate::process::Environment;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) fn candidates(environment: &Environment) -> Vec<PathBuf> {
    let mut paths = locations(|name| std::env::var_os(name));
    paths.push(environment.shell().into());
    if let Some(path) = environment.path() {
        for directory in std::env::split_paths(path).filter(|path| path.is_absolute()) {
            paths.extend(
                [
                    "pwsh.exe",
                    "powershell.exe",
                    "cmd.exe",
                    "nu.exe",
                    "bash.exe",
                ]
                .map(|name| directory.join(name)),
            );
        }
    }
    if let Ok(git) = which::which_in("git.exe", environment.path(), "/")
        && let Some(root) = git_root(&git)
    {
        paths.extend(git_shell(root));
    }
    for hive in [
        windows_registry::CURRENT_USER,
        windows_registry::LOCAL_MACHINE,
    ] {
        for prefix in ["SOFTWARE", "SOFTWARE\\WOW6432Node"] {
            registry(hive, prefix, &mut paths);
        }
    }
    paths
}

fn locations(value: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(system) = value("SystemRoot").map(PathBuf::from) {
        paths.push(system.join("System32/cmd.exe"));
        paths.push(system.join("System32/WindowsPowerShell/v1.0/powershell.exe"));
        paths.push(system.join("SysWOW64/WindowsPowerShell/v1.0/powershell.exe"));
    }
    for name in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = value(name).map(PathBuf::from) {
            paths.extend(powershell(&root.join("PowerShell")));
            paths.extend(git_shell(&root.join("Git")));
            paths.extend(additional(&root));
        }
    }
    if let Some(local) = value("LOCALAPPDATA").map(PathBuf::from) {
        for root in [local.join("Microsoft"), local.join("Programs")] {
            paths.extend(powershell(&root.join("PowerShell")));
        }
        paths.push(local.join("Microsoft/WinGet/Links/pwsh.exe"));
        paths.extend(git_shell(&local.join("Programs/Git")));
        paths.extend(additional(&local.join("Programs")));
        paths.push(local.join("Microsoft/WinGet/Links/nu.exe"));
    }
    if let Some(drive) = value("SystemDrive").map(|mut drive| {
        drive.push("\\");
        PathBuf::from(drive)
    }) {
        for name in ["msys64", "msys32", "msys2", "cygwin64", "cygwin"] {
            paths.push(drive.join(name).join("usr/bin/bash.exe"));
            paths.push(drive.join(name).join("bin/bash.exe"));
        }
    }
    paths
}

fn additional(root: &Path) -> Vec<PathBuf> {
    [
        "nu/bin/nu.exe",
        "nushell/nu.exe",
        "nushell/bin/nu.exe",
        "MSYS2/usr/bin/bash.exe",
        "Cygwin/bin/bash.exe",
        "Cygwin64/bin/bash.exe",
    ]
    .map(|path| root.join(path))
    .into()
}

pub(super) fn is_shell(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if [
        "wsl.exe",
        "wt.exe",
        "WindowsTerminal.exe",
        "iTerm",
        "iTerm2",
    ]
    .iter()
    .any(|excluded| name.eq_ignore_ascii_case(excluded))
    {
        return false;
    }
    if name.eq_ignore_ascii_case("bash.exe")
        && let Some(parent) = path.parent()
    {
        let parent_name = parent
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if ["System32", "SysWOW64", "Sysnative"]
            .iter()
            .any(|system| parent_name.eq_ignore_ascii_case(system))
            || (parent_name.eq_ignore_ascii_case("WindowsApps")
                && parent
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("Microsoft")))
        {
            return false;
        }
    }
    true
}

fn powershell(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("pwsh.exe"))
        .collect()
}

fn git_shell(root: &Path) -> Option<PathBuf> {
    [root.join("bin/bash.exe"), root.join("usr/bin/bash.exe")]
        .into_iter()
        .find(|path| path.is_file())
}

fn git_root(executable: &Path) -> Option<&Path> {
    let bin = executable.parent()?;
    let root = bin.parent()?;
    if root
        .file_name()?
        .to_str()?
        .to_ascii_lowercase()
        .starts_with("mingw")
    {
        root.parent()
    } else {
        Some(root)
    }
}

fn registry(hive: &windows_registry::Key, prefix: &str, paths: &mut Vec<PathBuf>) {
    if let Ok(key) = hive.open(format!("{prefix}\\GitForWindows"))
        && let Ok(root) = key.get_string("InstallPath")
    {
        paths.extend(git_shell(Path::new(&root)));
    }
    if let Ok(key) = hive.open(format!("{prefix}\\Cygwin\\setup"))
        && let Ok(root) = key.get_string("rootdir")
    {
        paths.push(PathBuf::from(root).join("bin/bash.exe"));
    }
    for name in ["MSYS2 64bit", "MSYS2 32bit", "MSYS2"] {
        if let Ok(key) = hive.open(format!(
            "{prefix}\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}"
        )) && let Ok(root) = key.get_string("InstallLocation")
        {
            paths.push(PathBuf::from(root).join("usr/bin/bash.exe"));
        }
    }
    for name in ["pwsh.exe", "powershell.exe", "nu.exe"] {
        if let Ok(key) = hive.open(format!(
            "{prefix}\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{name}"
        )) && let Ok(path) = key.get_string("")
        {
            paths.push(path.into());
        }
    }
    if let Ok(key) = hive.open(format!(
        "{prefix}\\Microsoft\\PowerShell\\1\\ShellIds\\Microsoft.PowerShell"
    )) && let Ok(path) = key.get_string("Path")
    {
        paths.push(path.into());
    }
    if let Ok(key) = hive.open(format!(
        "{prefix}\\Microsoft\\PowerShellCore\\InstalledVersions"
    )) && let Ok(versions) = key.keys()
    {
        for version in versions {
            if let Ok(version) = key.open(version)
                && let Ok(root) = version.get_string("InstallLocation")
            {
                paths.push(PathBuf::from(root).join("pwsh.exe"));
            }
        }
    }
}

#[cfg(test)]
mod tests;
