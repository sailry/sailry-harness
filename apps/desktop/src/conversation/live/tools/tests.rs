use super::*;
use crate::conversation::live::tests::{fixture, wait};
use core::prelude::v1::test;
use serde_json::json;

mod commands;
mod status;

fn redraw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn manual_activity_disclosure(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command":"printf first; exit 1"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command":"exit 7"}),
                ),
            ],
        );
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        }) else {
            panic!("session expected")
        };
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session.clone());
        fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Inspect activity".into(),
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .first()
                        .is_some_and(|run| run.status == Status::Completed)
                })
        });
        let completed = view.read_with(visual, |view, _| view.history.clone());
        let page = &completed.snapshot.as_ref().unwrap().page;
        let first = &completed.calls[0];
        let result = first.result(page).unwrap();
        assert_eq!(result["data"]["outcome"]["data"], 1);
        assert_eq!(result["data"]["stdout"]["text"], "first");
        assert_eq!(result["data"]["stderr"]["text"], "");
        assert!(!summary(first, page).detail.failed);
        assert!(summary(first, page).detail.state.is_none());
        let summaries = completed.calls.iter().collect::<Vec<_>>();
        let heading = group_summary(&summaries, page);
        assert!(!heading.detail.failed);
        assert!(heading.detail.state.is_none());
        assert!(!has_content(&completed.calls[1], page));
        assert!(
            completed
                .calls
                .iter()
                .all(|call| call.content.as_ref().is_some_and(|content| {
                    content
                        .blocks
                        .iter()
                        .all(|block| matches!(block, sailry_protocol::tool::Block::Text(_)))
                }))
        );
        let turn = first.turn;
        let group = format!("live-{turn}-tool-group-{}", first.source.key());
        let row = format!("live-{turn}-tools-{}", first.source.key());
        let output = format!("live-tool-result-{turn}-{}", first.source.key());
        let bounds = |visual: &mut VisualTestContext, id: &str| {
            visual.debug_bounds(Box::leak(id.to_owned().into_boxed_str()))
        };
        let mut running = completed.clone();
        Arc::make_mut(&mut Arc::make_mut(running.snapshot.as_mut().unwrap()).page).runs[0].status =
            Status::Running;
        let last = &mut Arc::make_mut(&mut running.calls)[1];
        last.state = State::Running;
        last.response = None;
        view.update(visual, |view, cx| view.update_history(running.clone(), cx));
        redraw(visual);
        assert!(bounds(visual, &group).is_some());
        assert!(bounds(visual, &row).is_none());
        assert!(bounds(visual, &format!("{group}-summary-shimmer")).is_some());
        assert!(bounds(visual, &format!("live-turn-phase-{turn}-turn_generating")).is_none());
        let summaries = [&running.calls[0], &running.calls[1]];
        let heading = group_summary(&summaries, &completed.snapshot.as_ref().unwrap().page);
        assert!(heading.detail.running);
        assert_eq!(
            heading.detail.text,
            summary(
                &running.calls[1],
                &completed.snapshot.as_ref().unwrap().page
            )
            .detail
            .text
        );
        fixture::tap(visual, &group);
        assert!(bounds(visual, &format!("{group}-content")).is_some());
        assert!(bounds(visual, &format!("{group}-scroll")).is_none());
        fixture::tap(visual, &row);
        assert!(bounds(visual, &output).is_some());
        // Returned tools keep activity until the next visible model content arrives.
        let mut gap = running.clone();
        gap.calls = completed.calls.clone();
        Arc::make_mut(&mut Arc::make_mut(gap.snapshot.as_mut().unwrap()).page)
            .entries
            .retain(|entry| {
                entry.author == "user"
                    || !entry
                        .parts
                        .iter()
                        .all(|part| matches!(part, sailry_protocol::conversation::Part::Text(_)))
            });
        view.update(visual, |view, cx| view.update_history(gap, cx));
        redraw(visual);
        assert!(bounds(visual, &format!("{group}-summary-shimmer")).is_some());
        assert!(bounds(visual, &format!("{row}-summary-shimmer")).is_none());
        let last_row = format!("live-{turn}-tools-{}", completed.calls[1].source.key());
        assert!(bounds(visual, &format!("{last_row}-summary-shimmer")).is_some());
        assert!(
            bounds(
                visual,
                &format!("live-turn-phase-{turn}-turn_awaiting_response")
            )
            .is_none()
        );
        assert!(bounds(visual, &output).is_some());
        // A completed tool restores the non-tool phase without changing either manual choice.
        running.calls = completed.calls.clone();
        view.update(visual, |view, cx| view.update_history(running, cx));
        redraw(visual);
        assert!(bounds(visual, &output).is_some());
        assert!(bounds(visual, &format!("{group}-summary-shimmer")).is_none());
        assert!(bounds(visual, &format!("live-turn-phase-{turn}-turn_generating")).is_some());
        fixture::tap(visual, &group);
        view.update(visual, |view, cx| {
            view.update_history(completed.clone(), cx)
        });
        redraw(visual);
        assert!(bounds(visual, &group).is_none());
        assert!(bounds(visual, &format!("live-turn-text-{turn}")).is_some());
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        assert!(bounds(visual, &group).is_some());
        assert!(bounds(visual, &row).is_none());
        fixture::tap(visual, &group);
        assert!(bounds(visual, &output).is_some());
        let second = &completed.calls[1];
        fixture::tap(
            visual,
            &format!("live-{turn}-tools-{}", second.source.key()),
        );
        assert!(
            bounds(
                visual,
                &format!("live-tool-result-{turn}-{}", second.source.key())
            )
            .is_none()
        );
        view.read_with(visual, |view, _| {
            assert!(
                !view
                    .expanded
                    .contains_key(&(turn, format!("tools-{}", second.source.key())))
            );
        });
        let mut empty = completed.clone();
        for entry in
            &mut Arc::make_mut(&mut Arc::make_mut(empty.snapshot.as_mut().unwrap()).page).entries
        {
            for part in &mut entry.parts {
                if let sailry_protocol::conversation::Part::ToolResult { result, .. } = part {
                    result["data"]["stdout"]["text"] = json!("");
                }
            }
        }
        view.update(visual, |view, cx| view.update_history(empty, cx));
        redraw(visual);
        fixture::tap(visual, &group);
        assert!(bounds(visual, &format!("{group}-content")).is_none());
        assert!(bounds(visual, &output).is_none());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui_kit::test]
