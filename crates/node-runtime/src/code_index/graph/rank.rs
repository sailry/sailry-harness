//! Personalized PageRank over weighted file references, after Aider's
//! repository map. Fixed iteration order keeps equal inputs bit-identical.
const DAMPING: f64 = 0.85;
const ROUNDS: usize = 100;
const TOLERANCE: f64 = 1e-10;

/// Ranks for `nodes` summing to one. Edges are `(from, to, weight)` with
/// positive weights; `focus` restricts teleports, or all nodes when empty.
pub(super) fn pagerank(nodes: usize, edges: &[(usize, usize, f64)], focus: &[usize]) -> Vec<f64> {
    if nodes == 0 {
        return Vec::new();
    }
    let mut teleport = vec![0.0; nodes];
    let focus: Vec<usize> = focus.iter().copied().filter(|node| *node < nodes).collect();
    if focus.is_empty() {
        teleport.fill(1.0 / nodes as f64);
    } else {
        for node in &focus {
            teleport[*node] += 1.0 / focus.len() as f64;
        }
    }
    let mut outgoing = vec![0.0; nodes];
    for &(from, _, weight) in edges {
        outgoing[from] += weight;
    }
    let mut rank = teleport.clone();
    for _ in 0..ROUNDS {
        let dangling: f64 = rank
            .iter()
            .zip(&outgoing)
            .filter(|(_, total)| **total <= 0.0)
            .map(|(value, _)| value)
            .sum();
        let mut next: Vec<f64> = teleport
            .iter()
            .map(|share| (1.0 - DAMPING + DAMPING * dangling) * share)
            .collect();
        for &(from, to, weight) in edges {
            next[to] += DAMPING * rank[from] * weight / outgoing[from];
        }
        let delta: f64 = next
            .iter()
            .zip(&rank)
            .map(|(new, old)| (new - old).abs())
            .sum();
        rank = next;
        if delta < TOLERANCE {
            break;
        }
    }
    rank
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favors_referenced_hubs_deterministically() {
        let edges = [
            (0, 2, 1.0),
            (1, 2, 2.0),
            (3, 2, 1.0),
            (2, 0, 0.5),
            (3, 1, 1.0),
        ];
        let first = pagerank(4, &edges, &[]);
        let second = pagerank(4, &edges, &[]);
        assert_eq!(
            first
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
        assert!((first.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        let top = (0..4).max_by(|left, right| first[*left].total_cmp(&first[*right]));
        assert_eq!(top, Some(2));
    }

    #[test]
    fn personalizes_teleports_with_focus() {
        let edges = [(0, 1, 1.0), (2, 3, 1.0)];
        let neutral = pagerank(4, &edges, &[]);
        let focused = pagerank(4, &edges, &[2]);
        assert!(focused[3] > neutral[3]);
        assert!(focused[1] < neutral[1]);
        assert!((focused.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(pagerank(0, &[], &[]).is_empty());
    }
}
