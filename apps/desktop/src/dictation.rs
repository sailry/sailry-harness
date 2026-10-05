//! Local draft input; audio never enters Node, Link, or conversation history.
mod capture;
pub(crate) mod permission;
pub(crate) use capture::microphones;
pub(crate) use sailry_speech::{MAX_SAMPLES, SAMPLE_RATE, Sampler, model};
#[cfg(test)]
mod tests;

use sailry_link::CancellationToken;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};

pub(crate) type Result<T> = std::result::Result<T, &'static str>;

#[derive(Default)]
pub(crate) struct Service {
    owner: Arc<Mutex<()>>,
}
impl gpui_kit::Global for Service {}

pub(crate) enum Update {
    Preparing,
    Recording,
    Partial(String),
    Transcribing,
    Finished(Result<String>),
}

impl Service {
    pub fn shared(cx: &mut gpui_kit::App) -> Arc<Mutex<()>> {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        cx.global::<Self>().owner.clone()
    }
}

pub(crate) fn directory(cx: &gpui_kit::App) -> Option<PathBuf> {
    cx.try_global::<crate::preferences::Preferences>()
        .and_then(|preferences| preferences.directory())
        .map(|path| path.join("models").join("sensevoice-int8"))
}

pub(crate) async fn run(
    directory: PathBuf,
    options: crate::preferences::Dictation,
    owner: Arc<Mutex<()>>,
    finish: CancellationToken,
    cancel: CancellationToken,
    updates: mpsc::Sender<Update>,
) -> Result<String> {
    if cancel.is_cancelled() {
        return Ok(String::new());
    }
    let _owner = owner.try_lock_owned().map_err(|_| "dictation_busy")?;
    let _ = updates.send(Update::Preparing).await;
    let check = directory.clone();
    let ready = tokio::task::spawn_blocking(move || model::ready(&check))
        .await
        .map_err(|_| "dictation_failed")?;
    if cancel.is_cancelled() {
        return Ok(String::new());
    }
    if !ready {
        return Err("dictation_model_missing");
    }
    permission::verify()?;
    let path = directory;
    if cancel.is_cancelled() {
        return Ok(String::new());
    }
    // Device setup and inference stay off GPUI's executor. Authorization belongs to the central UI.
    tokio::task::spawn_blocking(move || {
        let recognizer = model::Recognizer::new(&path, options.language).map_err(error_key)?;
        let mut recorder = capture::Recorder::new(options.microphone.as_deref())?;
        if cancel.is_cancelled() {
            return Ok(String::new());
        }
        recorder.start()?;
        let _ = updates.blocking_send(Update::Recording);
        let mut decoded = 0;
        let mut last_decode = std::time::Instant::now();
        while !finish.is_cancelled() && !cancel.is_cancelled() && !recorder.full() {
            recorder.check()?;
            if last_decode.elapsed() >= Duration::from_millis(800) {
                if let Some(samples) = recorder.snapshot(decoded) {
                    decoded = samples.len();
                    let text = recognizer.recognize(&samples).map_err(error_key)?;
                    if !text.is_empty() && !cancel.is_cancelled() {
                        let _ = updates.blocking_send(Update::Partial(text));
                    }
                }
                last_decode = std::time::Instant::now();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let samples = recorder.finish()?;
        if cancel.is_cancelled() {
            return Ok(String::new());
        }
        let _ = updates.blocking_send(Update::Transcribing);
        let text = recognizer.recognize(&samples).map_err(error_key)?;
        // Inference is synchronous; a cancelled result must never change a draft.
        Ok(if cancel.is_cancelled() {
            String::new()
        } else {
            text
        })
    })
    .await
    .map_err(|_| "dictation_failed")?
}

pub(crate) fn error_key(error: sailry_speech::Error) -> &'static str {
    use sailry_speech::Error;
    match error {
        Error::Storage => "dictation_storage",
        Error::Download => "dictation_download",
        Error::InvalidModel => "dictation_model_invalid",
        Error::Cancelled => "dictation_cancelled",
        Error::Recognition | Error::InvalidAudio => "dictation_failed",
    }
}
