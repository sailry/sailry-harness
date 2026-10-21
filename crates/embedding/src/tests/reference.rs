use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Reference {
    model_revision: String,
    implementation: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    text: String,
    vector: Vec<f32>,
}

fn compare(id: ModelId) {
    let root = std::env::var_os("SAILRY_EMBEDDING_REFERENCES")
        .expect("set SAILRY_EMBEDDING_REFERENCES to verified models and reference vectors");
    let directory = Path::new(&root).join(id.key());
    let reference: Reference = serde_json::from_str(match id {
        ModelId::GraniteMulti => include_str!("../../tests/references/granite-multi.json"),
        ModelId::BgeSmallZh => include_str!("../../tests/references/bge-small-zh.json"),
        ModelId::PotionMulti => include_str!("../../tests/references/potion-multi.json"),
    })
    .unwrap();
    assert_eq!(reference.model_revision, id.descriptor().revision);
    assert_eq!(
        reference.implementation,
        match id {
            ModelId::PotionMulti =>
                "model2vec@52cb9ffacd3df382cca0a67e3b8746fadec5cecc;numpy=2.4.4",
            _ => "transformers=4.56.2;torch=2.8.0",
        }
    );
    assert!(
        reference.cases.len() >= 3,
        "include multilingual, repeated-token and empty input cases"
    );
    assert!(reference.cases.iter().any(|case| case.text.is_empty()));
    assert!(
        reference
            .cases
            .iter()
            .any(|case| case.text == "alpha alpha beta")
    );
    assert!(reference.cases.iter().any(|case| !case.text.is_ascii()));
    let assets = id
        .descriptor()
        .assets
        .iter()
        .map(|asset| sailry_model_assets::File {
            name: asset.name,
            size: asset.bytes,
            sha256: asset.sha256,
        })
        .collect::<Vec<_>>();
    assert!(
        sailry_model_assets::ready(&directory, &assets),
        "reference model files do not match the pinned catalog"
    );
    let mut engine = Engine::load(id, &directory).unwrap();
    let texts = reference
        .cases
        .iter()
        .map(|case| case.text.as_str())
        .collect::<Vec<_>>();
    let vectors = engine.encode(&texts).unwrap();
    assert_eq!(vectors.len(), reference.cases.len());
    assert!(engine.encode(&[]).unwrap().is_empty());
    for (actual, case) in vectors.iter().zip(&reference.cases) {
        assert_eq!(actual.len(), engine.dimension());
        assert_eq!(case.vector.len(), actual.len());
        assert!(case.vector.iter().all(|value| value.is_finite()));
        let difference = actual
            .iter()
            .zip(&case.vector)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0f32, f32::max);
        assert!(
            difference < 5e-4,
            "reference differs by {difference} for {:?}",
            case.text
        );
        let dot = actual
            .iter()
            .zip(&case.vector)
            .map(|(&a, &b)| f64::from(a) * f64::from(b))
            .sum::<f64>();
        let actual_norm = actual
            .iter()
            .map(|&value| f64::from(value).powi(2))
            .sum::<f64>()
            .sqrt();
        let expected_norm = case
            .vector
            .iter()
            .map(|&value| f64::from(value).powi(2))
            .sum::<f64>()
            .sqrt();
        if actual_norm == 0.0 || expected_norm == 0.0 {
            assert_eq!(actual, &case.vector);
        } else {
            assert!(dot / (actual_norm * expected_norm) >= 0.99);
        }
    }
}

#[test]
#[ignore = "requires verified local model files and pinned Python reference vectors"]
fn granite_matches_reference() {
    compare(ModelId::GraniteMulti);
}

#[test]
#[ignore = "requires verified local model files and pinned Python reference vectors"]
fn bge_matches_reference() {
    compare(ModelId::BgeSmallZh);
}

#[test]
#[ignore = "requires verified local model files and pinned Python reference vectors"]
fn potion_matches_reference() {
    compare(ModelId::PotionMulti);
}
