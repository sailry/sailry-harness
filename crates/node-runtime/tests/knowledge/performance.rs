use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

type Chunk = (String, String, String, String, Option<Vec<u8>>);

#[derive(Debug, PartialEq, Eq)]
struct Indexed {
    revision: String,
    modified: u64,
    embedded: bool,
    chunks: Vec<Chunk>,
}

fn records(profile: &Path, id: KnowledgeId) -> BTreeMap<String, Indexed> {
    let db = rusqlite::Connection::open(profile.join("storage/knowledge.db")).unwrap();
    let mut sources = db.prepare("SELECT id,locator,revision,modified_ms,embedded FROM sources WHERE collection=?1 ORDER BY locator").unwrap();
    let mut chunks = db
        .prepare(
            "SELECT key,title,text,position,vector FROM chunks WHERE source=?1 ORDER BY ordinal",
        )
        .unwrap();
    sources
        .query_map([id.to_string()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })
        .unwrap()
        .map(|row| {
            let (id, path, revision, modified, embedded) = row.unwrap();
            let chunks = chunks
                .query_map([id], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (
                path,
                Indexed {
                    revision,
                    modified: u64::try_from(modified).unwrap(),
                    embedded,
                    chunks,
                },
            )
        })
        .collect()
}

async fn ready(client: &Client, id: KnowledgeId, generation: u64, profile: &Path) -> Collection {
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let state = list(client)
                .await
                .into_iter()
                .find(|entry| entry.id == id)
                .unwrap();
            if state.progress.generation == generation
                && state.progress.status == Status::Ready
                && records(profile, id).len() == 500
            {
                assert_eq!(state.progress.sources, 500);
                assert_eq!(state.progress.completed, 500);
                assert_eq!(state.progress.skipped, 0);
                assert_eq!(state.progress.model, Some("potion-multi".to_owned()));
                return state;
            }
            assert_ne!(
                state.progress.status,
                Status::Paused,
                "collection indexing failed"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("mixed corpus reached ready state")
}

async fn retrieval(
    client: &Client,
    id: KnowledgeId,
    text: &str,
    mode: Mode,
    offset: u32,
    path: Option<&str>,
) -> Page {
    let mut input = query(text, id);
    input.mode = mode;
    input.offset = offset;
    input.limit = PAGE_LIMIT;
    input.path = path.map(str::to_owned);
    let admission = client
        .dispatch(client.prepare(Command::QueryKnowledge(input)))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    let Output::KnowledgeSearch(page) = admission.completion.await.unwrap().unwrap() else {
        panic!("search expected")
    };
    assert!(page.untrusted);
    page
}

fn corpus(root: &Path) {
    fs::create_dir(root).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/office/fixtures");
    for index in 0..500 {
        let (extension, source) = match index % 6 {
            0 => ("txt", None),
            1 => ("md", None),
            2 => ("pdf", Some("report.pdf")),
            3 => ("docx", Some("report.docx")),
            4 => ("xlsx", Some("book.xlsx")),
            5 => ("pptx", Some("deck.pptx")),
            _ => unreachable!(),
        };
        let path = root.join(format!("source-{index:03}.{extension}"));
        if let Some(source) = source {
            fs::copy(fixtures.join(source), path).unwrap();
        } else {
            fs::write(
                path,
                format!("Corpus source {index}\nData knowledge storage uses 向量数据库\n"),
            )
            .unwrap();
        }
    }
}

#[tokio::test]
#[ignore = "requires the pinned Potion model cache and performs real offline CPU inference"]
async fn indexes_mixed_corpus_and_reuses_unchanged_sources() {
    let cache = std::env::var_os("SAILRY_KNOWLEDGE_MODEL_CACHE")
        .map(PathBuf::from)
        .expect("SAILRY_KNOWLEDGE_MODEL_CACHE is required");
    for remote in [false, true] {
        let fixture = Fixture::open(Some(&cache)).await;
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
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
        assert_eq!(
            models
                .iter()
                .find(|info| info.id == "potion-multi")
                .unwrap()
                .state,
            ModelState::Installed
        );
        let mixed = fixture.root.join("mixed");
        corpus(&mixed);
        let mut definition = definition(Source::Worktree {
            worktree: fixture.worktree,
            path: "mixed".into(),
        });
        definition.model = Some("potion-multi".to_owned());
        let started = Instant::now();
        let collection = super::saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition,
                    plugin: None,
                },
            )
            .await,
        );
        tokio::time::timeout(Duration::from_secs(180), async {
            loop {
                let progress = list(&client)
                    .await
                    .into_iter()
                    .find(|entry| entry.id == collection.id)
                    .unwrap()
                    .progress;
                if progress.status == Status::Indexing
                    && progress.completed > 0
                    && progress.completed < 500
                {
                    break;
                }
                assert_ne!(
                    progress.status,
                    Status::Ready,
                    "bulk indexing completed before the foreground probe"
                );
                assert_ne!(progress.status, Status::Paused, "bulk indexing failed");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let foreground = Instant::now();
        let live = tokio::time::timeout(
            Duration::from_secs(5),
            retrieval(
                &client,
                collection.id,
                "English document knowledge",
                Mode::Semantic,
                0,
                None,
            ),
        )
        .await
        .expect("foreground query waited behind bulk indexing");
        assert!(live.indexing);
        assert!(!live.hits.is_empty());
        assert_eq!(live.model, Some("potion-multi".to_owned()));
        let foreground_ms = foreground.elapsed().as_millis();
        let profile = fixture.temp.path().join("node");
        let first_state = ready(
            &client,
            collection.id,
            collection.progress.generation,
            &profile,
        )
        .await;
        let first_ms = started.elapsed().as_millis();
        let before = records(&profile, collection.id);
        assert_eq!(before.len(), 500);
        assert!(before.values().all(|source| source.embedded));
        assert!(
            before
                .values()
                .any(|source| source.chunks.iter().any(|chunk| chunk.4.is_some()))
        );
        let encoded = fixture.node.indexed_knowledge_inputs();
        assert!(encoded >= 500);
        let mut seen = BTreeSet::new();
        let probes = (0..5)
            .map(|number| ("Corpus source", Some(format!("mixed/source-{number}"))))
            .chain(
                ["PDF acceptance", "中文内容", "合计", "中文幻灯片"]
                    .into_iter()
                    .map(|term| (term, None)),
            );
        for (term, path) in probes {
            let mut offset = 0;
            let mut opened = false;
            loop {
                let page = retrieval(
                    &client,
                    collection.id,
                    term,
                    Mode::Keyword,
                    offset,
                    path.as_deref(),
                )
                .await;
                assert!(!page.indexing);
                assert!(!page.hits.is_empty());
                for hit in &page.hits {
                    seen.insert(hit.citation.path.clone());
                    if !opened {
                        let input = CitationRequest {
                            context: false,
                            collection: collection.id,
                            chunk: hit.citation.chunk.clone(),
                            revision: hit.citation.revision.clone(),
                            offset: 0,
                            limit: 64 * 1024,
                        };
                        let Output::KnowledgeCitation(context) =
                            execute(&client, Command::ReadKnowledgeCitation(input)).await
                        else {
                            panic!("citation expected")
                        };
                        assert_eq!(context.citation.position, hit.citation.position);
                        assert!(context.text.contains(term));
                        opened = true;
                    }
                }
                let Some(next) = page.next_offset else { break };
                assert!(next > offset);
                offset = next;
            }
        }
        assert_eq!(seen.len(), 500);
        assert_eq!(fixture.node.indexed_knowledge_inputs(), encoded);
        let paused = super::saved(
            execute(
                &client,
                Command::PauseKnowledgeCollection {
                    id: collection.id,
                    expected_revision: first_state.revision,
                    paused: true,
                },
            )
            .await,
        );
        let retained = retrieval(
            &client,
            collection.id,
            "Corpus source",
            Mode::Keyword,
            0,
            None,
        )
        .await;
        assert!(!retained.hits.is_empty());
        assert_eq!(fixture.node.indexed_knowledge_inputs(), encoded);
        fs::write(
            mixed.join("source-000.txt"),
            "Updated singleton source\nOnly this document changed its knowledge text\n",
        )
        .unwrap();
        let resumed = super::saved(
            execute(
                &client,
                Command::PauseKnowledgeCollection {
                    id: collection.id,
                    expected_revision: paused.revision,
                    paused: false,
                },
            )
            .await,
        );
        ready(
            &client,
            collection.id,
            resumed.progress.generation,
            &profile,
        )
        .await;
        let after = records(&profile, collection.id);
        assert_eq!(after.len(), 500);
        assert_eq!(before["mixed/source-000.txt"].chunks.len(), 1);
        assert_ne!(
            before["mixed/source-000.txt"],
            after["mixed/source-000.txt"]
        );
        for (path, original) in &before {
            if path != "mixed/source-000.txt" {
                assert_eq!(
                    &after[path], original,
                    "unchanged source was rewritten: {path}"
                );
            }
        }
        assert_eq!(fixture.node.indexed_knowledge_inputs() - encoded, 1);
        let rebuild = super::saved(
            execute(
                &client,
                Command::RebuildKnowledgeCollection {
                    id: collection.id,
                    expected_revision: resumed.revision,
                },
            )
            .await,
        );
        assert_eq!(rebuild.rebuild, 1);
        ready(
            &client,
            collection.id,
            rebuild.progress.generation,
            &profile,
        )
        .await;
        let rebuilt = records(&profile, collection.id);
        assert_eq!(rebuilt, after);
        assert!(fixture.node.indexed_knowledge_inputs() > encoded + 1);
        let Fixture {
            temp,
            root: _,
            node,
            controller,
            worktree: _,
        } = fixture;
        drop(client);
        node.shutdown().await.unwrap();
        fs::remove_file(profile.join("storage/knowledge.db")).unwrap();
        let node = Node::start_empty_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        ready(
            &client,
            collection.id,
            rebuild.progress.generation,
            &profile,
        )
        .await;
        let restored = records(&profile, collection.id);
        assert_eq!(restored, after);
        let restored_query = retrieval(
            &client,
            collection.id,
            "document knowledge",
            Mode::Semantic,
            0,
            None,
        )
        .await;
        assert_eq!(restored_query.model, Some("potion-multi".to_owned()));
        assert!(!restored_query.hits.is_empty());
        println!(
            "mixed corpus remote={remote}, sources=500, initial_ms={first_ms}, foreground_ms={foreground_ms}, initial_encoded={encoded}, incremental_encoded=1"
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        drop(temp);
    }
}
