use super::*;

fn text(value: &str) -> Vec<u8> {
    [
        (value.len() as u64).to_le_bytes().as_slice(),
        value.as_bytes(),
    ]
    .concat()
}
fn metadata() -> Vec<(&'static str, u32, Vec<u8>)> {
    vec![
        ("general.architecture", 8, text("modern-bert")),
        (
            "modern-bert.context_length",
            4,
            8192u32.to_le_bytes().to_vec(),
        ),
        ("modern-bert.decision.type", 8, text("laya")),
        (
            "modern-bert.decision.max_head_tokens",
            4,
            256u32.to_le_bytes().to_vec(),
        ),
        ("tokenizer.chat_template.systemone", 8, text("{{ state }}")),
    ]
}
fn fixture(values: Vec<(&str, u32, Vec<u8>)>, tensors: &[(u32, u64, u64)]) -> Vec<u8> {
    let mut bytes = b"GGUF".to_vec();
    bytes.extend(3u32.to_le_bytes());
    bytes.extend((tensors.len() as u64).to_le_bytes());
    bytes.extend((values.len() as u64).to_le_bytes());
    for (key, kind, value) in values {
        bytes.extend(text(key));
        bytes.extend(kind.to_le_bytes());
        bytes.extend(value);
    }
    for (index, (kind, dimension, offset)) in tensors.iter().enumerate() {
        bytes.extend(text(&format!("weight-{index}")));
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(dimension.to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(offset.to_le_bytes());
    }
    bytes.resize(bytes.len().next_multiple_of(32) + 512, 0);
    bytes
}
fn read(bytes: &[u8]) -> Result<Metadata> {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), bytes).unwrap();
    validate(file.path())
}

#[test]
fn reads_calibrated_metadata() {
    let mut values = metadata();
    values.extend([
        (
            "modern-bert.decision.temperature.choice.3_5",
            6,
            0.75f32.to_le_bytes().to_vec(),
        ),
        (
            "modern-bert.decision.score_bias.3.1",
            12,
            0.25f64.to_le_bytes().to_vec(),
        ),
    ]);
    let parsed = read(&fixture(values, &[(8, 32, 0), (12, 256, 64)])).unwrap();
    assert_eq!(parsed.architecture, "modern-bert");
    assert_eq!(parsed.algorithm, Algorithm::Laya);
    assert_eq!(parsed.context, 8192);
    assert_eq!(parsed.temperatures["choice.3_5"], 0.75);
    assert_eq!(parsed.bias[&3], [0.0, 0.25, 0.0]);
}

#[test]
fn rejects_invalid_headers_and_lengths() {
    let bytes = fixture(metadata(), &[(0, 4, 0)]);
    for length in [0, 4, 12, 24, 32, bytes.len() - 512] {
        assert!(read(&bytes[..length]).is_err());
    }
    for (start, value) in [(4, 2u64), (8, MAX_ENTRIES + 1), (16, MAX_ENTRIES + 1)] {
        let mut invalid = bytes.clone();
        let width = if start == 4 { 4 } else { 8 };
        invalid[start..start + width].copy_from_slice(&value.to_le_bytes()[..width]);
        assert!(read(&invalid).is_err());
    }
    let mut values = metadata();
    values.push(("oversized", 8, u64::MAX.to_le_bytes().to_vec()));
    assert!(read(&fixture(values, &[(0, 4, 0)])).is_err());
}

#[test]
fn rejects_invalid_metadata() {
    for entry in [
        ("split.count", 4, 2u32.to_le_bytes().to_vec()),
        ("general.alignment", 4, 3u32.to_le_bytes().to_vec()),
        ("general.alignment", 8, text("32")),
        ("unknown", 13, Vec::new()),
        ("boolean", 7, vec![2]),
        (
            "array",
            9,
            [9u32.to_le_bytes().as_slice(), 1u64.to_le_bytes().as_slice()].concat(),
        ),
        (
            "array",
            9,
            [
                0u32.to_le_bytes().as_slice(),
                u64::MAX.to_le_bytes().as_slice(),
            ]
            .concat(),
        ),
        (
            "modern-bert.decision.temperature.choice",
            6,
            0.0f32.to_le_bytes().to_vec(),
        ),
        (
            "modern-bert.decision.temperature.choice",
            6,
            f32::NAN.to_le_bytes().to_vec(),
        ),
        (
            "modern-bert.decision.score_bias.2.2",
            6,
            1.0f32.to_le_bytes().to_vec(),
        ),
    ] {
        let mut values = metadata();
        values.push(entry);
        assert!(read(&fixture(values, &[(0, 4, 0)])).is_err());
    }
    let mut values = metadata();
    values.push(("general.architecture", 8, text("bert")));
    assert!(read(&fixture(values, &[(0, 4, 0)])).is_err());
}

#[test]
fn rejects_invalid_tensor_ranges() {
    for tensors in [
        vec![(0, 4, 1)],
        vec![(0, 129, 0)],
        vec![(0, u64::MAX, 0)],
        vec![(8, 31, 0)],
        vec![(4, 32, 0)],
        vec![(0, 4, 0), (0, 4, 0)],
        vec![(0, 4, u64::MAX - 31)],
    ] {
        assert!(read(&fixture(metadata(), &tensors)).is_err());
    }
}

#[test]
fn requires_supported_decision_metadata() {
    for missing in [
        "general.architecture",
        "modern-bert.decision.type",
        "modern-bert.context_length",
        "tokenizer.chat_template.systemone",
    ] {
        let mut values = metadata();
        values.retain(|(key, _, _)| *key != missing);
        assert!(read(&fixture(values, &[(0, 4, 0)])).is_err());
    }
    let mut values = metadata();
    values.retain(|(key, _, _)| *key != "modern-bert.decision.type");
    values.push(("modern-bert.decision.type", 8, text("openjev-type")));
    assert!(read(&fixture(values.clone(), &[(0, 4, 0)])).is_err());
    values.last_mut().unwrap().2 = text("openjev");
    assert_eq!(
        read(&fixture(values, &[(0, 4, 0)])).unwrap().algorithm,
        Algorithm::OpenJev
    );
}

#[test]
fn bounds_file_size_before_reading() {
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file()
        .set_len(sailry_protocol::models::MAX_ASSET_BYTES + 1)
        .unwrap();
    assert!(matches!(validate(file.path()), Err(Error::Invalid(_))));
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_links() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("source"),
        fixture(metadata(), &[(0, 4, 0)]),
    )
    .unwrap();
    let path = directory.path().join("model.gguf");
    std::os::unix::fs::symlink(directory.path().join("source"), &path).unwrap();
    assert!(matches!(validate(&path), Err(Error::Invalid(_))));
}
