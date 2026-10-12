use super::*;
use sailry_protocol::models::Format;

mod registry;

struct Fixture {
    _directory: tempfile::TempDir,
    owner: Arc<Models>,
    stamp: files::Stamp,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let (root, handle) = files::open(directory.path()).unwrap();
        let model = Model::BgeSmallZh;
        let path = files::directory(&root, model);
        std::fs::create_dir_all(&path).unwrap();
        // Sparse files exercise receipt identity without acquiring model weights.
        for asset in descriptor(model).assets {
            std::fs::File::create(path.join(asset.name))
                .unwrap()
                .set_len(asset.bytes)
                .unwrap();
        }
        let definition = catalog::builtin(model).unwrap();
        let stamp = files::stamp(&root, model, &definition).unwrap();
        let owner = Models::start(
            root,
            handle,
            Settings {
                model: Some(model),
                ..Default::default()
            },
            catalog::definitions(&[]).unwrap(),
            BTreeMap::from([(model, stamp.clone())]),
            Duration::from_millis(20),
        )
        .unwrap();
        Self {
            _directory: directory,
            owner,
            stamp,
        }
    }

    fn work(&self, text: &str) -> Work {
        let (reply, _) = oneshot::channel();
        Work {
            model: Model::BgeSmallZh,
            definition: Arc::new(catalog::builtin(Model::BgeSmallZh).unwrap()),
            revision: 0,
            generation: 0,
            epoch: 0,
            stamp: self.stamp.clone(),
            input: Input::Encode(sailry_embedding::Input::Query, vec![text.into()]),
            cancel: CancellationToken::new(),
            reply,
        }
    }
}

