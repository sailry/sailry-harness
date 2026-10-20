use super::*;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::PathBuf, sync::atomic::AtomicUsize};

#[derive(Deserialize)]
pub(super) struct Golden {
    model_sha256: String,
    responses: Vec<Value>,
    inputs: Vec<BTreeMap<String, Input>>,
}

#[derive(Deserialize)]
struct Input {
    prompt: String,
    tokens: Vec<i32>,
    markers: Vec<usize>,
}

pub(super) fn models() -> BTreeMap<String, Golden> {
    let mut value: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/golden.json")).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["llama_build"], "b11756");
    assert_eq!(value["policy"]["questions"], "independent_forward");
    assert_eq!(value["policy"]["slot_erase"], true);
    serde_json::from_value(value["models"].take()).unwrap()
}

pub(super) fn requests() -> Vec<Request> {
    serde_json::from_str(include_str!("../../../tests/fixtures/requests.json")).unwrap()
}

pub(super) fn path(name: &str, golden: &Golden) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.runtime/local-model-host/models")
        .join(name)
        .join("model.gguf");
    let mut file =
        File::open(&path).expect("run tests/golden.py to prepare the pinned model fixtures");
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    assert_eq!(format!("{:x}", hash.finalize()), golden.model_sha256);
    path
}

fn expected(mut value: Value) -> Response {
    for answer in value["answers"].as_object_mut().unwrap().values_mut() {
        if answer["type"] == "score" {
            for field in ["legend", "probabilities"] {
                let object = answer[field].as_object().unwrap();
                answer[field] = Value::Array(
                    (0..object.len())
                        .map(|index| object[&index.to_string()].clone())
                        .collect(),
                );
            }
        }
    }
    serde_json::from_value(value).unwrap()
}

fn compare(actual: &Value, expected: &Value) -> f64 {
    match (actual, expected) {
        (Value::Number(left), Value::Number(right)) => {
            let difference = (left.as_f64().unwrap() - right.as_f64().unwrap()).abs();
            assert!(
                difference <= 1e-4,
                "native output differs from the official server: {left} / {right}"
            );
            difference
        }
        (Value::Object(left), Value::Object(right)) => {
            assert_eq!(
                left.keys().collect::<Vec<_>>(),
                right.keys().collect::<Vec<_>>()
            );
            left.iter()
                .map(|(key, value)| compare(value, &right[key]))
                .fold(0.0, f64::max)
        }
        (Value::Array(left), Value::Array(right)) => {
            assert_eq!(left.len(), right.len());
            left.iter()
                .zip(right)
                .map(|(left, right)| compare(left, right))
                .fold(0.0, f64::max)
        }
        _ => {
            assert_eq!(actual, expected);
            0.0
        }
    }
}

#[test]
#[ignore = "requires pinned GGUF fixtures and the packaged b11756 CPU runtime"]
fn matches_official_server() {
    let requests = requests();
    let mut maximum = 0.0f64;
    for (name, golden) in models() {
        let path = path(&name, &golden);
        assert_eq!(golden.responses.len(), requests.len());
        assert_eq!(golden.inputs.len(), requests.len());
        for cycle in 0..2 {
            let mut engine = Engine::open(&path).unwrap();
            for ((request, response), inputs) in requests
                .iter()
                .zip(&golden.responses)
                .zip(&golden.inputs)
                .take(if cycle == 0 { requests.len() } else { 1 })
            {
                for (name, question) in &request.questions {
                    let input = &inputs[name];
                    let prompt = engine.prompt(&request.state, name, question).unwrap();
                    assert_eq!(
                        engine.model.tokenize(&input.prompt, true).unwrap(),
                        input.tokens
                    );
                    assert_eq!(
                        prompt.tokens, input.tokens,
                        "prompt tokens differ for {name}"
                    );
                    assert_eq!(
                        prompt.markers, input.markers,
                        "head positions differ for {name}"
                    );
                }
                let actual = engine.decide(request, || false).unwrap();
                let expected = expected(response.clone());
                assert_eq!(actual.usage, expected.usage);
                maximum = maximum.max(compare(
                    &serde_json::to_value(actual.answers).unwrap(),
                    &serde_json::to_value(expected.answers).unwrap(),
                ));
            }
            drop(engine);
        }
    }
    eprintln!("Maximum probability/score difference from b11756 server: {maximum}");
}

fn prompt<'a>(engine: &Engine, request: &'a Request) -> Prompt<'a> {
    let (name, question) = request.questions.first_key_value().unwrap();
    engine.prompt(&request.state, name, question).unwrap()
}

fn interrupted<F: Fn() -> bool + Sync>(
    engine: &Engine,
    prompt: &Prompt<'_>,
    cancelled: F,
    panics: bool,
) {
    let context = engine
        .model
        .context(prompt.tokens.len(), engine.metadata.algorithm)
        .unwrap();
    let mut guard = Abort {
        context: &context,
        check: Box::new(Check {
            cancelled,
            panicked: AtomicBool::new(false),
            aborted: AtomicBool::new(false),
        }),
    };
    let data = std::ptr::from_mut(guard.check.as_mut()).cast();
    // SAFETY: this test guard owns the callback state through the production forward.
    unsafe {
        context
            .api
            .llama_set_abort_callback(context.handle.as_ptr(), Some(abort::<F>), data);
    }
    let result = engine
        .model
        .forward(&context, prompt, &engine.labels, &guard);
    if panics {
        assert!(guard.check.panicked.load(Ordering::Relaxed));
        assert!(matches!(result, Err(Error::Native(_))));
    } else {
        assert!(
            guard.check.aborted.load(Ordering::Relaxed),
            "native CPU execution did not call the abort hook"
        );
        assert!(matches!(result, Err(Error::Cancelled)));
    }
}

#[test]
#[ignore = "requires pinned GGUF fixtures and the packaged b11756 CPU runtime"]
fn cancels_active_forward_and_reuses_weights() {
    let request = &requests()[0];
    for (name, golden) in models() {
        let mut engine = Engine::open(&path(&name, &golden)).unwrap();
        assert!(matches!(
            engine.decide(request, || true),
            Err(Error::Cancelled)
        ));
        let prompt = prompt(&engine, request);
        let calls = AtomicUsize::new(0);
        interrupted(
            &engine,
            &prompt,
            || calls.fetch_add(1, Ordering::Relaxed) > 1,
            false,
        );
        assert!(engine.decide(request, || false).is_ok());
    }
}

#[test]
#[ignore = "requires pinned GGUF fixtures and the packaged b11756 CPU runtime"]
fn catches_native_callback_panics() {
    let models = models();
    let request = &requests()[0];
    let mut engine = Engine::open(&path("julia", &models["julia"])).unwrap();
    let prompt = prompt(&engine, request);
    let calls = AtomicUsize::new(0);
    interrupted(
        &engine,
        &prompt,
        || {
            assert_eq!(
                calls.fetch_add(1, Ordering::Relaxed),
                0,
                "fixture callback panic"
            );
            false
        },
        true,
    );
    assert!(engine.decide(request, || false).is_ok());
}
