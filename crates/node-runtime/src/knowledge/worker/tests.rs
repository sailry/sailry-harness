use super::*;

#[tokio::test]
async fn queues_edits_without_cancelling_current_pass() {
    let directory = tempfile::tempdir().unwrap();
    let registry = crate::models::registry::Registry::new(
        Some(directory.path().into()),
        CancellationToken::new(),
    );
    let models = models::Models::new(registry.clone());
    let (sender, receiver) = mpsc::sync_channel(1);
    let stop = CancellationToken::new();
    let owner = Arc::new(Service {
        models,
        index: Index::open(&directory.path().join("knowledge.turso"))
            .await
            .unwrap(),
        ingress: Weak::new(),
        wake: sender,
        cancel: Mutex::new(stop.child_token()),
        stop,
        worker: Mutex::new(None),
        refreshes: Default::default(),
    });
    let current = owner.cancel.lock().unwrap().clone();
    owner.refresh();
    owner.refresh();
    assert!(!current.is_cancelled());
    receiver.try_recv().unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    // A change arriving after admission remains queued for the following pass.
    owner.refresh();
    assert!(!current.is_cancelled());
    receiver.try_recv().unwrap();
    owner.invalidate();
    assert!(current.is_cancelled());
    assert!(!owner.cancel.lock().unwrap().is_cancelled());
    receiver.try_recv().unwrap();
    let active = owner.cancel.lock().unwrap().clone();
    let watched = directory.path().canonicalize().unwrap().join("watched");
    std::fs::create_dir(&watched).unwrap();
    let mut watches = watches::Watches::new(Arc::downgrade(&owner));
    assert!(!watches.needs_poll());
    watches.begin();
    let mut scan = source::worktree(&watched, "", CancellationToken::new()).unwrap();
    watches.add(&watched, scan.take_watches());
    watches.finish(true);
    receiver.try_recv().unwrap();
    assert!(!active.is_cancelled());
    watches.begin();
    let mut scan = source::worktree(&watched, "", CancellationToken::new()).unwrap();
    watches.add(&watched, scan.take_watches());
    watches.finish(true);
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    watches.begin();
    let mut scan = source::worktree(&watched, "", CancellationToken::new()).unwrap();
    watches.add(&watched, scan.take_watches());
    watches.finish(false);
    assert!(watches.needs_poll());
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    drop(watches);
    owner.shutdown().await;
    registry.shutdown().await;
    assert!(owner.cancel.lock().unwrap().is_cancelled());
}
