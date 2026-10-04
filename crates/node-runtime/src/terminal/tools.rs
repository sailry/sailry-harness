use sailry_protocol::{
    ErrorCode, Fault,
    terminal::{Settings, Tool, ToolInfo},
};
use std::path::{Path, PathBuf};

pub fn list(root: &Path, settings: &Settings) -> Vec<ToolInfo> {
    Tool::ALL
        .into_iter()
        .map(|tool| ToolInfo {
            tool,
            available: resolve(root, settings, tool).is_ok(),
        })
        .collect()
}

pub fn resolve(root: &Path, settings: &Settings, tool: Tool) -> Result<PathBuf, Fault> {
    let mut command = portable_pty::CommandBuilder::new(tool.executable());
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
