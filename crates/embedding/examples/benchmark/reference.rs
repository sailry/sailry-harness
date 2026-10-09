use sailry_embedding::{Engine, ModelId};
use serde::Deserialize;
use serde_json::{Value, json};
use std::error::Error;

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

pub(super) fn compare(model: ModelId, engine: &mut Engine) -> Result<Value, Box<dyn Error>> {
    let reference: Reference = serde_json::from_str(match model {
        ModelId::GraniteMulti => include_str!("../../tests/references/granite-multi.json"),
        ModelId::BgeSmallZh => include_str!("../../tests/references/bge-small-zh.json"),
        ModelId::PotionMulti => include_str!("../../tests/references/potion-multi.json"),
    })?;
    if reference.model_revision != model.descriptor().revision || reference.cases.len() < 6 {
        return Err("reference vectors do not match the model revision or required cases".into());
    }
    let inputs = reference
        .cases
        .iter()
        .map(|case| case.text.as_str())
        .collect::<Vec<_>>();
    let actual = engine.encode(&inputs)?;
    if actual.len() != reference.cases.len() {
        return Err("reference inference returned the wrong vector count".into());
    }
    let cases = actual
        .iter()
        .zip(&reference.cases)
        .enumerate()
        .map(|(index, (actual, case))| comparison(engine, index, actual, case))
        .collect::<Result<Vec<_>, _>>()?;
    let passed = cases
        .iter()
        .all(|case| case["passed"].as_bool() == Some(true));
    Ok(
        json!({"implementation": reference.implementation, "cases": cases, "minimum_cosine": 0.99, "maximum_difference_limit": 5e-4, "passed": passed}),
    )
}

fn comparison(
    engine: &Engine,
    index: usize,
    actual: &[f32],
    case: &Case,
) -> Result<Value, Box<dyn Error>> {
    if actual.len() != case.vector.len() || actual.len() != engine.dimension() {
        return Err("reference vector dimension differs from the model".into());
    }
    let difference = actual
        .iter()
        .zip(&case.vector)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let dot = actual
        .iter()
        .zip(&case.vector)
        .map(|(&a, &b)| f64::from(a) * f64::from(b))
        .sum::<f64>();
    let norm = |values: &[f32]| {
        values
            .iter()
            .map(|&value| f64::from(value).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let denominator = norm(actual) * norm(&case.vector);
    let cosine = if denominator == 0. {
        if actual == case.vector.as_slice() {
            1.
        } else {
            0.
        }
    } else {
        dot / denominator
    };
    let passed = actual
        .iter()
        .chain(&case.vector)
        .all(|value| value.is_finite())
        && cosine >= 0.99
        && difference < 5e-4;
    Ok(json!({
        "case": index,
        "input_tokens": engine.offsets(&case.text)?.len(),
        "cosine": cosine,
        "maximum_difference": difference,
        "passed": passed,
    }))
}
