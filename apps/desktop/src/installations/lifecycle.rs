use super::*;
use std::collections::BTreeSet;

/// Reconcile only a successful host inventory, including the local Node.
pub(crate) fn reconcile(nodes: impl IntoIterator<Item = NodeId>, cx: &mut App) {
    registry(cx).update(cx, |queue, cx| {
        queue.known = Some(nodes.into_iter().collect());
        queue.changed(cx);
    });
}

impl Queue {
    pub(super) fn prune(&mut self) {
        self.sources
            .retain(|node, _| needed(*node, self.known.as_ref(), &self.entries));
    }
}

fn needed(node: NodeId, known: Option<&BTreeSet<NodeId>>, entries: &BTreeMap<Key, Entry>) -> bool {
    // Retained rows still need their original client for completion, stable-ID
    // recovery or dismissal. A disconnected host is not a removed host.
    known.is_none_or(|known| known.contains(&node))
        || entries
            .keys()
            .any(|(location, _)| *location == Location::Node(node))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn retains_registered_and_unresolved_sources() {
        let node = NodeId([1; 32]);
        let other = NodeId([2; 32]);
        let known = BTreeSet::from([node]);
        let entries = BTreeMap::new();
        assert!(needed(node, None, &entries));
        assert!(needed(node, Some(&known), &entries));
        assert!(!needed(other, Some(&known), &entries));
    }

    #[test]
    fn retains_rows_until_completion_or_dismissal() {
        let node = NodeId([1; 32]);
        let known = BTreeSet::new();
        let id = RequestId::new();
        for status in [
            Presentation::Waiting,
            Presentation::Starting,
            Presentation::Observed(Status::Queued),
            Presentation::Observed(Status::Running {
                phase: sailry_protocol::installation::Phase::Inspecting,
                progress: None,
            }),
            Presentation::Observed(Status::Failed {
                error: Fault::invalid("fixture"),
            }),
            Presentation::Observed(Status::Uncertain {
                error: Fault::new(ErrorCode::OutcomeUnknown, "fixture"),
            }),
        ] {
            let finished = status.terminal();
            let mut entries = BTreeMap::from([(
                (Location::Node(node), id),
                Entry {
                    order: 1,
                    resource: Resource::Plugin {
                        name: "fixture".into(),
                    },
                    status,
                    retry: None,
                    finished,
                    connected: false,
                    dismissing: false,
                },
            )]);
            assert!(needed(node, Some(&known), &entries));
            entries.remove(&(Location::Node(node), id));
            assert!(!needed(node, Some(&known), &entries));
        }
    }
}
