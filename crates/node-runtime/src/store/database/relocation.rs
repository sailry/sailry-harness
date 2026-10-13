//! Keep entry relocation from moving registered resources or active Node storage.
use super::*;
use std::path::PathBuf;

impl Database {
    pub(super) fn begin_relocation(
        &mut self,
        caller: NodeId,
        id: RequestId,
        paths: &[(PathBuf, String)],
        access: crate::files::Access,
    ) -> Result<(), Fault> {
        let paths = self.validate_with(paths, access)?;
        // The existing admission lifecycle releases these paths on completion.
        // Registrations cannot enter a subtree while relocation is queued/running.
        self.relocations.insert((caller, id), paths);
        Ok(())
    }

    pub(in crate::store) fn validate_relocation(
        &self,
        paths: &[(PathBuf, String)],
    ) -> Result<Vec<PathBuf>, Fault> {
        self.validate_with(paths, crate::files::Access::Restricted)
    }

    pub(in crate::store) fn validate_with(
        &self,
        paths: &[(PathBuf, String)],
        access: crate::files::Access,
    ) -> Result<Vec<PathBuf>, Fault> {
        for (_, path) in paths {
            crate::files::path::entries_with(path, access)?;
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
            // Registered checkouts and session-owned workspaces remain editable.
            // Storage, ancestors and unrelated profile directories stay protected.
            let managed = path.starts_with(root) && self.managed(root, &trees)?;
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

    fn managed(&self, root: &Path, trees: &[Worktree]) -> Result<bool, Fault> {
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
            let main = super::super::worktrees::project_root(&self.connection, project)?;
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
        ).map_err(storage_error)?;
        let owners = query
            .query_map([tree.id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(storage_error)?;
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
mod tests {
    use super::*;

    fn configuration() -> SessionConfig {
        SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            credential: None,
        }
    }

    fn revision(
        database: &Database,
        session: SessionId,
        worktree: WorktreeId,
        config: &SessionConfig,
        revision: u64,
    ) {
        database.connection.execute(
            "INSERT INTO session_revisions(session,revision,config,roles,worktree) VALUES(?1,?2,?3,?4,?5)",
            params![session.to_string(), revision as i64, serde_json::to_vec(config).unwrap(), serde_json::to_vec(&role::Snapshot::default()).unwrap(), worktree.to_string()],
        ).unwrap();
        database
            .connection
            .execute(
                "UPDATE sessions SET revision=?2 WHERE id=?1",
                params![session.to_string(), revision as i64],
            )
            .unwrap();
    }

    #[test]
    fn confines_session_edits() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().canonicalize().unwrap().join("node");
        std::fs::create_dir_all(profile.join("storage")).unwrap();
        let database = Database::open(
            &profile.join("storage/node.sqlite3"),
            NodeId([48; 32]),
            Some(profile.clone()),
        )
        .unwrap();
        let session = SessionId::new();
        let worktree = WorktreeId::new();
        let root = profile
            .join("workspaces/sessions")
            .join(session.to_string());
        std::fs::create_dir_all(&root).unwrap();
        crate::store::worktrees::register(
            &database.connection,
            &Worktree {
                id: worktree,
                project: None,
                path: root.to_str().unwrap().into(),
                main: false,
            },
        )
        .unwrap();
        let check = |root: &Path, path: &str| {
            database.validate_relocation(&[(root.to_owned(), path.into())])
        };
        assert_eq!(
            check(&root, "new.txt").unwrap_err().code,
            ErrorCode::Conflict
        );
        database
            .connection
            .execute(
                "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
                params![SessionId::new().to_string(), worktree.to_string()],
            )
            .unwrap();
        assert_eq!(
            check(&root, "new.txt").unwrap_err().code,
            ErrorCode::Conflict
        );
        let sibling = WorktreeId::new();
        let sibling_root = profile
            .join("workspaces/sessions")
            .join(SessionId::new().to_string());
        std::fs::create_dir_all(&sibling_root).unwrap();
        crate::store::worktrees::register(
            &database.connection,
            &Worktree {
                id: sibling,
                project: None,
                path: sibling_root.to_str().unwrap().into(),
                main: false,
            },
        )
        .unwrap();
        database
            .connection
            .execute(
                "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
                params![session.to_string(), sibling.to_string()],
            )
            .unwrap();
        assert_eq!(
            check(&root, "new.txt").unwrap_err().code,
            ErrorCode::Conflict
        );
        database
            .connection
            .execute(
                "UPDATE sessions SET worktree=?2 WHERE id=?1",
                params![session.to_string(), worktree.to_string()],
            )
            .unwrap();
        assert_eq!(
            check(&root, "new.txt").unwrap_err().code,
            ErrorCode::Conflict
        );
        revision(&database, session, worktree, &configuration(), 1);
        assert_eq!(check(&root, "new.txt").unwrap(), vec![root.join("new.txt")]);
        std::fs::write(root.join("existing.txt"), "preserve").unwrap();
        assert!(check(&root, "existing.txt").is_ok());
        for (base, path) in [
            (profile.clone(), "storage/node.sqlite3".to_owned()),
            (profile.join("workspaces"), "sessions".to_owned()),
            (profile.join("workspaces/sessions"), session.to_string()),
            (
                profile.clone(),
                format!("workspaces/sessions/{session}/new.txt"),
            ),
            (sibling_root, "new.txt".to_owned()),
        ] {
            assert_eq!(check(&base, &path).unwrap_err().code, ErrorCode::Conflict);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(profile.join("storage"), root.join("link")).unwrap();
            assert_eq!(
                check(&root, "link/node.sqlite3").unwrap_err().code,
                ErrorCode::Conflict
            );
        }
        database.close().unwrap();
    }

    #[test]
    fn confines_bound_workspace_edits() {
        let mut ssh = configuration();
        ssh.resource = Some(connection::Resource::Ssh(SshId::new()));
        let mut sql = configuration();
        sql.resource = Some(connection::Resource::Database(DatabaseId::new()));
        let mut assistant = configuration();
        assistant.assistant = Some(plugin::conversation::Binding {
            package: plugin::Reference {
                name: "fixture".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            id: "assistant".into(),
        });
        for config in [ssh, sql, assistant] {
            let temp = tempfile::tempdir().unwrap();
            let profile = temp.path().canonicalize().unwrap().join("node");
            std::fs::create_dir_all(profile.join("storage")).unwrap();
            let database = Database::open(
                &profile.join("storage/node.sqlite3"),
                NodeId([49; 32]),
                Some(profile.clone()),
            )
            .unwrap();
            let session = SessionId::new();
            let worktree = WorktreeId::new();
            let mut other = config.clone();
            let (root, sibling) = match config.resource {
                Some(resource) => {
                    other.resource = Some(match resource {
                        connection::Resource::Ssh(_) => connection::Resource::Ssh(SshId::new()),
                        connection::Resource::Database(_) => {
                            connection::Resource::Database(DatabaseId::new())
                        }
                    });
                    (
                        crate::workspaces::connection(&profile, resource),
                        crate::workspaces::connection(&profile, other.resource.unwrap()),
                    )
                }
                None => {
                    other.assistant.as_mut().unwrap().id = "other".into();
                    (
                        crate::workspaces::assistant(&profile, config.assistant.as_ref().unwrap())
                            .unwrap(),
                        crate::workspaces::assistant(&profile, other.assistant.as_ref().unwrap())
                            .unwrap(),
                    )
                }
            };
            for path in [&root, &sibling] {
                std::fs::create_dir_all(path).unwrap();
            }
            crate::store::worktrees::register(
                &database.connection,
                &Worktree {
                    id: worktree,
                    project: None,
                    path: root.to_str().unwrap().into(),
                    main: false,
                },
            )
            .unwrap();
            database
                .connection
                .execute(
                    "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
                    params![session.to_string(), worktree.to_string()],
                )
                .unwrap();
            let check = |base: &Path, path: &str| {
                database.validate_relocation(&[(base.to_owned(), path.into())])
            };
            assert_eq!(
                check(&root, "new.txt").unwrap_err().code,
                ErrorCode::Conflict
            );
            revision(&database, session, worktree, &other, 1);
            assert_eq!(
                check(&root, "new.txt").unwrap_err().code,
                ErrorCode::Conflict
            );
            revision(&database, session, worktree, &config, 2);
            assert_eq!(check(&root, "new.txt").unwrap(), vec![root.join("new.txt")]);
            revision(&database, session, worktree, &other, 3);
            assert_eq!(
                check(&root, "new.txt").unwrap_err().code,
                ErrorCode::Conflict
            );
            database
                .connection
                .execute(
                    "UPDATE sessions SET revision=2 WHERE id=?1",
                    [session.to_string()],
                )
                .unwrap();
            assert!(check(&root, "new.txt").is_ok());
            database
                .connection
                .execute(
                    "UPDATE worktrees SET main=1 WHERE id=?1",
                    [worktree.to_string()],
                )
                .unwrap();
            assert_eq!(
                check(&root, "new.txt").unwrap_err().code,
                ErrorCode::Conflict
            );
            database
                .connection
                .execute(
                    "UPDATE worktrees SET main=0 WHERE id=?1",
                    [worktree.to_string()],
                )
                .unwrap();
            for malformed in [
                profile.join("assistants/not-a-hash/workspace"),
                profile
                    .join("connections/other")
                    .join(SshId::new().to_string())
                    .join("workspace"),
                root.join("nested"),
            ] {
                std::fs::create_dir_all(&malformed).unwrap();
                database
                    .connection
                    .execute(
                        "UPDATE worktrees SET path=?2 WHERE id=?1",
                        params![worktree.to_string(), malformed.to_str().unwrap()],
                    )
                    .unwrap();
                assert_eq!(
                    check(&malformed, "new.txt").unwrap_err().code,
                    ErrorCode::Conflict
                );
            }
            database
                .connection
                .execute(
                    "UPDATE worktrees SET path=?2 WHERE id=?1",
                    params![worktree.to_string(), root.to_str().unwrap()],
                )
                .unwrap();
            assert!(check(&root, "new.txt").is_ok());
            for (base, path) in [
                (profile.clone(), "storage/node.sqlite3".to_owned()),
                (root.parent().unwrap().to_owned(), "workspace".to_owned()),
                (sibling.clone(), "new.txt".to_owned()),
                (root.clone(), "../private.txt".to_owned()),
            ] {
                assert!(check(&base, &path).is_err());
            }
            #[cfg(unix)]
            {
                for (name, target) in [("storage", profile.join("storage")), ("sibling", sibling)] {
                    std::fs::write(target.join("private.txt"), "protected").unwrap();
                    std::os::unix::fs::symlink(&target, root.join(name)).unwrap();
                    assert_eq!(
                        check(&root, &format!("{name}/private.txt"))
                            .unwrap_err()
                            .code,
                        ErrorCode::Conflict
                    );
                    assert_eq!(
                        std::fs::read_to_string(target.join("private.txt")).unwrap(),
                        "protected"
                    );
                }
            }
            database.close().unwrap();
        }
    }

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

    #[test]
    fn recognizes_project_checkouts() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let main = base.join("project");
        let profile = base.join("node");
        std::fs::create_dir(&main).unwrap();
        std::fs::create_dir_all(profile.join("storage")).unwrap();
        let database = Database::open(
            &profile.join("storage/node.sqlite3"),
            NodeId([49; 32]),
            Some(profile),
        )
        .unwrap();
        let (Output::Project(project), _) = crate::store::projects::register(
            &database.connection,
            "Project",
            main.to_str().unwrap(),
            projects::Appearance::default(),
        )
        .unwrap() else {
            panic!("project expected")
        };
        let id = WorktreeId::new();
        let root = crate::workspaces::worktrees(&main).join(id.to_string());
        std::fs::create_dir_all(&root).unwrap();
        let tree = Worktree {
            id,
            project: Some(project.id),
            path: root.to_str().unwrap().into(),
            main: false,
        };
        crate::store::worktrees::register(&database.connection, &tree).unwrap();
        let trees = crate::store::worktrees::list(&database.connection).unwrap();
        assert!(database.managed(&root, &trees).unwrap());
        assert!(!database.managed(&root.join("nested"), &trees).unwrap());
        assert_eq!(
            database
                .validate_relocation(&[(root.clone(), "new.txt".into())])
                .unwrap(),
            [root.join("new.txt")]
        );
        assert_eq!(
            database
                .validate_relocation(&[(main, ".sailry".into())])
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        database.close().unwrap();
    }
}
