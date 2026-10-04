use std::{collections::BTreeSet, path::PathBuf};

pub fn list() -> Vec<String> {
    let mut candidates = Vec::new();
    #[cfg(unix)]
    {
        if let Ok(shells) = std::fs::read_to_string("/etc/shells") {
            candidates.extend(shells.lines().filter_map(|line| {
                let path = line.split('#').next()?.trim();
                path.starts_with('/').then(|| PathBuf::from(path))
            }));
        }
        candidates.extend(std::env::var_os("SHELL").map(PathBuf::from));
    }
    #[cfg(windows)]
    candidates.extend(std::env::var_os("COMSPEC").map(PathBuf::from));
    #[cfg(windows)]
    let names = ["pwsh", "powershell", "cmd"];
    #[cfg(not(windows))]
    let names = ["sh", "bash", "zsh", "fish", "nu", "pwsh"];
    candidates.extend(names.into_iter().filter_map(|name| which::which(name).ok()));
    candidates
        .into_iter()
        .filter_map(|path| which::which(path).ok())
        .filter_map(|path| path.into_os_string().into_string().ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
