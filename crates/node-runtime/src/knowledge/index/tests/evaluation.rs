use super::*;
use sailry_embedding::{Engine, ModelId};
use serde::Deserialize;
use std::{path::PathBuf, time::Instant};

#[derive(Deserialize)]
struct Corpus {
    documents: Vec<Source>,
    queries: Vec<Question>,
}

#[derive(Deserialize)]
struct Source {
    id: String,
    title: String,
    text: String,
}

#[derive(Deserialize)]
struct Question {
    text: String,
    expected: Vec<String>,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!("corpus.json")).unwrap()
}

pub(super) fn engine() -> Engine {
    let root = std::env::var_os("SAILRY_EMBEDDING_REFERENCES")
        .expect("set SAILRY_EMBEDDING_REFERENCES to the isolated verified model cache");
    let descriptor = ModelId::GraniteMulti.descriptor();
    let directory = PathBuf::from(root).join("granite-multi");
    let files = descriptor
        .assets
        .iter()
        .map(|asset| sailry_model_assets::File {
            name: asset.name,
            size: asset.bytes,
            sha256: asset.sha256,
        })
        .collect::<Vec<_>>();
    assert!(
        sailry_model_assets::ready(&directory, &files),
        "Granite assets differ from catalog revision {}",
        descriptor.revision
    );
    let started = Instant::now();
    let engine = Engine::load(descriptor.id, &directory).unwrap();
    println!(
        "model=granite-multi revision={} load_ms={:.3} dimension={}",
        descriptor.revision,
        started.elapsed().as_secs_f64() * 1000.,
        engine.dimension()
    );
    engine
}

fn encode(engine: &mut Engine, inputs: &[String]) -> Vec<Vec<f32>> {
    let mut vectors = Vec::with_capacity(inputs.len());
    for batch in inputs.chunks(4) {
        vectors.extend(
            engine
                .encode(&batch.iter().map(String::as_str).collect::<Vec<_>>())
                .unwrap(),
        );
    }
    for vector in &vectors {
        let norm = vector.iter().map(|value| value * value).sum::<f32>();
        assert!(
            (norm - 1.).abs() < 1e-4,
            "engine must return normalized vectors"
        );
    }
    vectors
}

