//! Offline speech recognition on the controller; no Node, transport, or UI dependency.
pub mod model;
mod sampler;
pub use sampler::Sampler;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use tokio_util::sync::CancellationToken;

pub const SAMPLE_RATE: u32 = 16_000;
pub const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 300;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("speech model storage failed")]
    Storage,
    #[error("speech model download failed")]
    Download,
    #[error("speech model validation failed")]
    InvalidModel,
    #[error("speech operation cancelled")]
    Cancelled,
    #[error("speech recognition failed")]
    Recognition,
    #[error("invalid speech audio")]
    InvalidAudio,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Auto,
    Chinese,
    English,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Chinese, Self::English];

    pub fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }
}

/// Accept mono PCM from a platform recorder; recognition and bounds are shared.
pub fn transcribe(
    directory: &std::path::Path,
    samples: &[f32],
    rate: u32,
    language: Language,
) -> Result<String> {
    if !(8_000..=192_000).contains(&rate)
        || samples.len() > rate as usize * 300
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(Error::InvalidAudio);
    }
    if rate == SAMPLE_RATE {
        return model::recognize(directory, samples, language);
    }
    let mut sampler = Sampler::new(rate);
    let mut audio = Vec::with_capacity(samples.len() * SAMPLE_RATE as usize / rate as usize);
    for &sample in samples {
        sampler.push(sample, |value| audio.push(value));
    }
    model::recognize(directory, &audio, language)
}

#[cfg(test)]
mod tests;
