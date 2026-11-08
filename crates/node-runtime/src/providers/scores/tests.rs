use super::*;
use sailry_protocol::Effort;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/model-scores.json");

#[test]
fn parses_published_measurements() {
    let data = parse::parse(FIXTURE).unwrap();
    assert_eq!(data.batch, "overall-f26004137653ddaeb3be2933");
    assert_eq!(data.published_at, "2026-11-07T07:48:26.226Z");
    assert_eq!(data.entries.len(), 4);
    let score = &data
        .entries
        .iter()
        .find(|entry| entry.model == "anthropic/claude-sonnet-5.5")
        .unwrap()
        .score;
    assert_eq!(score.axes, [84, 86, 85, 73, 66]);
    assert_eq!(score.overall, 81.95);
    assert_eq!(score.elapsed_ms, Some(2_912_078));
    assert!((score.cost_usd.unwrap() - 2.802464).abs() < 0.00001);
    assert!(
        data.entries
            .iter()
            .any(|entry| entry.model == "k3.1-pro" && entry.score.effort == Effort::Max)
    );
    assert!(
        !data
            .entries
            .iter()
            .any(|entry| entry.model == "union-alpha")
    );
}

#[test]
fn orders_duplicate_candidates() {
    let mut data: Value = serde_json::from_slice(FIXTURE).unwrap();
    let mut duplicate = data["snapshot"]["entries"][0].clone();
    duplicate["candidate_id"] = json!("a:measured:xhigh");
    duplicate["overall_score"] = json!(81.0);
    data["snapshot"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let expected = parse::parse(&serde_json::to_vec(&data).unwrap()).unwrap();
    data["snapshot"]["entries"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        parse::parse(&serde_json::to_vec(&data).unwrap()).unwrap(),
        expected
    );
    assert_eq!(expected.entries.len(), 4);
    assert_eq!(expected.entries[0].score.overall, 81.0);
}

#[test]
fn rejects_invalid_downloads() {
    for bytes in [b"x".as_slice(), b"{}", b"{\"snapshot\":{}}"] {
        assert!(parse::parse(bytes).is_err());
    }
    let mut data: Value = serde_json::from_slice(FIXTURE).unwrap();
    data["snapshot"]["entries"] = json!(vec![data["snapshot"]["entries"][0].clone(); 2001]);
    assert!(parse::parse(&serde_json::to_vec(&data).unwrap()).is_err());
}

#[test]
fn skips_malformed_rows() {
    let original: Value = serde_json::from_slice(FIXTURE).unwrap();
    let count = parse::parse(FIXTURE).unwrap().entries.len();
    for (field, value) in [
        ("reasoning_effort", json!("minimal")),
        ("backend_score", Value::Null),
        ("frontend_score", json!(120)),
        ("overall_score", json!(-1)),
    ] {
        let mut data = original.clone();
        data["snapshot"]["entries"][0][field] = value;
        let parsed = parse::parse(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(parsed.entries.len(), count - 1, "{field}");
    }
    let mut data = original;
    data["snapshot"]["entries"][0]["backend_score"] = json!(84.6);
    let parsed = parse::parse(&serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(parsed.entries.len(), count);
    assert!(parsed.entries.iter().any(|entry| entry.score.axes[0] == 85));
}

#[test]
fn maps_default_efforts_without_metrics() {
    let mut data: Value = serde_json::from_slice(FIXTURE).unwrap();
    data["metrics"] = json!({});
    for effort in ["default", "unknown"] {
        data["snapshot"]["entries"][0]["reasoning_effort"] = json!(effort);
        let parsed = parse::parse(&serde_json::to_vec(&data).unwrap()).unwrap();
        let score = &parsed.entries[0].score;
        assert_eq!(score.effort, Effort::Default);
        assert_eq!(score.elapsed_ms, None);
        assert_eq!(score.cost_usd, None);
    }
}

#[test]
fn matches_exact_normalized_names() {
    let models = BTreeSet::from(["claude-opus-5.5".into(), "kimi-for-coding".into()]);
    for candidate in [
        "Claude_Opus_5.5",
        "anthropic/claude-opus-5-5",
        "claude-opus-5.5-20261107",
        "claude-opus-5-5-2026-11-07",
        "claude-opus-5-5-latest",
    ] {
        assert_eq!(
            matching::resolve(&[candidate.into()], &models, &BTreeMap::new()),
            Some("claude-opus-5.5".into())
        );
    }
    for candidate in ["claude-opus-5", "claude-opus-5-5-preview", "unrelated"] {
        assert!(matching::resolve(&[candidate.into()], &models, &BTreeMap::new()).is_none());
    }
    assert_eq!(
        matching::resolve(&["k2.8".into()], &models, &BTreeMap::new()),
        Some("kimi-for-coding".into())
    );
}

#[test]
fn keeps_ambiguous_names_unmatched() {
    let models = BTreeSet::from([
        "claude-opus-5.5".into(),
        "claude-opus-5-5".into(),
        "other".into(),
    ]);
    assert!(matching::resolve(&["claude-opus-5-5".into()], &models, &BTreeMap::new()).is_none());
    let aliases = BTreeMap::from([("shown-name".into(), "other".into())]);
    assert_eq!(
        matching::resolve(&["shown_name".into()], &models, &aliases),
        Some("other".into())
    );
    assert!(
        matching::resolve(
            &["shown-name".into(), "claude-opus-5.5".into()],
            &models,
            &aliases
        )
        .is_none()
    );
}