#[tokio::test]
async fn invalidates_changed_identity() {
    let fixture = Fixture::new();
    let infos = fixture.owner.infos().await.unwrap();
    assert_eq!(
        infos
            .iter()
            .find(|info| info.model == Model::BgeSmallZh)
            .unwrap()
            .state,
        ModelState::Installed
    );
    let mut updates = fixture.owner.subscribe();
    let path = files::directory(&fixture.owner.root, Model::BgeSmallZh).join("config.json");
    let replacement = path.with_extension("replacement");
    std::fs::File::create(&replacement)
        .unwrap()
        .set_len(descriptor(Model::BgeSmallZh).assets[0].bytes)
        .unwrap();
    std::fs::rename(replacement, path).unwrap();
    let infos = fixture.owner.infos().await.unwrap();
    assert_eq!(
        infos
            .iter()
            .find(|info| info.model == Model::BgeSmallZh)
            .unwrap()
            .state,
        ModelState::Missing
    );
    assert!(updates.has_changed().unwrap());
    updates.borrow_and_update();
    assert_eq!(
        fixture
            .owner
            .current(&fixture.work("old"))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn preserves_capture_and_cancellation() {
    let fixture = Fixture::new();
    let work = fixture.work("captured");
    fixture.owner.current(&work).unwrap();
    fixture
        .owner
        .configure(
            Settings {
                revision: 1,
                model: Some(Model::PotionMulti),
                mirror: None,
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        fixture.owner.current(&work).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    work.cancel.cancel();
    assert_eq!(
        fixture.owner.current(&work).unwrap_err().code,
        ErrorCode::Cancelled
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        fixture
            .owner
            .install(Model::GraniteMulti, &cancel, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(fixture.owner.state.lock().unwrap().active.is_empty());
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn permits_installed_collection_override() {
    let fixture = Fixture::new();
    let stale = fixture.work("captured");
    fixture
        .owner
        .configure(
            Settings {
                revision: 1,
                model: None,
                mirror: None,
            },
            &[],
        )
        .unwrap();
    let mut captured = fixture.work("collection override");
    captured.revision = 1;
    captured.generation = 1;
    fixture.owner.current(&captured).unwrap();
    assert_eq!(
        fixture.owner.current(&stale).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn cancellation_never_publishes_installation() {
    let fixture = Fixture::new();
    let model = Model::GraniteMulti;
    let cancel = CancellationToken::new();
    fixture.owner.state.lock().unwrap().active.insert(
        model,
        Transfer {
            cancel: cancel.clone(),
            percent: 34,
            speed: 12,
            published: Instant::now(),
        },
    );
    let installation = Installation {
        owner: fixture.owner.clone(),
        model,
    };
    assert!(fixture.owner.cancel(model));
    assert!(cancel.is_cancelled());
    drop(installation);
    let infos = fixture.owner.infos().await.unwrap();
    assert_eq!(
        infos.iter().find(|info| info.model == model).unwrap().state,
        ModelState::Missing
    );
    assert!(!fixture.owner.cancel(model));
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn removes_only_owned_revision() {
    let fixture = Fixture::new();
    let parent = fixture.owner.root.join(Model::BgeSmallZh.key().as_ref());
    let other = parent.join("another-revision");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("keep"), b"owned elsewhere").unwrap();
    fixture.owner.remove(Model::BgeSmallZh).await.unwrap();
    assert!(other.join("keep").is_file());
    assert!(!files::directory(&fixture.owner.root, Model::BgeSmallZh).exists());
    assert!(fixture.owner.state.lock().unwrap().removing.is_empty());
    fixture.owner.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinked_model_directory() {
    let fixture = Fixture::new();
    let external = tempfile::tempdir().unwrap();
    let path = files::directory(&fixture.owner.root, Model::BgeSmallZh);
    std::fs::remove_dir_all(&path).unwrap();
    std::fs::write(external.path().join("keep"), b"external").unwrap();
    std::os::unix::fs::symlink(external.path(), &path).unwrap();
    assert!(fixture.owner.remove(Model::BgeSmallZh).await.is_err());
    assert_eq!(
        std::fs::read(external.path().join("keep")).unwrap(),
        b"external"
    );
    assert_eq!(
        fixture
            .owner
            .infos()
            .await
            .unwrap()
            .iter()
            .find(|info| info.model == Model::BgeSmallZh)
            .unwrap()
            .state,
        ModelState::Missing
    );
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn queries_precede_index_batches() {
    let fixture = Fixture::new();
    let (query, mut queries) = mpsc::channel(8);
    let (index, mut indexes) = mpsc::channel(4);
    let (_control, mut controls) = mpsc::channel(4);
    let mut offsets = fixture.work("chunk input");
    offsets.input = Input::Offsets("chunk input".into());
    index.try_send(offsets).unwrap();
    for offset in 1..4 {
        index
            .try_send(fixture.work(&format!("index-{offset}")))
            .unwrap();
    }
    assert!(matches!(
        index.try_send(fixture.work("overflow")),
        Err(mpsc::error::TrySendError::Full(_))
    ));
    query.try_send(fixture.work("query")).unwrap();
    let stop = CancellationToken::new();
    let event = worker::next(
        &mut queries,
        &mut indexes,
        &mut controls,
        &stop,
        tokio::time::Instant::now() + IDLE,
    )
    .await;
    let worker::Next::Work(work) = event else {
        panic!("query work expected");
    };
    let Input::Encode(_, texts) = work.input else {
        panic!("encoding input expected");
    };
    assert_eq!(texts, ["query"]);
    let event = worker::next(
        &mut queries,
        &mut indexes,
        &mut controls,
        &stop,
        tokio::time::Instant::now() + IDLE,
    )
    .await;
    let worker::Next::Work(work) = event else {
        panic!("index work expected");
    };
    let Input::Offsets(text) = work.input else {
        panic!("offset input expected");
    };
    assert_eq!(text, "chunk input");
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn idle_deadline_yields_unload() {
    let (_query, mut queries) = mpsc::channel(8);
    let (_index, mut indexes) = mpsc::channel(4);
    let (_control, mut controls) = mpsc::channel(4);
    let stop = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(20);
    let event = tokio::time::timeout(
        Duration::from_secs(1),
        worker::next(&mut queries, &mut indexes, &mut controls, &stop, deadline),
    )
    .await
    .unwrap();
    assert!(matches!(event, worker::Next::Idle));
    assert!(tokio::time::Instant::now() >= deadline);
    stop.cancel();
    let event = worker::next(&mut queries, &mut indexes, &mut controls, &stop, deadline).await;
    assert!(matches!(event, worker::Next::Stop));
}

#[test]
fn validates_explicit_https_mirror() {
    let settings = Settings {
        mirror: Some("https://models.example.invalid/cache".into()),
        ..Default::default()
    };
    assert_eq!(
        source(&settings).unwrap(),
        "https://models.example.invalid/cache"
    );
    for url in [
        "http://example.invalid",
        "https://user:secret@example.invalid",
        "https://example.invalid?query=1",
    ] {
        assert!(
            Models::validate(&Settings {
                mirror: Some(url.into()),
                ..Default::default()
            })
            .is_err()
        );
    }
}
