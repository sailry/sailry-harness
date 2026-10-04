//! Registered worktree access stays bound to the admitted turn.
use super::*;

pub(super) async fn validate(binding: &Binding, command: &Command) -> Result<(), Fault> {
    let target = match command {
        Command::InspectGit { worktree }
        | Command::ReadGitDiff { worktree, .. }
        | Command::ReadGitLog { worktree, .. }
        | Command::ListWorktrees { worktree }
        | Command::RemoveWorktree { worktree, .. } => *worktree,
        Command::CreateManagedWorktree { source, .. } => *source,
        Command::RegisterWorktree { .. } => binding.worktree,
        _ => return Ok(()),
    };
    let entries = binding.ingress.agent_worktrees(binding.turn).await?;
    if !entries.iter().any(|entry| entry.id == target) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "worktree is outside this turn's project",
        ));
    }
    Ok(())
}
