use super::*;
use serde_json::json;
use std::io::Write;
use tokenizers::{
    models::wordlevel::WordLevel, pre_tokenizers::whitespace::WhitespaceSplit,
    processors::template::TemplateProcessing,
};

#[path = "tests/reference.rs"]
mod reference;

fn fixture() -> (tempfile::TempDir, Engine) {
    let directory = tempfile::tempdir().unwrap();
    let dimension = ModelId::PotionMulti.descriptor().dimension;
    let vocabulary = ["[UNK]", "alpha", "beta", "東京", "[CLS]", "[SEP]"];
    let model = WordLevel::builder()
        .vocab(
            vocabulary
                .into_iter()
                .enumerate()
                .map(|(id, token)| (token.to_owned(), id as u32))
                .collect(),
        )
        .unk_token("[UNK]".into())
        .build()
        .unwrap();
    let mut tokenizer = Tokenizer::new(model);
    tokenizer.with_pre_tokenizer(Some(WhitespaceSplit));
    tokenizer.with_post_processor(Some(
        TemplateProcessing::builder()
            .try_single("[CLS] $0 [SEP]")
            .unwrap()
            .special_tokens(vec![("[CLS]", 4), ("[SEP]", 5)])
            .build()
            .unwrap(),
    ));
    tokenizer
        .with_truncation(Some(tokenizers::TruncationParams {
            max_length: 10,
            ..Default::default()
        }))
        .unwrap();
    tokenizer.with_padding(Some(tokenizers::PaddingParams {
        pad_id: 4,
        pad_token: "[CLS]".into(),
        ..Default::default()
    }));
    tokenizer
        .save(directory.path().join("tokenizer.json"), false)
        .unwrap();
    fs::write(
        directory.path().join("config.json"),
        br#"{"normalize":true}"#,
    )
    .unwrap();
    let mut vectors = vec![0.0f32; vocabulary.len() * dimension];
    vectors[0] = 100.0;
    vectors[dimension] = 1.0;
    vectors[2 * dimension + 1] = 1.0;
    vectors[3 * dimension + 2] = 1.0;
    vectors[4 * dimension + 3] = 100.0;
    vectors[5 * dimension + 3] = 100.0;
    write_table(
        &directory.path().join("model.safetensors"),
        vocabulary.len(),
        dimension,
        &vectors,
    );
    let engine = Engine::load(ModelId::PotionMulti, directory.path()).unwrap();
    (directory, engine)
}

fn write_table(path: &Path, rows: usize, dimension: usize, values: &[f32]) {
    let header = serde_json::to_vec(&json!({"embeddings": {
        "dtype": "F32", "shape": [rows, dimension], "data_offsets": [0, values.len() * 4]
    }}))
    .unwrap();
    let mut file = fs::File::create(path).unwrap();
    file.write_all(&(header.len() as u64).to_le_bytes())
        .unwrap();
    file.write_all(&header).unwrap();
    for value in values {
        file.write_all(&value.to_le_bytes()).unwrap();
    }
}

#[test]
fn preserves_frequency_and_filters_unknown() {
    let (_directory, mut engine) = fixture();
    let vectors = engine
        .encode(&["alpha alpha beta missing", "東京", "", "missing"])
        .unwrap();
    assert_eq!(vectors.len(), 4);
    assert!(
        vectors
            .iter()
            .all(|vector| vector.len() == engine.dimension())
    );
    let expected = 5.0f32.sqrt();
    assert!((vectors[0][0] - 2.0 / expected).abs() < 1e-6);
    assert!((vectors[0][1] - 1.0 / expected).abs() < 1e-6);
    assert!(vectors[0][2..].iter().all(|value| *value == 0.0));
    assert_eq!(vectors[1][2], 1.0);
    assert!(vectors[2..].iter().flatten().all(|value| *value == 0.0));
}

#[test]
fn loads_non_catalog_static_dimension() {
    let (directory, _) = fixture();
    let values = [
        10., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 100., 0., 0., 100., 0., 0.,
    ];
    write_table(&directory.path().join("model.safetensors"), 6, 3, &values);
    let mut engine =
        Engine::load_format(Format::Static { dimension: 3 }, directory.path()).unwrap();
    assert_eq!(engine.dimension(), 3);
    let vectors = engine.encode(&["alpha beta", "missing"]).unwrap();
    assert!((vectors[0][0] - 0.5f32.sqrt()).abs() < 1e-6);
    assert!((vectors[0][1] - 0.5f32.sqrt()).abs() < 1e-6);
    assert_eq!(vectors[1], [0., 0., 0.]);
}

#[test]
fn rejects_unsupported_embedding_formats() {
    let directory = tempfile::tempdir().unwrap();
    for format in [
        Format::SenseVoice {},
        Format::Static { dimension: 0 },
        Format::Transformer {
            dimension: 4097,
            pooling: Pooling::Mean,
            token_types: false,
        },
    ] {
        assert!(matches!(
            Engine::load_format(format, directory.path()),
            Err(Error::Invalid(_))
        ));
    }
}

