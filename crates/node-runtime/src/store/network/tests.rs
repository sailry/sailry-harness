use super::*;

#[tokio::test]
async fn persists_intent_before_applying() {
    let root = tempfile::tempdir().unwrap();
    let link = sailry_link::Link::controller(
        root.path().join("link"),
        NetworkScope::CustomRelays(vec!["https://127.0.0.1:9".into()]),
    )
    .await
    .unwrap();
    let id = NodeId(*link.handle().address().id.as_bytes());
    let path = root.path().canonicalize().unwrap().join("node.sqlite3");
    let mut db = Database::open(&path, id, None).unwrap();
    db.network_link = Some(link.handle());
    let desired = Selection::Custom {
        relays: vec!["https://127.0.0.1:10".into()],
    };
    let request = Request::new(
        id,
        Command::SaveNetworkSettings {
            expected_revision: 0,
            selection: desired.clone(),
        },
    );
    let (reply, _) = oneshot::channel();
    let execution = prepare(&mut db, id, request.clone(), reply)
        .map_err(|failure| failure.2)
        .unwrap();
    let settings = read(&db.connection, &link.handle()).unwrap();
    assert_eq!(settings.revision, 1);
    assert_eq!(settings.selection, Some(desired.clone()));
    assert_ne!(settings.selection, settings.effective);
    assert_eq!(db.network_request, Some(request.id));
    let newer = Request::new(
        id,
        Command::SaveNetworkSettings {
            expected_revision: 1,
            selection: Selection::Iroh,
        },
    );
    let (reply, _) = oneshot::channel();
    let error = prepare(&mut db, id, newer, reply).err().unwrap().2;
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(read(&db.connection, &link.handle()).unwrap(), settings);
    release(&mut db, &request);
    drop(execution);
    let stale = Request::new(
        id,
        Command::SaveNetworkSettings {
            expected_revision: 0,
            selection: Selection::Iroh,
        },
    );
    let (reply, _) = oneshot::channel();
    assert_eq!(
        prepare(&mut db, id, stale, reply).err().unwrap().2.code,
        ErrorCode::RevisionConflict
    );
    db.close().unwrap();
    let db = Database::open(&path, id, None).unwrap();
    assert_eq!(read(&db.connection, &link.handle()).unwrap(), settings);
    assert!(
        matches!(scope(&db.connection, NetworkScope::Internet).unwrap(), NetworkScope::CustomRelays(relays) if relays == vec!["https://127.0.0.1:10"])
    );
    assert!(matches!(
        scope(&db.connection, NetworkScope::default()).unwrap(),
        NetworkScope::Direct(_)
    ));
    db.close().unwrap();
    link.close().await.unwrap();
}
