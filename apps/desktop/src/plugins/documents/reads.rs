use sailry_link::CancellationToken;
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

#[derive(Default)]
pub(super) struct Reads {
    sequence: u64,
    entries: BTreeMap<String, (u64, Weak<CancellationToken>)>,
}

impl Reads {
    pub(super) fn start(&mut self, path: String, stop: Arc<CancellationToken>) -> u64 {
        self.entries.retain(|_, (_, stop)| stop.upgrade().is_some());
        self.invalidate(&path);
        self.sequence += 1;
        self.entries
            .insert(path, (self.sequence, Arc::downgrade(&stop)));
        self.sequence
    }

    pub(super) fn current(&self, path: &str, sequence: u64) -> bool {
        self.entries.get(path).is_some_and(|(current, stop)| {
            *current == sequence && stop.upgrade().is_some_and(|stop| !stop.is_cancelled())
        })
    }

    pub(super) fn invalidate(&mut self, path: &str) {
        if let Some((_, stop)) = self.entries.remove(path)
            && let Some(stop) = stop.upgrade()
        {
            stop.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::documents::Opening;
    use sailry_protocol::{ErrorCode, Fault};

    #[test]
    fn superseded_read_preserves_successor() {
        let scope = CancellationToken::new();
        let previous = Arc::new(scope.child_token());
        let current = Arc::new(scope.child_token());
        let mut reads = Reads::default();
        let old_sequence = reads.start("file.rs".into(), previous.clone());
        let sequence = reads.start("file.rs".into(), current.clone());

        assert!(previous.is_cancelled());
        assert!(!scope.is_cancelled());
        assert!(!reads.current("file.rs", old_sequence));
        assert!(reads.current("file.rs", sequence));
        assert!(!current.is_cancelled());
    }

    #[test]
    fn closing_invalidates_only_its_read() {
        let mut reads = Reads::default();
        let closing = Arc::new(CancellationToken::new());
        let remaining = Arc::new(CancellationToken::new());
        let closed_sequence = reads.start("closing.rs".into(), closing.clone());
        let sequence = reads.start("remaining.rs".into(), remaining.clone());

        reads.invalidate("closing.rs");

        assert!(closing.is_cancelled());
        assert!(!reads.current("closing.rs", closed_sequence));
        assert!(reads.current("remaining.rs", sequence));
        assert!(!remaining.is_cancelled());
    }

    #[tokio::test]
    async fn current_failure_keeps_feedback_active() {
        for code in [
            ErrorCode::NotFound,
            ErrorCode::Unavailable,
            ErrorCode::OutcomeUnknown,
        ] {
            let mut reads = Reads::default();
            let stop = Arc::new(CancellationToken::new());
            let sequence = reads.start("missing.rs".into(), stop.clone());
            let (reply, receive) = tokio::sync::oneshot::channel();
            reply.send(Err(Fault::new(code, "Read failed"))).unwrap();

            assert!(reads.current("missing.rs", sequence));
            let fault = Opening {
                receive,
                stop: stop.clone(),
            }
            .read()
            .await
            .unwrap_err();
            assert_eq!(fault.code, code);
            assert_eq!(fault.message, "Read failed");
            assert!(!stop.is_cancelled());
        }
    }

    #[test]
    fn closing_scope_invalidates_completion() {
        let scope = CancellationToken::new();
        let stop = Arc::new(scope.child_token());
        let mut reads = Reads::default();
        let sequence = reads.start("file.rs".into(), stop.clone());

        scope.cancel();

        assert!(!reads.current("file.rs", sequence));
        assert!(stop.is_cancelled());
    }

    #[tokio::test]
    async fn queued_completion_can_be_invalidated() {
        for replace in [false, true] {
            let mut reads = Reads::default();
            let stop = Arc::new(CancellationToken::new());
            let sequence = reads.start("file.rs".into(), stop.clone());
            let (reply, receive) = tokio::sync::oneshot::channel();
            reply
                .send(Err(Fault::new(ErrorCode::Unavailable, "Read failed")))
                .unwrap();
            assert!(reads.current("file.rs", sequence));

            let successor = Arc::new(CancellationToken::new());
            if replace {
                reads.start("file.rs".into(), successor.clone());
            } else {
                reads.invalidate("file.rs");
            }

            assert!(stop.is_cancelled());
            assert!(!reads.current("file.rs", sequence));
            assert!(!successor.is_cancelled());
            let fault = Opening { receive, stop }.read().await.unwrap_err();
            assert_eq!(fault.code, ErrorCode::Cancelled);
        }
    }
}
