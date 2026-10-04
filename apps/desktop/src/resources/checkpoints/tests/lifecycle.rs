use super::*;
use crate::live::FileTransport;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn observed(inner: Arc<dyn sailry_link::Transport>) -> Arc<FileTransport> {
    Arc::new(FileTransport {
        inner,
        requests: Mutex::new(Vec::new()),
        hold: AtomicBool::new(false),
        drop_next: AtomicBool::new(false),
        ready: Arc::new(tokio::sync::Semaphore::new(0)),
    })
}

#[gpui::test]
fn retries_original_receipt(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote, 1);
        let mut transport = None;
        let (shell, visual) = fixture.mount_with(cx, |inner| {
            let next = observed(inner);
            transport = Some(next.clone());
            next
        });
        let transport = transport.unwrap();
        let root = fixture.chat.directory.path().join("project");
        let state = restore(visual, &fixture, &shell);
        // Load metadata separately so the dropped receipt is the mutation response.
        start(visual, &state, Some("not-a-changed-file"));
        transport.drop_next.store(true, Ordering::SeqCst);
        start(visual, &state, Some("file-0.txt"));
        assert_eq!(
            state.read_with(visual, |state, _| state.issue),
            Some("checkpoint_unknown")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            before(0)
        );
        let request = transport.requests.lock().unwrap().last().unwrap().clone();
        std::fs::write(root.join("file-0.txt"), "Replacement survives retry").unwrap();
        crate::feedback::tests::settle(visual);
        tap(visual, "turn-changes-retry");
        wait(visual, |cx| !state.read(cx).pending);
        assert!(state.read_with(visual, |state, _| state.restored(Some("file-0.txt"))));
        assert_eq!(transport.requests.lock().unwrap().last().unwrap(), &request);
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            "Replacement survives retry"
        );
        let state = restore(visual, &fixture, &shell);
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.session_scope.active = crate::shell::session_scope::Key::Draft;
                cx.notify();
            })
        });
        start(visual, &state, Some("file-0.txt"));
        assert_eq!(
            state.read_with(visual, |state, _| state.issue),
            Some("files_context_changed")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            "Replacement survives retry"
        );
        fixture.close();
    }
}
