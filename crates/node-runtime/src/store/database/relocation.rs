//! Keep entry relocation from moving registered resources or active Node storage.
use super::*;
use std::path::PathBuf;

impl Database {
    pub(super) async fn begin_relocation(
        &mut self,
        caller: NodeId,
        id: RequestId,
        paths: &[(PathBuf, String)],
        access: crate::files::Access,
    ) -> Result<(), Fault> {
        let paths = self.validate_with(paths, access).await?;
        // The existing admission lifecycle releases these paths on completion.
        // Registrations cannot enter a subtree while relocation is queued/running.
        self.relocations.lock().unwrap().insert((caller, id), paths);
        Ok(())
    }

    pub(in crate::store) async fn validate_relocation(
        &self,
        paths: &[(PathBuf, String)],
    ) -> Result<Vec<PathBuf>, Fault> {
        self.validate_with(paths, crate::files::Access::Restricted)
            .await
    }

    pub(in crate::store) async fn validate_with(
        &self,
        paths: &[(PathBuf, String)],
        access: crate::files::Access,
    ) -> Result<Vec<PathBuf>, Fault> {
        for (_, path) in paths {
            crate::files::path::entries_with(path, access)?;
        }
        let trees = super::super::worktrees::list(&self.connection).await?;
        let database = (self.path != Path::new(":memory:")).then(|| self.path.clone());
        let mut resolved = Vec::with_capacity(paths.len());
        for (root, relative) in paths {
            let path = root.join(relative);
            // Canonicalization accounts for native case aliases, but the leaf may
            // itself be a symlink: moving that link does not relocate its target.
            let path = match path.symlink_metadata() {
                Ok(metadata) if !metadata.is_symlink() => {
                    path.canonicalize().map_err(crate::files::io_error)?
                }
                _ => path
                    .parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .map(|parent| parent.join(path.file_name().unwrap()))
                    .unwrap_or(path),
            };
            // Registered checkouts and session-owned workspaces remain editable.
            // Storage, ancestors and unrelated profile directories stay protected.
            let managed = path.starts_with(root) && self.managed(root, &trees).await?;
            if trees
                .iter()
                .any(|tree| Path::new(&tree.path).starts_with(&path))
                || database
                    .as_ref()
                    .is_some_and(|database| database.starts_with(&path))
                || self.profile.as_ref().is_some_and(|profile| {
                    profile.starts_with(&path) || path.starts_with(profile) && !managed
                })
            {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "entry contains registered resources or Node storage",
                ));
            }
            resolved.push(path);
        }
        Ok(resolved)
    }

    async fn managed(&self, root: &Path, trees: &[Worktree]) -> Result<bool, Fault> {
        let Some(tree) = trees
            .iter()
            .find(|tree| !tree.main && Path::new(&tree.path) == root)
        else {
            return Ok(false);
        };
        if self
            .profile
            .as_ref()
            .is_some_and(|profile| root == profile.join("worktrees").join(tree.id.to_string()))
        {
            return Ok(true);
        }
        if let Some(project) = tree.project {
            let main = super::super::worktrees::project_root(&self.connection, project).await?;
            return Ok(
                root == crate::workspaces::worktrees(&main).join(tree.id.to_string())
                    && crate::workspaces::managed(root, &main),
            );
        }
        let Some(profile) = &self.profile else {
            return Ok(false);
        };
        if !crate::workspaces::managed(root, profile) {
            return Ok(false);
        }
        let mut query = self.connection.prepare(
            "SELECT s.id,r.config FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision
             WHERE s.project IS NULL AND s.worktree=?1 AND r.worktree=?1",
        ).await.map_err(storage_error)?;
        let owners = {
            let mut rows = query
                .query([tree.id.to_string()])
                .await
                .map_err(storage_error)?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<Vec<u8>>(1)?)))(&row);
                values.push(mapped);
            }
            values.into_iter()
        };
        for owner in owners {
            let (session, config) = owner.map_err(storage_error)?;
            let config: SessionConfig = serde_json::from_slice(&config).map_err(storage_error)?;
            let owned = match (config.resource, config.assistant.as_ref()) {
                (Some(resource), _) => crate::workspaces::connection(profile, resource),
                (None, Some(binding)) => crate::workspaces::assistant(profile, binding)?,
                (None, None) => {
                    crate::workspaces::session(profile, session.parse().map_err(storage_error)?)
                }
            };
            if owned == root {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
