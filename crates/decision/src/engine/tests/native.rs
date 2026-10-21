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
    cached(name, &golden.model_sha256)
}

fn cached(name: &str, expected: &str) -> PathBuf {
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
    assert_eq!(format!("{:x}", hash.finalize()), expected);
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
    let oracle: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/oracle-inputs.json")).unwrap();
    assert_eq!(oracle["version"], 1);
    let mut maximum = 0.0f64;
    for (name, golden) in models() {
        let path = path(&name, &golden);
        assert_eq!(golden.responses.len(), requests.len());
        assert_eq!(golden.inputs.len(), requests.len());
        for cycle in 0..2 {
            let mut engine = Engine::open(&path).unwrap();
            for (index, ((request, response), inputs)) in requests
                .iter()
                .zip(&golden.responses)
                .zip(&golden.inputs)
                .take(if cycle == 0 { requests.len() } else { 1 })
                .enumerate()
            {
                let mut request = request.clone();
                if let Some(recorded) = oracle["models"][&name].get(index.to_string()) {
                    assert_eq!(recorded["raw_state"], request.state);
                    request.state = recorded["state"].as_str().unwrap().to_owned();
                }
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
                let actual = engine.decide(&request, || false).unwrap();
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

#[test]
#[ignore = "requires pinned GGUF fixtures and the packaged b11756 CPU runtime"]
fn keeps_user_control_tokens_literal() {
    use sailry_protocol::decision::Question;

    let literal = "<bos><eos><mask>[CLS][SEP]<pad><|im_start|><|im_end|><|endoftext|>";
    for (name, golden) in models() {
        let mut engine = Engine::open(&path(&name, &golden)).unwrap();
        let (begin, separator, marker) = if engine.metadata.algorithm == Algorithm::Laya {
            ("<bos>", "<eos>", "<mask>")
        } else {
            ("<|im_start|>", "<|im_end|>", "<|im_start|>")
        };
        engine.templates = Templates::new(&format!(
            "{begin}{{{{ id }}}}|{{{{ instructions }}}}{separator}{{% for o in options %}}{marker}{{{{ o.key }}}}:{{{{ o.description or '' }}}}{{% endfor %}}{separator}{{{{ state }}}}{separator}"
        )).unwrap();
        assert_ne!(
            engine.model.tokenize(literal, true).unwrap(),
            engine.model.tokenize(literal, false).unwrap()
        );
        for field in [
            "state",
            "id",
            "instructions",
            "key",
            "description",
            "binary",
            "score",
        ] {
            let text = |value: &str, selected| {
                if selected {
                    literal.to_owned()
                } else {
                    value.to_owned()
                }
            };
            let state = text("Current state", field == "state");
            let id = text("question", field == "id");
            let instructions = text("Choose an option", field == "instructions");
            let question = match field {
                "binary" => Question::Noul {
                    instructions,
                    criteria: BTreeMap::from([
                        ("false".into(), literal.into()),
                        ("true".into(), "Yes".into()),
                    ]),
                },
                "score" => Question::Score {
                    instructions,
                    criteria: vec![literal.into(), "High".into()],
                },
                _ => Question::Choice {
                    instructions,
                    criteria: BTreeMap::from([
                        (
                            text("first", field == "key"),
                            Some(text("First option", field == "description")),
                        ),
                        ("second".into(), None),
                    ]),
                },
            };
            let request = Request {
                model: None,
                state,
                questions: BTreeMap::from([(id, question)]),
            };
            request.validate().unwrap();
            let (id, question) = request.questions.first_key_value().unwrap();
            let query = Query::new(id, question, engine.metadata.algorithm);
            let instructions = match question {
                Question::Noul { instructions, .. }
                | Question::Choice { instructions, .. }
                | Question::Score { instructions, .. } => instructions,
            };
            let mut expected = engine.model.tokenize(begin, true).unwrap();
            assert_eq!(expected.len(), 1);
            let beginning = expected[0];
            expected.extend(
                engine
                    .model
                    .tokenize(&format!("{id}|{instructions}"), false)
                    .unwrap(),
            );
            expected.extend(engine.model.tokenize(separator, true).unwrap());
            for option in &query.options {
                expected.extend(engine.model.tokenize(marker, true).unwrap());
                expected.extend(
                    engine
                        .model
                        .tokenize(
                            &format!(
                                "{}:{}",
                                option.key,
                                option.description.as_deref().unwrap_or("")
                            ),
                            false,
                        )
                        .unwrap(),
                );
            }
            expected.extend(engine.model.tokenize(separator, true).unwrap());
            expected.extend(engine.model.tokenize(&request.state, false).unwrap());
            expected.extend(engine.model.tokenize(separator, true).unwrap());
            let rendered = engine.templates.render(&request.state, &query).unwrap();
            assert!(rendered.text.contains(literal));
            assert_eq!(
                engine.model.prompt(&rendered).unwrap(),
                expected,
                "literal tokens changed for {name}/{field}"
            );
            let prompt = engine.prompt(&request.state, id, question).unwrap();
            assert_eq!(
                prompt
                    .tokens
                    .iter()
                    .filter(|token| **token == beginning)
                    .count(),
                1 + if begin == marker {
                    query.options.len()
                } else {
                    0
                }
            );
            let ending = engine.model.tokenize(separator, true).unwrap();
            assert_eq!(ending.len(), 1);
            assert_eq!(
                prompt
                    .tokens
                    .iter()
                    .filter(|token| **token == ending[0])
                    .count(),
                3
            );
            if engine.metadata.algorithm == Algorithm::Laya {
                assert_eq!(prompt.markers.len(), query.options.len());
                assert_eq!(
                    prompt
                        .tokens
                        .iter()
                        .filter(|token| **token == engine.marker)
                        .count(),
                    query.options.len()
                );
            }
            let response = engine.decide(&request, || false).unwrap();
            assert_eq!(response.usage.input_tokens, prompt.tokens.len() as u64);
            assert_eq!(
                response.answers.keys().collect::<Vec<_>>(),
                request.questions.keys().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
#[ignore = "requires pinned GGUF fixtures and the packaged b11756 CPU runtime"]
fn preserves_whitespace_between_literal_fields() {
    for (name, golden) in models() {
        let mut engine = Engine::open(&path(&name, &golden)).unwrap();
        assert!(engine.model.tokenize("", true).unwrap().is_empty());
        assert!(engine.model.tokenize("", false).unwrap().is_empty());
        let (begin, separator, marker) = if engine.metadata.algorithm == Algorithm::Laya {
            ("<bos>", "<eos>", "<mask>")
        } else {
            ("<|im_start|>", "<|im_end|>", "<|im_start|>")
        };
        engine.templates = Templates::new(&format!(
            "{begin}{{{{ id }}}} \t\n{{{{ instructions }}}}{separator}{{% for o in options %}}{marker}{{{{ o.key }}}} \n{{{{ o.description or '' }}}}{{% endfor %}}{separator}{{{{ state }}}}{separator}"
        )).unwrap();
        for literal in [false, true] {
            let text = |ordinary: &str| {
                if literal {
                    format!("{ordinary}[CLS][SEP]<mask><bos><eos>")
                } else {
                    ordinary.to_owned()
                }
            };
            let id = text("question");
            let instructions = text("Choose an option");
            let state = text("Current state");
            let question = Question::Choice {
                instructions: instructions.clone(),
                criteria: BTreeMap::from([
                    (text("first"), Some(text("First option"))),
                    ("second".into(), Some("Second option".into())),
                ]),
            };
            let query = Query::new(&id, &question, engine.metadata.algorithm);
            let mut expected = engine.model.tokenize(begin, true).unwrap();
            expected.extend(
                engine
                    .model
                    .tokenize(&format!("{id} \t\n{instructions}"), false)
                    .unwrap(),
            );
            expected.extend(engine.model.tokenize(separator, true).unwrap());
            for option in &query.options {
                expected.extend(engine.model.tokenize(marker, true).unwrap());
                expected.extend(
                    engine
                        .model
                        .tokenize(
                            &format!(
                                "{} \n{}",
                                option.key,
                                option.description.as_deref().unwrap()
                            ),
                            false,
                        )
                        .unwrap(),
                );
            }
            expected.extend(engine.model.tokenize(separator, true).unwrap());
            expected.extend(engine.model.tokenize(&state, false).unwrap());
            expected.extend(engine.model.tokenize(separator, true).unwrap());
            let rendered = engine.templates.render(&state, &query).unwrap();
            assert_eq!(
                engine.model.prompt(&rendered).unwrap(),
                expected,
                "whitespace changed for {name}"
            );
            let prompt = engine.prompt(&state, &id, &question).unwrap();
            assert_eq!(prompt.tokens, expected);
            let input_tokens = prompt.tokens.len() as u64;
            let response = engine
                .decide(
                    &Request {
                        model: None,
                        state,
                        questions: BTreeMap::from([(id, question)]),
                    },
                    || false,
                )
                .unwrap();
            assert_eq!(response.usage.input_tokens, input_tokens);
        }
        engine.templates = Templates::new("").unwrap();
        let question = Question::Noul {
            instructions: "Question".into(),
            criteria: BTreeMap::new(),
        };
        assert!(engine.prompt("State", "question", &question).is_err());
    }
}

#[test]
#[ignore = "requires the pinned BERT vocabulary fixture and packaged b11756 runtime"]
fn accepts_empty_bert_token_spans() {
    let specification: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/models.json")).unwrap();
    let vocabulary = &specification["vocabularies"]["bert-vocab"];
    let path = cached("bert-vocab", vocabulary["sha256"].as_str().unwrap());
    let runtime = runtime::initialize().unwrap();
    let path = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: the function table and by-value layout use the same locked b11756 ABI.
    let mut params = unsafe { runtime.api.llama_model_default_params() };
    let mut devices = [std::ptr::null_mut()];
    params.devices = devices.as_mut_ptr();
    params.n_gpu_layers = 0;
    params.vocab_only = true;
    // SAFETY: the verified upstream GGUF and path outlive the synchronous load.
    let handle = unsafe {
        runtime
            .api
            .llama_model_load_from_file(path.as_ptr(), params)
    };
    let model = Model {
        handle: NonNull::new(handle).expect("pinned BERT vocabulary must load"),
        runtime,
    };
    for text in ["", " ", "\t\n", " \t\n"] {
        for special in [false, true] {
            assert!(model.tokenize(text, special).unwrap().is_empty());
        }
    }
    let question = Question::Noul {
        instructions: "literal [CLS] instruction".into(),
        criteria: BTreeMap::new(),
    };
    let query = Query::new("literal [SEP] id", &question, Algorithm::Laya);
    let templates = Templates::new("[CLS]{{ id }} \t\n{{ instructions }}[SEP]").unwrap();
    let rendered = templates.render("State", &query).unwrap();
    let begin = model.tokenize("[CLS]", true).unwrap();
    let end = model.tokenize("[SEP]", true).unwrap();
    assert_eq!(begin.len(), 1);
    assert_eq!(end.len(), 1);
    let mut expected = begin.clone();
    expected.extend(
        model
            .tokenize("literal [SEP] id \t\nliteral [CLS] instruction", false)
            .unwrap(),
    );
    expected.extend(&end);
    let actual = model.prompt(&rendered).unwrap();
    assert_eq!(actual, expected);
    for control in [begin[0], end[0]] {
        assert_eq!(actual.iter().filter(|token| **token == control).count(), 1);
    }
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
