use super::*;

fn configuration() -> SessionConfig {
    SessionConfig {
        features: Default::default(),
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: "fixture".into(),
        effort: Effort::Default,
        mode: WorkMode::Code,
        options: Default::default(),
        permission: Permission::Ask,
        credential: None,
    }
}

async fn revision(
    database: &Database,
    session: SessionId,
    worktree: WorktreeId,
    config: &SessionConfig,
    revision: u64,
) {
    database.connection.execute(
        "INSERT INTO session_revisions(session,revision,config,roles,worktree) VALUES(?1,?2,?3,?4,?5)",
        params![session.to_string(), revision as i64, serde_json::to_vec(config).unwrap(), serde_json::to_vec(&role::Snapshot::default()).unwrap(), worktree.to_string()],
    ).await.unwrap();
    database
        .connection
        .execute(
            "UPDATE sessions SET revision=?2 WHERE id=?1",
            params![session.to_string(), revision as i64],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn confines_session_edits() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().canonicalize().unwrap().join("node");
    std::fs::create_dir_all(profile.join("storage")).unwrap();
    let database = Database::open(
        &profile.join("storage/node.turso"),
        NodeId([48; 32]),
        Some(profile.clone()),
    )
    .await
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
    .await
    .unwrap();
    let check = async |root: &Path, path: &str| {
        database
            .validate_relocation(&[(root.to_owned(), path.into())])
            .await
    };
    assert_eq!(
        check(&root, "new.txt").await.unwrap_err().code,
        ErrorCode::Conflict
    );
    database
        .connection
        .execute(
            "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
            params![SessionId::new().to_string(), worktree.to_string()],
        )
        .await
        .unwrap();
    assert_eq!(
        check(&root, "new.txt").await.unwrap_err().code,
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
    .await
    .unwrap();
    database
        .connection
        .execute(
            "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
            params![session.to_string(), sibling.to_string()],
        )
        .await
        .unwrap();
    assert_eq!(
        check(&root, "new.txt").await.unwrap_err().code,
        ErrorCode::Conflict
    );
    database
        .connection
        .execute(
            "UPDATE sessions SET worktree=?2 WHERE id=?1",
            params![session.to_string(), worktree.to_string()],
        )
        .await
        .unwrap();
    assert_eq!(
        check(&root, "new.txt").await.unwrap_err().code,
        ErrorCode::Conflict
    );
    revision(&database, session, worktree, &configuration(), 1).await;
    assert_eq!(
        check(&root, "new.txt").await.unwrap(),
        vec![root.join("new.txt")]
    );
    std::fs::write(root.join("existing.txt"), "preserve").unwrap();
    assert!(check(&root, "existing.txt").await.is_ok());
    for (base, path) in [
        (profile.clone(), "storage/node.sqlite3".to_owned()),
        (profile.clone(), "storage/node.turso".to_owned()),
        (profile.join("workspaces"), "sessions".to_owned()),
        (profile.join("workspaces/sessions"), session.to_string()),
        (
            profile.clone(),
            format!("workspaces/sessions/{session}/new.txt"),
        ),
        (sibling_root, "new.txt".to_owned()),
    ] {
        assert_eq!(
            check(&base, &path).await.unwrap_err().code,
            ErrorCode::Conflict
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(profile.join("storage"), root.join("link")).unwrap();
        assert_eq!(
            check(&root, "link/node.sqlite3").await.unwrap_err().code,
            ErrorCode::Conflict
        );
    }
    database.close().await.unwrap();
}

#[tokio::test]
async fn confines_bound_workspace_edits() {
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
            &profile.join("storage/node.turso"),
            NodeId([49; 32]),
            Some(profile.clone()),
        )
        .await
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
        .await
        .unwrap();
        database
            .connection
            .execute(
                "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
                params![session.to_string(), worktree.to_string()],
            )
            .await
            .unwrap();
        let check = async |base: &Path, path: &str| {
            database
                .validate_relocation(&[(base.to_owned(), path.into())])
                .await
        };
        assert_eq!(
            check(&root, "new.txt").await.unwrap_err().code,
            ErrorCode::Conflict
        );
        revision(&database, session, worktree, &other, 1).await;
        assert_eq!(
            check(&root, "new.txt").await.unwrap_err().code,
            ErrorCode::Conflict
        );
        revision(&database, session, worktree, &config, 2).await;
        assert_eq!(
            check(&root, "new.txt").await.unwrap(),
            vec![root.join("new.txt")]
        );
        revision(&database, session, worktree, &other, 3).await;
        assert_eq!(
            check(&root, "new.txt").await.unwrap_err().code,
            ErrorCode::Conflict
        );
        database
            .connection
            .execute(
                "UPDATE sessions SET revision=2 WHERE id=?1",
                [session.to_string()],
            )
            .await
            .unwrap();
        assert!(check(&root, "new.txt").await.is_ok());
        database
            .connection
            .execute(
                "UPDATE worktrees SET main=1 WHERE id=?1",
                [worktree.to_string()],
            )
            .await
            .unwrap();
        assert_eq!(
            check(&root, "new.txt").await.unwrap_err().code,
            ErrorCode::Conflict
        );
        database
            .connection
            .execute(
                "UPDATE worktrees SET main=0 WHERE id=?1",
                [worktree.to_string()],
            )
            .await
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
                .await
                .unwrap();
            assert_eq!(
                check(&malformed, "new.txt").await.unwrap_err().code,
                ErrorCode::Conflict
            );
        }
        database
            .connection
            .execute(
                "UPDATE worktrees SET path=?2 WHERE id=?1",
                params![worktree.to_string(), root.to_str().unwrap()],
            )
            .await
            .unwrap();
        assert!(check(&root, "new.txt").await.is_ok());
        for (base, path) in [
            (profile.clone(), "storage/node.sqlite3".to_owned()),
            (profile.clone(), "storage/node.turso".to_owned()),
            (root.parent().unwrap().to_owned(), "workspace".to_owned()),
            (sibling.clone(), "new.txt".to_owned()),
            (root.clone(), "../private.txt".to_owned()),
        ] {
            assert!(check(&base, &path).await.is_err());
        }
        #[cfg(unix)]
        {
            for (name, target) in [("storage", profile.join("storage")), ("sibling", sibling)] {
                std::fs::write(target.join("private.txt"), "protected").unwrap();
                std::os::unix::fs::symlink(&target, root.join(name)).unwrap();
                assert_eq!(
                    check(&root, &format!("{name}/private.txt"))
                        .await
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
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn confines_managed_edits() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().canonicalize().unwrap().join("node");
    std::fs::create_dir_all(profile.join("storage")).unwrap();
    let database = Database::open(
        &profile.join("storage/node.turso"),
        NodeId([47; 32]),
        Some(profile.clone()),
    )
    .await
    .unwrap();
    let id = WorktreeId::new();
    let root = profile.join("worktrees").join(id.to_string());
    std::fs::create_dir_all(&root).unwrap();
    let check = async |root: &Path, path: &str| {
        database
            .validate_relocation(&[(root.to_owned(), path.into())])
            .await
    };
    assert!(check(&root, "new.txt").await.is_err());
    crate::store::worktrees::register(
        &database.connection,
        &Worktree {
            id,
            project: None,
            path: root.to_str().unwrap().into(),
            main: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        check(&root, "new.txt").await.unwrap(),
        vec![root.join("new.txt")]
    );
    std::fs::write(root.join("existing.txt"), "preserve").unwrap();
    assert!(check(&root, "existing.txt").await.is_ok());
    assert_eq!(
        check(&root, ".git").await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    for (base, path) in [
        (profile.clone(), "storage/node.sqlite3".to_owned()),
        (profile.clone(), "storage/node.turso".to_owned()),
        (profile.clone(), "other.txt".to_owned()),
        (profile.clone(), format!("worktrees/{id}/new.txt")),
        (profile.join("worktrees"), id.to_string()),
    ] {
        assert_eq!(
            check(&base, &path).await.unwrap_err().code,
            ErrorCode::Conflict
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(profile.join("storage"), root.join("link")).unwrap();
        assert_eq!(
            check(&root, "link/new.txt").await.unwrap_err().code,
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
    .await
    .unwrap();
    assert_eq!(
        check(&root, "nested").await.unwrap_err().code,
        ErrorCode::Conflict
    );
    database.close().await.unwrap();
}

#[tokio::test]
async fn recognizes_project_checkouts() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let main = base.join("project");
    let profile = base.join("node");
    std::fs::create_dir(&main).unwrap();
    std::fs::create_dir_all(profile.join("storage")).unwrap();
    let database = Database::open(
        &profile.join("storage/node.turso"),
        NodeId([49; 32]),
        Some(profile),
    )
    .await
    .unwrap();
    let (Output::Project(project), _) = crate::store::projects::register(
        &database.connection,
        "Project",
        main.to_str().unwrap(),
        projects::Appearance::default(),
    )
    .await
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
    crate::store::worktrees::register(&database.connection, &tree)
        .await
        .unwrap();
    let trees = crate::store::worktrees::list(&database.connection)
        .await
        .unwrap();
    assert!(database.managed(&root, &trees).await.unwrap());
    assert!(
        !database
            .managed(&root.join("nested"), &trees)
            .await
            .unwrap()
    );
    assert_eq!(
        database
            .validate_relocation(&[(root.clone(), "new.txt".into())])
            .await
            .unwrap(),
        [root.join("new.txt")]
    );
    assert_eq!(
        database
            .validate_relocation(&[(main, ".sailry".into())])
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    database.close().await.unwrap();
}
