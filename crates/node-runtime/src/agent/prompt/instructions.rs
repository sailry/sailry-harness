//! Project guides use the admitted Node worktree and its existing file reader.
use crate::{files::Files, store::Ingress};
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, FileContent, Output, WorktreeId};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const FILENAMES: [&str; 3] = ["AGENTS.md", "CLAUDE.md", "GEMINI.md"];

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
    let directories = candidates(scopes)?;
    let root = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(cancelled()),
        root = ingress.worktree_root(worktree) => root?,
    };
    load_from(&ingress.files, root, worktree, directories, stop).await
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
    Ok(directories.into_iter().collect())
}

async fn load_from(
    files: &Files,
    root: PathBuf,
    worktree: WorktreeId,
    directories: Vec<String>,
    stop: &CancellationToken,
) -> Result<String, Fault> {
    let mut instruction = String::new();
    for directory in directories {
        for filename in FILENAMES {
            let path = if directory.is_empty() {
                filename.into()
            } else {
                format!("{directory}/{filename}")
            };
            let Some(file) = read(files, &root, worktree, &path, stop).await? else {
                continue;
            };
            if !file.text.trim().is_empty() {
                if instruction.is_empty() {
                    instruction.push_str(
                        "Project instructions\nThese guides are already loaded; do not reread them with a shell. They apply only within their stated project-relative scope. More specific guides override ancestor guidance within that scope. Explicit user instructions and this turn's mode and permissions remain authoritative.",
                    );
                }
                let scope = if directory.is_empty() {
                    "."
                } else {
                    &directory
                };
                instruction.push_str(&format!("\n\nGuide: {path}\nScope: {scope}\n{}", file.text));
            }
            break;
        }
    }
    Ok(instruction)
}

pub(super) async fn read(
    files: &Files,
    root: &Path,
    worktree: WorktreeId,
    path: &str,
    stop: &CancellationToken,
) -> Result<Option<FileContent>, Fault> {
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    let output = files
        .inspect(
            root.to_path_buf(),
            Command::ReadFile {
                worktree,
                path: path.into(),
            },
            stop.clone(),
        )
        .await;
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    let file = match output {
        Ok(Output::FileContent(file)) => file,
        Ok(_) => {
            return Err(Fault::new(
                ErrorCode::Internal,
                "instruction reader returned an unexpected output",
            ));
        }
        Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
        Err(error) => {
            return Err(Fault::new(
                error.code,
                format!(
                    "instructions at {path} could not be read: {}",
                    error.message
                ),
            ));
        }
    };
    if file.truncated {
        return Err(Fault::invalid(format!(
            "instructions at {path} exceed the file read limit"
        )));
    }
    Ok(Some(file))
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "instruction loading was cancelled")
}

#[cfg(test)]
mod tests;
