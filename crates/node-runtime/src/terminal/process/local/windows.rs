use portable_pty::CommandBuilder;
use sailry_protocol::{Fault, terminal::Settings};
use std::{
    ffi::OsString,
    path::{Component, Path, Prefix},
};

pub(super) fn configure(
    settings: &Settings,
    root: &Path,
    command: &mut CommandBuilder,
) -> Result<(), Fault> {
    if settings.shell.is_empty() {
        return Ok(());
    }
    command.get_argv_mut().push(settings.shell.clone().into());
    command.args(&settings.shell_arguments);
    let executable = Path::new(&settings.shell);
    if executable
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("wsl.exe"))
    {
        let selected = settings
            .shell_arguments
            .windows(2)
            .find(|pair| matches!(pair[0].as_str(), "--distribution" | "-d"))
            .map(|pair| pair[1].as_str());
        let directory = directory(root, selected)?;
        command.arg("--cd");
        command.arg(directory);
        if matches!(root.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)))
        {
            command.cwd(
                executable
                    .parent()
                    .ok_or_else(|| Fault::invalid("WSL executable path is invalid"))?,
            );
        }
    }
    Ok(())
}

fn directory(root: &Path, selected: Option<&str>) -> Result<OsString, Fault> {
    let mut components = root.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(Fault::invalid(
            "WSL worktree must be an absolute Windows path",
        ));
    };
    match prefix.kind() {
        Prefix::Disk(_) | Prefix::VerbatimDisk(_) if root.is_absolute() => {
            Ok(crate::process::pty_directory(root).into_os_string())
        }
        Prefix::UNC(server, distribution) | Prefix::VerbatimUNC(server, distribution)
            if server.to_str().is_some_and(|server| {
                server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost")
            }) =>
        {
            let distribution = distribution
                .to_str()
                .ok_or_else(|| Fault::invalid("WSL distribution name is not Unicode"))?;
            if !selected.is_some_and(|selected| selected.eq_ignore_ascii_case(distribution)) {
                return Err(Fault::invalid(
                    "WSL worktree requires its matching distribution",
                ));
            }
            let path = components
                .as_path()
                .to_str()
                .ok_or_else(|| Fault::invalid("WSL worktree path is not Unicode"))?;
            Ok(format!("/{}", path.trim_start_matches('\\').replace('\\', "/")).into())
        }
        _ => Err(Fault::invalid(
            "WSL worktree requires a local drive or matching distribution path",
        )),
    }
}

#[cfg(test)]
mod tests;
