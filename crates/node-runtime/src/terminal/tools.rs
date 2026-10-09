use sailry_protocol::{
    ErrorCode, Fault,
    terminal::{Settings, Tool, ToolInfo},
};
use std::path::{Path, PathBuf};

pub fn list(
    root: &Path,
    settings: &Settings,
    environment: &crate::process::Environment,
) -> Vec<ToolInfo> {
    Tool::ALL
        .into_iter()
        .map(|tool| ToolInfo {
            tool,
            available: resolve(root, settings, tool, environment).is_ok(),
        })
        .collect()
}

pub fn resolve(
    root: &Path,
    settings: &Settings,
    tool: Tool,
    environment: &crate::process::Environment,
) -> Result<PathBuf, Fault> {
    let mut command = portable_pty::CommandBuilder::new(tool.executable());
    environment.apply_terminal(&mut command);
    for (key, value) in &settings.environment {
        command.env(key, value);
    }
    which::which_in(tool.executable(), command.get_env("PATH"), root).map_err(|_| {
        Fault::new(
            ErrorCode::NotConfigured,
            "terminal tool is unavailable on this Node",
        )
    })
}
