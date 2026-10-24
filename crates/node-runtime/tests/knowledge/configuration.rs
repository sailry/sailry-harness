use super::*;

#[tokio::test]
async fn shares_cas_receipts_and_canonical_events() {
    let fixture = Fixture::start().await;
    fs::write(fixture.root.join("reference.txt"), "Original source").unwrap();
    let [local, remote] = fixture.clients();
    for (writer, reader) in [(&local, &remote), (&remote, &local)] {
        let Output::LocalModelSettings(previous) =
            execute(writer, Command::ReadLocalModelSettings).await
        else {
            panic!("settings expected")
        };
        let mut updates = reader.subscribe().await.unwrap();
        assert!(matches!(updates.next().await.unwrap(), Update::Snapshot(_)));
        let mut settings = previous.clone();
        settings.mirror = Some("https://example.com/models/".into());
        let request = writer.prepare(Command::SaveLocalModelSettings(settings));
        let admitted = writer.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let saved = admitted.completion.await.unwrap().unwrap();
        assert_eq!(writer.execute(request.clone()).await.unwrap(), saved);
        assert_eq!(
            writer.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(saved.clone())))
        );
        assert_eq!(
            reader.outcome(&request).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        assert_eq!(
            execute(reader, Command::ReadLocalModelSettings).await,
            saved
        );
        assert_eq!(
            writer
                .execute(writer.prepare(Command::SaveLocalModelSettings(previous)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Update::Event(envelope) = updates.next().await.unwrap()
                    && matches!(envelope.event, Event::LocalModelSettingsChanged { .. })
                {
                    break envelope;
                }
            }
        })
        .await
        .unwrap();

        let persisted: i64 = {
            let sql = ("SELECT count(*) FROM events WHERE cursor=?1").to_owned();
            let parameters = turso::params![event.cursor as i64];
            fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::knowledge::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::knowledge::database_fixture::fault)?;
                        let value: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                        value.map_err(crate::knowledge::database_fixture::fault)
                    })
                })
                .await
        }
        .unwrap();
        assert_eq!(persisted, 1);
        let definition = definition(Source::Worktree {
            worktree: fixture.worktree,
            path: "reference.txt".into(),
        });
        let request = writer.prepare(Command::CreateKnowledgeCollection {
            definition: definition.clone(),
            plugin: None,
        });
        let admission = writer.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let saved = admission.completion.await.unwrap().unwrap();
        let first = super::saved(saved.clone());
        assert_eq!(writer.execute(request.clone()).await.unwrap(), saved);
        assert_eq!(
            writer.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(saved.clone())))
        );
        assert_eq!(
            reader.outcome(&request).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        assert_eq!(
            list(reader)
                .await
                .iter()
                .find(|entry| entry.id == first.id)
                .unwrap()
                .definition,
            definition
        );
        let mut changed = definition;
        changed.name = "Updated reference".into();
        let updated = super::saved(
            execute(
                reader,
                Command::UpdateKnowledgeCollection {
                    id: first.id,
                    expected_revision: 1,
                    definition: changed,
                },
            )
            .await,
        );
        assert_eq!(updated.revision, 2);
        assert_eq!(
            writer
                .execute(writer.prepare(Command::RebuildKnowledgeCollection {
                    id: first.id,
                    expected_revision: 1
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let rebuilt = super::saved(
            execute(
                writer,
                Command::RebuildKnowledgeCollection {
                    id: first.id,
                    expected_revision: 2,
                },
            )
            .await,
        );
        assert_eq!(rebuilt.revision, 3);
        assert_eq!(rebuilt.progress.generation, 3);
        assert_eq!(rebuilt.rebuild, 1);
        let deleted = execute(
            reader,
            Command::DeleteKnowledgeCollection {
                id: first.id,
                expected_revision: 3,
            },
        )
        .await;
        assert_eq!(deleted, Output::KnowledgeCollectionDeleted { id: first.id });
        assert!(list(writer).await.iter().all(|entry| entry.id != first.id));
        assert_eq!(
            fs::read_to_string(fixture.root.join("reference.txt")).unwrap(),
            "Original source"
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn confines_model_lifecycle_to_controller() {
    let fixture = Fixture::start().await;
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let Output::LocalModels(sailry_protocol::models::Catalog { models, .. }) = execute(
            &client,
            Command::ReadLocalModels {
                kind: Some(sailry_protocol::models::Kind::Embedding),
            },
        )
        .await
        else {
            panic!("models expected")
        };
        assert_eq!(models.len(), 3);
        assert!(models.iter().all(|info| {
            info.state == ModelState::Missing
                && info
                    .definition
                    .format
                    .dimension()
                    .is_some_and(|dimension| dimension > 0)
                && !info.definition.license.is_empty()
        }));
        let name = format!("models-reader-{index}");
        let context = install(
            &client,
            &fixture.root,
            fixture.worktree,
            &name,
            0,
            &[Action::ReadKnowledge, Action::WriteKnowledge],
        )
        .await;
        for command in [
            Command::ReadLocalModels {
                kind: Some(sailry_protocol::models::Kind::Embedding),
            },
            Command::ReadLocalModelSettings,
            Command::InstallLocalModel {
                id: "potion-multi".to_owned(),
            },
            Command::DeleteLocalModel {
                id: "potion-multi".to_owned(),
            },
            Command::CancelLocalModel {
                id: "potion-multi".to_owned(),
            },
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
        let request = client.prepare(Command::CancelLocalModel {
            id: "potion-multi".to_owned(),
        });
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(!admission.receipt.durable);
        assert_eq!(
            admission.completion.await.unwrap().unwrap(),
            Output::LocalModel {
                id: "potion-multi".to_owned()
            }
        );
        assert_eq!(
            client.outcome(&request).await.unwrap_err().code,
            ErrorCode::InvalidRequest,
        );

        let catalog_events = async || {
            let bodies = fixture
                .node
                .with_storage(|db| {
                    Box::pin(async move {
                        let mut rows = db
                            .query("SELECT body FROM events ORDER BY cursor", ())
                            .await
                            .map_err(crate::knowledge::database_fixture::fault)?;
                        let mut bodies = Vec::new();
                        while let Some(row) = rows
                            .next()
                            .await
                            .map_err(crate::knowledge::database_fixture::fault)?
                        {
                            bodies.push(
                                row.get::<Vec<u8>>(0)
                                    .map_err(crate::knowledge::database_fixture::fault)?,
                            );
                        }
                        Ok(bodies)
                    })
                })
                .await
                .unwrap();
            bodies
                .into_iter()
                .map(|body| serde_json::from_slice::<Event>(&body).unwrap())
                .filter(|event| matches!(event, Event::LocalModelsChanged { .. }))
                .count()
        };
        let before = catalog_events().await;
        let request = client.prepare(Command::DeleteLocalModel {
            id: "potion-multi".to_owned(),
        });
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let deleted = admission.completion.await.unwrap().unwrap();
        assert_eq!(
            deleted,
            Output::LocalModel {
                id: "potion-multi".to_owned()
            }
        );
        assert_eq!(client.execute(request).await.unwrap(), deleted);
        let Output::LocalModels(after) = execute(
            &client,
            Command::ReadLocalModels {
                kind: Some(sailry_protocol::models::Kind::Embedding),
            },
        )
        .await
        else {
            panic!("models expected")
        };
        assert_eq!(after.models, models);
        assert_eq!(catalog_events().await, before);
        execute(
            &client,
            Command::RemovePlugin {
                name,
                expected_revision: 1,
            },
        )
        .await;
    }
    fixture.close().await;
}
