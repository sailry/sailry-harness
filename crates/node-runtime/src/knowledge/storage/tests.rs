use super::*;
use crate::knowledge::index::{Binding, Document, Index, Part};
use sailry_protocol::{KnowledgeId, plugin};

fn query(mode: Mode) -> storage::Search {
    storage::Search {
        terms: vec!["literal".into()],
        query: Some("Original unsorted query".into()),
        mode,
        weights: [3, 1],
        all: vec![],
        any: vec![],
        offset: 0,
        limit: 100,
    }
}

fn plan() -> Plan {
    Plan {
        package: plugin::Reference {
            name: "private-memory".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        collections: vec![],
        keyword: vec![
            ("shared".into(), 1),
            ("lexical".into(), 2),
            ("plain".into(), 3),
        ],
        eligible: BTreeSet::from(["shared".into(), "lexical".into(), "semantic".into()]),
        settings_revision: 0,
    }
}

fn hit(collection: KnowledgeId, key: &str) -> Hit {
    Hit {
        title: "Raw title".into(),
        text: "Source chunk".into(),
        score: 900_000,
        stale: false,
        citation: Citation {
            collection,
            chunk: "b".repeat(64),
            revision: "c".repeat(64),
            path: key.into(),
            position: Position::Lines { start: 1, end: 2 },
            link: String::new(),
        },
    }
}

#[test]
fn deduplicates_metadata_before_single_fusion() {
    let id = KnowledgeId::new();
    let selected = select(
        &query(Mode::Hybrid),
        &plan(),
        vec![
            hit(id, "shared"),
            hit(id, "shared"),
            hit(id, "semantic"),
            hit(id, "foreign"),
        ],
        false,
        true,
    );
    assert_eq!(selected.mode, Mode::Hybrid);
    assert_eq!(selected.stamps.len(), 2);
    assert_eq!(selected.keys, ["shared", "lexical", "semantic", "plain"]);
    assert!(!selected.indexing);
}

#[test]
fn retains_unindexed_and_partial_keyword_results() {
    let id = KnowledgeId::new();
    let complete = select(
        &query(Mode::Semantic),
        &plan(),
        vec![hit(id, "semantic")],
        false,
        true,
    );
    assert_eq!(complete.keys, ["plain", "semantic"]);
    assert_eq!(complete.mode, Mode::Hybrid);
    let partial = select(
        &query(Mode::Semantic),
        &plan(),
        vec![hit(id, "semantic")],
        true,
        true,
    );
    assert!(partial.keys.contains(&"lexical".into()));
    assert!(partial.indexing);
    let missing = select(&query(Mode::Semantic), &plan(), vec![], false, false);
    assert_eq!(missing.mode, Mode::Keyword);
    assert_eq!(missing.keys, ["shared", "lexical", "plain"]);
    assert!(missing.stamps.is_empty());
}

#[test]
fn filters_any_tags_before_bounded_vector_ranks() {
    let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut index = Index::open(&directory.path().join("knowledge.db")).unwrap();
    let binding = Binding {
        id: KnowledgeId::new(),
        generation: 1,
        model: Some(("bge-small-zh".to_owned(), 2)),
        scope: [0; 32],
        rebuild: 0,
        paused: false,
    };
    index.bind(&binding).unwrap();
    for number in 0..121 {
        let selected = number == 120;
        let document = Document {
            locator: format!("meta/{number}"),
            revision: "d".repeat(64),
            modified_ms: 10,
            tags: vec![if selected { "project" } else { "foreign" }.into()],
            embedded: true,
        };
        let part = Part {
            title: "Raw title".into(),
            text: "vector lookup".into(),
            position: Position::Lines { start: 1, end: 1 },
            vector: Some(
                vector::Vector::quantize(if selected { &[0.9, 0.1] } else { &[1., 0.] }).unwrap(),
            ),
        };
        index.replace(&binding, &document, &[part]).unwrap();
    }
    let query = Query {
        query: "vector".into(),
        collections: vec![binding.id],
        mode: Mode::Semantic,
        tags: vec![],
        path: None,
        modified_after_ms: None,
        modified_before_ms: None,
        limit: 5,
        offset: 0,
    };
    let embeddings = [("bge-small-zh".to_owned(), vec![1., 0.])];
    let stop = CancellationToken::new();
    let unfiltered = index
        .candidates(&query, &[binding.clone()], &embeddings, &[], &stop)
        .unwrap();
    assert_eq!(unfiltered.len(), 100);
    assert!(unfiltered.iter().all(|hit| hit.citation.path != "meta/120"));
    let filtered = index
        .candidates(
            &query,
            &[binding.clone()],
            &embeddings,
            &["project".into(), "global".into()],
            &stop,
        )
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].citation.path, "meta/120");
    let mut wrong_all = query;
    wrong_all.tags = vec!["foreign".into()];
    assert!(
        index
            .candidates(
                &wrong_all,
                &[binding.clone()],
                &embeddings,
                &["project".into()],
                &stop
            )
            .unwrap()
            .is_empty()
    );
}
