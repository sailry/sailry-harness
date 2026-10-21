use super::*;
use sailry_protocol::{
    decision,
    models::{Hub, Source},
};

fn definition() -> Definition {
    let pinned: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../decision/tests/fixtures/models.json"
    )))
    .unwrap();
    let model = &pinned["models"]["julia"];
    let source = Source {
        hub: Hub::HuggingFace,
        repository: model["repository"].as_str().unwrap().into(),
        revision: model["revision"].as_str().unwrap().into(),
    };
    Definition {
        source: source.clone(),
        name: "Julia scope fixture".into(),
        license: model["license"].as_str().unwrap().into(),
        format: Format::Decision {},
        query_prefix: None,
        document_prefix: None,
        assets: vec![Asset {
            source,
            path: model["file"].as_str().unwrap().into(),
            name: "model.gguf".into(),
            bytes: model["size"].as_u64().unwrap(),
            sha256: model["sha256"].as_str().unwrap().into(),
        }],
    }
}

fn input(model: Option<String>) -> decision::Request {
    decision::Request {
        model,
        state: "Save is visible".into(),
        questions: BTreeMap::from([(
            "save".into(),
            decision::Question::Noul {
                instructions: "Is Save visible?".into(),
                criteria: BTreeMap::new(),
            },
        )]),
    }
}

async fn seed(fixture: &Fixture, definition: &Definition, cached: &std::path::Path) -> String {
    let id = fixture.register(definition).await;
    let owner = &fixture.node.ingress.local_models;
    let directory = files::directory(&owner.root().unwrap(), &id, definition).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::copy(cached, directory.join("model.gguf")).unwrap();
    id
}

fn decide(engine: &mut Engine, cancel: &CancellationToken) -> Result<decision::Response, Fault> {
    let Engine::Decision(engine) = engine else {
        panic!("decision engine expected")
    };
    engine
        .decide(&input(None), || cancel.is_cancelled())
        .map_err(crate::decision::error)
}

#[tokio::test]
#[ignore = "requires the pinned Julia GGUF and packaged CPU runtime"]
async fn keeps_decision_work_independent_from_ocr() {
    let cached = PathBuf::from(
        std::env::var_os("SAILRY_DECISION_MODEL_CACHE").expect("pinned Julia GGUF path required"),
    );
    for remote in [false, true] {
        let fixture = Fixture::start(remote).await;
        let definition = definition();
        let id = seed(&fixture, &definition, &cached).await;
        fixture
            .execute(Command::InstallLocalModel { id: id.clone() })
            .await;
        let owner = fixture.node.ingress.local_models.clone();
        let resource = owner.resource(Kind::Decision, Some(&id), None).unwrap();
        let (work, release, _) = active(&owner, resource, decide).await;

        let mut queued = definition;
        queued.name = "Queued Julia scope fixture".into();
        let queued = seed(&fixture, &queued, &cached).await;
        let mut admission = fixture
            .client
            .dispatch(
                fixture
                    .client
                    .prepare(Command::InstallLocalModel { id: queued.clone() }),
            )
            .await
            .unwrap();
        let mut updates = owner.subscribe();
        timeout(Duration::from_secs(10), async {
            loop {
                let downloading =
                    updates.borrow_and_update().models.iter().any(|model| {
                        model.id == queued && matches!(model.state, State::Downloading)
                    });
                if downloading {
                    break;
                }
                updates.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let downloading = owner.state.lock().unwrap().transfers[&queued].clone();
        fixture.register(&ocr().definition).await;
        let mut settings = fixture.settings().await;
        settings.choices.get_mut(&Kind::Ocr).unwrap().enabled = false;
        fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await;
        assert!(!downloading.is_cancelled());
        assert!(matches!(
            admission.completion.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(!work.is_finished());

        // Cancelling one installation must not cancel another model's inference.
        fixture
            .execute(Command::CancelLocalModel { id: queued })
            .await;
        assert_eq!(
            timeout(Duration::from_secs(2), admission.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert!(!work.is_finished());
        release.send(()).unwrap();
        let response = timeout(Duration::from_secs(10), work)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(response.usage.input_tokens > 0);
        assert!(
            matches!(&response.answers["save"], decision::Answer::Noul { noul } if noul.is_finite())
        );

        let resource = owner.resource(Kind::Decision, Some(&id), None).unwrap();
        let (work, _release, stopped) = active(&owner, resource, decide).await;
        let mut settings = fixture.settings().await;
        settings.choices.get_mut(&Kind::Decision).unwrap().enabled = false;
        fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await;
        assert_eq!(
            timeout(Duration::from_secs(2), work)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        timeout(Duration::from_secs(2), stopped)
            .await
            .unwrap()
            .unwrap();
        let mut settings = fixture.settings().await;
        settings.choices.get_mut(&Kind::Decision).unwrap().enabled = true;
        fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await;
        assert_eq!(fixture.settings().await.selected(Kind::Decision), None);
        assert!(matches!(
            fixture.execute(Command::Decide(input(Some(id)))).await,
            Output::Decision(_)
        ));
        fixture.close().await;
    }
}
