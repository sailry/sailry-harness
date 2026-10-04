use super::*;
use cpal::{
    SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::sync::{
    Mutex as SyncMutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct Recorder {
    stream: cpal::Stream,
    samples: Arc<SyncMutex<Vec<f32>>>,
    failed: Arc<AtomicBool>,
}
impl Recorder {
    pub fn new(selected: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();
        let device = match selected {
            Some(id) => id.parse().ok().and_then(|id| host.device_by_id(&id)),
            None => host.default_input_device(),
        }
        .ok_or("dictation_microphone")?;
        let config = device
            .default_input_config()
            .map_err(|_| "dictation_microphone")?;
        let samples = Arc::new(SyncMutex::new(Vec::new()));
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                build::<f32>(&device, config.into(), samples.clone(), failed.clone())
            }
            SampleFormat::I16 => {
                build::<i16>(&device, config.into(), samples.clone(), failed.clone())
            }
            SampleFormat::U16 => {
                build::<u16>(&device, config.into(), samples.clone(), failed.clone())
            }
            _ => Err("dictation_microphone"),
        }?;
        Ok(Self {
            stream,
            samples,
            failed,
        })
    }
    pub fn start(&self) -> Result<()> {
        self.stream.play().map_err(|_| "dictation_microphone")
    }
    pub fn check(&self) -> Result<()> {
        if self.failed.load(Ordering::Acquire) {
            Err("dictation_microphone")
        } else {
            Ok(())
        }
    }
    pub fn full(&self) -> bool {
        self.samples.lock().unwrap().len() >= MAX_SAMPLES
    }
    pub fn snapshot(&self, after: usize) -> Option<Vec<f32>> {
        let samples = self.samples.lock().unwrap();
        (samples.len() >= after + SAMPLE_RATE as usize).then(|| samples.clone())
    }
    pub fn finish(&mut self) -> Result<Vec<f32>> {
        self.stream.pause().map_err(|_| "dictation_microphone")?;
        self.check()?;
        let samples = std::mem::take(&mut *self.samples.lock().unwrap());
        validate(&samples)?;
        Ok(samples)
    }
}

fn validate(samples: &[f32]) -> Result<()> {
    if samples.is_empty() {
        return Err("dictation_no_audio");
    }
    if samples.iter().all(|sample| sample.abs() <= f32::EPSILON) {
        return Err("dictation_silent");
    }
    Ok(())
}

pub(crate) fn microphones() -> Result<Vec<(String, String)>> {
    cpal::default_host()
        .input_devices()
        .map_err(|_| "dictation_microphone")?
        .map(|device| {
            let id = device.id().map_err(|_| "dictation_microphone")?;
            let description = device.description().map_err(|_| "dictation_microphone")?;
            Ok((id.to_string(), description.name().to_owned()))
        })
        .collect()
}
impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stream.pause();
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    samples: Arc<SyncMutex<Vec<f32>>>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    if config.channels == 0 || config.sample_rate == 0 {
        return Err("dictation_microphone");
    }
    let channels = usize::from(config.channels);
    let mut sampler = Sampler::new(config.sample_rate);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut samples = samples.lock().unwrap();
                for frame in data.chunks_exact(channels) {
                    let value = frame
                        .iter()
                        .map(|sample| sample.to_sample::<f32>())
                        .sum::<f32>()
                        / channels as f32;
                    sampler.push(value, |value| {
                        if samples.len() < MAX_SAMPLES {
                            samples.push(value);
                        }
                    });
                }
            },
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|_| "dictation_microphone")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_levels() {
        assert_eq!(validate(&[]), Err("dictation_no_audio"));
        assert_eq!(validate(&[0.; 100]), Err("dictation_silent"));
        assert!(validate(&[0.0001, -0.0001]).is_ok());
    }
}
