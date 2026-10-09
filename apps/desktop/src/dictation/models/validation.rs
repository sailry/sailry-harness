use super::Result;
use ort::{
    session::Session,
    value::{Tensor, TensorElementType, ValueType},
};
use std::{collections::BTreeSet, path::Path};

pub(super) fn check(graph: &Path, tokens: &Path) -> Result<()> {
    sailry_model_assets::onnx::validate(graph).map_err(|_| "dictation_model_invalid")?;
    sailry_model_assets::runtime::initialize().map_err(|_| "dictation_model_invalid")?;
    validate(graph, tokens).map_err(|_| "dictation_model_invalid")?;
    // Sherpa's metadata reader terminates the process on missing fields. Validate them first.
    super::model::Recognizer::from_paths(graph, tokens, sailry_speech::Language::Auto)
        .map_err(super::error_key)?;
    Ok(())
}

fn validate(graph: &Path, tokens: &Path) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::builder()?
        .with_independent_thread_pool()
        .map_err(ort::Error::<()>::from)?
        .with_no_environment_execution_providers()
        .map_err(ort::Error::<()>::from)?
        .with_execution_providers([ort::ep::CPU::default().build()])
        .map_err(ort::Error::<()>::from)?
        .with_parallel_execution(false)
        .map_err(ort::Error::<()>::from)?
        .with_intra_threads(std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4)))
        .map_err(ort::Error::<()>::from)?
        .with_inter_threads(1)
        .map_err(ort::Error::<()>::from)?
        .with_config_entry("session.intra_op.allow_spinning", "0")
        .map_err(ort::Error::<()>::from)?
        .commit_from_file(graph)?;
    let metadata = session.metadata()?;
    if metadata
        .custom("comment")
        .is_some_and(|value| value.contains("Nano"))
    {
        return Err("unsupported SenseVoice variant".into());
    }
    let integer = |key: &str| -> std::result::Result<i32, Box<dyn std::error::Error>> {
        let value: i32 = metadata
            .custom(key)
            .ok_or("missing SenseVoice metadata")?
            .parse()?;
        if value < 0 {
            return Err("invalid SenseVoice metadata".into());
        }
        Ok(value)
    };
    let vocab = integer("vocab_size")?;
    let window = integer("lfr_window_size")?;
    let shift = integer("lfr_window_shift")?;
    if !(1..=1_000_000).contains(&vocab)
        || !(1..=32).contains(&window)
        || !(1..=window).contains(&shift)
        || integer("normalize_samples")? > 1
    {
        return Err("invalid SenseVoice dimensions".into());
    }
    let blank: i32 = metadata
        .custom("blank_id")
        .unwrap_or_else(|| "0".into())
        .parse()?;
    if !(0..vocab).contains(&blank) {
        return Err("invalid SenseVoice blank token".into());
    }
    let itn = integer("with_itn")?;
    for key in [
        "without_itn",
        "lang_auto",
        "lang_zh",
        "lang_en",
        "lang_ja",
        "lang_ko",
        "lang_yue",
    ] {
        if integer(key)? >= vocab {
            return Err("invalid SenseVoice token metadata".into());
        }
    }
    if itn >= vocab {
        return Err("invalid SenseVoice ITN token".into());
    }
    let language = integer("lang_auto")?;
    let dimension = window as usize * 80;
    for key in ["neg_mean", "inv_stddev"] {
        let values = metadata
            .custom(key)
            .ok_or("missing SenseVoice normalization")?
            .split(',')
            .map(str::trim)
            .map(str::parse::<f32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if values.len() != dimension || values.iter().any(|value| !value.is_finite()) {
            return Err("invalid SenseVoice normalization".into());
        }
    }
    vocabulary(tokens, vocab as usize)?;
    drop(metadata);
    // Sherpa supplies the inputs in this order, independently of their graph names.
    if session.inputs().len() != 4 || session.outputs().len() != 1
        || !matches!(session.inputs()[0].dtype(), ValueType::Tensor { ty: TensorElementType::Float32, shape, .. } if shape.len() == 3 && matches!(shape[0], -1 | 1) && shape[1] == -1 && shape[2] == dimension as i64)
        || session.inputs()[1..].iter().any(|input| !matches!(input.dtype(), ValueType::Tensor { ty: TensorElementType::Int32, shape, .. } if shape.len() == 1 && matches!(shape[0], -1 | 1)))
        || !matches!(session.outputs()[0].dtype(), ValueType::Tensor { ty: TensorElementType::Float32, shape, .. } if shape.len() == 3 && matches!(shape[0], -1 | 1) && shape[1] == -1 && shape[2] == i64::from(vocab)) {
        return Err("incompatible SenseVoice graph interface".into());
    }
    let names = session
        .inputs()
        .iter()
        .map(|input| input.name().to_owned())
        .collect::<Vec<_>>();
    let frames = 8usize;
    let inputs: Vec<(String, ort::session::SessionInputValue<'static>)> = vec![
        (
            names[0].clone(),
            Tensor::from_array(([1, frames, dimension], vec![0f32; frames * dimension]))?.into(),
        ),
        (
            names[1].clone(),
            Tensor::from_array(([1], vec![frames as i32]))?.into(),
        ),
        (
            names[2].clone(),
            Tensor::from_array(([1], vec![language]))?.into(),
        ),
        (
            names[3].clone(),
            Tensor::from_array(([1], vec![itn]))?.into(),
        ),
    ];
    let outputs = session.run(inputs)?;
    let (shape, values) = outputs[0].try_extract_tensor::<f32>()?;
    if **shape != [1, frames as i64 + 4, i64::from(vocab)]
        || values.iter().any(|value| !value.is_finite())
    {
        return Err("invalid SenseVoice inference output".into());
    }
    Ok(())
}