#[test]
fn filters_declared_unigram_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let model = tokenizers::models::unigram::Unigram::from(
        vec![
            ("alpha".into(), -1.0),
            ("[UNK]".into(), -1.0),
            ("beta".into(), -1.0),
        ],
        Some(1),
        false,
    )
    .unwrap();
    let mut tokenizer = Tokenizer::new(model);
    tokenizer.with_pre_tokenizer(Some(WhitespaceSplit));
    tokenizer
        .save(directory.path().join("tokenizer.json"), false)
        .unwrap();
    fs::write(
        directory.path().join("config.json"),
        br#"{"normalize":true}"#,
    )
    .unwrap();
    let dimension = ModelId::PotionMulti.descriptor().dimension;
    let mut values = vec![0.0f32; 3 * dimension];
    values[0] = 1.0;
    values[dimension + 2] = 100.0;
    values[2 * dimension + 1] = 1.0;
    write_table(
        &directory.path().join("model.safetensors"),
        3,
        dimension,
        &values,
    );
    let mut engine = Engine::load(ModelId::PotionMulti, directory.path()).unwrap();
    let encoded = engine
        .tokenizer
        .encode("alpha missing beta", false)
        .unwrap();
    assert!(encoded.get_ids().contains(&1));
    let vectors = engine
        .encode(&["alpha missing beta", "missing", ""])
        .unwrap();
    let expected = 0.5f32.sqrt();
    assert!((vectors[0][0] - expected).abs() < 1e-6);
    assert!((vectors[0][1] - expected).abs() < 1e-6);
    assert!(vectors[0][2..].iter().all(|value| *value == 0.0));
    assert!(vectors[1..].iter().flatten().all(|value| *value == 0.0));
}

#[test]
fn keeps_offsets_without_truncation() {
    let (_directory, mut engine) = fixture();
    let text = "東京 alpha";
    let offsets = engine.offsets(text).unwrap();
    assert_eq!(offsets, vec![(0, 6), (7, 12)]);
    assert_eq!(&text[offsets[0].0..offsets[0].1], "東京");
    let oversized = vec!["alpha"; MAX_INPUT_TOKENS + 1].join(" ");
    assert_eq!(
        engine.offsets(&oversized).unwrap().len(),
        MAX_INPUT_TOKENS + 1
    );
    assert!(matches!(engine.encode(&[&oversized]), Err(Error::TooLong)));
    let at_limit = vec!["alpha"; MAX_INPUT_TOKENS].join(" ");
    assert_eq!(engine.encode(&[&at_limit]).unwrap()[0][0], 1.0);
}

#[test]
fn bounds_private_workers() {
    let pool = pool::create().unwrap();
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    assert_eq!(pool.current_num_threads(), (available / 2).clamp(1, 4));
    pool.install(|| {
        use rayon::prelude::*;
        let threads = (0..128)
            .into_par_iter()
            .map(|_| rayon::current_num_threads())
            .collect::<Vec<_>>();
        assert!(
            threads
                .iter()
                .all(|count| *count == pool.current_num_threads())
        );
    });
}

#[test]
fn rejects_invalid_table_layout() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("weights");
    fs::write(&path, (1024u64 * 1024 + 1).to_le_bytes()).unwrap();
    assert!(matches!(
        table::Table::load(&path, None),
        Err(Error::Invalid(_))
    ));
    write_table(&path, 2, 2, &[0.0, 1.0, 2.0]);
    assert!(matches!(
        table::Table::load(&path, None),
        Err(Error::Invalid(_))
    ));
    write_table(&path, 2, 2, &[0.0, 1.0, 2.0, 3.0]);
    let mut table = table::Table::load(&path, Some(0)).unwrap();
    assert!(matches!(table.mean(&[2]), Err(Error::Invalid(_))));
    assert!(matches!(
        table::Table::load(&path, Some(2)),
        Err(Error::Invalid(_))
    ));
    write_table(&path, 1, 1, &[f32::NAN]);
    assert!(matches!(
        table::Table::load(&path, None).unwrap().mean(&[0]),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn normalization_is_finite() {
    let mut vector = [3.0f32, 4.0];
    normalize(&mut vector).unwrap();
    assert!((vector[0] - 0.6).abs() < 1e-6);
    assert!((vector[1] - 0.8).abs() < 1e-6);
    normalize(&mut [0.0, 0.0]).unwrap();
    assert!(matches!(
        normalize(&mut [f32::INFINITY]),
        Err(Error::Invalid(_))
    ));
    let mut vector = [f32::MAX, f32::MAX];
    normalize(&mut vector).unwrap();
    assert!(vector.iter().all(|value| value.is_finite()));
}

#[test]
fn catalog_is_pinned() {
    for model in catalog::MODELS {
        assert_eq!(model.revision.len(), 40);
        assert!(model.revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(model.assets.len(), 3);
        for asset in model.assets {
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(asset.bytes > 0);
            let revision = asset
                .source
                .map_or(model.revision, |source| source.revision);
            assert_eq!(revision.len(), 40);
            assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(model.url(asset).contains(revision));
            assert!(!asset.name.contains('/'));
        }
    }
}

#[test]
fn keeps_converted_graph_source_separate() {
    let model = ModelId::BgeSmallZh.descriptor();
    let asset = model
        .assets
        .iter()
        .find(|asset| asset.name == "model.onnx")
        .unwrap();
    assert_eq!(
        model.url(asset),
        "https://huggingface.co/Xenova/bge-small-zh-v1.5/resolve/75c43b069aac4d136ba6bc1122f995fedcfd2781/onnx/model.onnx"
    );
    assert_eq!(
        model.url_at(asset, "https://mirror.example/"),
        "https://mirror.example/Xenova/bge-small-zh-v1.5/resolve/75c43b069aac4d136ba6bc1122f995fedcfd2781/onnx/model.onnx"
    );
    assert!(model.url(&model.assets[0]).contains(
        "BAAI/bge-small-zh-v1.5/resolve/7999e1d3359715c523056ef9478215996d62a620/config.json"
    ));
    let model = ModelId::GraniteMulti.descriptor();
    assert!(model.url(&model.assets[2]).ends_with("/onnx/model.onnx"));
}
