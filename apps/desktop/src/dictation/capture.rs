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
    samples: Arc<SyncMutex<Buffer>>,
    failed: Arc<AtomicBool>,
}

#[derive(Default)]
struct Buffer {
    pending: Vec<i16>,
    count: usize,
}

impl Buffer {
    fn push(&mut self, sample: f32) {
        if self.count < MAX_SAMPLES {
            self.pending.push(pcm(sample));
            self.count += 1;
        }
    }
}

pub(super) fn pcm(sample: f32) -> i16 {
    (sample * 32768.)
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
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
        let samples = Arc::new(SyncMutex::new(Buffer::default()));
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
        self.samples.lock().unwrap().count >= MAX_SAMPLES
    }
    pub fn take(&self) -> Vec<i16> {
        std::mem::take(&mut self.samples.lock().unwrap().pending)
    }
    pub fn finish(self) -> Result<Vec<i16>> {
        self.stream.pause().map_err(|_| "dictation_microphone")?;
        self.check()?;
        let samples = self.samples.clone();
        // Dispose the stream before draining so no callback can append PCM
        // behind the final chunk or its end-of-input notification.
        drop(self);
        Ok(std::mem::take(&mut samples.lock().unwrap().pending))
    }
}

pub(super) fn validate(samples: &[f32]) -> Result<()> {
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
    samples: Arc<SyncMutex<Buffer>>,
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
                        samples.push(value);
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

    #[test]
    fn chunks_preserve_resampled_pcm() {
        let mut sampler = Sampler::new(48_000);
        let mut buffer = Buffer::default();
        for sample in [0.5, 0.5, 0.5, -0.25, -0.25, -0.25] {
            sampler.push(sample, |sample| buffer.push(sample));
        }
        assert_eq!(std::mem::take(&mut buffer.pending), [16384, -8192]);
        assert_eq!(buffer.count, 2);
        buffer.push(-1.);
        buffer.push(1.);
        assert_eq!(buffer.pending, [i16::MIN, i16::MAX]);
    }

    #[test]
    fn draining_keeps_the_duration_bound() {
        let mut buffer = Buffer {
            count: MAX_SAMPLES - 1,
            ..Default::default()
        };
        buffer.push(0.5);
        assert_eq!(std::mem::take(&mut buffer.pending), [16384]);
        buffer.push(0.5);
        assert!(buffer.pending.is_empty());
        assert_eq!(buffer.count, MAX_SAMPLES);
    }
}
