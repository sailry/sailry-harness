use super::*;
use crate::preferences::{MessageDisplay, Preferences};

fn bounds(visual: &mut VisualTestContext, selector: &str) -> Option<Bounds<Pixels>> {
    visual.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

fn completed(view: &View) -> bool {
    view.history.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot
            .page
            .runs
            .iter()
            .any(|run| run.status == Status::Completed)
    })
}

#[gpui::test]
fn resizes_without_resetting_view(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let mut fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::reasoned_tools(vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                serde_json::json!({"command": "printf 'First line\\nSecond line\\nThird line\\n'"}),
            )]))
        });
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }) else {
            panic!("session expected")
        };
        fixture.session = session;
        let path = fixture.directory.path().join("desktop/preferences.json");
        cx.update(|cx| cx.set_global(Preferences::open(path.clone())));
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1000.), px(1200.)));
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Run the fixture tools".into(),
        });
        wait(visual, |cx| completed(view.read(cx)));
        let history = view.read_with(visual, |view, _| view.history.clone());
        let snapshot = history.snapshot.as_ref().unwrap();
        let call = &history.calls[0];
        let turn = call.turn;
        let thought = snapshot
            .page
            .entries
            .iter()
            .find_map(|entry| {
                entry.parts.iter().enumerate().find_map(|(index, part)| {
                    matches!(part, Part::Thinking(_))
                        .then(|| format!("{turn}-{}-{index}", entry.id))
                })
            })
            .unwrap();
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        fixture::tap(visual, &thought);
        fixture::tap(visual, &format!("live-{turn}-tools-{}", call.source.key()));
        let retained = view.update(visual, |view, cx| {
            let values = view.texts.borrow().values().cloned().collect::<Vec<_>>();
            for text in &values {
                text.update(cx, |text, cx| text.select_all(cx));
            }
            (view.expanded.clone(), values)
        });
        let draft = "Keep this unsent draft";
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value(draft, window, cx));
            });
        });
        let mut previous = None;
        for size in crate::preferences::Data::MESSAGE_TEXT_SIZES {
            visual.update(|_, cx| {
                crate::preferences::update(cx, |data| data.message_text_size = Some(size));
            });
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let heights = [
                format!("live-user-text-{turn}"),
                format!("live-turn-text-{turn}"),
                format!("{thought}-content"),
                format!("tool-content-{turn}-{}-0-content", call.source.key()),
            ]
            .map(|selector| bounds(visual, &selector).unwrap().size.height);
            if let Some(before) = previous {
                for (height, before) in heights.into_iter().zip(before) {
                    assert!(
                        height > before,
                        "text size must remeasure conversation content"
                    );
                }
            }
            previous = Some(heights);
            view.read_with(visual, |view, cx| {
                assert_eq!(view.message_text_size, size);
                assert_eq!(view.input.read(cx).value().as_ref(), draft);
                assert_eq!(view.expanded, retained.0);
                assert!(Arc::ptr_eq(
                    view.history.snapshot.as_ref().unwrap(),
                    snapshot
                ));
                for text in &retained.1 {
                    assert!(!text.read(cx).selected_text().is_empty());
                    assert!(view.texts.borrow().values().any(|value| value == text));
                }
            });
        }
        assert_eq!(fixture.task_requests(), 2);
        assert_eq!(Preferences::open(path).data.message_text_size(), 18);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn switches_without_changing_history(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::reasoned_tools(vec![
                (
                    crate::agent_fixture::plugin_tool("files", "read_file"),
                    serde_json::json!({"path": "source.txt"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("files", "read_file"),
                    serde_json::json!({"path": "source.txt"}),
                ),
            ]))
        });
        std::fs::write(
            fixture.directory.path().join("project/source.txt"),
            "Fixture text",
        )
        .unwrap();
        let path = fixture.directory.path().join("desktop/preferences.json");
        cx.update(|cx| cx.set_global(Preferences::open(path.clone())));
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.start();
        wait(visual, |cx| completed(view.read(cx)));
        let history = view.read_with(visual, |view, _| view.history.clone());
        assert_eq!(history.calls.len(), 2);
        let snapshot = history.snapshot.as_ref().unwrap();
        let turn = history.calls[0].turn;
        let thoughts: Vec<_> = snapshot
            .page
            .entries
            .iter()
            .flat_map(|entry| {
                entry
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|&(_, part)| matches!(part, Part::Thinking(_)))
                    .map(|(index, _)| format!("{turn}-{}-{index}", entry.id))
            })
            .collect();
        assert_eq!(thoughts.len(), 3);
        let groups: Vec<_> = history
            .calls
            .iter()
            .map(|call| format!("live-{turn}-tool-group-{}", call.source.key()))
            .collect();
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        for id in thoughts.iter().chain(&groups) {
            assert!(bounds(visual, id).is_some(), "missing detailed row {id}");
        }
        // Both directions invalidate measured rows without a new Node event.
        for mode in [
            MessageDisplay::Compact,
            MessageDisplay::Detailed,
            MessageDisplay::Compact,
        ] {
            visual.update(|_, cx| {
                crate::preferences::update(cx, |data| data.message_display = Some(mode))
            });
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            for id in &thoughts {
                assert_eq!(
                    bounds(visual, id).is_some(),
                    mode == MessageDisplay::Detailed
                );
            }
            assert!(bounds(visual, &groups[0]).is_some());
            assert_eq!(
                bounds(visual, &groups[1]).is_some(),
                mode == MessageDisplay::Detailed
            );
            assert!(bounds(visual, &format!("live-turn-text-{turn}")).is_some());
            view.read_with(visual, |view, _| {
                assert!(Arc::ptr_eq(
                    view.history.snapshot.as_ref().unwrap(),
                    snapshot
                ));
            });
        }
        assert_eq!(fixture.task_requests(), 3);
        assert_eq!(
            Preferences::open(path.clone()).data.message_display,
            Some(MessageDisplay::Compact)
        );
        visual.update(|window, _| window.remove_window());
        cx.update(|cx| cx.set_global(Preferences::open(path)));
        let (restored, visual) =
            fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| completed(restored.read(cx)));
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        for id in &thoughts {
            assert!(bounds(visual, id).is_none());
        }
        assert!(bounds(visual, &groups[0]).is_some());
        assert!(bounds(visual, &groups[1]).is_none());
        assert_eq!(fixture.task_requests(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
