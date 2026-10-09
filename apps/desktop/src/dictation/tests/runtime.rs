//! Actual speech and embedding inference share one native runtime image.
#![cfg(target_os = "macos")]

use sailry_embedding::{Engine, ModelId};
use sailry_speech::{Language, model::Recognizer};
use serde::Deserialize;
use std::{ffi::CStr, path::PathBuf, process::Command};

#[derive(Deserialize)]
struct Reference {
    model_revision: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    text: String,
    vector: Vec<f32>,
}

fn speech_root() -> PathBuf {
    std::env::var_os("SAILRY_DICTATION_FIXTURES")
        .map(PathBuf::from)
        .expect("set SAILRY_DICTATION_FIXTURES to verified speech fixtures")
}

fn embedding() -> Engine {
    let directory = std::env::var_os("SAILRY_EMBEDDING_REFERENCES")
        .map(PathBuf::from)
        .expect("set SAILRY_EMBEDDING_REFERENCES to verified reference models")
        .join("bge-small-zh");
    let mut engine = Engine::load(ModelId::BgeSmallZh, &directory).unwrap();
    check_embedding(&mut engine);
    engine
}

fn check_embedding(engine: &mut Engine) {
    let reference: Reference = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/embedding/tests/references/bge-small-zh.json"
    )))
    .unwrap();
    assert_eq!(
        reference.model_revision,
        ModelId::BgeSmallZh.descriptor().revision
    );
    let case = &reference.cases[0];
    let output = engine.encode(&[&case.text]).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].len(), case.vector.len());
    let difference = output[0]
        .iter()
        .zip(&case.vector)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0f32, f32::max);
    assert!(
        output[0].iter().all(|value| value.is_finite()) && difference < 5e-4,
        "embedding reference differs by {difference}"
    );
}

fn speech() -> Recognizer {
    let root = speech_root();
    assert!(sailry_speech::model::ready(&root.join("model")));
    let recognizer = Recognizer::new(&root.join("model"), Language::Auto).unwrap();
    check_speech(&recognizer);
    recognizer
}

fn check_speech(recognizer: &Recognizer) {
    let root = speech_root();
    let cases: Vec<(String, Vec<String>)> =
        serde_json::from_slice(&std::fs::read(root.join("expected.json")).unwrap()).unwrap();
    for (name, expected) in cases {
        assert_eq!(expected.len(), 1, "use the complete expected transcript");
        let path = root.join(name).with_extension("f32le");
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes.len() % 4, 0);
        let samples = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect::<Vec<_>>();
        assert!(!samples.is_empty());
        for gain in [1., 0.001] {
            let audio = samples.iter().map(|value| value * gain).collect::<Vec<_>>();
            let actual = recognizer.recognize(&audio).unwrap();
            assert_eq!(actual.to_lowercase(), expected[0].to_lowercase());
        }
    }
}

fn api(library: &libloading::Library) -> &ort::sys::OrtApi {
    // SAFETY: the packaged pinned runtime exports the API base with this exact C ABI.
    let get = unsafe {
        library
            .get::<unsafe extern "system" fn() -> *const ort::sys::OrtApiBase>(b"OrtGetApiBase\0")
            .unwrap()
    };
    // SAFETY: the symbol's base and API remain valid while the library is borrowed.
    unsafe {
        let base = get();
        assert!(!base.is_null());
        assert_eq!(
            CStr::from_ptr(((*base).GetVersionString)())
                .to_str()
                .unwrap(),
            sailry_model_assets::runtime::VERSION
        );
        let api = ((*base).GetApi)(ort::sys::ORT_API_VERSION);
        assert!(!api.is_null());
        &*api
    }
}

fn image(api: &ort::sys::OrtApi) -> PathBuf {
    // SAFETY: dladdr initializes this structure from a live native function address.
    unsafe {
        let mut info = std::mem::zeroed::<libc::Dl_info>();
        let address = api.CreateEnv as *const () as *const libc::c_void;
        assert_ne!(libc::dladdr(address, &mut info), 0);
        assert!(!info.dli_fname.is_null());
        PathBuf::from(CStr::from_ptr(info.dli_fname).to_str().unwrap())
            .canonicalize()
            .unwrap()
    }
}

fn check_image() {
    let path = sailry_model_assets::runtime::path().unwrap();
    let speech = path.parent().unwrap().join("libsherpa-onnx-c-api.dylib");
    // SAFETY: both paths identify fixed packaged libraries; handles outlive their API views.
    let (runtime, speech) = unsafe {
        (
            libloading::Library::new(&path).unwrap(),
            libloading::Library::new(speech).unwrap(),
        )
    };
    let runtime_api = api(&runtime);
    let speech_api = api(&speech);
    assert!(std::ptr::eq(runtime_api, speech_api));
    assert!(std::ptr::fn_addr_eq(
        runtime_api.CreateEnv,
        ort::api().CreateEnv
    ));
    assert_eq!(image(runtime_api), path);
    assert_eq!(image(speech_api), path);
    assert_eq!(image(ort::api()), path);
    eprintln!(
        "Speech and embedding use one ORT native image: {}",
        path.display()
    );
}

#[test]
#[ignore = "requires verified speech and embedding assets plus packaged shared native libraries"]
fn same_image() {
    let (_, module) = module_path!().split_once("::").unwrap();
    for order in ["embedding-first", "speech-first"] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                &format!("{module}::child"),
                "--nocapture",
            ])
            .env("SAILRY_SHARED_ORT_ORDER", order)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{order} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("{order}: {}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
#[ignore = "only invoked by same_image in a fresh subprocess"]
fn child() {
    let (mut engine, recognizer) = match std::env::var("SAILRY_SHARED_ORT_ORDER").as_deref() {
        Ok("embedding-first") => {
            let engine = embedding();
            let recognizer = speech();
            (engine, recognizer)
        }
        Ok("speech-first") => {
            let recognizer = speech();
            let engine = embedding();
            (engine, recognizer)
        }
        _ => panic!("run the parent same_image test"),
    };
    check_image();
    check_embedding(&mut engine);
    check_speech(&recognizer);
}
