//! Agent inventory follows the admitted execution location and existing Node ownership.
use super::*;

#[cfg(test)]
mod tests;

impl Ingress {
    pub(crate) async fn agent_worktrees(&self, turn: TurnId) -> Result<Vec<Worktree>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Worktrees { turn, reply })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(super) fn list(database: &Database, turn: TurnId) -> Result<Vec<Worktree>, Fault> {
    let run = calls::active(&database.connection, turn)?;
    let root = database.worktree_root(run.worktree)?;
    if !root.is_dir() {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "worktree directory is unavailable",
        ));
    }
    let entries = super::super::worktrees::list(&database.connection)?;
    let source = entries
        .iter()
        .find(|entry| entry.id == run.worktree)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "worktree does not exist on this Node"))?;
    let project = source.project;
    Ok(entries
        .into_iter()
        .filter(|entry| match project {
            Some(project) => entry.project == Some(project),
            None => entry.id == run.worktree,
        })
        .collect())
}