fn vocabulary(path: &Path, count: usize) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 32 * 1024 * 1024 {
        return Err("invalid SenseVoice vocabulary file".into());
    }
    let text = std::fs::read_to_string(path)?;
    let mut ids = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    for line in text.lines() {
        let (symbol, id) = line
            .rsplit_once(char::is_whitespace)
            .ok_or("invalid SenseVoice vocabulary")?;
        let id: usize = id.trim().parse()?;
        if symbol.trim().is_empty()
            || symbol.contains('\0')
            || id >= count
            || !ids.insert(id)
            || !symbols.insert(symbol)
        {
            return Err("invalid SenseVoice vocabulary entry".into());
        }
    }
    if ids.len() != count {
        return Err("incomplete SenseVoice vocabulary".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_rejects_duplicate_ids() {
        let directory = tempfile::tempdir().unwrap();
        let tokens = directory.path().join("tokens.txt");
        std::fs::write(&tokens, "<blank> 0\nhello 0\n").unwrap();
        assert!(vocabulary(&tokens, 2).is_err());
        std::fs::write(&tokens, "<blank> 0\nhello 1\n").unwrap();
        vocabulary(&tokens, 2).unwrap();
        assert!(vocabulary(&tokens, 3).is_err());
    }

    #[test]
    fn rejects_non_graph_before_sherpa() {
        let directory = tempfile::tempdir().unwrap();
        let graph = directory.path().join("model.onnx");
        let tokens = directory.path().join("tokens.txt");
        std::fs::write(&graph, b"not an ONNX graph").unwrap();
        std::fs::write(&tokens, "<blank> 0\n").unwrap();
        assert_eq!(check(&graph, &tokens), Err("dictation_model_invalid"));
    }

    #[test]
    #[ignore = "requires verified SenseVoice fixtures and packaged shared runtime"]
    fn validates_real_sensevoice() {
        let directory = std::env::var_os("SAILRY_DICTATION_FIXTURES")
            .map(std::path::PathBuf::from)
            .expect("set SAILRY_DICTATION_FIXTURES")
            .join("model");
        check(
            &directory.join("model.int8.onnx"),
            &directory.join("tokens.txt"),
        )
        .unwrap();
    }
}
