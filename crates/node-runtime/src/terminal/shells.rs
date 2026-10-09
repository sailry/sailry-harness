use std::{collections::BTreeSet, path::PathBuf};

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
    }
    #[cfg(windows)]
    candidates.extend(Some(environment.shell()).map(PathBuf::from));
    #[cfg(windows)]
    let names = ["pwsh", "powershell", "cmd"];
    #[cfg(not(windows))]
    let names = ["sh", "bash", "zsh", "fish", "nu", "pwsh"];
    candidates.extend(
        names
            .into_iter()
            .filter_map(|name| which::which_in(name, environment.path(), "/").ok()),
    );
    candidates
        .into_iter()
        .filter_map(|path| which::which_in(path, environment.path(), "/").ok())
        .filter_map(|path| path.into_os_string().into_string().ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
