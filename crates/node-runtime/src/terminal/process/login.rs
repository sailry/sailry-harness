use crate::terminal::failure;
use portable_pty::CommandBuilder;
use sailry_protocol::Fault;
use std::path::Path;

pub(super) fn configure(
    configured: &str,
    root: &Path,
    command: &mut CommandBuilder,
) -> Result<(), Fault> {
    if !configured.is_empty() {
        let shell = which::which_in(configured, command.get_env("PATH"), root).map_err(failure)?;
        command.env("SHELL", shell);
    }
    Ok(())
}
