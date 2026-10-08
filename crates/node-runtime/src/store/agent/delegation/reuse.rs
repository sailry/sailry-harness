use super::*;

#[cfg(test)]
mod tests;

pub(super) fn restore(
    db: &Connection,
    parent: &Run,
    previous: &Session,
    args: &Arguments,
    has_role: bool,
) -> Result<(Session, Option<role::Profile>), Fault> {
    let session = super::super::super::commands::session(db, args.session.unwrap())?;
    let origin = session
        .delegation
        .as_ref()
        .ok_or_else(|| Fault::new(ErrorCode::WrongTarget, "session is not a delegated child"))?;
    if origin.session != parent.session || session.project != previous.project {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "child belongs to another parent",
        ));
    }
    let busy: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
        [session.id.to_string()], |row| row.get(0),
    ).map_err(storage_error)?;
    if busy {
        return Err(Fault::new(
            ErrorCode::Busy,
            "child has active or pending work",
        ));
    }
    if args
        .worktree
        .is_some_and(|worktree| worktree != session.worktree)
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "child worktree is immutable",
        ));
    }
    if let Some(project) = previous.project {
        super::super::super::worktrees::select(db, project, Some(session.worktree))?;
    } else if session.worktree != parent.worktree {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "child belongs to another worktree",
        ));
    }
    let config = super::super::permissions::config(db, parent.turn)?;
    let permission = |permission| match permission {
        Permission::Ask => 0,
        Permission::Project => 1,
        Permission::Full => 2,
    };
    if config.mode == WorkMode::Plan && session.config.mode != WorkMode::Plan
        || permission(session.config.permission) > permission(config.permission)
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "child exceeds the parent turn's permissions",
        ));
    }
    let source: String = db.query_row(
        "SELECT t.id FROM turns t JOIN agent_runs a ON a.turn=t.id WHERE t.session=?1 AND json_extract(a.body,'$.status')!='queued' ORDER BY t.rowid DESC LIMIT 1",
        [session.id.to_string()], |row| row.get(0),
    ).map_err(storage_error)?;
    let source = source.parse().map_err(storage_error)?;
    let role = child(db, source)?
        .ok_or_else(|| storage_error("child execution has no delegation"))?
        .role;
    if has_role {
        let selected = match args.role.as_deref() {
            Some(key) => Some(
                super::super::super::sessions::roles::read(db, parent.session, parent.revision)?
                    .profiles
                    .into_iter()
                    .find(|role| role.key == key)
                    .ok_or_else(|| {
                        sailry_protocol::Fault::invalid("role is not in the parent turn snapshot")
                    })?
                    .id,
            ),
            None => None,
        };
        if selected != role.as_ref().map(|role| role.id) {
            return Err(Fault::new(ErrorCode::Conflict, "child role is immutable"));
        }
    }
    Ok((session, role))
}
