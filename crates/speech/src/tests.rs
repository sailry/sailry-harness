use super::*;

#[test]
fn resampling_preserves_duration() {
    for rate in [8000, 16000, 22050, 44100, 48000, 96000] {
        let mut sampler = Sampler::new(rate);
        let mut samples = vec![];
        for _ in 0..rate {
            sampler.push(0.25, |sample| samples.push(sample));
        }
        assert_eq!(samples.len(), SAMPLE_RATE as usize);
        assert!(samples.iter().all(|sample| (sample - 0.25).abs() < 0.0001));
    }
}

#[test]
fn skips_silent_audio() {
    assert_eq!(
        transcribe(
            std::path::Path::new("missing"),
            &vec![0.; 16000],
            SAMPLE_RATE,
            Language::Auto
        )
        .unwrap(),
        ""
    );
}

#[test]
fn rejects_invalid_audio() {
    let path = std::path::Path::new("missing");
    for (audio, rate) in [
        (vec![0.], 0),
        (vec![f32::NAN], SAMPLE_RATE),
        (vec![0.; MAX_SAMPLES + 1], SAMPLE_RATE),
    ] {
        assert_eq!(
            transcribe(path, &audio, rate, Language::Auto),
            Err(Error::InvalidAudio)
        );
    }
}

#[tokio::test]
async fn cancelled_download_stays_unready() {
    let directory = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut progress = vec![];
    let result =
        model::prepare_with_progress(directory.path(), &cancel, |value| progress.push(value)).await;
    assert_eq!(result, Err(Error::Cancelled));
    assert!(progress.is_empty());
    assert!(!model::ready(directory.path()));
}

#[test]
#[ignore = "requires WAV fixtures; downloads the pinned model if absent; does not open audio devices"]
fn transcribes_offline_fixtures() {
    let root =
        PathBuf::from(std::env::var_os("SAILRY_DICTATION_FIXTURES").expect("fixture directory"));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(model::prepare(
            &root.join("model"),
            &CancellationToken::new(),
        ))
        .unwrap();
    let samples: Vec<(String, Vec<String>)> =
        serde_json::from_slice(&std::fs::read(root.join("expected.json")).unwrap()).unwrap();
    for (file, expected) in samples {
        let wave = sherpa_onnx::Wave::read(root.join(&file).to_str().unwrap()).unwrap();
        for gain in [1., 0.001] {
            let audio: Vec<_> = wave.samples().iter().map(|value| value * gain).collect();
            let text = transcribe(
                &root.join("model"),
                &audio,
                wave.sample_rate() as u32,
                Language::Auto,
            )
            .unwrap();
            eprintln!("{file}: {text}");
            assert!(!text.is_empty(), "empty transcript for {file}");
            for expected in &expected {
                assert!(
                    text.to_lowercase().contains(&expected.to_lowercase()),
                    "unexpected transcript for {file}: {text}"
                );
            }
        }
    }
}