fn groups_expand_to_content(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        for mixed in [false, true] {
            let fixture = fixture::Fixture::with_tools(
                remote,
                (0..14)
                    .map(|index| {
                        if mixed && index % 2 == 1 {
                            (
                                crate::agent_fixture::plugin_tool("files", "list_directory"),
                                json!({"path":"."}),
                            )
                        } else {
                            (
                                crate::agent_fixture::plugin_tool("commands", "run_command"),
                                json!({"command":"printf ready"}),
                            )
                        }
                    })
                    .collect(),
            );
            let mut config = fixture.session.config.clone();
            config.permission = sailry_protocol::Permission::Full;
            let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: 1,
                config,
            }) else {
                panic!("session expected")
            };
            let (view, visual) = fixture::open(cx, fixture.binding.clone(), session.clone());
            fixture.execute(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Inspect tool groups".into(),
            });
            wait(visual, |cx| {
                view.read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| {
                        snapshot
                            .page
                            .runs
                            .first()
                            .is_some_and(|run| run.status == Status::Completed)
                    })
            });
            let (turn, key) = view.read_with(visual, |view, _| {
                assert_eq!(view.history.calls.len(), 14);
                let first = &view.history.calls[0];
                (
                    first.turn,
                    format!(
                        "tool-{}-{}",
                        if mixed { "sequence" } else { "group" },
                        first.source.key()
                    ),
                )
            });
            fixture::tap(visual, &format!("live-turn-work-{turn}"));
            fixture::tap(visual, &format!("live-{turn}-{key}"));
            let selector: &'static str =
                Box::leak(format!("live-{turn}-{key}-content").into_boxed_str());
            let expanded = visual.debug_bounds(selector).unwrap();
            assert!(expanded.size.height > px(320.));
            let handle = visual.update(|window, _| window.window_handle());
            for height in [480., 900.] {
                visual.simulate_window_resize(handle, size(px(1000.), px(height)));
                redraw(visual);
                assert_eq!(
                    visual.debug_bounds(selector).unwrap().size.height,
                    expanded.size.height
                );
            }
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
