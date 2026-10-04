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
