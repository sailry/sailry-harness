use super::*;
use std::time::{Duration, Instant};

const SOURCES: usize = 100_000;
const QUERIES: usize = 20;

fn vector(state: &mut u64, dimension: usize) -> Vec<f32> {
    let mut values = (0..dimension)
        .map(|_| {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            ((*state >> 40) as f32 / (1u32 << 24) as f32) * 2. - 1.
        })
        .collect::<Vec<_>>();
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    for value in &mut values {
        *value /= norm;
    }
    values
}

async fn populate(index: &Index, binding: &Binding, dimension: usize) {
    // Batched fixture setup uses the actual schema; queries use production search.
    let mut lease = index.lease(false).await.unwrap();
    let tx = lease.connection_mut().transaction().await.unwrap();
    let mut source = tx.prepare("INSERT INTO sources(collection,locator,revision,modified_ms,tags,embedded) VALUES(?1,?2,?3,?4,?5,1) RETURNING id").await.unwrap();
    let mut chunk = tx.prepare("INSERT INTO chunks(key,source,ordinal,title,text,field0,field1,cjk0,cjk1,position,vector) VALUES(?1,?2,0,?3,?4,?5,?6,'','',?7,?8)").await.unwrap();
    let mut state = 0x29a4_f617_dc82_653bu64;
    let position = serde_json::to_string(&Position::Lines { start: 1, end: 3 }).unwrap();
    for number in 0..SOURCES {
        let locator = format!("docs/segment-{:02}/{number:06}.md", number % 10);
        let tags = if (number / 10) % 2 == 0 {
            "[\"selected\"]"
        } else {
            "[\"other\"]"
        };
        let source_id: i64 = source
            .query_row(params![
                binding.id.to_string(),
                locator,
                "fixture-content",
                i64::try_from(number).unwrap(),
                tags
            ])
            .await
            .unwrap()
            .get(0)
            .unwrap();
        let values = vector(&mut state, dimension);
        let bytes = Vector::quantize(&values).unwrap().encode();
        assert_eq!(bytes.len(), dimension + 4);
        let key = blake3::hash(&number.to_le_bytes()).to_hex().to_string();
        chunk
            .execute(params![
                key,
                source_id,
                "Background note",
                "Stored passage for scan measurement",
                text::field("Background note"),
                text::field("Stored passage for scan measurement"),
                position.as_str(),
                bytes
            ])
            .await
            .unwrap();
    }
    drop(source);
    drop(chunk);
    tx.commit().await.unwrap();
}

fn percentile(samples: &mut [Duration], percent: usize) -> Duration {
    samples.sort_unstable();
    samples[(samples.len() * percent).div_ceil(100).saturating_sub(1)]
}

fn report(label: &str, samples: &mut [Duration], candidates: usize) {
    let p50 = percentile(samples, 50);
    let p95 = percentile(samples, 95);
    println!(
        "workload={label} warm_queries={} eligible_vectors={candidates} p50_ms={:.3} p95_ms={:.3}",
        samples.len(),
        p50.as_secs_f64() * 1000.,
        p95.as_secs_f64() * 1000.
    );
}

#[tokio::test]
#[ignore = "requires verified Granite assets; allocates a 100k-vector isolated index"]
async fn measures_full_and_filtered_int8_scans() {
    let mut engine = super::evaluation::engine();
    let dimension = engine.dimension();
    assert_eq!(dimension, 384);
    let inputs = [
        "How do I stop a process and all of its children?",
        "模型下载取消以后，不要发布尚未验证的权重",
        "Keep a document draft after another writer changes the source",
        "多台控制端同时保存设置时应检查修订号",
    ];
    engine.encode(&inputs).unwrap();
    let mut encoding = Vec::new();
    let mut embeddings = Vec::new();
    for number in 0..QUERIES {
        let started = Instant::now();
        embeddings.push(
            engine
                .encode(&[inputs[number % inputs.len()]])
                .unwrap()
                .remove(0),
        );
        encoding.push(started.elapsed());
    }
    report("query-encoding-only", &mut encoding, 0);
    drop(engine);
    let (_directory, index) = open().await;
    let binding = Binding {
        id: KnowledgeId::new(),
        generation: 1,
        scope: [0; 32],
        rebuild: 0,
        paused: false,
        model: Some(("granite-multi".to_owned(), dimension)),
    };
    index.bind(&binding).await.unwrap();
    let started = Instant::now();
    populate(&index, &binding, dimension).await;
    println!(
        "fixture_sources={SOURCES} dimension={dimension} vector_fixture=deterministic-normalized-synthetic int8_blob_bytes={} setup_ms={:.3}",
        dimension + 4,
        started.elapsed().as_secs_f64() * 1000.
    );
    assert_eq!(
        index.counts(&binding).await.unwrap(),
        (SOURCES as u64, SOURCES as u64)
    );
    for filtered in [false, true] {
        let mut query = query();
        query.query = "cancellation of owned children".into();
        query.mode = Mode::Semantic;
        if filtered {
            query.tags = vec!["selected".into()];
            query.path = Some("docs/segment-03/".into());
            query.modified_after_ms = Some(25_000);
            query.modified_before_ms = Some(74_999);
        }
        for vector in &embeddings[..3] {
            index
                .search(
                    &query,
                    &[binding.clone()],
                    &[("granite-multi".to_owned(), vector.clone())],
                    false,
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
        }
        let mut samples = Vec::new();
        for vector in &embeddings {
            let started = Instant::now();
            let page = index
                .search(
                    &query,
                    &[binding.clone()],
                    &[("granite-multi".to_owned(), vector.clone())],
                    false,
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            samples.push(started.elapsed());
            assert_eq!(page.mode, Mode::Semantic);
            assert_eq!(page.hits.len(), 5);
            assert_eq!(page.model, Some("granite-multi".to_owned()));
            assert!(
                page.hits
                    .iter()
                    .all(|hit| hit.citation.collection == binding.id)
            );
            if filtered {
                assert!(
                    page.hits
                        .iter()
                        .all(|hit| hit.citation.path.starts_with("docs/segment-03/"))
                );
                for hit in &page.hits {
                    let number = hit
                        .citation
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .strip_suffix(".md")
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    assert!((25_000..75_000).contains(&number));
                    assert_eq!((number / 10) % 2, 0);
                }
            }
        }
        report(
            if filtered {
                "filtered-int8-scan"
            } else {
                "full-int8-scan"
            },
            &mut samples,
            if filtered { 2_500 } else { SOURCES },
        );
    }
}
