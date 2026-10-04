//! Standard Ghostty shell hooks, installed for this PTY without editing user files.
//! Zsh and Fish explicitly opt into OSC 133 prompt redraw: libghostty-vt defaults
//! it off, unlike the full Ghostty application these hooks were written for.
use super::super::failure;
use portable_pty::CommandBuilder;
use sailry_protocol::Fault;
use std::{ffi::OsString, path::Path};

pub(super) fn configure(command: &mut CommandBuilder) -> Result<Option<tempfile::TempDir>, Fault> {
    let shell = command.get_shell();
    let name = Path::new(&shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    // Ghostty skips Apple's patched Bash: it ignores ENV even with --posix,
    // so injecting would only leave a user's login shell in POSIX mode.
    // See the pinned Ghostty src/termio/shell_integration.zig::detectShell.
    #[cfg(target_os = "macos")]
    if name == "bash"
        && std::fs::canonicalize(&shell).is_ok_and(|path| path == Path::new("/bin/bash"))
    {
        return Ok(None);
    }
    if !matches!(name, "zsh" | "bash" | "fish") {
        return Ok(None);
    }
    let directory = tempfile::Builder::new()
        .prefix("sailry-shell-")
        .tempdir()
        .map_err(failure)?;
    let root = directory.path();
    let write = |name: &str, text: &str| std::fs::write(root.join(name), text).map_err(failure);
    command.env("GHOSTTY_SHELL_FEATURES", "title");
    match name {
        "zsh" => {
            write(".zshenv", include_str!("integration/zsh/.zshenv"))?;
            write(
                "ghostty-integration",
                include_str!("integration/zsh/ghostty-integration"),
            )?;
            if let Some(original) = command.get_env("ZDOTDIR").map(OsString::from) {
                command.env("GHOSTTY_ZSH_ZDOTDIR", original);
            }
            command.env("ZDOTDIR", root);
        }
        "bash" => {
            write(
                "ghostty.bash",
                include_str!("integration/bash/ghostty.bash"),
            )?;
            write(
                "bash-preexec.sh",
                include_str!("integration/bash/bash-preexec.sh"),
            )?;
            if let Some(original) = command.get_env("ENV").map(OsString::from) {
                command.env("GHOSTTY_BASH_ENV", original);
            }
            command.env("ENV", root.join("ghostty.bash"));
            command.env("GHOSTTY_BASH_INJECT", "1");
            if command.get_env("HISTFILE").is_none()
                && let Some(home) = command.get_env("HOME")
            {
                let path = Path::new(home).join(".bash_history");
                command.env("HISTFILE", path);
                command.env("GHOSTTY_BASH_UNEXPORT_HISTFILE", "1");
            }
            // Ghostty's Bash script restores POSIX mode and runs the standard
            // login startup files before installing the prompt hooks.
            *command.get_argv_mut() = vec![
                shell.into(),
                "--posix".into(),
                "--login".into(),
                "-i".into(),
            ];
        }
        "fish" => {
            std::fs::create_dir_all(root.join("fish/vendor_conf.d")).map_err(failure)?;
            write(
                "fish/vendor_conf.d/ghostty-shell-integration.fish",
                include_str!("integration/fish/vendor_conf.d/ghostty-shell-integration.fish"),
            )?;
            let existing = command
                .get_env("XDG_DATA_DIRS")
                .unwrap_or(std::ffi::OsStr::new("/usr/local/share:/usr/share"));
            let mut paths = OsString::from(root);
            paths.push(":");
            paths.push(existing);
            command.env("XDG_DATA_DIRS", paths);
            command.env("GHOSTTY_SHELL_INTEGRATION_XDG_DIR", root);
        }
        _ => unreachable!(),
    }
    Ok(Some(directory))
}
