use super::*;

async fn stage(fixture: &Fixture, name: &str) -> String {
    let (mut record, contents) = record(2, &[100., 0., 1., 0., 0., 1.]);
    record.definition.name = name.into();
    let id = fixture.register(&record.definition).await;
    let path = files::directory(
        &fixture.node.ingress.local_models.root().unwrap(),
        &id,
        &record.definition,
    )
    .unwrap();
    std::fs::create_dir_all(&path).unwrap();
    for (name, bytes) in contents {
        std::fs::write(path.join(name), bytes).unwrap();
    }
    id
}

async fn block(owner: &Arc<Registry>, id: &str) -> (JoinHandle<Result<(), Fault>>, Sender<()>) {
    let resource = owner.resource(Kind::Embedding, id, None).unwrap();
    let (entered, entering) = oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let owner = owner.clone();
    let work = tokio::spawn(async move {
        owner
            .execute(
                resource,
                Lane::Interactive,
                &CancellationToken::new(),
                move |_, _| {
                    let _ = entered.send(());
                    waiting.recv().map_err(failure)
                },
            )
            .await
    });
    timeout(Duration::from_secs(10), entering)
        .await
        .unwrap()
        .unwrap();
    (work, release)
}

async fn queued(owner: &Registry, id: &str) -> CancellationToken {
    timeout(Duration::from_secs(10), async {
        while owner.runtime.pending_background() == 0 {
            tokio::task::yield_now().await;
        }
        owner.state.lock().unwrap().transfers[id].clone()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn finishes_after_selection_and_disable_changes() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote).await;
        let installed = fixture.embedding().await;
        let owner = fixture.node.ingress.local_models.clone();
        for enabled in [true, false] {
            let id = stage(
                &fixture,
                if enabled {
                    "Selected vector"
                } else {
                    "Unselected vector"
                },
            )
            .await;
            let (work, release) = block(&owner, &installed).await;
            let mut admission = fixture
                .client
                .dispatch(
                    fixture
                        .client
                        .prepare(Command::InstallLocalModel { id: id.clone() }),
                )
                .await
                .unwrap();
            let downloading = queued(&owner, &id).await;
            let mut settings = fixture.settings().await;
            let choice = settings.choices.get_mut(&Kind::Embedding).unwrap();
            choice.enabled = enabled;
            if enabled {
                choice.model = Some(id.clone());
            }
            fixture
                .execute(Command::SaveLocalModelSettings(settings))
                .await;
            assert!(!downloading.is_cancelled());
            assert!(matches!(
                admission.completion.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert_eq!(
                timeout(Duration::from_secs(2), work)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::Cancelled
            );
            release.send(()).unwrap();
            timeout(Duration::from_secs(10), admission.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(matches!(
                owner
                    .list(None)
                    .unwrap()
                    .iter()
                    .find(|model| model.id == id)
                    .unwrap()
                    .state,
                State::Installed
            ));
            assert_eq!(fixture.settings().await.selected(Kind::Decision), None);
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn cancels_only_explicit_installation_lifecycles() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote).await;
        let installed = fixture.embedding().await;
        let owner = fixture.node.ingress.local_models.clone();
        for reason in ["cancel", "mirror", "delete"] {
            let id = stage(&fixture, reason).await;
            let (work, release) = block(&owner, &installed).await;
            let admission = fixture
                .client
                .dispatch(
                    fixture
                        .client
                        .prepare(Command::InstallLocalModel { id: id.clone() }),
                )
                .await
                .unwrap();
            let downloading = queued(&owner, &id).await;
            let deleting = match reason {
                "delete" => Some(
                    fixture
                        .client
                        .dispatch(
                            fixture
                                .client
                                .prepare(Command::DeleteLocalModel { id: id.clone() }),
                        )
                        .await
                        .unwrap(),
                ),
                "mirror" => {
                    let mut settings = fixture.settings().await;
                    settings.mirror = Some("https://mirror.example.com/".into());
                    fixture
                        .execute(Command::SaveLocalModelSettings(settings))
                        .await;
                    None
                }
                _ => {
                    fixture
                        .execute(Command::CancelLocalModel { id: id.clone() })
                        .await;
                    None
                }
            };
            assert_eq!(
                timeout(Duration::from_secs(2), admission.completion)
                    .await
                    .unwrap_or_else(|error| panic!("installation did not finish after {reason} (remote: {remote}, cancelled: {}): {error}", downloading.is_cancelled()))
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::Cancelled
            );
            assert!(
                downloading.is_cancelled(),
                "installation remained active after {reason} (remote: {remote})"
            );
            assert!(!work.is_finished());
            release.send(()).unwrap();
            timeout(Duration::from_secs(10), work)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if let Some(deleting) = deleting {
                timeout(Duration::from_secs(10), deleting.completion)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(matches!(
                    owner
                        .list(None)
                        .unwrap()
                        .iter()
                        .find(|model| model.id == id)
                        .unwrap()
                        .state,
                    State::Missing
                ));
            }
            timeout(Duration::from_secs(10), async {
                while owner.runtime.pending_background() != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(matches!(
                owner
                    .list(None)
                    .unwrap()
                    .iter()
                    .find(|model| model.id == installed)
                    .unwrap()
                    .state,
                State::Installed
            ));
            assert_eq!(fixture.settings().await.selected(Kind::Decision), None);
        }
        fixture.close().await;
    }
}
