//! A patch is parsed and resolved into a structured change set in this tool layer,
//! so the durable command and its controllers keep typed entries. The set is then
//! validated and previewed before its single approval, and validated again after it
//! because files may change while the decision is pending. Rejected sets return
//! per-file reasons without a durable write.
use super::*;
use crate::files::{changes::Planned, patch};
use sailry_protocol::{
    ChangePreview, ChangesStatus, ErrorCode, Fault, GitDiff, GitDiffScope, MAX_PREVIEW_BYTES,
    Output, PreviewFile, conversation::Decision,
};

impl HostOperation {
    pub(super) async fn apply_patch(
        &self,
        context: &CallContext,
        arguments: Value,
        authorized: Value,
    ) -> Result<Value, Fault> {
        if self.mode == sailry_protocol::WorkMode::Plan {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "planning turns cannot authorize tools with side effects",
            ));
        }
        let hunks = patch::parse(&files::patch(arguments)?)?;
        let worktree = self
            .context
            .worktree
            .ok_or_else(|| Fault::invalid("file operation requires a worktree"))?;
        let root = self.ingress.worktree_root(worktree).await?;
        let mut paths = Vec::new();
        let mut owners = Vec::new();
        for (index, hunk) in hunks.iter().enumerate() {
            for path in hunk.paths() {
                paths.push((root.clone(), path.to_owned()));
                owners.push(index);
            }
        }
        let mut faults = vec![None; hunks.len()];
        for (index, fault) in owners
            .into_iter()
            .zip(self.ingress.validate_paths(paths).await?)
        {
            if faults[index].is_none() {
                faults[index] = fault;
            }
        }
        let observations = context.observed.clone().unwrap_or_default();
        let targets = hunks
            .into_iter()
            .zip(faults)
            .map(|(hunk, fault)| {
                let observed = hunk
                    .paths()
                    .next()
                    .and_then(|path| observations.revision(path))
                    .map(str::to_owned);
                patch::Target {
                    hunk,
                    observed,
                    fault,
                }
            })
            .collect();
        let (changes, checked): (Vec<_>, Vec<_>) = self
            .ingress
            .files
            .resolve_patch(root.clone(), targets, self.stop.clone())
            .await?
            .into_iter()
            .unzip();
        let planned = match self
            .ingress
            .files
            .plan_changes(root.clone(), changes.clone(), checked, self.stop.clone())
            .await?
        {
            Ok(planned) => planned,
            Err(rejected) => return self.finish(context, Output::FileChanges(rejected)),
        };
        let mut request = Request::new(
            self.transport.target(),
            Command::ApplyChanges {
                worktree,
                changes: changes.clone(),
            },
        )
        .with_plugin(self.context.clone());
        let preview = tokio::task::spawn_blocking(move || preview(planned))
            .await
            .map_err(|_| Fault::new(ErrorCode::Internal, "file worker failed"))??;
        let binding = crate::store::agent::ApprovalBinding {
            owner: self.context.package.clone(),
            tool: self.name.clone(),
            operation: Some(self.operation),
        };
        let decision = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "file change approval was cancelled")),
            decision = self.ingress.confirm_changes(self.turn, context.request_with(authorized.clone()), binding, preview) => decision?,
        };
        if !matches!(
            decision,
            Decision::Approve | Decision::ApproveSession | Decision::ApprovePrefix(_)
        ) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "file changes were denied",
            ));
        }
        if let Err(rejected) = self
            .ingress
            .files
            .plan_changes(root, changes, Vec::new(), self.stop.clone())
            .await?
        {
            return self.finish(context, Output::FileChanges(rejected));
        }
        request.id = self
            .authorize(context, authorized, Some(request.command.clone()))
            .await?;
        self.ingress
            .checkpoint_changes(self.turn, &request, self.stop.clone())
            .await?;
        let output = self.dispatch(context, request).await?;
        let output = serde_json::from_value(output)
            .map_err(|error| Fault::new(ErrorCode::Internal, error.to_string()))?;
        self.finish(context, output)
    }

    /// Rejected and partial sets keep their structured result and are marked failed.
    fn finish(&self, context: &CallContext, output: Output) -> Result<Value, Fault> {
        if let Output::FileChanges(changes) = &output
            && changes.status != ChangesStatus::Applied
            && let Some(timeline) = self.ingress.agents.timeline(self.turn)
        {
            timeline.fail_tool(
                context.branch(),
                context.function_call_id(),
                &self.name,
                changes
                    .files
                    .iter()
                    .find_map(|file| file.error.as_ref().map(|error| error.code))
                    .map(sailry_protocol::tool::Failure::from)
                    .unwrap_or_else(|| sailry_protocol::tool::Failure::tool(None)),
                None,
            );
        }
        serde_json::to_value(output)
            .map_err(|error| Fault::new(ErrorCode::Internal, error.to_string()))
    }
}

