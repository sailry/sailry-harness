//! Opt-in measurements on self-authored synthetic Chinese UI text, not a default-model qualification.
use sailry_decision::Engine;
use sailry_protocol::{
    decision::{Answer, Question, Request},
    models::Definition,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::File,
    io::Read,
    path::Path,
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    request: Request,
    expected: Expected,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Expected {
    Noul { value: bool },
    Choice { value: String },
    Score { value: usize },
}

#[derive(Default)]
struct Measures {
    correct: usize,
    brier: f64,
    latency: Vec<f64>,
}

#[derive(Serialize)]
struct Summary {
    samples: usize,
    accuracy: f64,
    brier: f64,
    p50_ms: f64,
    p95_ms: f64,
}

impl Measures {
    fn summarize(mut self) -> Summary {
        self.latency.sort_by(f64::total_cmp);
        let samples = self.latency.len();
        Summary {
            samples,
            accuracy: self.correct as f64 / samples as f64,
            brier: self.brier / samples as f64,
            p50_ms: percentile(&self.latency, 0.5),
            p95_ms: percentile(&self.latency, 0.95),
        }
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    sorted[((sorted.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1)]
}

fn main() -> Result<()> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 && arguments.len() != 3 {
        return Err("usage: qualify <definition.json> <model-directory> [cases.jsonl]".into());
    }
    let definition: Definition = serde_json::from_reader(File::open(&arguments[0])?)?;
    definition.validate()?;
    let directory = Path::new(&arguments[1]);
    verify(&definition, directory)?;
    let input = match arguments.get(2) {
        Some(path) => std::fs::read_to_string(path)?,
        None => include_str!("../tests/fixtures/qualification.jsonl").to_owned(),
    };
    let cases = cases(&input)?;
    let metadata = sailry_decision::validate(&directory.join("model.gguf"))?;
    let started = Instant::now();
    let mut engine = Engine::load(&definition, directory, false)?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.;
    // Warm the actual path for each question type before recording steady-state latency.
    let mut warmed = BTreeSet::new();
    for case in &cases {
        if warmed.insert(case.kind()) {
            engine.decide(&case.request, || false)?;
        }
    }
    let mut metrics = BTreeMap::<&str, Measures>::new();
    let mut failures = Vec::new();
    for case in &cases {
        let started = Instant::now();
        let response = engine.decide(&case.request, || false)?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        let answer = response.answers.get("check").ok_or("answer is missing")?;
        let (correct, brier) = evaluate(case, answer)?;
        let measurement = metrics.entry(case.kind()).or_default();
        measurement.correct += usize::from(correct);
        measurement.brier += brier;
        measurement.latency.push(elapsed);
        if !correct {
            failures.push(case.id.as_str());
        }
    }
    let metrics = metrics
        .into_iter()
        .map(|(kind, measurement)| (kind, measurement.summarize()))
        .collect::<BTreeMap<_, _>>();
    let report = serde_json::json!({
        "dataset":"self-authored synthetic Chinese accessibility and OCR-style text",
        "labels":"author-assigned; not independently human-reviewed",
        "default_qualified":false,
        "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "logical_cpus":std::thread::available_parallelism()?.get(),
        "inference_threads":sailry_model_assets::session::threads(),
        "model":definition,"algorithm":format!("{:?}",metadata.algorithm),
        "load_ms":load_ms,"peak_rss_bytes":peak_rss()?,
        "rss_scope":"whole process high-water mark, including verification and model loading",
        "accuracy":"threshold 0.5 for noul; argmax label for choice and score",
        "brier":"binary squared error for noul; summed multiclass squared error for choice and score",
        "metrics":metrics,"incorrect_cases":failures
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn verify(definition: &Definition, directory: &Path) -> Result<()> {
    for asset in &definition.assets {
        let path = directory.join(&asset.name);
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.is_symlink() || metadata.len() != asset.bytes {
            return Err("model asset is not an ordinary file of the pinned size".into());
        }
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut bytes = [0u8; 65536];
        loop {
            let count = file.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            hasher.update(&bytes[..count]);
        }
        if format!("{:x}", hasher.finalize()) != asset.sha256 {
            return Err("model asset checksum does not match its definition".into());
        }
    }
    Ok(())
}

impl Case {
    fn kind(&self) -> &'static str {
        match self.expected {
            Expected::Noul { .. } => "noul",
            Expected::Choice { .. } => "choice",
            Expected::Score { .. } => "score",
        }
    }
}

fn cases(input: &str) -> Result<Vec<Case>> {
    let cases = input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<std::result::Result<Vec<Case>, _>>()?;
    let mut ids = BTreeSet::new();
    let mut counts = BTreeMap::new();
    for case in &cases {
        case.request.validate()?;
        if case.id.is_empty() || !ids.insert(&case.id) || case.request.questions.len() != 1 {
            return Err("case identity or question count is invalid".into());
        }
        let question = case
            .request
            .questions
            .get("check")
            .ok_or("case question must be named check")?;
        let valid = match (&case.expected, question) {
            (Expected::Noul { .. }, Question::Noul { .. }) => true,
            (Expected::Choice { value }, Question::Choice { criteria, .. }) => {
                criteria.contains_key(value)
            }
            (Expected::Score { value }, Question::Score { criteria, .. }) => {
                *value < criteria.len()
            }
            _ => false,
        };
        if !valid {
            return Err("case label does not match its question".into());
        }
        *counts.entry(case.kind()).or_insert(0) += 1;
    }
    if ["noul", "choice", "score"]
        .iter()
        .any(|kind| counts.get(kind).copied().unwrap_or(0) < 50)
    {
        return Err("at least fifty cases per question type are required".into());
    }
    Ok(cases)
}

fn evaluate(case: &Case, answer: &Answer) -> Result<(bool, f64)> {
    match (&case.expected, answer, case.request.questions.get("check")) {
        (Expected::Noul { value }, Answer::Noul { noul }, Some(Question::Noul { .. })) => {
            let probability = f64::from(*noul);
            probability_value(probability)?;
            Ok((
                (probability >= 0.5) == *value,
                (probability - f64::from(u8::from(*value))).powi(2),
            ))
        }
        (
            Expected::Choice { value },
            Answer::Choice {
                choice,
                probabilities,
                ..
            },
            Some(Question::Choice { criteria, .. }),
        ) => {
            if probabilities.len() != criteria.len()
                || !criteria.keys().all(|key| probabilities.contains_key(key))
            {
                return Err("choice labels differ from the question".into());
            }
            let values = criteria
                .keys()
                .map(|key| f64::from(probabilities[key]))
                .collect::<Vec<_>>();
            let expected = criteria
                .keys()
                .position(|key| key == value)
                .ok_or("choice label is missing")?;
            Ok((choice == value, multiclass(&values, expected)?))
        }
        (
            Expected::Score { value },
            Answer::Score {
                probabilities,
                legend,
                ..
            },
            Some(Question::Score { criteria, .. }),
        ) => {
            if legend != criteria || probabilities.len() != criteria.len() {
                return Err("score labels differ from the question".into());
            }
            let values = probabilities
                .iter()
                .map(|value| f64::from(*value))
                .collect::<Vec<_>>();
            let predicted = values
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1).then_with(|| right.0.cmp(&left.0)))
                .map(|(index, _)| index)
                .ok_or("score probabilities are empty")?;
            Ok((predicted == *value, multiclass(&values, *value)?))
        }
        _ => Err("answer type differs from the question".into()),
    }
}

fn probability_value(value: f64) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err("answer probability is invalid".into());
    }
    Ok(())
}

