use super::*;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

#[derive(serde::Deserialize)]
struct Fixture {
    image: String,
    text: Vec<String>,
    sha256: String,
}

fn evaluate(tier: &str) {
    let root = PathBuf::from(
        std::env::var_os("SAILRY_OCR_REFERENCE").expect("SAILRY_OCR_REFERENCE is required"),
    );
    let directory = root.join(tier);
    let definition: Definition =
        serde_json::from_slice(&std::fs::read(directory.join("definition.json")).unwrap()).unwrap();
    let fixtures: Vec<Fixture> =
        serde_json::from_slice(&std::fs::read(root.join("fixtures.json")).unwrap()).unwrap();
    assert!(!fixtures.is_empty());
    let started = Instant::now();
    let mut engine = Engine::load(&definition, &directory).unwrap();
    eprintln!("{tier} load_ms={}", started.elapsed().as_millis());
    for fixture in fixtures {
        let bytes = std::fs::read(root.join(&fixture.image)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), fixture.sha256);
        let image = ::image::load_from_memory(&bytes).unwrap().into_rgb8();
        let started = Instant::now();
        let lines = engine.recognize(&image, || false).unwrap();
        eprintln!(
            "{tier} {} elapsed_ms={} lines={:?}",
            fixture.image,
            started.elapsed().as_millis(),
            lines
        );
        assert_eq!(
            lines.iter().map(|line| &line.text).collect::<Vec<_>>(),
            fixture.text.iter().collect::<Vec<_>>(),
            "{}",
            fixture.image
        );
        assert!(lines.iter().all(|line| line.confidence.is_finite() && (0.0..=1.001).contains(&line.confidence)));
        assert!(matches!(
            engine.recognize(&image, || true),
            Err(Error::Cancelled)
        ));
    }
}

#[test]
#[ignore = "requires pinned OCR models and independent bilingual fixtures"]
fn transcribes_tiny_bilingual_samples() {
    evaluate("pp-ocrv6-tiny");
}

#[test]
#[ignore = "requires pinned OCR models and independent bilingual fixtures"]
fn transcribes_mobile_bilingual_samples() {
    evaluate("pp-ocrv5-mobile");
}
