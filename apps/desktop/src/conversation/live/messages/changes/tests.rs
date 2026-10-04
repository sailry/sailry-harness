use super::*;
use crate::conversation::live::tests::{fixture, wait};
use core::prelude::v1::test;

#[gpui_kit::test]
fn opens_files_and_preserves_diff(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let mut fixture = fixture::Fixture::with_tools(remote, (0..4).map(|index| (
            crate::agent_support::plugin_tool("files", "write_file"), serde_json::json!({"path": format!("nested/file-{index}.txt"), "text": "one\ntwo\n", "expected_revision": null})
        )).collect());
        std::fs::create_dir(fixture.directory.path().join("project/nested")).unwrap();
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Project;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        }) else {
            panic!("session expected")
        };
        fixture.session = session;
        let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: 2,
            message: "Create files".into(),
        }) else {
            panic!("turn expected")
        };
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .changes
                .get(&turn.id)
                .is_some_and(|card| card.read(cx).data.is_some())
        });
        let data = view.read_with(visual, |view, cx| {
            view.changes[&turn.id].read(cx).data.clone().unwrap()
        });
        assert_eq!(data.files.len(), 4);
        let text_selector: &'static str =
            Box::leak(format!("live-turn-text-{}", turn.id).into_boxed_str());
        let card_selector: &'static str =
            Box::leak(format!("live-turn-changes-{}", turn.id).into_boxed_str());
        for width in [520., 1440., 1000.] {
            let handle = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            wait(visual, |_| true);
            let text = visual.debug_bounds(text_selector).unwrap();
            let card = visual.debug_bounds(card_selector).unwrap();
            assert_eq!(card.left(), text.left());
            assert_eq!(card.right(), text.right());
            assert!(card.top() - text.bottom() >= px(24.));
        }
        for action in ["review", "undo"] {
            let selector = Box::leak(format!("live-turn-{action}-{}", turn.id).into_boxed_str());
            assert_eq!(visual.debug_bounds(selector).unwrap().size.height, px(32.));
        }
        assert!(
            data.files
                .iter()
                .all(|file| (file.additions, file.deletions) == (2, 0))
        );
        let fourth = Box::leak(format!("live-turn-file-{}-3", turn.id).into_boxed_str());
        let more = format!("live-turn-files-more-{}", turn.id);
        let row = visual
            .debug_bounds(Box::leak(
                format!("live-turn-file-{}-0", turn.id).into_boxed_str(),
            ))
            .unwrap();
        let label = visual
            .debug_bounds(Box::leak(
                format!("live-turn-file-label-{}-0", turn.id).into_boxed_str(),
            ))
            .unwrap();
        let more_label = visual
            .debug_bounds(Box::leak(
                format!("live-turn-files-more-label-{}", turn.id).into_boxed_str(),
            ))
            .unwrap();
        assert_eq!(label.left() - row.left(), px(12.));
        assert_eq!(more_label.left(), label.left());
        assert!(visual.debug_bounds(fourth).is_none());
        fixture::tap(visual, &more);
        assert!(visual.debug_bounds(fourth).is_some());
        let deltas: Vec<_> = (0..4)
            .map(|index| {
                visual
                    .debug_bounds(Box::leak(
                        format!("live-turn-delta-{}-{index}", turn.id).into_boxed_str(),
                    ))
                    .unwrap()
            })
            .collect();
        assert!(deltas.windows(2).all(|pair| {
            pair[0].left() == pair[1].left() && pair[0].right() == pair[1].right()
        }));
        for (index, delta) in deltas.iter().enumerate() {
            let undo = visual
                .debug_bounds(Box::leak(
                    format!("live-turn-undo-file-{}-{index}", turn.id).into_boxed_str(),
                ))
                .unwrap();
            assert_eq!(undo.right(), delta.right());
            assert_eq!(row.right() - delta.right(), px(12.));
        }
        assert!(
            deltas
                .windows(2)
                .all(|pair| pair[1].top() - pair[0].top() >= px(40.))
        );
        fixture::tap(visual, &more);
        assert!(visual.debug_bounds(fourth).is_none());
        fixture::tap(visual, &more);
        assert!(visual.debug_bounds(fourth).is_some());
        let opened = Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = opened.clone();
        let undone = Arc::new(std::sync::Mutex::new(Vec::new()));
        let undo_output = undone.clone();
        let _events = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::GitFile(_, path) = event {
                    output.lock().unwrap().push(path.clone());
                }
                if let Event::UndoChanges(_, path) = event {
                    undo_output.lock().unwrap().push(path.clone());
                }
            })
        });
        fixture::tap(visual, &format!("live-turn-undo-file-{}-1", turn.id));
        assert_eq!(*undone.lock().unwrap(), [Some(data.files[1].path.clone())]);
        assert!(opened.lock().unwrap().is_empty());
        fixture::tap(visual, &format!("live-turn-file-{}-1", turn.id));
        fixture::tap(visual, &format!("live-turn-review-{}", turn.id));
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(
            *opened.lock().unwrap(),
            [Some(data.files[1].path.clone()), None]
        );
        std::fs::write(
            fixture.directory.path().join("project/nested/file-0.txt"),
            "manual edit\n",
        )
        .unwrap();
        let Output::TurnDiff(restored) = fixture.execute(Command::ReadTurnDiff {
            session: fixture.session.id,
            turn: turn.id,
        }) else {
            panic!("turn diff expected")
        };
        assert_eq!(restored, data);
        // Sending hides the old card immediately, before admission is observed.
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.submit("Continue".into(), window, cx);
                assert!(view.turn_changes(turn.id, window, cx).is_none());
            });
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.page.runs.iter().any(|run| run.turn != turn.id))
        });
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-changes-{}", turn.id).into_boxed_str()
                ))
                .is_none()
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert!(view.turn_changes(turn.id, window, cx).is_none());
            });
            window.remove_window();
        });
        fixture.close();
    }
}
