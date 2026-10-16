use super::*;
use sailry_protocol::models::Definition as ModelDefinition;

mod fixture;

async fn register(client: &Client, definition: &ModelDefinition) -> Model {
    let request = client.prepare(Command::RegisterKnowledgeModel {
        definition: definition.clone(),
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let output = admission.completion.await.unwrap().unwrap();
    assert_eq!(client.execute(request.clone()).await.unwrap(), output);
    assert_eq!(
        client.outcome(&request).await.unwrap(),
        RequestOutcome::Completed(Box::new(Ok(output.clone())))
    );
    let Output::KnowledgeModel { model } = output else {
        panic!("registered model expected")
    };
    assert!(matches!(model, Model::Registered { .. }));
    // Read immediately rather than waiting for the background catalog observer.
    let info = models(client)
        .await
        .into_iter()
        .find(|info| info.model == model)
        .unwrap();
    let mut normalized = definition.clone();
    normalized
        .assets
        .sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(info.definition, normalized);
    model
}

async fn models(client: &Client) -> Vec<ModelInfo> {
    let Output::KnowledgeModels(models) = execute(client, Command::ReadKnowledgeModels).await
    else {
        panic!("models expected")
    };
    models
}

async fn ready(client: &Client, id: KnowledgeId, model: Option<Model>) -> Collection {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let collection = list(client)
                .await
                .into_iter()
                .find(|collection| collection.id == id)
                .unwrap();
            if collection.progress.status == Status::Ready && collection.progress.model == model {
                assert_eq!(collection.progress.sources, 2);
                assert_eq!(collection.progress.completed, 2);
                assert_eq!(collection.progress.skipped, 0);
                return collection;
            }
            assert_ne!(
                collection.progress.status,
                Status::Paused,
                "registered model indexing failed"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("registered model corpus reached ready state")
}

async fn semantic(client: &Client, id: KnowledgeId, model: Model) {
    let mut input = query("alpha", id);
    input.mode = Mode::Semantic;
    let page = search(client, None, input).await;
    assert_eq!(page.mode, Mode::Semantic);
    assert_eq!(page.model, Some(model));
    assert!(!page.indexing);
    assert!(!page.hits.is_empty());
    let first = &page.hits[0];
    assert!(first.text.contains("alpha"));
    assert_eq!(first.score, 1_000_000);
    assert!(!first.stale);
}

#[tokio::test]
async fn shares_installation_restore_and_isolated_removal() {
    for remote in [false, true] {
        let fixture = Fixture::start().await;
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
        let first = fixture::Assets::new(3, &[100., 0., 0., 1., 0., 0., 0., 1., 0.]);
        let first_model = register(&client, &first.definition).await;
        let mut reordered = first.definition.clone();
        reordered.assets.reverse();
        assert_eq!(register(&client, &reordered).await, first_model);
        assert_eq!(models(&client).await.len(), Model::ALL.len() + 1);
        assert_eq!(
            models(&client)
                .await
                .iter()
                .find(|info| info.model == first_model)
                .unwrap()
                .state,
            ModelState::Missing
        );
        let second = fixture::Assets::new(2, &[100., 0., 0., 1., 1., 0.]);
        let second_model = register(&client, &second.definition).await;
        assert_ne!(first_model, second_model);
        let profile = fixture.temp.path().join("node");
        let first_path = first.seed(&profile, first_model);
        let second_path = second.seed(&profile, second_model);
        for (model, path) in [(first_model, &first_path), (second_model, &second_path)] {
            let request = client.prepare(Command::InstallKnowledgeModel { model });
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let output = admission.completion.await.unwrap().unwrap();
            assert_eq!(output, Output::KnowledgeModel { model });
            assert_eq!(client.execute(request).await.unwrap(), output);
            let Output::Installations(tasks) = execute(
                &client,
                Command::ReadInstallations {
                    requests: vec![admission.receipt.id],
                },
            )
            .await
            else {
                panic!("installation observation expected")
            };
            assert!(tasks.is_empty());
            assert_eq!(
                fs::read_to_string(path.join("validated")).unwrap().len(),
                64
            );
            assert_eq!(
                models(&client)
                    .await
                    .iter()
                    .find(|info| info.model == model)
                    .unwrap()
                    .state,
                ModelState::Installed
            );
        }
        let Output::KnowledgeSettings(previous) =
            execute(&client, Command::ReadKnowledgeSettings).await
        else {
            panic!("settings expected")
        };
        let mut selected = previous.clone();
        selected.model = Some(first_model);
        let Output::KnowledgeSettings(selected) =
            execute(&client, Command::SaveKnowledgeSettings(selected)).await
        else {
            panic!("settings expected")
        };
        assert_eq!(selected.model, Some(first_model));
        assert_eq!(
            client
                .execute(client.prepare(Command::SaveKnowledgeSettings(previous)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let corpus = fixture.root.join("corpus");
        fs::create_dir(&corpus).unwrap();
        fs::write(corpus.join("alpha.txt"), "alpha").unwrap();
        fs::write(corpus.join("beta.txt"), "beta").unwrap();
        let following = saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition: definition(Source::Worktree {
                        worktree: fixture.worktree,
                        path: "corpus".into(),
                    }),
                    plugin: None,
                },
            )
            .await,
        );
        let mut pinned = following.definition.clone();
        pinned.name = "Second registered model".into();
        pinned.model = Some(second_model);
        let pinned = saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition: pinned,
                    plugin: None,
                },
            )
            .await,
        );
        ready(&client, following.id, Some(first_model)).await;
        ready(&client, pinned.id, Some(second_model)).await;
        semantic(&client, following.id, first_model).await;
        semantic(&client, pinned.id, second_model).await;
        let context = install(
            &client,
            &fixture.root,
            fixture.worktree,
            "registered-model-reader",
            0,
            &[Action::ReadKnowledge, Action::WriteKnowledge],
        )
        .await;
        for command in [
            Command::RegisterKnowledgeModel {
                definition: first.definition.clone(),
            },
            Command::InstallKnowledgeModel { model: first_model },
            Command::DeleteKnowledgeModel { model: first_model },
            Command::CancelKnowledgeModel { model: first_model },
            Command::SaveKnowledgeSettings(selected.clone()),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let before = models(&client).await;
        drop(client);
        let Fixture {
            temp,
            root,
            node,
            controller,
            worktree: _,
        } = fixture;
        node.shutdown().await.unwrap();
        let node =
            Node::start_empty_with_plugin_catalog(temp.path().join("node"), "http://127.0.0.1:9/")
                .await
                .unwrap();
        let client = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            Client::new(node.local())
        };
        assert_eq!(models(&client).await, before);
        assert_eq!(
            execute(&client, Command::ReadKnowledgeSettings).await,
            Output::KnowledgeSettings(selected)
        );
        assert_eq!(register(&client, &first.definition).await, first_model);
        ready(&client, following.id, Some(first_model)).await;
        ready(&client, pinned.id, Some(second_model)).await;
        semantic(&client, following.id, first_model).await;
        let request = client.prepare(Command::DeleteKnowledgeModel { model: first_model });
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        assert_eq!(
            admission.completion.await.unwrap().unwrap(),
            Output::KnowledgeModel { model: first_model }
        );
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::KnowledgeModel { model: first_model }
        );
        let after = models(&client).await;
        assert_eq!(after.len(), before.len());
        for model in [first_model, second_model] {
            let old = before.iter().find(|info| info.model == model).unwrap();
            let new = after.iter().find(|info| info.model == model).unwrap();
            assert_eq!(new.definition, old.definition);
            assert_eq!(
                new.state,
                if model == first_model {
                    ModelState::Missing
                } else {
                    ModelState::Installed
                }
            );
        }
        assert!(!first_path.exists());
        assert!(second_path.join("validated").is_file());
        ready(&client, following.id, None).await;
        let fallback = search(&client, None, query("alpha", following.id)).await;
        assert_eq!(fallback.mode, Mode::Keyword);
        assert_eq!(fallback.model, None);
        assert!(fallback.hits.iter().any(|hit| hit.text.contains("alpha")));
        ready(&client, pinned.id, Some(second_model)).await;
        semantic(&client, pinned.id, second_model).await;
        assert_eq!(
            fs::read_to_string(root.join("corpus/alpha.txt")).unwrap(),
            "alpha"
        );
        assert_eq!(
            fs::read_to_string(root.join("corpus/beta.txt")).unwrap(),
            "beta"
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_prefixes_through_registration_and_retrieval() {
    for remote in [false, true] {
        let fixture = Fixture::start().await;
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
        let mut assets = fixture::Assets::new(2, &[100., 0., 1., 0., 0., 1.]);
        assets.definition.query_prefix = Some("beta ".into());
        assets.definition.document_prefix = Some("alpha ".into());
        let model = register(&client, &assets.definition).await;
        let mut other = assets.definition.clone();
        other.query_prefix = None;
        assert_ne!(register(&client, &other).await, model);
        let profile = fixture.temp.path().join("node");
        assets.seed(&profile, model);
        assert_eq!(
            execute(&client, Command::InstallKnowledgeModel { model }).await,
            Output::KnowledgeModel { model }
        );
        let corpus = fixture.root.join("prefixes");
        fs::create_dir(&corpus).unwrap();
        fs::write(corpus.join("alpha.txt"), "alpha").unwrap();
        fs::write(corpus.join("beta.txt"), "beta").unwrap();
        let mut definition = definition(Source::Worktree {
            worktree: fixture.worktree,
            path: "prefixes".into(),
        });
        definition.model = Some(model);
        let collection = saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition,
                    plugin: None,
                },
            )
            .await,
        );
        ready(&client, collection.id, Some(model)).await;
        let mut input = query("alpha", collection.id);
        input.mode = Mode::Semantic;
        let page = search(&client, None, input).await;
        assert_eq!(page.mode, Mode::Semantic);
        assert_eq!(page.model, Some(model));
        assert_eq!(page.hits.len(), 2);
        assert_eq!(page.hits[0].text, "beta");
        assert_eq!(page.hits[0].score, 1_000_000);
        assert_eq!(page.hits[1].text, "alpha");
        assert!(page.hits[1].score.abs_diff(707_107) < 10_000);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
