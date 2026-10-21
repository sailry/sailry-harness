use super::{native, *};

fn session() -> ort::session::Session {
    sailry_model_assets::runtime::initialize().unwrap();
    sailry_model_assets::session::builder()
        .unwrap()
        .commit_from_memory(include_bytes!("../../../tests/fixtures/coexist.onnx"))
        .unwrap()
}

fn scale(session: &mut ort::session::Session) {
    let input = ort::value::Tensor::from_array(([1usize, 2], vec![4.0f32, 5.0])).unwrap();
    let output = session.run(ort::inputs!["input" => input]).unwrap();
    let (shape, values) = output["output"].try_extract_tensor::<f32>().unwrap();
    assert_eq!(&shape[..], [1, 2]);
    assert_eq!(values, [8.0, 15.0]);
}

fn run(onnx_first: bool) {
    let mut early = onnx_first.then(session);
    if let Some(session) = early.as_mut() {
        scale(session);
    }
    let models = native::models();
    let path = native::path("julia", &models["julia"]);
    let mut engine = Engine::open(&path, false).unwrap();
    let mut onnx = early.unwrap_or_else(session);
    let request = &native::requests()[0];
    scale(&mut onnx);
    assert_eq!(engine.decide(request, || false).unwrap().answers.len(), 3);
    scale(&mut onnx);
    drop(engine);
    scale(&mut onnx);
    let mut engine = Engine::open(&path, false).unwrap();
    drop(onnx);
    assert_eq!(engine.decide(request, || false).unwrap().answers.len(), 3);
}

#[test]
#[ignore = "requires pinned Julia, packaged ORT 1.28.2 and b11756 CPU runtimes"]
fn onnx_first() {
    run(true);
}

#[test]
#[ignore = "requires pinned Julia, packaged ORT 1.28.2 and b11756 CPU runtimes"]
fn gguf_first() {
    run(false);
}
