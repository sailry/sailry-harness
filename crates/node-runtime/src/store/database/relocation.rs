//! Keep entry relocation from moving registered resources or active Node storage.
use super::*;
use std::path::PathBuf;

impl Database {
    pub(super) fn begin_relocation(
        &mut self,
        caller: NodeId,
        id: RequestId,
        paths: &[(PathBuf, String)],
    ) -> Result<(), Fault> {
        let paths = self.validate_relocation(paths)?;
        // The existing admission lifecycle releases these paths on completion.
        // Registrations cannot enter a subtree while relocation is queued/running.
        self.relocations.insert((caller, id), paths);
        Ok(())
    }

    pub(in crate::store) fn validate_relocation(
        &self,
        paths: &[(PathBuf, String)],
    ) -> Result<Vec<PathBuf>, Fault> {
        for (_, path) in paths {
            crate::files::path::entry_components(path)?;
        }
        let trees = super::super::worktrees::list(&self.connection)?;
        let database = self.connection.path().map(PathBuf::from);
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
            // Managed checkouts are editable project content inside the profile.
            // Only their registered root grants access; storage and ancestors stay protected.
            let managed = path.starts_with(root)
                && self.profile.as_ref().is_some_and(|profile| {
                    trees.iter().any(|tree| {
                        !tree.main
                            && Path::new(&tree.path) == root
                            && *root == profile.join("worktrees").join(tree.id.to_string())
                    })
                });
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confines_managed_edits() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().canonicalize().unwrap().join("node");
        std::fs::create_dir_all(profile.join("storage")).unwrap();
        let database = Database::open(
            &profile.join("storage/node.sqlite3"),
            NodeId([47; 32]),
            Some(profile.clone()),
        )
        .unwrap();
        let id = WorktreeId::new();
        let root = profile.join("worktrees").join(id.to_string());
        std::fs::create_dir_all(&root).unwrap();
        let check = |root: &Path, path: &str| {
            database.validate_relocation(&[(root.to_owned(), path.into())])
        };
        assert!(check(&root, "new.txt").is_err());
        crate::store::worktrees::register(
            &database.connection,
            &Worktree {
                id,
                project: None,
                path: root.to_str().unwrap().into(),
                main: false,
            },
        )
        .unwrap();
        assert_eq!(check(&root, "new.txt").unwrap(), vec![root.join("new.txt")]);
        std::fs::write(root.join("existing.txt"), "preserve").unwrap();
        assert!(check(&root, "existing.txt").is_ok());
        assert_eq!(
            check(&root, ".git").unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        for (base, path) in [
            (profile.clone(), "storage/node.sqlite3".to_owned()),
            (profile.clone(), "other.txt".to_owned()),
            (profile.clone(), format!("worktrees/{id}/new.txt")),
            (profile.join("worktrees"), id.to_string()),
        ] {
            assert_eq!(check(&base, &path).unwrap_err().code, ErrorCode::Conflict);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(profile.join("storage"), root.join("link")).unwrap();
            assert_eq!(
                check(&root, "link/new.txt").unwrap_err().code,
                ErrorCode::Conflict
            );
        }
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        crate::store::worktrees::register(
            &database.connection,
            &Worktree {
                id: WorktreeId::new(),
                project: None,
                path: nested.to_str().unwrap().into(),
                main: false,
            },
        )
        .unwrap();
        assert_eq!(
            check(&root, "nested").unwrap_err().code,
            ErrorCode::Conflict
        );
        database.close().unwrap();
    }
}
