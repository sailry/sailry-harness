//! Local draft input; audio never enters Node, Link, or conversation history.
mod adapters;
mod capture;
pub(crate) use adapters::{error_key as speech_error_key, session};
pub(crate) mod models;
pub(crate) mod permission;
pub(crate) use capture::microphones;
pub(crate) use sailry_speech::{MAX_SAMPLES, SAMPLE_RATE, Sampler, model};
#[cfg(test)]
mod tests;

use sailry_link::CancellationToken;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

pub(crate) type Result<T> = std::result::Result<T, &'static str>;

#[derive(Default)]
pub(crate) struct Service {
    owner: Arc<Mutex<()>>,
    models: Option<Arc<models::Manager>>,
}
impl gpui_kit::Global for Service {}

pub(crate) enum Update {
    Preparing,
    Partial(String),
    Finished(Result<String>),
}

impl Service {
    pub fn models(cx: &mut gpui_kit::App) -> Result<Arc<models::Manager>> {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        if let Some(manager) = &cx.global::<Self>().models {
            return Ok(manager.clone());
        }
        let directory = cx
            .try_global::<crate::preferences::Preferences>()
            .and_then(|preferences| preferences.directory())
            .ok_or("dictation_unavailable")?;
        let manager = models::Manager::open(directory.to_path_buf())?;
        cx.global_mut::<Self>().models = Some(manager.clone());
        Ok(manager)
    }

    pub fn shared(cx: &mut gpui_kit::App) -> Arc<Mutex<()>> {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        cx.global::<Self>().owner.clone()
    }
}

pub(crate) fn capture(cx: &mut gpui_kit::App) -> Result<models::Capture> {
    let selected = crate::preferences::data(cx)
        .dictation
        .unwrap_or_default()
        .model;
    Service::models(cx)?.capture(selected)
}

pub(crate) fn directory(cx: &gpui_kit::App) -> Option<PathBuf> {
    cx.try_global::<crate::preferences::Preferences>()
        .and_then(|preferences| preferences.directory())
        .map(|path| path.join("models").join("sensevoice-int8"))
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
