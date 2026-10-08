use super::instructions;
use crate::{files::Files, store::Ingress};
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, WorktreeId};
use std::path::Path;

pub(in crate::agent) async fn capture(
    ingress: &Ingress,
    worktree: WorktreeId,
    stop: &CancellationToken,
) -> Result<String, Fault> {
    let Some(root) = ingress.profile() else {
        return Ok(String::new());
    };
    load_from(&ingress.files, root, worktree, stop).await
}

async fn load_from(
    files: &Files,
    root: &Path,
    worktree: WorktreeId,
    stop: &CancellationToken,
) -> Result<String, Fault> {
    let Some(file) = instructions::read(files, root, worktree, "instructions.md", stop).await?
    else {
        return Ok(String::new());
    };
    if file.text.trim().is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "Personal instructions\nThese are the user's preferences for tasks on this machine. Explicit instructions for the current task, work mode, tool permissions and delegated scope remain authoritative. Apply scoped project guides within their stated directories.\nSource: instructions.md\n{}",
        file.text
    ))
}

#[cfg(test)]
mod tests;
