use ort::{
    session::Session,
    value::{Tensor, TensorElementType, ValueType},
};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Failure {
    key: &'static str,
    stage: &'static str,
}

impl Failure {
    fn invalid(stage: &'static str) -> Self {
        Self {
            key: "dictation_model_invalid",
            stage,
        }
    }

    pub(super) fn worker() -> Self {
        Self::invalid("validation_worker")
    }

    pub(super) fn key(self) -> &'static str {
        self.key
    }

    pub(super) fn stage(self) -> &'static str {
        self.stage
    }
}

pub(super) fn check(graph: &Path, tokens: &Path) -> std::result::Result<(), Failure> {
    sailry_model_assets::onnx::validate(graph).map_err(|_| Failure::invalid("onnx_parse"))?;
    sailry_model_assets::runtime::initialize().map_err(|_| Failure::invalid("runtime_load"))?;
    validate(graph, tokens)?;
    // Sherpa's metadata reader terminates the process on missing fields. Validate them first.
    super::model::Recognizer::from_paths(graph, tokens, sailry_speech::Language::Auto).map_err(
        |error| Failure {
            key: super::error_key(error),
            stage: "recognizer",
        },
    )?;
    Ok(())
}

fn validate(graph: &Path, tokens: &Path) -> std::result::Result<(), Failure> {
    let mut session = load_session(graph)?;
    let (vocab, window, itn, language) = metadata(&session)?;
    let dimension = window as usize * 80;
    vocabulary(tokens, vocab as usize).map_err(|_| Failure::invalid("vocabulary"))?;
    // Sherpa supplies the inputs in this order, independently of their graph names.
    if session.inputs().len() != 4 || session.outputs().len() != 1
        || !matches!(session.inputs()[0].dtype(), ValueType::Tensor { ty: TensorElementType::Float32, shape, .. } if shape.len() == 3 && matches!(shape[0], -1 | 1) && shape[1] == -1 && shape[2] == dimension as i64)
        || session.inputs()[1..].iter().any(|input| !matches!(input.dtype(), ValueType::Tensor { ty: TensorElementType::Int32, shape, .. } if shape.len() == 1 && matches!(shape[0], -1 | 1)))
        || !matches!(session.outputs()[0].dtype(), ValueType::Tensor { ty: TensorElementType::Float32, shape, .. } if shape.len() == 3 && matches!(shape[0], -1 | 1) && shape[1] == -1 && shape[2] == i64::from(vocab)) {
        return Err(Failure::invalid("interface"));
    }
    inference(&mut session, vocab, dimension, language, itn)
        .map_err(|_| Failure::invalid("inference"))
}

fn load_session(graph: &Path) -> std::result::Result<Session, Failure> {
    (|| -> std::result::Result<Session, Box<dyn std::error::Error>> {
        Ok(sailry_model_assets::session::builder()?.commit_from_file(graph)?)
    })()
    .map_err(|_| Failure::invalid("session_load"))
}

fn metadata(session: &Session) -> std::result::Result<(i32, i32, i32, i32), Failure> {
    (|| -> std::result::Result<(i32, i32, i32, i32), Box<dyn std::error::Error>> {
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
        Ok((vocab, window, itn, language))
    })()
    .map_err(|_| Failure::invalid("metadata"))
}

fn inference(
    session: &mut Session,
    vocab: i32,
    dimension: usize,
    language: i32,
    itn: i32,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
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
        let error = check(&graph, &tokens).unwrap_err();
        assert_eq!(error.key(), "dictation_model_invalid");
        assert_eq!(error.stage(), "onnx_parse");
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
