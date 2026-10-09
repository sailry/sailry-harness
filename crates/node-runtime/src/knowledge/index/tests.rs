use super::*;
use sailry_link::CancellationToken;

mod evaluation;
mod files;
mod integers;
mod keywords;
mod performance;

fn open() -> (tempfile::TempDir, Index) {
    let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let index = Index::open(&directory.path().join("knowledge.db")).unwrap();
    (directory, index)
}

fn binding() -> Binding {
    Binding {
        id: KnowledgeId::new(),
        generation: 1,
        model: Some((Model::BgeSmallZh, 2)),
        scope: [0; 32],
        rebuild: 0,
        paused: false,
    }
}

fn document(locator: &str) -> Document {
    Document {
        locator: locator.into(),
        revision: "source-content".into(),
        modified_ms: 20,
        tags: vec!["selected".into()],
        embedded: true,
    }
}

#[test]
fn retains_cached_text_when_paused() {
    let (_directory, mut index) = open();
    let original = binding();
    index.bind(original).unwrap();
    index
        .replace(
            original,
            &document("notes.md"),
            &[part("向量数据库", Some(&[1., 0.]))],
        )
        .unwrap();
    let paused = Binding {
        generation: 2,
        paused: true,
        ..original
    };
    let page = index
        .search(
            &query(),
            &[paused],
            &[(Model::BgeSmallZh, vec![1., 0.])],
            false,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(page.mode, Mode::Keyword);
    assert_eq!(page.hits.len(), 1);
    assert!(!index.current(paused).unwrap());
    let citation = &page.hits[0].citation;
    let request = CitationRequest {
        context: false,
        collection: citation.collection,
        chunk: citation.chunk.clone(),
        revision: citation.revision.clone(),
        offset: 0,
        limit: 1000,
    };
    assert_eq!(
        index.citation(paused, &request).unwrap().citation,
        *citation
    );
    for blocked in [
        Binding {
            scope: [1; 32],
            ..paused
        },
        Binding {
            rebuild: 1,
            ..paused
        },
    ] {
        assert!(
            index
                .search(&query(), &[blocked], &[], false, &CancellationToken::new())
                .unwrap()
                .hits
                .is_empty()
        );
        assert_eq!(
            index.citation(blocked, &request).err().unwrap().code,
            ErrorCode::RevisionConflict
        );
    }
    index.bind(paused).unwrap();
    assert_eq!(
        index
            .replace(original, &document("notes.md"), &[])
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    let stale = Binding {
        paused: true,
        ..original
    };
    assert!(
        index
            .search(&query(), &[stale], &[], false, &CancellationToken::new())
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        index.citation(stale, &request).err().unwrap().code,
        ErrorCode::RevisionConflict
    );
    let resumed = Binding {
        generation: 3,
        paused: false,
        ..paused
    };
    assert!(
        index
            .search(&query(), &[resumed], &[], false, &CancellationToken::new())
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        index.citation(resumed, &request).err().unwrap().code,
        ErrorCode::RevisionConflict
    );
    index.bind(resumed).unwrap();
    let page = index
        .search(
            &query(),
            &[resumed],
            &[(Model::BgeSmallZh, vec![1., 0.])],
            false,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(page.mode, Mode::Hybrid);
    assert_eq!(page.hits.len(), 1);
}

#[test]
fn clears_changed_sources_and_explicit_rebuilds() {
    for rebuild in [false, true] {
        let (_directory, mut index) = open();
        let original = binding();
        index.bind(original).unwrap();
        index
            .replace(
                original,
                &document("notes.md"),
                &[part("向量数据库", Some(&[1., 0.]))],
            )
            .unwrap();
        let changed = Binding {
            generation: 2,
            scope: if rebuild { original.scope } else { [1; 32] },
            rebuild: u64::from(rebuild),
            ..original
        };
        index.bind(changed).unwrap();
        assert_eq!(index.counts(changed).unwrap(), (0, 0));
        assert!(
            index
                .search(&query(), &[changed], &[], false, &CancellationToken::new())
                .unwrap()
                .hits
                .is_empty()
        );
        assert_eq!(
            index
                .replace(original, &document("notes.md"), &[])
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
    }
}

#[test]
fn tracks_processed_and_missing_embeddings() {
    let (_directory, mut index) = open();
    let binding = binding();
    index.bind(binding).unwrap();
    let mut document = document("notes.md");
    document.embedded = false;
    index
        .replace(binding, &document, &[part("向量数据库", None)])
        .unwrap();
    assert!(index.unchanged(binding, &document).unwrap());
    document.embedded = true;
    assert!(!index.unchanged(binding, &document).unwrap());
    index
        .replace(binding, &document, &[part("unknown tokens", None)])
        .unwrap();
    assert!(index.unchanged(binding, &document).unwrap());
    document.embedded = false;
    assert!(index.unchanged(binding, &document).unwrap());
}

#[test]
fn invalidates_links_when_chunk_boundaries_change() {
    let (_directory, mut index) = open();
    let original = binding();
    index.bind(original).unwrap();
    let document = document("notes.md");
    index
        .replace(original, &document, &[part("向量数据库", Some(&[1., 0.]))])
        .unwrap();
    let citation = index
        .search(&query(), &[original], &[], false, &CancellationToken::new())
        .unwrap()
        .hits
        .remove(0)
        .citation;
    let changed = Binding {
        generation: 2,
        model: Some((Model::GraniteMulti, 3)),
        ..original
    };
    index.bind(changed).unwrap();
    let mut chunk = part("向量数据库", Some(&[1., 0., 0.]));
    chunk.position = Position::Lines { start: 5, end: 9 };
    index.replace(changed, &document, &[chunk]).unwrap();
    let current = index
        .search(&query(), &[changed], &[], false, &CancellationToken::new())
        .unwrap()
        .hits
        .remove(0)
        .citation;
    assert_ne!(citation.chunk, current.chunk);
    assert_ne!(citation.link, current.link);
    let request = CitationRequest {
        context: false,
        collection: citation.collection,
        chunk: citation.chunk,
        revision: citation.revision,
        offset: 0,
        limit: 1000,
    };
    assert_eq!(
        index.citation(changed, &request).err().unwrap().code,
        ErrorCode::NotFound
    );
}

fn part(text: &str, vector: Option<&[f32]>) -> Part {
    Part {
        title: "Source heading".into(),
        text: text.into(),
        position: Position::Lines { start: 4, end: 8 },
        vector: vector.map(|values| Vector::quantize(values).unwrap()),
    }
}

fn query() -> Query {
    Query {
        query: "向量".into(),
        collections: Vec::new(),
        mode: Mode::Hybrid,
        tags: Vec::new(),
        path: None,
        modified_after_ms: None,
        modified_before_ms: None,
        limit: 5,
        offset: 0,
    }
}

#[test]
fn filters_both_ranks_before_fusion() {
    let (_directory, mut index) = open();
    let selected = binding();
    let foreign = binding();
    for binding in [selected, foreign] {
        index.bind(binding).unwrap();
        index
            .replace(
                binding,
                &document("docs/selected.md"),
                &[part("我们使用向量数据库", Some(&[1., 0.]))],
            )
            .unwrap();
    }
    let mut excluded = document("docs/excluded.md");
    excluded.tags = vec!["excluded".into()];
    index
        .replace(selected, &excluded, &[part("向量数据库", Some(&[1., 0.]))])
        .unwrap();
    let mut query = query();
    query.tags = vec!["selected".into()];
    query.path = Some("docs/selected".into());
    query.modified_after_ms = Some(20);
    query.modified_before_ms = Some(20);
    for mode in [Mode::Keyword, Mode::Semantic, Mode::Hybrid] {
        query.mode = mode;
        let page = index
            .search(
                &query,
                &[selected],
                &[(Model::BgeSmallZh, vec![1., 0.])],
                false,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(page.mode, mode);
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].citation.collection, selected.id);
        assert_eq!(page.hits[0].citation.path, "docs/selected.md");
        assert_eq!(page.hits[0].citation.revision, "source-content");
        assert_eq!(
            page.hits[0].citation.position,
            Position::Lines { start: 4, end: 8 }
        );
        assert!(page.untrusted);
    }
    query.modified_after_ms = Some(21);
    query.modified_before_ms = None;
    let page = index
        .search(
            &query,
            &[selected],
            &[(Model::BgeSmallZh, vec![1., 0.])],
            false,
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(page.hits.is_empty());
}

#[test]
fn falls_back_without_mixing_models() {
    let (_directory, mut index) = open();
    let original = binding();
    index.bind(original).unwrap();
    let document = document("note.md");
    index
        .replace(original, &document, &[part("向量数据库", Some(&[1., 0.]))])
        .unwrap();
    assert!(index.unchanged(original, &document).unwrap());
    let changed = Binding {
        generation: 2,
        model: Some((Model::GraniteMulti, 3)),
        ..original
    };
    index.bind(changed).unwrap();
    assert!(!index.unchanged(changed, &document).unwrap());
    let page = index
        .search(
            &query(),
            &[changed],
            &[(Model::GraniteMulti, vec![1., 0., 0.])],
            true,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(page.mode, Mode::Keyword);
    assert_eq!(page.model, None);
    assert!(page.indexing);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(
        index.replace(original, &document, &[]).unwrap_err().code,
        ErrorCode::Cancelled
    );
    index.retain(&[]).unwrap();
    assert_eq!(
        index.replace(changed, &document, &[]).unwrap_err().code,
        ErrorCode::Cancelled
    );
    assert!(
        index
            .search(&query(), &[changed], &[], false, &CancellationToken::new())
            .unwrap()
            .hits
            .is_empty()
    );
}

#[test]
fn replaces_source_and_rolls_back_invalid_vectors() {
    let (_directory, mut index) = open();
    let binding = binding();
    index.bind(binding).unwrap();
    let mut first = document("first.md");
    let second = document("second.md");
    index
        .replace(binding, &first, &[part("向量 first", Some(&[1., 0.]))])
        .unwrap();
    index
        .replace(binding, &second, &[part("向量 second", Some(&[0., 1.]))])
        .unwrap();
    first.revision = "changed-content".into();
    assert_eq!(
        index
            .replace(binding, &first, &[part("replacement", Some(&[1., 0., 0.]))])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(index.counts(binding).unwrap(), (2, 2));
    assert_eq!(
        index
            .search(&query(), &[binding], &[], false, &CancellationToken::new())
            .unwrap()
            .hits
            .len(),
        2
    );
    index
        .replace(binding, &first, &[part("replacement", Some(&[1., 0.]))])
        .unwrap();
    let page = index
        .search(&query(), &[binding], &[], false, &CancellationToken::new())
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].citation.path, second.locator);
    index.prune(binding, &[first.locator]).unwrap();
    assert_eq!(index.counts(binding).unwrap(), (1, 1));
    assert!(
        index
            .search(&query(), &[binding], &[], false, &CancellationToken::new())
            .unwrap()
            .hits
            .is_empty()
    );
}

#[test]
fn removes_unregistered_collections_after_reopening() {
    type Rows = Vec<Vec<rusqlite::types::Value>>;

    fn contents(index: &Index, id: KnowledgeId) -> Vec<Rows> {
        [
            "SELECT * FROM collections WHERE id=?1 ORDER BY id",
            "SELECT * FROM sources WHERE collection=?1 ORDER BY id",
            "SELECT c.* FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1 ORDER BY c.id",
            "SELECT f.rowid,f.* FROM chunks_fts f JOIN chunks c ON c.id=f.rowid JOIN sources s ON s.id=c.source WHERE s.collection=?1 ORDER BY f.rowid",
        ]
        .into_iter()
        .map(|sql| {
            let mut statement = index.db.prepare(sql).unwrap();
            let columns = statement.column_count();
            statement
                .query_map([id.to_string()], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap()
                .map(Result::unwrap)
                .collect()
        })
        .collect()
    }

    let (directory, mut index) = open();
    let kept = binding();
    let removed = Binding {
        id: KnowledgeId::new(),
        ..kept
    };
    for (binding, text) in [(kept, "向量 kept source"), (removed, "向量 deleted source")] {
        index.bind(binding).unwrap();
        index
            .replace(
                binding,
                &document("notes.md"),
                &[part(text, Some(&[1., 0.]))],
            )
            .unwrap();
    }
    let original = contents(&index, kept.id);
    assert!(original.iter().all(|rows| rows.len() == 1));
    drop(index);

    let mut index = Index::open(&directory.path().join("knowledge.db")).unwrap();
    index.retain(&[kept.id]).unwrap();
    assert_eq!(contents(&index, kept.id), original);
    assert!(contents(&index, removed.id).iter().all(Vec::is_empty));
    for table in ["collections", "sources", "chunks", "chunks_fts"] {
        let count: i64 = index
            .db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1, "{table}");
    }
    let page = index
        .search(
            &query(),
            &[kept, removed],
            &[],
            false,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].text, "向量 kept source");
    assert_eq!(page.hits[0].citation.collection, kept.id);
    assert_eq!(
        index
            .replace(removed, &document("notes.md"), &[])
            .unwrap_err()
            .code,
        ErrorCode::Cancelled,
    );
    index.retain(&[kept.id]).unwrap();
    assert_eq!(contents(&index, kept.id), original);
    index.retain(&[]).unwrap();
    for table in ["collections", "sources", "chunks", "chunks_fts"] {
        let count: i64 = index
            .db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

#[test]
fn pages_stable_candidates_and_observes_cancel() {
    let (_directory, mut index) = open();
    let binding = Binding {
        model: None,
        ..binding()
    };
    index.bind(binding).unwrap();
    for number in 0..3 {
        index
            .replace(
                binding,
                &document(&format!("{number}.md")),
                &[part("向量", None)],
            )
            .unwrap();
    }
    let mut query = query();
    query.limit = 1;
    let cancel = CancellationToken::new();
    let first = index
        .search(&query, &[binding], &[], false, &cancel)
        .unwrap();
    query.offset = first.next_offset.unwrap();
    let second = index
        .search(&query, &[binding], &[], false, &cancel)
        .unwrap();
    assert_ne!(first.hits[0].citation.chunk, second.hits[0].citation.chunk);
    query.offset = second.next_offset.unwrap();
    assert!(
        index
            .search(&query, &[binding], &[], false, &cancel)
            .unwrap()
            .next_offset
            .is_none()
    );
    cancel.cancel();
    query.offset = 0;
    assert_eq!(
        index
            .search(&query, &[binding], &[], false, &cancel)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}
