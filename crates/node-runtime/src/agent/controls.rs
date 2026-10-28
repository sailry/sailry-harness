use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, SessionId, TurnId, WorktreeId};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

struct Active {
    session: SessionId,
    stop: CancellationToken,
    runner: CancellationToken,
    operations: usize,
    media: Option<super::media::Binding>,
    scratch: Option<Arc<tempfile::TempDir>>,
    python: Vec<crate::python::Access>,
    finishing: bool,
    timeline: Arc<super::observation::Timeline>,
    sources: Arc<Mutex<Vec<super::model::observation::Source>>>,
    observations: Vec<Arc<Mutex<super::model::observation::Capture>>>,
    halted: Option<Fault>,
}

struct Child {
    turn: TurnId,
    stop: CancellationToken,
}

#[derive(Default)]
pub(crate) struct Controls {
    pub(super) mcp: super::mcp::Pool,
    pub(crate) wake: Notify,
    pub(crate) stopped: CancellationToken,
    active: Mutex<BTreeMap<TurnId, Active>>,
    children: Mutex<BTreeMap<(SessionId, SessionId), Child>>,
    pub(super) changed: Notify,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) child_observation: Mutex<Option<std::time::Duration>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) approval_timeout: Mutex<Option<std::time::Duration>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) question_timeout: Mutex<Option<std::time::Duration>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) approval_expiry: Arc<Notify>,
}

impl Controls {
    pub(crate) async fn close(&self) -> Result<(), sailry_protocol::Fault> {
        self.mcp.close().await
    }
    pub fn register(&self, turn: TurnId, session: SessionId, stop: CancellationToken) {
        self.active.lock().unwrap().insert(
            turn,
            Active {
                session,
                runner: stop.child_token(),
                stop,
                operations: 0,
                media: None,
                scratch: None,
                python: Vec::new(),
                finishing: false,
                observations: Vec::new(),
                timeline: super::observation::Timeline::new(),
                sources: Default::default(),
                halted: None,
            },
        );
    }

