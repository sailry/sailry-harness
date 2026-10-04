use super::*;
use sailry_protocol::conversation::{Page, RunKind};

#[path = "manual_compaction/receipt.rs"]
mod receipt;

fn page(view: &Entity<View>, cx: &App) -> Arc<Page> {
    view.read(cx)
        .history
        .snapshot
        .as_ref()
        .unwrap()
        .page
        .clone()
}

fn finished(view: &Entity<View>, cx: &mut VisualTestContext, count: usize) {
    wait(cx, |cx| {
        let view = view.read(cx);
        !view.pending
            && view.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.page.runs.len() == count
                    && snapshot.page.runs.iter().all(|run| {
                        !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                    })
            })
    });
}

fn request(view: &Entity<View>, visual: &mut VisualTestContext) {
    // Slash commands start the draft; remove only the separator inserted by this helper.
    visual.update(|window, cx| view.update(cx, |view, cx| view.focus(window, cx)));
    visual.simulate_keystrokes("cmd-up");
    visual.simulate_input("/compact ");
    visual.simulate_keystrokes("left");
    wait(visual, |_| true);
    assert!(visual.debug_bounds("live-reference-picker").is_some());
    visual.simulate_keystrokes("enter right backspace cmd-down");
}

#[gpui::test]
fn restores_summary_and_keeps_draft(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::compaction(false))
        });
        std::fs::write(
            fixture.directory.path().join("project/source.txt"),
            "Verified evidence 中文 🙂",
        )
        .unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input(&"Original constraint ".repeat(100));
        click(visual, "live-chat-send");
        finished(&view, visual, 1);
        let before = view.read_with(visual, |_, cx| page(&view, cx));
        click(visual, "live-chat-input");
        visual.simulate_input("Keep draft 中文 🙂");
        request(&view, visual);
        visual.simulate_input(" after compaction");
        finished(&view, visual, 2);
        let compacted = view.read_with(visual, |view, cx| {
            assert_eq!(
                view.input.read(cx).value(),
                "Keep draft 中文 🙂 after compaction"
            );
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(&page.entries[..before.entries.len()], before.entries);
            let run = page.runs.last().unwrap();
            assert_eq!(run.kind, RunKind::Compaction);
            assert_eq!(
                super::super::compaction::status(run, page),
                "chat_compacted"
            );
            assert_eq!(view.history.snapshot.as_ref().unwrap().statistics.turns, 1);
            assert_eq!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .statistics
                    .context_tokens,
                None
            );
            page.clone()
        });
        let turn = compacted.runs.last().unwrap().turn;
        let selector = |part: &str| {
            Box::leak(format!("live-turn-{part}-{turn}").into_boxed_str()) as &'static str
        };
        assert!(visual.debug_bounds(selector("footer")).is_none());
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-status-{turn}-chat_compacted").into_boxed_str()
                ))
                .is_none()
        );
        let row = visual
            .debug_bounds(Box::leak(format!("live-turn-{turn}").into_boxed_str()))
            .unwrap();
        assert!(row.size.height <= px(40.), "compaction is one activity row");
        request(&view, visual);
        finished(&view, visual, 3);
        let noop = view.read_with(visual, |_, cx| page(&view, cx));
        assert_eq!(noop.entries, compacted.entries);
        assert_eq!(
            super::super::compaction::status(noop.runs.last().unwrap(), &noop),
            "chat_compact_unneeded"
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(view.read_with(visual, |_, cx| page(&view, cx)), noop);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn failure_preserves_input(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::compaction(true))
        });
        std::fs::write(
            fixture.directory.path().join("project/source.txt"),
            "Evidence",
        )
        .unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input(&"Original constraint ".repeat(100));
        click(visual, "live-chat-send");
        finished(&view, visual, 1);
        let before = view.read_with(visual, |_, cx| page(&view, cx));
        click(visual, "live-chat-input");
        visual.simulate_input("Retained draft");
        request(&view, visual);
        finished(&view, visual, 2);
        view.read_with(visual, |view, cx| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.entries, before.entries);
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed);
            assert_eq!(
                super::super::compaction::status(run, page),
                "chat_compact_failed"
            );
            assert_eq!(view.input.read(cx).value(), "Retained draft");
            assert!(view.can_compact());
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn queue_lifecycle(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::start(true))
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input(&"Pending constraint ".repeat(100));
        click(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx).active().is_some() && !view.read(cx).pending
        });
        for remove in [true, false] {
            request(&view, visual);
            wait(visual, |cx| {
                page(&view, cx).queue.items.len() == 1 && !view.read(cx).pending
            });
            let pending = view.read_with(visual, |_, cx| page(&view, cx).queue.items[0].clone());
            assert_eq!(pending.kind, RunKind::Compaction);
            assert!(!view.read_with(visual, |view, _| view.can_compact()));
            click(visual, "live-queue");
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("queue-edit-{}", pending.turn).into_boxed_str()
                    ))
                    .is_none()
            );
            if remove {
                fixture::tap(visual, &format!("queue-delete-{}", pending.turn));
                wait(visual, |cx| page(&view, cx).queue.items.is_empty());
                visual.simulate_keystrokes("escape");
            } else {
                fixture::tap(visual, &format!("queue-send-{}", pending.turn));
                wait(visual, |cx| {
                    page(&view, cx)
                        .runs
                        .iter()
                        .any(|run| run.turn == pending.turn && run.status == Status::Running)
                });
                visual.simulate_keystrokes("escape");
                click(visual, "live-chat-send");
                finished(&view, visual, 3);
                view.read_with(visual, |view, _| {
                    let snapshot = view.history.snapshot.as_ref().unwrap();
                    let run = snapshot.page.runs.last().unwrap();
                    assert_eq!(run.status, Status::Cancelled);
                    assert_eq!(
                        super::super::compaction::status(run, &snapshot.page),
                        "chat_compact_cancelled"
                    );
                    assert!(view.rows.contains(&run.turn));
                    assert!(
                        snapshot
                            .page
                            .entries
                            .iter()
                            .all(|entry| entry.turn != run.turn)
                    );
                });
            }
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