fn multiclass(values: &[f64], expected: usize) -> Result<f64> {
    if expected >= values.len() {
        return Err("expected label is outside the probability vector".into());
    }
    for value in values {
        probability_value(*value)?;
    }
    if (values.iter().sum::<f64>() - 1.).abs() > 0.001 {
        return Err("answer probabilities do not sum to one".into());
    }
    Ok(values
        .iter()
        .enumerate()
        .map(|(index, value)| (value - f64::from(u8::from(index == expected))).powi(2))
        .sum())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn peak_rss() -> Result<Option<u64>> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided native struct on a zero return value.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: the successful call above initialized every field of rusage.
    let usage = unsafe { usage.assume_init() };
    let bytes = u64::try_from(usage.ru_maxrss)?;
    // XNU returns bytes; Linux returns KiB. Other platforms are not reported as measured.
    Ok(Some(if cfg!(target_os = "linux") {
        bytes.checked_mul(1024).ok_or("resident memory overflow")?
    } else {
        bytes
    }))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn peak_rss() -> Result<Option<u64>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_each_question_type() {
        let corpus = cases(include_str!("../tests/fixtures/qualification.jsonl")).unwrap();
        assert_eq!(corpus.len(), 150);
        for kind in ["noul", "choice", "score"] {
            assert_eq!(corpus.iter().filter(|case| case.kind() == kind).count(), 50);
        }
    }

    #[test]
    fn computes_calibration_and_nearest_rank() {
        assert!((multiclass(&[0.25, 0.75], 1).unwrap() - 0.125).abs() < 1e-8);
        assert_eq!(percentile(&[1., 2., 3., 4.], 0.5), 2.);
        assert_eq!(percentile(&[1., 2., 3., 4.], 0.95), 4.);
        assert!(multiclass(&[0.2, 0.2], 0).is_err());
        assert!(multiclass(&[f64::NAN, 1.], 0).is_err());
    }
}
