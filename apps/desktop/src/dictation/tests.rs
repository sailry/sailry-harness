use super::*;

#[tokio::test]
async fn missing_model() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("model");
    let (updates, _) = mpsc::channel(4);
    assert_eq!(
        run(
            path.clone(),
            Default::default(),
            Arc::default(),
            CancellationToken::new(),
            CancellationToken::new(),
            updates
        )
        .await,
        Err("dictation_model_missing")
    );
    assert!(!path.exists());
}

#[tokio::test]
async fn concurrent_recording() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("model");
    let owner = Arc::new(Mutex::new(()));
    let guard = owner.lock().await;
    let (updates, _) = mpsc::channel(4);
    let result = run(
        path.clone(),
        Default::default(),
        owner.clone(),
        CancellationToken::new(),
        CancellationToken::new(),
        updates,
    )
    .await;
    assert_eq!(result, Err("dictation_busy"));
    assert!(!path.exists());
    drop(guard);
}

#[tokio::test]
async fn cancelled_input() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("model");
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (updates, _) = mpsc::channel(4);
    assert_eq!(
        run(
            path.clone(),
            Default::default(),
            Arc::default(),
            CancellationToken::new(),
            cancel,
            updates
        )
        .await,
        Ok(String::new())
    );
    assert!(!path.exists());
}
