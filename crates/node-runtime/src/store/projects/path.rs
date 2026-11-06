//! Rebinding changes only the main checkout; admitted execution keeps its resource identity.
use super::*;

pub(super) async fn check(db: &Connection, project: ProjectId, path: &str) -> Result<(), Fault> {
    let main = super::super::worktrees::select(db, project, None).await?;
    let active: bool = {
        let mut statement = db
            .prepare(
                "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn
                 JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
                 JOIN worktrees w ON w.id=r.worktree WHERE w.project=?1
                 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
            )
            .await
            .map_err(storage_error)?;
        statement
            .query_row([project.to_string()])
            .await
            .and_then(|row| row.get(0))
            .map_err(storage_error)?
    };
    if active {
        return Err(Fault::new(
            ErrorCode::Busy,
            "project has active or queued turns",
        ));
    }
    let trees = super::super::worktrees::list(db).await?;
    let terminals = super::super::terminals::list(db).await?;
    if terminals
        .iter()
        .any(|info| info.worktree == Some(main) && info.status != terminal::Status::Closed)
        || pending(db, project, main, &trees, &terminals).await?
    {
        return Err(Fault::new(
            ErrorCode::Busy,
            "project checkout has an open or pending resource",
        ));
    }
    let branches: Vec<_> = trees
        .into_iter()
        .filter(|tree| tree.project == Some(project) && !tree.main)
        .map(|tree| tree.path)
        .collect();
    if !branches.is_empty() {
        let root = PathBuf::from(path);
        tokio::task::spawn_blocking(move || {
            for branch in branches {
                crate::git::worktrees::resolve(&root, &branch)?;
            }
            Ok::<_, Fault>(())
        })
        .await
        .map_err(storage_error)??;
    }
    Ok(())
}

async fn pending(
    db: &Connection,
    project: ProjectId,
    main: WorktreeId,
    trees: &[Worktree],
    terminals: &[terminal::Info],
) -> Result<bool, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM requests WHERE status='admitted' AND json_valid(body)")
        .await
        .map_err(storage_error)?;
    let mut rows = statement.query(()).await.map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body: Vec<u8> = row.get(0).map_err(storage_error)?;
        let request: Request = serde_json::from_slice(&body).map_err(storage_error)?;
        let blocks = match request.command {
            Command::CreateTerminal(launch) | Command::OpenToolTerminal { launch, .. } => {
                launch.worktree == main
            }
            Command::OpenTerminal { terminal, .. } => terminals
                .iter()
                .any(|info| info.id == terminal && info.worktree == Some(main)),
            Command::CreateWorktree { project: owner, .. }
            | Command::RegisterWorktree { project: owner, .. }
            | Command::CreateManagedWorktree { project: owner, .. } => owner == project,
            Command::RemoveWorktree { worktree, .. } => trees
                .iter()
                .any(|tree| tree.id == worktree && tree.project == Some(project)),
            _ => false,
        };
        if blocks {
            return Ok(true);
        }
    }
    Ok(false)
}
