//! Recently viewed inventories reopen instantly while Node refreshes them.
use sailry_protocol::{NodeId, SessionId, conversation::assets::Group};
use std::collections::VecDeque;

const CAPACITY: usize = 16;

/// Groups read for one structural history revision, newest first.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Loaded {
    pub revision: u64,
    /// Newest history sequence the client had when the request started.
    pub head: Option<u64>,
    pub groups: Vec<Group>,
    pub before: Option<u64>,
}

impl Loaded {
    /// Appended events keep earlier groups valid; a structural change does not.
    pub fn reusable(&self, stamp: Option<(u64, u64)>) -> bool {
        stamp.is_some_and(|(revision, _)| revision == self.revision)
    }

    pub fn current(&self, stamp: Option<(u64, u64)>) -> bool {
        stamp.is_some_and(|(revision, head)| revision == self.revision && self.head == Some(head))
    }
}

/// Inventories of sessions other than the displayed one, most recent first.
#[derive(Default)]
pub(super) struct Cache {
    entries: VecDeque<(NodeId, SessionId, Loaded)>,
}

impl Cache {
    pub fn put(&mut self, target: NodeId, session: SessionId, loaded: Loaded) {
        self.take(target, session);
        self.entries.push_front((target, session, loaded));
        self.entries.truncate(CAPACITY);
    }

    pub fn take(&mut self, target: NodeId, session: SessionId) -> Option<Loaded> {
        let index = self
            .entries
            .iter()
            .position(|(node, id, _)| *node == target && *id == session)?;
        self.entries.remove(index).map(|(_, _, loaded)| loaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(revision: u64, head: u64) -> Loaded {
        Loaded {
            revision,
            head: Some(head),
            groups: Vec::new(),
            before: Some(head),
        }
    }

    #[test]
    fn reuses_appended_history() {
        let value = loaded(3, 10);
        assert!(value.current(Some((3, 10))));
        assert!(value.reusable(Some((3, 12))));
        assert!(!value.current(Some((3, 12))));
        assert!(!value.reusable(Some((4, 10))));
        assert!(!value.reusable(None));
        assert!(!value.current(None));
    }

    #[test]
    fn scopes_entries_by_node_and_session() {
        let mut cache = Cache::default();
        let session = SessionId::new();
        let local = NodeId([1; 32]);
        cache.put(local, session, loaded(1, 5));
        assert_eq!(cache.take(NodeId([2; 32]), session), None);
        assert_eq!(cache.take(local, SessionId::new()), None);
        assert_eq!(cache.take(local, session), Some(loaded(1, 5)));
        assert_eq!(cache.take(local, session), None);
    }

    #[test]
    fn replaces_and_evicts_oldest() {
        let mut cache = Cache::default();
        let node = NodeId([1; 32]);
        let first = SessionId::new();
        cache.put(node, first, loaded(1, 1));
        cache.put(node, first, loaded(1, 2));
        let others: Vec<_> = (0..CAPACITY - 1).map(|_| SessionId::new()).collect();
        for session in &others {
            cache.put(node, *session, loaded(1, 1));
        }
        assert_eq!(cache.take(node, first), Some(loaded(1, 2)));
        cache.put(node, first, loaded(1, 3));
        cache.put(node, SessionId::new(), loaded(1, 1));
        assert_eq!(cache.take(node, others[0]), None);
        assert_eq!(cache.take(node, first), Some(loaded(1, 3)));
    }
}
