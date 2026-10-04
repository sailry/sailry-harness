//! Controller-local speech; Flutter owns capture and drafts, Rust owns models and inference.
use flutter_rust_bridge::frb;
use sailry_link::CancellationToken;
use sailry_speech::{Language, model};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};
use tokio::sync::Mutex;

#[frb(opaque)]
pub struct SpeechInput {
    directory: PathBuf,
    owner: Arc<Mutex<()>>,
    cancel: std::sync::Mutex<CancellationToken>,
    progress: Arc<AtomicU32>,
}

impl SpeechInput {
    /// Pass this controller's application-support directory, not an execution Node profile.
    pub fn new(directory: String) -> Self {
        Self {
            directory: PathBuf::from(directory)
                .join("models")
                .join("sensevoice-int8"),
            owner: Arc::default(),
            cancel: std::sync::Mutex::new(CancellationToken::new()),
            progress: Arc::default(),
        }
    }

    pub async fn ready(&self) -> Result<bool, String> {
        let path = self.directory.clone();
        tokio::task::spawn_blocking(move || model::ready(&path))
            .await
            .map_err(super::error)
    }

    pub async fn download(&self) -> Result<(), String> {
        let _guard = self
            .owner
            .clone()
            .try_lock_owned()
            .map_err(|_| "speech input is busy")?;
        let cancel = CancellationToken::new();
        *self.cancel.lock().unwrap() = cancel.clone();
        self.progress.store(0, Ordering::Release);
        model::prepare_with_progress(&self.directory, &cancel, |value| {
            self.progress.store(value.percent as u32, Ordering::Release);
        })
        .await
        .map(|_| ())
        .map_err(super::error)
    }

    pub fn download_progress(&self) -> u32 {
        self.progress.load(Ordering::Acquire)
    }

    pub fn cancel(&self) {
        self.cancel.lock().unwrap().cancel();
    }

    /// Mono floating-point PCM; language is auto, zh, or en. Does not send a message.
    pub async fn transcribe(
        &self,
        samples: Vec<f32>,
        sample_rate: u32,
        language: String,
    ) -> Result<String, String> {
        let language = match language.as_str() {
            "auto" => Language::Auto,
            "zh" => Language::Chinese,
            "en" => Language::English,
            _ => return Err("unsupported speech language".into()),
        };
        let guard = self
            .owner
            .clone()
            .try_lock_owned()
            .map_err(|_| "speech input is busy")?;
        let cancel = CancellationToken::new();
        *self.cancel.lock().unwrap() = cancel.clone();
        let directory = self.directory.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let result = sailry_speech::transcribe(&directory, &samples, sample_rate, language)
                .map_err(super::error);
            if cancel.is_cancelled() {
                Err("speech operation cancelled".into())
            } else {
                result
            }
        })
        .await
        .map_err(super::error)?
    }
}

impl Drop for SpeechInput {
    fn drop(&mut self) {
        self.cancel.lock().unwrap().cancel();
    }
}
