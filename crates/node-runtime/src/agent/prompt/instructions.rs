//! Project guides use the admitted Node worktree and its existing file reader.
use crate::{files::Files, store::Ingress};
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, FileContent, Output, WorktreeId};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const FILENAMES: [&str; 3] = ["AGENTS.md", "CLAUDE.md", "GEMINI.md"];
const PREAMBLE: &str = "Project instructions\nThese guides are already loaded; do not reread them with a shell. They apply only within their stated project-relative scope. More specific guides override ancestor guidance within that scope. Explicit user instructions and this turn's mode and permissions remain authoritative. Project guides do not set delegation policy; the work mode alone decides when to use subagents.";

pub(in crate::agent) struct Guide {
    pub path: String,
    pub revision: String,
    text: String,
}

impl Guide {
    pub fn key(&self) -> String {
        key(&self.path)
    }

    pub fn identity(&self) -> serde_json::Value {
        serde_json::json!({"path": self.path, "revision": self.revision, "content": self.render()})
    }

    pub fn render(&self) -> String {
        let scope = self.path.rsplit_once('/').map_or(".", |(parent, _)| parent);
        format!(
            "{PREAMBLE}\n\nGuide: {}\nScope: {scope}\nRevision: {}\n{}",
            self.path, self.revision, self.text
        )
    }
}

pub(in crate::agent) fn key(path: &str) -> String {
    format!(
        "sailry.context.guide:{}",
        blake3::hash(path.as_bytes()).to_hex()
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::agent) enum Scope {
    File(String),
    Directory(String),
}

pub(in crate::agent) async fn capture(
    ingress: &Ingress,
    worktree: WorktreeId,
    scopes: &[Scope],
    stop: &CancellationToken,
) -> Result<Vec<Guide>, Fault> {
    let directories = candidates(scopes)?;
    let root = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(cancelled()),
        root = ingress.worktree_root(worktree) => root?,
    };
    collect(&ingress.files, root, worktree, directories, stop).await
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

async fn collect(
    files: &Files,
    root: PathBuf,
    worktree: WorktreeId,
    directories: Vec<String>,
    stop: &CancellationToken,
) -> Result<Vec<Guide>, Fault> {
    let mut guides = Vec::new();
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
                guides.push(Guide {
                    path,
                    revision: file
                        .revision
                        .ok_or_else(|| Fault::invalid("project guide has no content revision"))?,
                    text: file.text,
                });
            }
            break;
        }
    }
    Ok(guides)
}

#[cfg(test)]
async fn load_from(
    files: &Files,
    root: PathBuf,
    worktree: WorktreeId,
    directories: Vec<String>,
    stop: &CancellationToken,
) -> Result<String, Fault> {
    Ok(collect(files, root, worktree, directories, stop)
        .await?
        .iter()
        .map(Guide::render)
        .collect::<Vec<_>>()
        .join("\n\n"))
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
