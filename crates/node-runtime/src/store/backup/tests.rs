use super::*;
use sailry_protocol::backup::Destination;

fn destination() -> Destination {
    Destination {
        kind: Kind::Webdav,
        endpoint: "https://backup.example.com".into(),
        root: "profiles".into(),
        bucket: String::new(),
        region: String::new(),
    }
}

#[test]
fn preserves_private_credentials_and_conflicts() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE backup_settings(singleton INTEGER PRIMARY KEY,body BLOB NOT NULL)",
    )
    .unwrap();
    let mut settings = Settings {
        destination: Some(destination()),
        ..Settings::default()
    };
    let command = Update {
        settings: settings.clone(),
        identity: SecretUpdate::Replace(Secret::new("private-user".into())),
        secret: SecretUpdate::Replace(Secret::new("private-password".into())),
        session_token: SecretUpdate::Clear,
    };
    let request = Request::new(
        sailry_protocol::NodeId([1; 32]),
        Command::SaveBackupSettings(command),
    );
    let (output, event) = execute(&db, &request).unwrap();
    let public = serde_json::to_string(&(output.clone(), event)).unwrap();
    assert!(!public.contains("private-user"));
    assert!(!public.contains("private-password"));
    assert_eq!(
        read(&db).unwrap().auth.secret.unwrap().expose(),
        "private-password"
    );
    assert_eq!(
        execute(&db, &request).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let Output::BackupSettings(saved) = output else {
        unreachable!()
    };
    settings.revision = saved.revision;
    settings.destination.as_mut().unwrap().kind = Kind::S3;
    settings.destination.as_mut().unwrap().bucket = "bucket".into();
    settings.destination.as_mut().unwrap().region = "us-east-1".into();
    let request = Request::new(
        sailry_protocol::NodeId([1; 32]),
        Command::SaveBackupSettings(Update {
            settings,
            identity: SecretUpdate::Keep,
            secret: SecretUpdate::Keep,
            session_token: SecretUpdate::Keep,
        }),
    );
    execute(&db, &request).unwrap();
    assert!(!read(&db).unwrap().settings.credentials_configured);
    assert!(read(&db).unwrap().auth.secret.is_none());
}

#[test]
fn confines_empty_restore_targets() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let active = root.join("active");
    std::fs::create_dir(&active).unwrap();
    assert!(target_path("relative", Some(&active)).is_err());
    assert_eq!(
        target_path(active.to_str().unwrap(), Some(&active))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let target = root.join("restored");
    assert_eq!(
        target_path(target.to_str().unwrap(), Some(&active)).unwrap(),
        target
    );
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("user.txt"), b"preserved").unwrap();
    assert_eq!(
        target_path(target.to_str().unwrap(), Some(&active))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    #[cfg(unix)]
    {
        let linked = root.join("linked");
        std::os::unix::fs::symlink(&target, &linked).unwrap();
        assert!(target_path(linked.to_str().unwrap(), Some(&active)).is_err());
    }
    assert_eq!(
        std::fs::read(target.join("user.txt")).unwrap(),
        b"preserved"
    );
}

#[test]
fn relocates_only_current_owned_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let original = root.join("original");
    let target = root.join("restored");
    let staging = root.join("staging");
    std::fs::create_dir_all(staging.join("storage")).unwrap();
    let db = Connection::open(staging.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,path TEXT); CREATE TABLE worktrees(id TEXT PRIMARY KEY,path TEXT); CREATE TABLE knowledge_collections(id TEXT PRIMARY KEY,body BLOB); CREATE TABLE session_revisions(body BLOB);").unwrap();
    let owned = original.join("worktrees/example");
    let external = root.join("external");
    for table in ["projects", "worktrees"] {
        db.execute(
            &format!("INSERT INTO {table}(id,path) VALUES('owned',?1),('external',?2)"),
            rusqlite::params![owned.to_str().unwrap(), external.to_str().unwrap()],
        )
        .unwrap();
    }
    let capture =
        serde_json::json!({"path":owned.to_str().unwrap(),"user":"unchanged"}).to_string();
    db.execute(
        "INSERT INTO session_revisions(body) VALUES(?1)",
        [capture.as_bytes()],
    )
    .unwrap();
    let collection = super::super::knowledge::Stored {
        collection: sailry_protocol::knowledge::Collection {
            id: sailry_protocol::KnowledgeId::new(),
            revision: 1,
            rebuild: 0,
            owner: sailry_protocol::knowledge::Owner::Controller,
            definition: sailry_protocol::knowledge::Definition {
                name: "Sources".into(),
                source: sailry_protocol::knowledge::Source::File {
                    path: owned.join("source.txt").to_string_lossy().into(),
                },
                model: None,
                globs: vec![],
                tags: vec![],
                shared_with: vec![],
            },
            paused: true,
            progress: Default::default(),
        },
        capture: super::super::knowledge::Capture {
            package: None,
            worktree: None,
            session: None,
        },
        automatic: false,
    };
    db.execute(
        "INSERT INTO knowledge_collections(id,body) VALUES(?1,?2)",
        rusqlite::params![
            collection.collection.id.to_string(),
            encode(&collection).unwrap()
        ],
    )
    .unwrap();
    drop(db);
    relocate(&staging, &original, &target).unwrap();
    let db = Connection::open(staging.join("storage/node.sqlite3")).unwrap();
    for table in ["projects", "worktrees"] {
        let owned: String = db
            .query_row(
                &format!("SELECT path FROM {table} WHERE id='owned'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owned, target.join("worktrees/example").to_string_lossy());
        let untouched: String = db
            .query_row(
                &format!("SELECT path FROM {table} WHERE id='external'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(untouched, external.to_string_lossy());
    }
    let history: Vec<u8> = db
        .query_row("SELECT body FROM session_revisions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(history, capture.as_bytes());
    let body: Vec<u8> = db
        .query_row("SELECT body FROM knowledge_collections", [], |row| {
            row.get(0)
        })
        .unwrap();
    let relocated: super::super::knowledge::Stored = serde_json::from_slice(&body).unwrap();
    let sailry_protocol::knowledge::Source::File { path } = relocated.collection.definition.source
    else {
        unreachable!()
    };
    assert_eq!(
        path,
        target
            .join("worktrees/example/source.txt")
            .to_string_lossy()
    );
    assert_eq!(relocated.capture.worktree, collection.capture.worktree);
    assert_eq!(
        relocated.collection.revision,
        collection.collection.revision
    );
    assert!(!original.exists());
}
