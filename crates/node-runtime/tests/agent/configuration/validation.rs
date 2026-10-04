use super::*;

#[tokio::test]
async fn caller_revision_and_atomic_import() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let source = Node::start(fixture.path().join("source")).await.unwrap();
    let target = Node::start(fixture.path().join("target")).await.unwrap();
    source
        .link()
        .pair(target.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let origin = Client::new(source.local());
    let destination = Client::new(source.link().remote(target.link().address()));
    let peer = Client::new(target.link().remote(source.link().address()));
    let server = Server::start(false).await;
    let (local, provider) = configured(&origin, &server.endpoint, &root).await;
    let (session, _) = configured(&destination, &server.endpoint, &root).await;
    let command = Command::CreateSessionAt {
        target: target.id(),
        project: session.project,
        worktree: Some(session.worktree),
        config: Box::new(local.config.clone()),
        provider_revision: provider.revision,
    };
    assert_eq!(
        peer.execute(peer.prepare(command.clone()))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut stale = command;
    if let Command::CreateSessionAt {
        provider_revision, ..
    } = &mut stale
    {
        *provider_revision = 0;
    }
    assert_eq!(
        origin
            .execute(origin.prepare(stale))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(snapshot(&destination).await.sessions, vec![session.clone()]);
    let mut bundle = SessionImport {
        source: source.id(),
        project: session.project,
        worktree: Some(session.worktree),
        config: local.config,
        provider,
        secret: Some(Secret::new("isolated-import-marker".into())),
        expires_at_ms: None,
        roles: role::Snapshot::default(),
        role_credentials: vec![],
    };
    let credential = Some(CredentialRef {
        node: source.id(),
        id: CredentialId::new(),
    });
    bundle.config.credential = credential.clone();
    bundle.provider.credential = credential;
    let mut invalid = bundle.clone();
    invalid.provider.endpoint = "file:///invalid".into();
    assert_eq!(
        destination
            .execute(destination.prepare(Command::ImportSession(Box::new(invalid))))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert!(credentials(&destination).await.is_empty());
    let mut expired = bundle.clone();
    expired.expires_at_ms = Some(1);
    assert_eq!(
        destination
            .execute(destination.prepare(Command::ImportSession(Box::new(expired))))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let mut wrong = bundle.clone();
    wrong.source = NodeId([42; 32]);
    assert_eq!(
        destination
            .execute(destination.prepare(Command::ImportSession(Box::new(wrong))))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut missing = bundle.clone();
    missing.worktree = Some(WorktreeId::new());
    assert_eq!(
        destination
            .execute(destination.prepare(Command::ImportSession(Box::new(missing))))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert!(credentials(&destination).await.is_empty());
    let request = destination.prepare(Command::ImportSession(Box::new(bundle)));
    assert!(!format!("{request:?}").contains("isolated-import-marker"));
    let output = destination.execute(request.clone()).await.unwrap();
    assert_eq!(destination.execute(request.clone()).await.unwrap(), output);
    assert_eq!(credentials(&destination).await.len(), 1);
    assert_eq!(snapshot(&destination).await.sessions.len(), 2);
    let Output::Session(session) = output else {
        panic!("session expected")
    };
    let credential = credentials(&destination).await.pop().unwrap();
    destination
        .execute(destination.prepare(Command::RevokeCredential {
            id: credential.id,
            expected_revision: credential.revision,
        }))
        .await
        .unwrap();
    assert_eq!(
        destination
            .execute(destination.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "revoked material".into(),
            }))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(server.requests.lock().unwrap().is_empty());
    source.shutdown().await.unwrap();
    target.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_keeps_uncertain_admission() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let source_path = fixture.path().join("source");
    let source = Node::start(&source_path).await.unwrap();
    let target = Node::start(fixture.path().join("target")).await.unwrap();
    source
        .link()
        .pair(target.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let origin = Client::new(source.local());
    let destination = Client::new(target.local());
    let server = Server::start(false).await;
    let (local, provider) = configured(&origin, &server.endpoint, &root).await;
    let (session, _) = configured(&destination, &server.endpoint, &root).await;
    let lock = rusqlite::Connection::open(target.profile().join("storage/node.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let request = origin.prepare(Command::CreateSessionAt {
        target: target.id(),
        project: session.project,
        worktree: Some(session.worktree),
        config: Box::new(local.config),
        provider_revision: provider.revision,
    });
    let admission = origin.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    // A receipt precedes preparation; this read waits until forwarding has been scheduled.
    snapshot(&origin).await;
    tokio::time::timeout(Duration::from_secs(5), source.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        admission.completion.await.unwrap().unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    let source = Node::start(&source_path).await.unwrap();
    let origin = Client::new(source.local());
    assert_eq!(
        origin.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    let current = snapshot(&destination).await;
    assert!(current.sessions.len() <= 2);
    assert!(server.requests.lock().unwrap().is_empty());
    source.shutdown().await.unwrap();
    target.shutdown().await.unwrap();
}
