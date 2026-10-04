//! Pinned offline SenseVoice assets shared by desktop and mobile controllers.
use super::*;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use tokio::io::AsyncWriteExt;

const SOURCE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07";
const FILES: &[(&str, u64, &str)] = &[
    (
        "model.int8.onnx",
        239233841,
        "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
    ),
    (
        "tokens.txt",
        315894,
        "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
    ),
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DownloadProgress {
    pub percent: f32,
    pub bytes_per_second: f64,
}

fn transfer_progress(
    completed: u64,
    total: u64,
    transferred: u64,
    elapsed: Duration,
) -> DownloadProgress {
    DownloadProgress {
        percent: (completed as f32 / total.max(1) as f32 * 100.).min(99.),
        // Cached files contribute to completion, never to network throughput.
        bytes_per_second: transferred as f64 / elapsed.as_secs_f64().max(0.001),
    }
}

pub async fn prepare(directory: &Path, cancel: &CancellationToken) -> Result<PathBuf> {
    prepare_with_progress(directory, cancel, |_| {}).await
}

pub fn ready(directory: &Path) -> bool {
    FILES
        .iter()
        .all(|&(name, size, checksum)| valid(&directory.join(name), size, checksum))
}

pub async fn prepare_with_progress(
    directory: &Path,
    cancel: &CancellationToken,
    mut progress: impl FnMut(DownloadProgress),
) -> Result<PathBuf> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|_| Error::Storage)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|_| Error::Download)?;
    let total: u64 = FILES.iter().map(|(_, size, _)| size).sum();
    let mut completed = 0u64;
    let mut transferred = 0u64;
    let started = std::time::Instant::now();
    for &(name, size, checksum) in FILES {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let path = directory.join(name);
        let check = path.clone();
        if tokio::task::spawn_blocking(move || valid(&check, size, checksum))
            .await
            .unwrap_or(false)
        {
            completed += size;
            progress(transfer_progress(
                completed,
                total,
                transferred,
                started.elapsed(),
            ));
            continue;
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let temporary = tempfile::NamedTempFile::new_in(directory).map_err(|_| Error::Storage)?;
        let mut output = tokio::fs::File::from_std(
            temporary
                .as_file()
                .try_clone()
                .map_err(|_| Error::Storage)?,
        );
        let mut response = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            response = client.get(format!("{SOURCE}/{name}")).send() => response
                .and_then(reqwest::Response::error_for_status).map_err(|_| Error::Download)?,
        };
        let mut received = 0u64;
        let mut hash = Sha256::new();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                chunk = response.chunk() => chunk.map_err(|_| Error::Download)?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            received += chunk.len() as u64;
            transferred += chunk.len() as u64;
            if received > size {
                return Err(Error::InvalidModel);
            }
            hash.update(&chunk);
            output.write_all(&chunk).await.map_err(|_| Error::Storage)?;
            progress(transfer_progress(
                completed + received,
                total,
                transferred,
                started.elapsed(),
            ));
        }
        output.flush().await.map_err(|_| Error::Storage)?;
        drop(output);
        if received != size || format!("{:x}", hash.finalize()) != checksum {
            return Err(Error::InvalidModel);
        }
        temporary.persist(path).map_err(|_| Error::Storage)?;
        completed += size;
    }
    progress(DownloadProgress {
        percent: 100.,
        bytes_per_second: 0.,
    });
    Ok(directory.to_owned())
}

fn valid(path: &Path, size: u64, checksum: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == size)
    {
        return false;
    }
    let mut buffer = [0; 64 * 1024];
    let mut hash = Sha256::new();
    loop {
        let Ok(count) = file.read(&mut buffer) else {
            return false;
        };
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    format!("{:x}", hash.finalize()) == checksum
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
        use sherpa_onnx::{
            OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig,
        };
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
            model: Some(
                directory
                    .join("model.int8.onnx")
                    .to_str()
                    .ok_or(Error::Storage)?
                    .into(),
            ),
            language: Some(language.code().into()),
            use_itn: true,
        };
        config.model_config.tokens = Some(
            directory
                .join("tokens.txt")
                .to_str()
                .ok_or(Error::Storage)?
                .into(),
        );
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

#[cfg(test)]
mod integrity {
    use super::*;
    #[test]
    fn rejects_partial_and_changed_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model");
        let checksum = format!("{:x}", Sha256::digest(b"model"));
        std::fs::write(&path, b"mod").unwrap();
        assert!(!valid(&path, 5, &checksum));
        std::fs::write(&path, b"other").unwrap();
        assert!(!valid(&path, 5, &checksum));
        std::fs::write(&path, b"model").unwrap();
        assert!(valid(&path, 5, &checksum));
    }

    #[test]
    fn excludes_cached_download_bytes() {
        let progress = transfer_progress(750, 1000, 250, Duration::from_secs(2));
        assert_eq!(progress.percent, 75.);
        assert_eq!(progress.bytes_per_second, 125.);
        assert_eq!(
            transfer_progress(1000, 1000, 0, Duration::ZERO).percent,
            99.
        );
        assert_eq!(
            transfer_progress(1000, 1000, 0, Duration::ZERO).bytes_per_second,
            0.
        );
    }
}
