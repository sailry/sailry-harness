//! Project guides use the admitted Node worktree and its existing file reader.
use crate::{files::Files, store::Ingress};
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, Output, WorktreeId};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    File(String),
    Directory(String),
}

pub(super) async fn load(
    ingress: &Ingress,
    worktree: WorktreeId,
    scopes: &[Scope],
    stop: &CancellationToken,
) -> Result<String, Fault> {
    let paths = candidates(scopes)?;
    let root = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(cancelled()),
        root = ingress.worktree_root(worktree) => root?,
    };
    load_from(&ingress.files, root, worktree, paths, stop).await
}

fn candidates(scopes: &[Scope]) -> Result<Vec<String>, Fault> {
    let mut directories = BTreeSet::from([String::new()]);
    for scope in scopes {
        let (path, directory) = match scope {
            Scope::File(path) => (path, false),
            Scope::Directory(path) => (path, true),
        };
        let parts = crate::files::path::components(path, directory)?;
        let count = parts.len() - usize::from(!directory);
        for depth in 1..=count {
            directories.insert(parts[..depth].join("/"));
        }
    }
    Ok(directories
        .into_iter()
        .map(|directory| {
            if directory.is_empty() {
                "AGENTS.md".into()
            } else {
                format!("{directory}/AGENTS.md")
            }
        })
        .collect())
}

async fn load_from(
    files: &Files,
    root: PathBuf,
    worktree: WorktreeId,
    paths: Vec<String>,
    stop: &CancellationToken,
) -> Result<String, Fault> {
    let mut instruction = String::new();
    for path in paths {
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let command = Command::ReadFile {
            worktree,
            path: path.clone(),
        };
        let output = files.inspect(root.clone(), command, stop.clone()).await;
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let file = match output {
            Ok(Output::FileContent(file)) => file,
            Ok(_) => {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "project instruction reader returned an unexpected output",
                ));
            }
            Err(error) if error.code == ErrorCode::NotFound => continue,
            Err(error) => {
                return Err(Fault::new(
                    error.code,
                    format!(
                        "project instructions at {path} could not be read: {}",
                        error.message
                    ),
                ));
            }
        };
        if file.truncated {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                format!("project instructions at {path} exceed the file read limit"),
            ));
        }
        if file.text.trim().is_empty() {
            continue;
        }
        if instruction.is_empty() {
            instruction.push_str(
                "Project instructions\nThese guides apply only within their stated worktree-relative scope. More specific guides override ancestor guidance within that scope. Explicit user instructions and this turn's mode and permissions remain authoritative.",
            );
        }
        let scope = path.strip_suffix("/AGENTS.md").unwrap_or(".");
        instruction.push_str(&format!("\n\nGuide: {path}\nScope: {scope}\n{}", file.text));
    }
    Ok(instruction)
}

fn cancelled() -> Fault {
    Fault::new(
        ErrorCode::Cancelled,
        "project instruction loading was cancelled",
    )
}

#[cfg(test)]
mod tests;
