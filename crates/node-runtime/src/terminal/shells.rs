use sailry_link::CancellationToken;
use sailry_protocol::{Fault, terminal::Shell};
use std::{collections::BTreeSet, path::PathBuf};

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

pub async fn list(
    environment: &crate::process::Environment,
    stop: &CancellationToken,
) -> Result<Vec<Shell>, Fault> {
    let source = environment.clone();
    let shells = tokio::task::spawn_blocking(move || native(&source))
        .await
        .map_err(|_| super::failure("shell discovery worker failed"))?;
    #[cfg(windows)]
    let shells = {
        let mut shells = shells;
        shells.extend(crate::wsl::shells(environment, stop).await?);
        shells.sort();
        shells.dedup();
        shells
    };
    #[cfg(not(windows))]
    let _ = stop;
    Ok(shells)
}

fn native(environment: &crate::process::Environment) -> Vec<Shell> {
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
    #[cfg(not(windows))]
    candidates.extend(
        ["sh", "bash", "zsh", "fish", "nu", "pwsh"]
            .into_iter()
            .filter_map(|name| which::which_in(name, environment.path(), "/").ok()),
    );
    available(candidates, environment)
}

fn available(candidates: Vec<PathBuf>, environment: &crate::process::Environment) -> Vec<Shell> {
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter_map(|path| which::which_in(path, environment.path(), "/").ok())
        .filter(|path| {
            #[cfg(windows)]
            let identity = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            #[cfg(not(windows))]
            let identity = path.clone();
            #[cfg(windows)]
            if !windows::is_shell(path) || !windows::is_shell(&identity) {
                return false;
            }
            let key = identity.to_string_lossy();
            let key = if cfg!(windows) {
                key.replace('/', "\\").to_lowercase()
            } else {
                key.into_owned()
            };
            seen.insert(key)
        })
        .filter_map(|path| path.into_os_string().into_string().ok())
        .map(|path| Shell {
            path,
            arguments: Vec::new(),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
