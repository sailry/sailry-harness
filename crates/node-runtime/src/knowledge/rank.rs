//! Fuse already-filtered candidate ranks without mixing incomparable raw scores.
use std::collections::BTreeMap;

const OFFSET: f32 = 60.;

pub(super) fn fuse(keyword: &[String], semantic: &[String]) -> Vec<(String, f32)> {
    let mut scores = BTreeMap::<String, f32>::new();
    for ranks in [keyword, semantic] {
        let mut seen = std::collections::BTreeSet::new();
        for (rank, key) in ranks.iter().enumerate() {
            if seen.insert(key) {
                *scores.entry(key.clone()).or_default() += 1. / (OFFSET + rank as f32 + 1.);
            }
        }
    }
    let mut ranks = scores.into_iter().collect::<Vec<_>>();
    ranks.sort_unstable_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewards_agreement_and_keeps_stable_ties() {
        let ranked = fuse(
            &["a".into(), "shared".into()],
            &["b".into(), "shared".into()],
        );
        assert_eq!(
            ranked
                .iter()
                .map(|entry| entry.0.as_str())
                .collect::<Vec<_>>(),
            ["shared", "a", "b"]
        );
        assert!((ranked[0].1 - 2. / 62.).abs() < f32::EPSILON);
    }

    #[test]
    fn counts_each_candidate_once_per_source() {
        let ranked = fuse(&["a".into(), "a".into()], &[]);
        assert_eq!(ranked, [("a".into(), 1. / 61.)]);
    }
}
