use super::*;

fn settle(visual: &mut VisualTestContext) {
    for _ in 0..8 {
        visual.executor().advance_clock(Duration::from_millis(100));
        wait(visual, |_| true);
    }
}

#[gpui::test]
fn restores_markers(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let summary = (1..=48).map(|index| format!("Record {index}: preserve the verified result and pending constraints 中文 🙂\n\n")).collect::<String>();
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::compaction_usage(
                false,
                summary.clone(),
                14_000,
            ))
        });
        std::fs::write(
            fixture.directory.path().join("project/source.txt"),
            "Original evidence 中文 🙂\n".repeat(1000),
        )
        .unwrap();
        for text in ["Read the source evidence", "Recent context", "Current task"] {
            let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: text.into(),
            }) else {
                panic!("turn expected")
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let page = fixture
                    .runtime
                    .block_on(
                        fixture
                            .binding
                            .client
                            .read_conversation(fixture.session.id, None, 1),
                    )
                    .unwrap()
                    .page;
                if page
                    .runs
                    .iter()
                    .any(|run| run.turn == turn.id && run.status == Status::Completed)
                {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        for _ in 0..2 {
            let (view, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| {
                view.read(cx).connected() && view.read(cx).history.snapshot.is_some()
            });
            let (turn, key) = view.read_with(visual, |view, _| {
                let entry = view
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .find(|entry| entry.parts == [Part::Compaction(summary.clone())])
                    .unwrap();
                (entry.turn, format!("{}-0", entry.id))
            });
            click(visual, "live-chat-input");
            visual.simulate_input("Keep the draft 中文 🙂");
            view.update(visual, |view, cx| {
                view.scroller
                    .update(cx, |scroller, cx| scroller.scroll_to_item(2, cx))
            });
            settle(visual);
            fixture::tap(visual, &format!("live-turn-work-{turn}"));
            settle(visual);
            assert!(!view.read_with(visual, |view, _| {
                view.expanded
                    .get(&(turn, key.clone()))
                    .copied()
                    .unwrap_or(false)
            }));
            fixture::tap(visual, &format!("live-context-{turn}-{key}"));
            settle(visual);
            assert!(!view.read_with(visual, |view, _| {
                view.expanded.contains_key(&(turn, key.clone()))
            }));
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("live-context-body-{turn}-{key}").into_boxed_str(),
                    ))
                    .is_none()
            );
            assert_eq!(
                view.read_with(visual, |view, cx| view.input.read(cx).value()),
                "Keep the draft 中文 🙂"
            );
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 5);
            visual.update(|window, _| window.remove_window());
        }
        fixture.close();
    }
}