/// Diffs share one bounded budget; counts stay complete when text is shortened.
fn preview(planned: Vec<Planned>) -> Result<ChangePreview, Fault> {
    let mut remaining = MAX_PREVIEW_BYTES;
    let mut files = Vec::with_capacity(planned.len());
    for entry in planned {
        let mut diff = GitDiff {
            path: entry
                .destination
                .clone()
                .unwrap_or_else(|| entry.path.clone()),
            scope: GitDiffScope::All,
            text: String::new(),
            additions: 0,
            deletions: 0,
            binary: false,
            truncated: false,
        };
        let after = match (&entry.after, &entry.before) {
            (Some(after), _) => Some(after.as_str()),
            (None, Some(_)) if entry.kind == sailry_protocol::ChangeKind::Deleted => Some(""),
            _ => None,
        };
        if let Some(after) = after {
            crate::store::agent::checkpoints::append_diff(
                &mut diff,
                entry.before.as_deref(),
                after,
                &mut remaining,
            )?;
        }
        files.push(PreviewFile {
            path: entry.path,
            destination: entry.destination,
            kind: entry.kind,
            additions: diff.additions,
            deletions: diff.deletions,
            diff: diff.text,
            truncated: diff.truncated,
        });
    }
    Ok(ChangePreview { files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{ChangeKind, FileChange, FileEdit};

    #[test]
    fn previews_each_file_within_one_budget() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::fs::write(root.join("edit.txt"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("move.txt"), "same\n").unwrap();
        std::fs::write(root.join("large.txt"), "x\n".repeat(40_000)).unwrap();
        let revision = |text: &str| blake3::hash(text.as_bytes()).to_hex().to_string();
        let changes = [
            FileChange::Create {
                path: "new.txt".into(),
                text: "added\n".into(),
            },
            FileChange::Edit {
                path: "edit.txt".into(),
                expected_revision: revision("one\ntwo\n"),
                edits: vec![FileEdit {
                    old_text: "two".into(),
                    new_text: "three".into(),
                    replace_all: false,
                }],
            },
            FileChange::Move {
                path: "move.txt".into(),
                destination: "moved.txt".into(),
                expected_revision: revision("same\n"),
            },
            FileChange::Edit {
                path: "large.txt".into(),
                expected_revision: revision(&"x\n".repeat(40_000)),
                edits: vec![FileEdit {
                    old_text: "x\n".into(),
                    new_text: "y\n".into(),
                    replace_all: true,
                }],
            },
        ];
        let planned = crate::files::changes::plan(
            crate::files::Access::Restricted.scope(&root),
            &changes,
            &[],
            &sailry_link::CancellationToken::new(),
        )
        .unwrap();
        let preview = preview(planned).unwrap();
        assert_eq!(
            preview
                .files
                .iter()
                .map(|file| file.kind)
                .collect::<Vec<_>>(),
            [
                ChangeKind::Added,
                ChangeKind::Modified,
                ChangeKind::Renamed,
                ChangeKind::Modified
            ]
        );
        assert_eq!(
            (preview.files[0].additions, preview.files[0].deletions),
            (1, 0)
        );
        assert!(preview.files[1].diff.contains("-two\n+three"));
        assert_eq!(preview.files[2].destination.as_deref(), Some("moved.txt"));
        assert!(preview.files[2].diff.is_empty());
        assert_eq!(
            (preview.files[3].additions, preview.files[3].deletions),
            (40_000, 40_000)
        );
        assert!(preview.files[3].truncated);
        assert!(
            preview
                .files
                .iter()
                .map(|file| file.diff.len())
                .sum::<usize>()
                <= MAX_PREVIEW_BYTES
        );
        // The preview reads only; validation leaves every file unchanged.
        assert!(!root.join("new.txt").exists() && root.join("move.txt").exists());
    }
}
