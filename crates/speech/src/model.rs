//! Pinned offline SenseVoice assets shared by desktop and mobile controllers.
use super::*;
pub use sailry_model_assets::Progress as DownloadProgress;
use sailry_model_assets::{Download, File};
use std::path::Path;

const SOURCE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07";
const FILES: &[File<'static>] = &[
    File {
        name: "model.int8.onnx",
        size: 239233841,
        sha256: "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
    },
    File {
        name: "tokens.txt",
        size: 315894,
        sha256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
    },
];

pub async fn prepare(directory: &Path, cancel: &CancellationToken) -> Result<PathBuf> {
    prepare_with_progress(directory, cancel, |_| {}).await
}

pub fn ready(directory: &Path) -> bool {
    sailry_model_assets::ready(directory, FILES)
}

pub async fn prepare_with_progress(
    directory: &Path,
    cancel: &CancellationToken,
    progress: impl FnMut(DownloadProgress),
) -> Result<PathBuf> {
    let downloads = FILES
        .iter()
        .map(|file| {
            Ok(Download {
                file: *file,
                url: format!("{SOURCE}/{}", file.name)
                    .parse()
                    .map_err(|_| Error::InvalidModel)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    sailry_model_assets::prepare(directory, &downloads, cancel, progress)
        .await
        .map_err(|error| match error {
            sailry_model_assets::Error::Storage(_) => Error::Storage,
            sailry_model_assets::Error::Download(_) => Error::Download,
            sailry_model_assets::Error::InvalidAsset => Error::InvalidModel,
            sailry_model_assets::Error::Cancelled => Error::Cancelled,
        })
}

pub fn recognize(directory: &Path, samples: &[f32], language: Language) -> Result<String> {
    if samples.len() < SAMPLE_RATE as usize / 5
        || samples.iter().all(|value| value.abs() <= f32::EPSILON)
    {
        return Ok(String::new());
    }
    Recognizer::new(directory, language)?.recognize(samples)
}

/// Reuse one loaded model for successive local draft revisions.
pub struct Recognizer(sherpa_onnx::OfflineRecognizer);
impl Recognizer {
    pub fn new(directory: &Path, language: Language) -> Result<Self> {
        Self::from_paths(
            &directory.join("model.int8.onnx"),
            &directory.join("tokens.txt"),
            language,
        )
    }

    /// Load a compatible SenseVoice graph and vocabulary supplied by the local controller.
    pub fn from_paths(model: &Path, tokens: &Path, language: Language) -> Result<Self> {
        use sherpa_onnx::{
            OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig,
        };
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
            model: Some(model.to_str().ok_or(Error::Storage)?.into()),
            language: Some(language.code().into()),
            use_itn: true,
        };
        config.model_config.tokens = Some(tokens.to_str().ok_or(Error::Storage)?.into());
        config.model_config.num_threads =
            std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4)) as i32;
        config.model_config.provider = Some("cpu".into());
        OfflineRecognizer::create(&config)
            .map(Self)
            .ok_or(Error::InvalidModel)
    }

    /// Decode cumulative 16 kHz mono audio; each result replaces the previous hypothesis.
    pub fn recognize(&self, samples: &[f32]) -> Result<String> {
        if samples.len() > crate::MAX_SAMPLES || samples.iter().any(|v| !v.is_finite()) {
            return Err(Error::InvalidAudio);
        }
        if samples.len() < SAMPLE_RATE as usize / 5
            || samples.iter().all(|value| value.abs() <= f32::EPSILON)
        {
            return Ok(String::new());
        }
        let stream = self.0.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        self.0.decode(&stream);
        let result = stream.get_result().ok_or(Error::Recognition)?;
        Ok(result.text.trim().to_owned())
    }
}