    pub(super) fn sources(
        &self,
        turn: TurnId,
    ) -> Arc<Mutex<Vec<super::model::observation::Source>>> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .map(|active| active.sources.clone())
            .unwrap_or_default()
    }

    pub(super) fn context_source(
        &self,
        turn: TurnId,
        kind: sailry_protocol::conversation::observation::CategoryKind,
        text: &str,
    ) {
        if text.is_empty() {
            return;
        }
        let sources = self.sources(turn);
        let mut sources = sources.lock().unwrap();
        if !sources
            .iter()
            .any(|source| source.kind == kind && source.text == text)
        {
            sources.push(super::model::observation::Source {
                kind,
                text: text.into(),
            });
        }
    }

    pub(super) fn start_timing(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.timeline = super::observation::Timeline::new();
        }
    }

    pub(crate) fn timeline(&self, turn: TurnId) -> Option<Arc<super::observation::Timeline>> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .map(|active| active.timeline.clone())
    }

    pub(super) fn observe_model(
        &self,
        turn: TurnId,
        capture: Arc<Mutex<super::model::observation::Capture>>,
    ) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.observations.push(capture);
        }
    }

    pub(super) fn take_observations(
        &self,
        turn: TurnId,
    ) -> Vec<Arc<Mutex<super::model::observation::Capture>>> {
        self.active
            .lock()
            .unwrap()
            .get_mut(&turn)
            .map(|active| std::mem::take(&mut active.observations))
            .unwrap_or_default()
    }

    pub fn bind_media(&self, turn: TurnId, binding: super::media::Binding) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.media.get_or_insert(binding);
        }
    }

    pub fn media(&self, turn: TurnId) -> Option<super::media::Binding> {
        self.active.lock().unwrap().get(&turn)?.media.clone()
    }

    pub(crate) fn stream(
        &self,
        turn: TurnId,
        session: SessionId,
        publish: impl FnOnce() -> Result<(), Fault>,
    ) -> Result<(), Fault> {
        let runs = self.active.lock().unwrap();
        let Some(active) = runs.get(&turn) else {
            return Ok(());
        };
        if active.session != session {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "stream belongs to another session",
            ));
        }
        if active.finishing
            || active.stop.is_cancelled()
            || active.runner.is_cancelled()
            || self.stopped.is_cancelled()
        {
            return Ok(());
        }
        // Serialize cancellation with publication; the feed checks its active turn again.
        publish()
    }

    pub(crate) fn bind_python(
        &self,
        turn: TurnId,
        ready: crate::python::Ready,
        directory: std::path::PathBuf,
    ) -> Result<(), Fault> {
        let mut runs = self.active.lock().unwrap();
        let active = runs
            .get_mut(&turn)
            .filter(|active| !active.finishing && !active.stop.is_cancelled())
            .ok_or_else(|| Fault::new(ErrorCode::Conflict, "turn is not running"))?;
        // The latest loaded skill selects the command interpreter. Retain all
        // bindings for sandbox access, moving a reloaded skill to the end.
        active.python.retain(|existing| {
            existing.ready.environment != ready.environment || existing.directory != directory
        });
        active
            .python
            .push(crate::python::Access { ready, directory });
        Ok(())
    }

    pub(crate) fn python(&self, turn: TurnId) -> Vec<crate::python::Access> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .map(|active| active.python.clone())
            .unwrap_or_default()
    }

    pub(crate) fn python_stop(&self, turn: TurnId) -> Option<CancellationToken> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .filter(|active| !active.finishing)
            .map(|active| active.stop.clone())
    }

    pub(crate) fn scratch(
        &self,
        turn: TurnId,
        profile: &Path,
        worktree: WorktreeId,
        session: SessionId,
    ) -> Result<Arc<tempfile::TempDir>, Fault> {
        let mut runs = self.active.lock().unwrap();
        let active = runs
            .get_mut(&turn)
            .filter(|active| active.session == session)
            .ok_or_else(|| Fault::new(ErrorCode::Conflict, "turn is not running"))?;
        if let Some(scratch) = &active.scratch {
            return Ok(scratch.clone());
        }
        let directory = tempfile::Builder::new()
            .prefix(&format!("command-{worktree}-{session}-{turn}-"))
            .tempdir_in(crate::profile::temporary(profile)?)
            .map_err(|error| {
                Fault::new(
                    ErrorCode::Unavailable,
                    format!("command temporary storage is unavailable: {error}"),
                )
            })?;
        let scratch = Arc::new(directory);
        active.scratch = Some(scratch.clone());
        Ok(scratch)
    }

    pub fn cancel(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.stop.cancel();
            for ((parent, _), child) in self.children.lock().unwrap().iter() {
                if *parent == active.session {
                    child.stop.cancel();
                }
            }
        }
    }

    pub(super) fn runner(&self, turn: TurnId) -> Option<CancellationToken> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .map(|active| active.runner.clone())
    }

    pub(crate) fn halt(&self, turn: TurnId, error: Fault) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.halted.get_or_insert(error);
            active.runner.cancel();
        }
    }

    pub(super) fn failure(&self, turn: TurnId) -> Option<Fault> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .and_then(|active| active.halted.clone())
    }

    pub(crate) fn running(&self, turn: TurnId) -> bool {
        !self.stopped.is_cancelled()
            && self
                .active
                .lock()
                .unwrap()
                .get(&turn)
                .is_some_and(|active| {
                    !active.finishing
                        && !active.stop.is_cancelled()
                        && !active.runner.is_cancelled()
                })
    }

    pub fn register_child(
        &self,
        parent: SessionId,
        child: SessionId,
        turn: TurnId,
        stop: CancellationToken,
    ) {
        self.children
            .lock()
            .unwrap()
            .insert((parent, child), Child { turn, stop });
        self.changed.notify_waiters();
    }

    pub fn finish_child(&self, parent: SessionId, child: SessionId, turn: TurnId) {
        let mut children = self.children.lock().unwrap();
        // An idle session can be reused after its durable completion, before
        // the previous task's cleanup guard has been released.
        if children
            .get(&(parent, child))
            .is_some_and(|active| active.turn == turn)
        {
            children.remove(&(parent, child));
        }
        self.changed.notify_waiters();
    }

    pub fn has_children(&self, parent: SessionId) -> bool {
        self.children
            .lock()
            .unwrap()
            .keys()
            .any(|(owner, _)| *owner == parent)
    }

    pub fn cancel_children(&self, parent: SessionId) {
        for ((owner, _), child) in self.children.lock().unwrap().iter() {
            if *owner == parent {
                child.stop.cancel();
            }
        }
    }

    pub fn stop(&self) {
        self.stopped.cancel();
        for active in self.active.lock().unwrap().values_mut() {
            active.stop.cancel();
        }
        for child in self.children.lock().unwrap().values() {
            child.stop.cancel();
        }
    }

    pub fn begin_operation(&self, turn: TurnId) -> Option<CancellationToken> {
        let mut runs = self.active.lock().unwrap();
        let active = runs.get_mut(&turn)?;
        if active.finishing {
            return None;
        }
        active.operations += 1;
        Some(active.stop.clone())
    }

    pub fn finish_operation(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.operations -= 1;
        }
        self.changed.notify_waiters();
    }

    pub(super) fn operations(&self, turn: TurnId) -> usize {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .map_or(0, |active| active.operations)
    }

    pub async fn drain(&self, turn: TurnId) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut runs = self.active.lock().unwrap();
                let Some(active) = runs.get_mut(&turn) else {
                    return;
                };
                // Sealing and registration use one lock, so no operation can enter
                // between the empty check and the durable turn completion.
                active.finishing = true;
                if active.operations == 0 {
                    return;
                }
            }
            notified.await;
        }
    }

    pub fn finish(&self, turn: TurnId) {
        self.active.lock().unwrap().remove(&turn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_scoped_operations_without_changing_ownership() {
        let controls = Controls::default();
        let turn = TurnId::new();
        assert_eq!(controls.operations(turn), 0);
        controls.register(turn, SessionId::new(), CancellationToken::new());
        assert!(controls.begin_operation(turn).is_some());
        assert!(controls.begin_operation(turn).is_some());
        assert_eq!(controls.operations(turn), 2);
        assert_eq!(controls.operations(TurnId::new()), 0);
        controls.finish_operation(turn);
        assert_eq!(controls.operations(turn), 1);
        controls.finish_operation(turn);
        assert_eq!(controls.operations(turn), 0);
        controls.finish(turn);
        assert_eq!(controls.operations(turn), 0);
    }

    #[tokio::test]
    async fn streams_only_active_owned_runs() {
        let controls = Controls::default();
        let session = SessionId::new();
        let turn = TurnId::new();
        let calls = std::cell::Cell::new(0);
        let publish = || {
            calls.set(calls.get() + 1);
            Ok(())
        };
        controls.register(turn, session, CancellationToken::new());
        controls.stream(turn, session, publish).unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(
            controls
                .stream(turn, SessionId::new(), publish)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        controls.cancel(turn);
        controls.stream(turn, session, publish).unwrap();
        controls.finish(turn);
        controls.stream(turn, session, publish).unwrap();

        let draining = TurnId::new();
        controls.register(draining, session, CancellationToken::new());
        controls.drain(draining).await;
        controls.stream(draining, session, publish).unwrap();

        let shutdown = TurnId::new();
        controls.register(shutdown, session, CancellationToken::new());
        controls.stop();
        controls.stream(shutdown, session, publish).unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn serializes_publication_with_cancellation() {
        use std::{sync::mpsc, time::Duration};

        let controls = Arc::new(Controls::default());
        let turn = TurnId::new();
        let session = SessionId::new();
        let stop = CancellationToken::new();
        controls.register(turn, session, stop.clone());
        let (entered, entering) = mpsc::channel();
        let (release, releasing) = mpsc::channel();
        let producer = controls.clone();
        let publishing = std::thread::spawn(move || {
            producer
                .stream(turn, session, || {
                    entered.send(()).unwrap();
                    releasing.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(())
                })
                .unwrap();
        });
        entering.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(controls.active.try_lock().is_err());

        let (attempted, attempting) = mpsc::channel();
        let (finished, finishing) = mpsc::channel();
        let cancelling = controls.clone();
        let cancellation = std::thread::spawn(move || {
            attempted.send(()).unwrap();
            cancelling.cancel(turn);
            finished.send(()).unwrap();
        });
        attempting.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            finishing.recv_timeout(Duration::from_millis(20)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(!stop.is_cancelled());
        release.send(()).unwrap();
        publishing.join().unwrap();
        finishing.recv_timeout(Duration::from_secs(2)).unwrap();
        cancellation.join().unwrap();
        assert!(stop.is_cancelled());
        controls
            .stream(turn, session, || panic!("cancelled stream was published"))
            .unwrap();
    }

    #[test]
    fn reuses_scoped_scratch_until_last_lease() {
        let profile = tempfile::tempdir().unwrap();
        let controls = Controls::default();
        let worktree = WorktreeId::new();
        let session = SessionId::new();
        let turn = TurnId::new();
        controls.register(turn, session, CancellationToken::new());
        let first = controls
            .scratch(turn, profile.path(), worktree, session)
            .unwrap();
        let second = controls
            .scratch(turn, profile.path(), worktree, session)
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let path = first.path().to_owned();
        assert_eq!(path.parent(), Some(profile.path().join("tmp").as_path()));
        assert!(
            controls
                .scratch(turn, profile.path(), worktree, SessionId::new())
                .is_err()
        );
        let other = TurnId::new();
        controls.register(other, session, CancellationToken::new());
        let separate = controls
            .scratch(other, profile.path(), worktree, session)
            .unwrap();
        assert_ne!(separate.path(), path);
        controls.finish(turn);
        assert!(
            controls
                .scratch(turn, profile.path(), worktree, session)
                .is_err()
        );
        drop(first);
        assert!(path.exists());
        drop(second);
        assert!(!path.exists());
        let other_path = separate.path().to_owned();
        drop(separate);
        assert!(other_path.exists());
        controls.finish(other);
        assert!(!other_path.exists());
    }

    #[tokio::test]
    async fn seals_operations_before_completion() {
        let controls = Controls::default();
        let turn = TurnId::new();
        controls.register(turn, SessionId::new(), CancellationToken::new());
        let token = controls.begin_operation(turn).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), controls.drain(turn))
                .await
                .is_err()
        );
        assert!(controls.begin_operation(turn).is_none());
        assert!(!token.is_cancelled());
        controls.finish_operation(turn);
        controls.drain(turn).await;
        controls.finish(turn);
        assert!(controls.begin_operation(turn).is_none());
    }

    #[test]
    fn older_cleanup_retains_reused_child() {
        let controls = Controls::default();
        let parent = SessionId::new();
        let child = SessionId::new();
        let previous = TurnId::new();
        let current = TurnId::new();
        let stop = CancellationToken::new();
        controls.register_child(parent, child, previous, CancellationToken::new());
        controls.register_child(parent, child, current, stop.clone());
        controls.finish_child(parent, child, previous);
        assert!(controls.has_children(parent));
        controls.cancel_children(parent);
        assert!(stop.is_cancelled());
        controls.finish_child(parent, child, current);
        assert!(!controls.has_children(parent));
    }

    #[test]
    fn completion_preserves_owned_children() {
        let controls = Controls::default();
        let parent = SessionId::new();
        let previous = TurnId::new();
        let current = TurnId::new();
        let child = CancellationToken::new();
        controls.register(previous, parent, CancellationToken::new());
        controls.register_child(parent, SessionId::new(), TurnId::new(), child.clone());
        assert!(!child.is_cancelled());
        controls.finish(previous);
        controls.register(current, parent, CancellationToken::new());
        controls.cancel(current);
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn drains_operations_without_waiting_for_children() {
        let controls = Controls::default();
        let session = SessionId::new();
        let turn = TurnId::new();
        let child = SessionId::new();
        let child_turn = TurnId::new();
        let child_stop = CancellationToken::new();
        controls.register(turn, session, CancellationToken::new());
        controls.begin_operation(turn).unwrap();
        controls.register_child(session, child, child_turn, child_stop.clone());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), controls.drain(turn))
                .await
                .is_err()
        );
        assert!(controls.begin_operation(turn).is_none());
        assert!(!child_stop.is_cancelled());
        controls.finish_operation(turn);
        tokio::time::timeout(std::time::Duration::from_millis(10), controls.drain(turn))
            .await
            .unwrap();
        controls.finish(turn);
        controls.cancel(turn);
        assert!(controls.has_children(session));
        assert!(!child_stop.is_cancelled());
        controls.finish_child(session, child, child_turn);
        assert!(!controls.has_children(session));
    }

    #[test]
    fn cancellation_keeps_the_same_owner() {
        for shutdown in [false, true] {
            let controls = Controls::default();
            let turn = TurnId::new();
            let token = CancellationToken::new();
            controls.register(turn, SessionId::new(), token.clone());
            let runner = controls.runner(turn).unwrap();
            assert!(controls.running(turn));
            if shutdown {
                controls.stop();
            } else {
                controls.cancel(turn);
            }
            assert!(token.is_cancelled());
            assert!(runner.is_cancelled());
            assert!(!controls.running(turn));
        }
    }

    #[tokio::test]
    async fn retains_changes_registered_before_waiting() {
        let controls = Controls::default();
        let parent = SessionId::new();
        let child = SessionId::new();
        let turn = TurnId::new();
        let stop = CancellationToken::new();
        controls.register_child(parent, child, turn, stop.clone());
        let changed = controls.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        assert!(controls.has_children(parent));
        controls.cancel_children(parent);
        assert!(stop.is_cancelled());
        controls.finish_child(parent, child, turn);
        tokio::time::timeout(std::time::Duration::ZERO, changed)
            .await
            .unwrap();
        assert!(!controls.has_children(parent));
    }
}
