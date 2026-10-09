use super::*;
use sailry_protocol::{RequestId, RequestOutcome};

#[gpui::test]
#[cfg(unix)]
fn retries_original_request_after_closing_dialog(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for stage in [false, true] {
            let fixture = versioned(remote, 1);
            let root = fixture.chat.directory.path().join("project");
            let mut transport = None;
            let (_, visual) = fixture.mount_with(cx, |inner| {
                let next = observed(inner);
                transport = Some(next.clone());
                next
            });
            let transport = transport.unwrap();
            let client = sailry_client::Client::new(transport.inner.clone());
            open(visual, &fixture, 0);
            transport.drop_next.store(true, Ordering::SeqCst);
            tap(
                visual,
                if stage {
                    "turn-hunks-stage-0"
                } else {
                    "turn-hunks-reject-0"
                },
            );
            element(visual, "turn-hunks-retry");
            let request = transport.requests.lock().unwrap().last().unwrap().clone();
            assert!(
                matches!(&request.command, Command::ApplyTurnHunk(selection) if selection.operation == if stage { hunk::Operation::Stage } else { hunk::Operation::Reject })
            );
            let outcome = fixture.chat.runtime.block_on(async {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
                loop {
                    let outcome = client.outcome(&request).await.unwrap();
                    if !matches!(
                        outcome,
                        RequestOutcome::NotAdmitted | RequestOutcome::Admitted
                    ) {
                        break outcome;
                    }
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "hunk completion deadline: remote={remote}, stage={stage}, {outcome:?}"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });
            assert!(
                matches!(outcome, RequestOutcome::Completed(_)),
                "remote={remote}, stage={stage}, {outcome:?}"
            );
            let expected = if stage { after(0) } else { before(0) };
            assert_eq!(
                std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
                expected
            );
            let bytes = std::fs::read(root.join(".git/index")).unwrap();
            close(visual);
            std::fs::write(
                root.join("file-0.txt"),
                "Replacement survives original-ID retry\n",
            )
            .unwrap();
            open(visual, &fixture, 0);
            element(visual, "turn-hunks-retry");
            tap(visual, "turn-hunks-retry");
            missing(visual, "turn-hunks-retry");
            let requests = transport.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0], request);
            assert_eq!(requests[1], request);
            drop(requests);
            assert_eq!(
                std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
                "Replacement survives original-ID retry\n"
            );
            assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), bytes);
            assert_eq!(
                indexed(&fixture, "file-0.txt"),
                if stage { after(0) } else { before(0) }
            );
            close(visual);
            fixture.close();
        }
    }
}

#[gpui::test]
fn releases_editor_guard_after_owner_is_dropped(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote, 1);
        let root = fixture.chat.directory.path().join("project");
        let mut transport = None;
        let (shell, visual) = fixture.mount_with(cx, |inner| {
            let next = observed(inner);
            transport = Some(next.clone());
            next
        });
        let transport = transport.unwrap();
        let client = sailry_client::Client::new(transport.inner.clone());
        let state = visual.update(|window, cx| {
            let source = shell.read(cx).current_chat().unwrap().clone();
            let binding = source.read(cx).binding();
            let state = cx.new(|_| {
                Review::new(
                    source.downgrade(),
                    binding,
                    fixture.chat.session.id,
                    fixture.turn,
                    "file-0.txt".into(),
                )
            });
            state.update(cx, |state, cx| state.open(window, cx));
            state
        });
        element(visual, "turn-hunks-reject-0");
        transport.hold.store(true, Ordering::SeqCst);
        tap(visual, "turn-hunks-reject-0");
        wait(visual, |_| {
            std::fs::read_to_string(root.join("file-0.txt")).unwrap() == before(0)
        });
        let request = transport.requests.lock().unwrap().last().unwrap().clone();
        let scope = (fixture.chat.node.id(), fixture.chat.session.worktree);
        let probe = RequestId::new();
        assert_eq!(
            visual.update(|_, cx| crate::plugins::file_transfers::recovery::reserve(
                scope,
                probe,
                "file-0.txt",
                cx
            )),
            Err("files_operation_busy")
        );
        close(visual);
        let weak = state.downgrade();
        drop(state);
        wait(visual, |_| weak.upgrade().is_none());
        transport.ready.add_permits(1);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            wait(visual, |_| true);
            if visual.update(|_, cx| {
                let free = crate::plugins::file_transfers::recovery::reserve(
                    scope,
                    probe,
                    "file-0.txt",
                    cx,
                )
                .is_ok();
                if free {
                    crate::plugins::file_transfers::recovery::release(probe, false, cx);
                }
                free
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "hunk cleanup deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(
            fixture
                .chat
                .runtime
                .block_on(client.outcome(&request))
                .unwrap(),
            RequestOutcome::Completed(_)
        ));
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            before(0)
        );
        fixture.close();
    }
}
