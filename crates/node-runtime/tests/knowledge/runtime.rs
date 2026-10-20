use super::*;
mod fixtures;

#[tokio::test]
async fn shares_runtime_configuration_and_conflicts() {
    let fixture = Fixture::start().await;
    let [local, remote] = fixture.clients();
    for (writer, reader) in [(&local, &remote), (&remote, &local)] {
        let Output::PythonSettings(previous) = execute(writer, Command::ReadPythonSettings).await
        else {
            panic!("Python settings expected")
        };
        let mut changed = previous.clone();
        changed.index = Some("https://pypi.org/simple/".into());
        let request = writer.prepare(Command::SavePythonSettings(changed));
        let admitted = writer.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let output = admitted.completion.await.unwrap().unwrap();
        assert_eq!(writer.execute(request.clone()).await.unwrap(), output);
        assert_eq!(execute(reader, Command::ReadPythonSettings).await, output);
        let Output::PythonSettings(mut newer) = output.clone() else {
            panic!("Python settings expected")
        };
        newer.index = Some("https://pypi.tuna.tsinghua.edu.cn/simple/".into());
        let current = execute(reader, Command::SavePythonSettings(newer)).await;
        assert_eq!(writer.execute(request).await.unwrap(), output);
        assert_eq!(execute(writer, Command::ReadPythonSettings).await, current);
        assert_eq!(execute(reader, Command::ReadPythonSettings).await, current);
        assert_eq!(
            writer
                .execute(writer.prepare(Command::SavePythonSettings(previous)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );

        let Output::LocalModelSettings(previous) =
            execute(writer, Command::ReadLocalModelSettings).await
        else {
            panic!("OCR settings expected")
        };
        let mut changed = previous.clone();
        let kind = sailry_protocol::models::Kind::Ocr;
        changed.choices.get_mut(&kind).unwrap().enabled = !previous.choice(kind).enabled;
        let output = execute(writer, Command::SaveLocalModelSettings(changed)).await;
        assert_eq!(
            execute(reader, Command::ReadLocalModelSettings).await,
            output
        );
        assert_eq!(
            writer
                .execute(writer.prepare(Command::SaveLocalModelSettings(previous)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::LocalModels(sailry_protocol::models::Catalog { models, .. }) = execute(
            reader,
            Command::ReadLocalModels {
                kind: Some(sailry_protocol::models::Kind::Ocr),
            },
        )
        .await
        else {
            panic!("OCR models expected")
        };
        assert_eq!(models.len(), 3);
        assert!(
            models
                .iter()
                .all(|model| model.state == sailry_protocol::models::State::Missing)
        );
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "requires the explicit pinned OCR reference fixture and packaged ONNX Runtime"]
async fn reads_images_and_indexes_scanned_sources() {
    let reference = PathBuf::from(
        std::env::var_os("SAILRY_OCR_REFERENCE").expect("OCR reference path required"),
    );
    let fixture = Fixture::start().await;
    let [local, remote] = fixture.clients();
    let Output::LocalModels(sailry_protocol::models::Catalog { models, .. }) = execute(
        &local,
        Command::ReadLocalModels {
            kind: Some(sailry_protocol::models::Kind::Ocr),
        },
    )
    .await
    else {
        panic!("OCR models expected")
    };
    let model = models
        .iter()
        .find(|model| model.definition.name == "PP-OCRv6 tiny")
        .unwrap();
    let directory = fixture.temp.path().join("node/models/ocr").join(&model.id);
    fs::create_dir(&directory).unwrap();
    for asset in &model.definition.assets {
        fs::copy(
            reference.join("pp-ocrv6-tiny").join(&asset.name),
            directory.join(&asset.name),
        )
        .unwrap();
    }
    let Output::LocalModels(sailry_protocol::models::Catalog { models, .. }) = execute(
        &remote,
        Command::ReadLocalModels {
            kind: Some(sailry_protocol::models::Kind::Ocr),
        },
    )
    .await
    else {
        panic!("OCR models expected")
    };
    assert_eq!(
        models
            .iter()
            .find(|entry| entry.id == model.id)
            .unwrap()
            .state,
        sailry_protocol::models::State::Missing
    );
    execute(
        &remote,
        Command::InstallLocalModel {
            id: model.id.clone(),
        },
    )
    .await;
    let Output::LocalModels(sailry_protocol::models::Catalog { models, .. }) = execute(
        &local,
        Command::ReadLocalModels {
            kind: Some(sailry_protocol::models::Kind::Ocr),
        },
    )
    .await
    else {
        panic!("OCR models expected")
    };
    assert_eq!(
        models
            .iter()
            .find(|entry| entry.id == model.id)
            .unwrap()
            .state,
        sailry_protocol::models::State::Installed
    );
    let Output::LocalModelSettings(mut settings) =
        execute(&remote, Command::ReadLocalModelSettings).await
    else {
        panic!("local model settings expected")
    };
    settings.choices.insert(
        sailry_protocol::models::Kind::Ocr,
        sailry_protocol::models::Choice {
            enabled: true,
            model: Some(model.id.clone()),
        },
    );
    execute(&remote, Command::SaveLocalModelSettings(settings)).await;
    fs::copy(reference.join("mixed.png"), fixture.root.join("scan.png")).unwrap();
    fixtures::scanned_pdf(&reference.join("mixed.png"), &fixture.root.join("scan.pdf"));
    let mut citations = Vec::new();
    for path in ["scan.png", "scan.pdf"] {
        for client in [&local, &remote] {
            let Output::OfficeContent(inspection) = execute(
                client,
                Command::ReadOffice {
                    worktree: fixture.worktree,
                    options: office::Read {
                        source: sailry_protocol::office::Source::Path(path.into()),
                        offset: 0,
                    },
                },
            )
            .await
            else {
                panic!("Office inspection expected")
            };
            assert!(
                inspection
                    .sections
                    .iter()
                    .any(|section| section.text.contains("Sailry")),
                "{inspection:?}"
            );
        }
        let collection = saved(
            execute(
                &remote,
                Command::CreateKnowledgeCollection {
                    definition: definition(Source::Worktree {
                        worktree: fixture.worktree,
                        path: path.into(),
                    }),
                    plugin: None,
                },
            )
            .await,
        );
        let citation = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let result = execute(
                    &local,
                    Command::QueryKnowledge(query("Sailry", collection.id)),
                )
                .await;
                if let Output::KnowledgeSearch(mut result) = result
                    && !result.hits.is_empty()
                {
                    break result.hits.remove(0).citation;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(citation.position, Position::Page { page: 1 });
        let input = CitationRequest {
            collection: collection.id,
            chunk: citation.chunk.clone(),
            revision: citation.revision.clone(),
            context: true,
            offset: 0,
            limit: 20000,
        };
        for client in [&local, &remote] {
            let Output::KnowledgeCitation(page) =
                execute(client, Command::ReadKnowledgeCitation(input.clone())).await
            else {
                panic!("OCR citation expected")
            };
            assert!(page.text.contains("Sailry"));
            assert!(page.untrusted && page.context);
        }
        citations.push(input);
    }
    let Output::LocalModelSettings(mut settings) =
        execute(&remote, Command::ReadLocalModelSettings).await
    else {
        panic!("OCR settings expected")
    };
    settings
        .choices
        .get_mut(&sailry_protocol::models::Kind::Ocr)
        .unwrap()
        .enabled = false;
    execute(&remote, Command::SaveLocalModelSettings(settings)).await;
    for client in [&local, &remote] {
        for input in &citations {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadKnowledgeCitation(input.clone())))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
        }
    }
    fixture.close().await;
}
