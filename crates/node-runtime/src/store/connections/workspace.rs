//! Connection assistants own a scratch workspace, never a registered user project.
use super::*;
use std::path::Path;

pub(in crate::store) fn workspace(
    db: &Connection,
    profile: Option<&Path>,
    resource: Resource,
) -> Result<WorktreeId, Fault> {
    let (kind, id) = match resource {
        Resource::Database(id) => {
            super::super::databases::read(db, id)?.ok_or_else(missing)?;
            ("database", id.to_string())
        }
        Resource::Ssh(id) => {
            super::super::ssh::read(db, id)?.ok_or_else(missing)?;
            ("ssh", id.to_string())
        }
    };
    let profile = profile.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "connection workspace storage is unavailable",
        )
    })?;
    let root = profile
        .join("connections")
        .join(kind)
        .join(id)
        .join("workspace");
    super::super::worktrees::scratch(db, &root)
}
