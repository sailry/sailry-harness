use super::*;
use core::prelude::v1::test;

fn score(effort: Effort, overall: f32) -> catalog::EffortScore {
    catalog::EffortScore {
        effort,
        overall,
        axes: [80; 5],
        elapsed_ms: None,
        cost_usd: None,
    }
}

#[test]
fn effort_tracks_the_selected_row() {
    assert_eq!(
        effective(true, Some(Effort::High), Effort::Low),
        Effort::High
    );
    assert_eq!(
        effective(false, Some(Effort::High), Effort::Low),
        Effort::Low
    );
    assert_eq!(effective(true, None, Effort::Medium), Effort::Medium);
}

#[test]
fn nearest_prefers_lower_then_higher() {
    let values = [
        score(Effort::Max, 95.),
        score(Effort::Medium, 80.),
        score(Effort::Low, 75.),
    ];
    assert_eq!(
        nearest(&values, Effort::High).unwrap().effort,
        Effort::Medium
    );
    assert_eq!(
        nearest(&values, Effort::Minimal).unwrap().effort,
        Effort::Low
    );
    assert_eq!(nearest(&values, Effort::Max).unwrap().overall, 95.);
    assert!(nearest(&[], Effort::High).is_none());
}

#[test]
fn default_scores_remain_distinct() {
    let values = [score(Effort::Default, 85.), score(Effort::High, 90.)];
    assert_eq!(nearest(&values, Effort::Default).unwrap().overall, 85.);
    assert_eq!(nearest(&values, Effort::Low).unwrap().effort, Effort::High);
    assert_eq!(
        nearest(&values[..1], Effort::High).unwrap().effort,
        Effort::Default
    );
    assert_eq!(
        nearest(&values, Effort::Budget(1024)).unwrap().effort,
        Effort::High
    );
}

#[test]
fn preview_requires_explicit_activation() {
    let mut picker = Picker::new();
    assert!(!picker.scores.preview);
    picker.preview_scores();
    assert!(picker.scores.preview);
    assert!(picker.scores.sources.is_empty());
    assert!(picker.scores.requested.is_empty());
}
