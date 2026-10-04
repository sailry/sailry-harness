use super::*;
use crate::conversation::live::tests::{fixture, wait};
use core::prelude::v1::test;

#[test]
fn keeps_final_prose_outside_work() {
    let blocks = [
        Block::Text("intro".into(), "Before".into()),
        Block::Thinking("reason".into(), "Reason".into()),
        Block::Tools(vec![]),
        Block::Text("progress".into(), "Progress update".into()),
        Block::Tools(vec![]),
        Block::Text("answer".into(), "After".into()),
    ];
    assert_eq!(answer_from(&blocks), 5);
    assert_eq!(answer_from(&blocks[5..]), 0);
}

fn stage(view: &Entity<View>, visual: &mut VisualTestContext, status: Status) {
    view.update(visual, |view, cx| {
        let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
        let page = Arc::make_mut(&mut snapshot.page);
        page.runs[0].status = status;
        if let Some(Part::Text(text)) = page
            .entries
            .iter_mut()
            .rev()
            .filter(|entry| entry.author != "user")
            .flat_map(|entry| entry.parts.iter_mut().rev())
            .find(|part| matches!(part, Part::Text(_)))
        {
            text.push_str(" next token");
        }
        view.scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(0..view.rows.len(), cx)
        });
        cx.notify();
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.run_until_parked();
}

fn header(visual: &mut VisualTestContext, turn: TurnId, status: Status) {
    let state = crate::conversation::live::subagents::status_key(status);
    let header = visual
        .debug_bounds(Box::leak(
            format!("live-turn-status-{turn}-{state}").into_boxed_str(),
        ))
        .unwrap();
    let trigger = visual
        .debug_bounds(Box::leak(format!("live-turn-work-{turn}").into_boxed_str()))
        .unwrap();
    let chevron = visual
        .debug_bounds(Box::leak(
            format!("live-turn-work-{turn}-chevron").into_boxed_str(),
        ))
        .unwrap();
    assert!(header.contains(&trigger.center()));
    assert_eq!(trigger.left(), header.left());
    assert_eq!(trigger.right(), header.right());
    assert!(trigger.contains(&chevron.center()));
    assert!(chevron.center().x > trigger.center().x);
    assert!(trigger.right() - chevron.right() <= px(8.));
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("live-turn-work-{turn}-summary").into_boxed_str(),
            ))
            .is_none(),
        "the former standalone work title must not remain below the status header"
    );
}

#[gpui_kit::test]
fn preserves_header_choice(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("files", "list_directory"),
                serde_json::json!({"path": ""}),
            )],
        );
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        fixture.start();
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
        let (turn, tool) = view.read_with(visual, |view, _| {
            let call = &view.history.calls[0];
            (
                call.turn,
                format!("live-{}-tools-{}", call.turn, call.source.key()),
            )
        });
        let work = format!("live-turn-work-{turn}");
        let tool: &'static str = Box::leak(tool.into_boxed_str());
        let answer: &'static str = Box::leak(format!("live-turn-text-{turn}").into_boxed_str());
        assert!(visual.debug_bounds(tool).is_none());
        assert!(visual.debug_bounds(answer).is_some());
        header(visual, turn, Status::Completed);
        stage(&view, visual, Status::Cancelled);
        assert!(visual.debug_bounds(tool).is_none());
        assert!(visual.debug_bounds(answer).is_some());
        header(visual, turn, Status::Cancelled);

        // Running work opens automatically until the status header is explicitly collapsed.
        stage(&view, visual, Status::Running);
        assert!(visual.debug_bounds(tool).is_some());
        header(visual, turn, Status::Running);
        fixture::tap(visual, &work);
        for status in [Status::Running, Status::Completed, Status::Cancelled] {
            stage(&view, visual, status);
            assert!(visual.debug_bounds(tool).is_none());
            assert!(visual.debug_bounds(answer).is_some());
            header(visual, turn, status);
        }

        // The trailing chevron is part of the same interactive status row.
        fixture::tap(visual, &format!("{work}-chevron"));
        for status in [Status::Running, Status::Completed, Status::Cancelled] {
            stage(&view, visual, status);
            assert!(visual.debug_bounds(tool).is_some());
            assert!(visual.debug_bounds(answer).is_some());
            header(visual, turn, status);
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
