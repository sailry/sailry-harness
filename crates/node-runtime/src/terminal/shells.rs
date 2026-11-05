use std::{collections::BTreeSet, path::PathBuf};

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

pub fn list(environment: &crate::process::Environment) -> Vec<String> {
    let mut candidates = Vec::new();
    #[cfg(unix)]
    {
        if let Ok(shells) = std::fs::read_to_string("/etc/shells") {
            candidates.extend(shells.lines().filter_map(|line| {
                let path = line.split('#').next()?.trim();
                path.starts_with('/').then(|| PathBuf::from(path))
            }));
        }
        candidates.extend(environment.login_shell().map(PathBuf::from));
        candidates.push(environment.shell().into());
    }
    #[cfg(target_os = "macos")]
    for directory in ["/bin", "/usr/bin", "/opt/homebrew/bin", "/usr/local/bin"] {
        candidates.extend(
            [
                "sh", "bash", "zsh", "fish", "nu", "pwsh", "csh", "tcsh", "ksh",
            ]
            .map(|name| PathBuf::from(directory).join(name)),
        );
    }
    #[cfg(windows)]
    candidates.extend(windows::candidates(environment));
    #[cfg(windows)]
    let names = ["pwsh.exe", "powershell.exe", "cmd.exe"];
    #[cfg(not(windows))]
    let names = ["sh", "bash", "zsh", "fish", "nu", "pwsh"];
    candidates.extend(
        names
            .into_iter()
            .filter_map(|name| which::which_in(name, environment.path(), "/").ok()),
    );
    available(candidates, environment)
}

fn available(candidates: Vec<PathBuf>, environment: &crate::process::Environment) -> Vec<String> {
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter_map(|path| which::which_in(path, environment.path(), "/").ok())
        .filter_map(|path| path.into_os_string().into_string().ok())
        .filter(|path| {
            let key = if cfg!(windows) {
                path.replace('/', "\\").to_lowercase()
            } else {
                path.clone()
            };
            seen.insert(key)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