fn ids(page: &Page) -> Vec<String> {
    page.hits
        .iter()
        .map(|hit| {
            hit.citation
                .path
                .strip_prefix("corpus/")
                .unwrap()
                .strip_suffix(".md")
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn recall(question: &Question, page: &Page) -> usize {
    let found = ids(page);
    question
        .expected
        .iter()
        .filter(|expected| found.contains(expected))
        .count()
}

#[test]
fn corpus_has_original_sources_and_ground_truth() {
    let corpus = corpus();
    assert_eq!(corpus.documents.len(), 30);
    assert_eq!(corpus.queries.len(), 30);
    let ids = corpus
        .documents
        .iter()
        .map(|source| source.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), corpus.documents.len());
    assert_eq!(
        corpus
            .documents
            .iter()
            .filter(|source| source.text.is_ascii())
            .count(),
        15
    );
    for source in &corpus.documents {
        assert!(source.text.chars().count() >= 120);
        assert!(!source.title.is_empty());
        assert!(
            source
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        );
    }
    for question in &corpus.queries {
        assert!(!question.text.is_empty());
        assert_eq!(question.expected.len(), 1);
        assert!(
            question
                .expected
                .iter()
                .all(|expected| ids.contains(expected))
        );
    }
}

#[test]
#[ignore = "requires verified pinned Granite assets; measures this 30-case corpus only"]
fn hybrid_improves_bilingual_recall_at_five() {
    let corpus = corpus();
    let mut engine = engine();
    let dimension = engine.dimension();
    let texts = corpus
        .documents
        .iter()
        .map(|source| format!("{}\n{}", source.title, source.text))
        .collect::<Vec<_>>();
    let started = Instant::now();
    let vectors = encode(&mut engine, &texts);
    println!(
        "documents={} document_encode_ms={:.3}",
        texts.len(),
        started.elapsed().as_secs_f64() * 1000.
    );
    let (_directory, mut index) = open();
    let binding = Binding {
        id: KnowledgeId::new(),
        generation: 1,
        scope: [0; 32],
        rebuild: 0,
        paused: false,
        model: Some(("granite-multi".to_owned(), dimension)),
    };
    index.bind(&binding).unwrap();
    for ((source, text), vector) in corpus.documents.iter().zip(&texts).zip(&vectors) {
        index
            .replace(
                &binding,
                &Document {
                    locator: format!("corpus/{}.md", source.id),
                    revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
                    modified_ms: 1,
                    tags: Vec::new(),
                    embedded: true,
                },
                &[Part {
                    title: source.title.clone(),
                    text: source.text.clone(),
                    position: Position::Lines { start: 1, end: 2 },
                    vector: Some(Vector::quantize(vector).unwrap()),
                }],
            )
            .unwrap();
    }
    assert_eq!(index.counts(&binding).unwrap(), (30, 30));
    let questions = corpus
        .queries
        .iter()
        .map(|question| question.text.clone())
        .collect::<Vec<_>>();
    let started = Instant::now();
    let query_vectors = encode(&mut engine, &questions);
    println!(
        "queries={} query_encode_ms={:.3}",
        questions.len(),
        started.elapsed().as_secs_f64() * 1000.
    );
    let mut keyword = 0;
    let mut semantic = 0;
    let mut hybrid = 0;
    let total = corpus
        .queries
        .iter()
        .map(|question| question.expected.len())
        .sum::<usize>();
    for (number, (question, vector)) in corpus.queries.iter().zip(query_vectors).enumerate() {
        let mut query = query();
        query.query = question.text.clone();
        query.mode = Mode::Keyword;
        let lexical = index
            .search(
                &query,
                &[binding.clone()],
                &[],
                false,
                &CancellationToken::new(),
            )
            .unwrap();
        query.mode = Mode::Semantic;
        let encoded = index
            .search(
                &query,
                &[binding.clone()],
                &[("granite-multi".to_owned(), vector.clone())],
                false,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(encoded.mode, Mode::Semantic);
        assert_eq!(encoded.model, Some("granite-multi".to_owned()));
        query.mode = Mode::Hybrid;
        let combined = index
            .search(
                &query,
                &[binding.clone()],
                &[("granite-multi".to_owned(), vector)],
                false,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(combined.mode, Mode::Hybrid);
        assert_eq!(combined.model, Some("granite-multi".to_owned()));
        assert!(combined.hits.len() <= 5);
        keyword += recall(question, &lexical);
        semantic += recall(question, &encoded);
        hybrid += recall(question, &combined);
        println!(
            "case={} query={:?} expected={:?} bm25={:?} semantic={:?} hybrid={:?}",
            number + 1,
            question.text,
            question.expected,
            ids(&lexical),
            ids(&encoded),
            ids(&combined)
        );
    }
    println!(
        "corpus=30-original-bilingual-sources queries={} bm25_recall_at_5={}/{} ({:.3}) semantic_recall_at_5={}/{} ({:.3}) hybrid_recall_at_5={}/{} ({:.3})",
        corpus.queries.len(),
        keyword,
        total,
        keyword as f64 / total as f64,
        semantic,
        total,
        semantic as f64 / total as f64,
        hybrid,
        total,
        hybrid as f64 / total as f64
    );
    assert!(
        hybrid >= 24,
        "Hybrid must find at least 24/30 corpus targets; got {hybrid}"
    );
    assert!(
        hybrid >= keyword + 6,
        "Hybrid must gain at least 6/30 corpus targets over BM25; Hybrid={hybrid}, BM25={keyword}"
    );
}
