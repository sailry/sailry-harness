use super::*;
use crate::conversation::live::tests::{fixture, wait};
use core::prelude::v1::test;
use serde_json::json;

fn bounds(visual: &mut VisualTestContext, selector: &str) -> Option<Bounds<Pixels>> {
    visual.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

fn install(fixture: &fixture::Fixture) -> sailry_protocol::plugin::Info {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/files");
    let target = fixture.directory.path().join("project/files-package");
    crate::plugins::fixture::copy_package(&source, &target);
    let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
        panic!();
    };
    let revision = snapshot
        .plugins
        .iter()
        .find(|plugin| plugin.name == "files")
        .map_or(0, |plugin| plugin.revision);
    let Output::Plugin(mut package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "files-package".into(),
        name: "files".into(),
        expected_revision: revision,
    }) else {
        panic!();
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    if !package.summary.enabled {
        let Output::Plugin(enabled) = fixture.execute(Command::SetPluginEnabled {
            name: "files".into(),
            expected_revision: package.summary.revision,
            enabled: true,
        }) else {
            panic!();
        };
        package = enabled;
    }
    package
}

#[gpui_kit::test]
fn retained_file_groups_and_write_bodies(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let original = "needle first\nneedle second\n";
        let created = "完整内容 🙂\n```rust\n";
        let replacement = "\t<literal replacement>\n";
        let alias = |name| crate::agent_fixture::plugin_tool("files", name);
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![
                (alias("read_file"), json!({"path":"source.txt"})),
                (alias("read_file"), json!({"path":"source.txt"})),
                (alias("read_file"), json!({"path":"missing.txt"})),
                (
                    alias("search_files"),
                    json!({"query":"needle","globs":["*.txt"]}),
                ),
                (
                    alias("write_file"),
                    json!({"path":"new.txt","text":created,"expected_revision":null}),
                ),
                (
                    alias("write_file"),
                    json!({"path":"source.txt","text":replacement,"expected_revision":blake3::hash(original.as_bytes()).to_hex().to_string()}),
                ),
            ],
        );
        std::fs::write(
            fixture.directory.path().join("project/source.txt"),
            original,
        )
        .unwrap();
        let package = install(&fixture);
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }) else {
            panic!();
        };
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session.clone());
        fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Inspect and edit files".into(),
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
        assert_eq!(
            view.read_with(visual, |view, _| view.history.calls.len()),
            6
        );
        fixture.execute(Command::RemovePlugin {
            name: "files".into(),
            expected_revision: package.summary.revision,
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session);
        wait(visual, |cx| view.read(cx).history.calls.len() == 6);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1000.), px(1400.)));
        let (turn, sources, fault) = view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let calls = &view.history.calls;
            assert!(calls.iter().all(|call| call.display(page).is_some()));
            assert_eq!(
                paths(&[&calls[0], &calls[1], &calls[2]], page)
                    .iter()
                    .map(|path| path.as_ref())
                    .collect::<Vec<_>>(),
                ["source.txt", "missing.txt"]
            );
            assert_eq!(
                paths(&[&calls[3]], page)
                    .iter()
                    .map(|path| path.as_ref())
                    .collect::<Vec<_>>(),
                ["source.txt"]
            );
            assert_eq!(calls[4].result(page).unwrap()["kind"], "file_written");
            assert_eq!(calls[5].result(page).unwrap()["kind"], "file_written");
            assert_eq!(
                approvals::details(&calls[4], page).prompt.as_deref(),
                Some("Create new.txt")
            );
            assert_eq!(
                approvals::details(&calls[5], page).prompt.as_deref(),
                Some("Replace source.txt")
            );
            (
                calls[0].turn,
                calls
                    .iter()
                    .map(|call| call.source.key())
                    .collect::<Vec<_>>(),
                output::fault(calls[2].result(page)).unwrap().message,
            )
        });
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        fixture::tap(visual, &format!("live-{turn}-tool-sequence-{}", sources[0]));
        let read_group = format!("live-{turn}-tool-group-{}", sources[0]);
        fixture::tap(visual, &read_group);
        let first = format!("live-tool-file-{turn}-{}-0", sources[0]);
        let second = format!("live-tool-file-{turn}-{}-1", sources[0]);
        let first_bounds = bounds(visual, &first).unwrap();
        let second_bounds = bounds(visual, &second).unwrap();
        assert_eq!(first_bounds.left(), second_bounds.left());
        assert_eq!(first_bounds.size.height, second_bounds.size.height);
        assert!(bounds(visual, &format!("live-tool-file-{turn}-{}-2", sources[0])).is_none());
        let failed = format!("{turn}-{}", sources[2]);
        assert!(bounds(visual, &format!("live-tool-error-{failed}")).is_some());
        fixture::tap(visual, &format!("live-tool-copy-{failed}"));
        visual.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), fault));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = events.clone();
        let _subscription = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::FileAt(worktree, path) = event {
                    captured.borrow_mut().push((*worktree, path.clone()));
                }
            })
        });
        // Existing history links retain their turn location after the view's selection changes.
        view.update(visual, |view, cx| {
            view.binding.worktree = Some(WorktreeId::new());
            cx.notify();
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        fixture::tap(visual, &first);
        assert_eq!(
            events.borrow().as_slice(),
            &[(fixture.session.worktree, "source.txt".into())]
        );
        fixture::tap(visual, &read_group);
        let search_group = format!("live-{turn}-tool-group-{}", sources[3]);
        fixture::tap(visual, &search_group);
        assert!(bounds(visual, &format!("live-tool-file-{turn}-{}-0", sources[3])).is_some());
        assert!(bounds(visual, &format!("live-tool-file-{turn}-{}-1", sources[3])).is_none());
        fixture::tap(visual, &search_group);
        fixture::tap(visual, &format!("live-{turn}-tool-group-{}", sources[4]));
        for (index, text, added) in [(4, created, true), (5, replacement, false)] {
            fixture::tap(visual, &format!("live-{turn}-tools-{}", sources[index]));
            let key = format!("{turn}-{}", sources[index]);
            assert_eq!(
                bounds(visual, &format!("conversation-diff-{key}")).is_some(),
                added
            );
            if added {
                assert_eq!(
                    bounds(visual, &format!("diff-header-{key}"))
                        .unwrap()
                        .size
                        .height,
                    px(32.)
                );
            }
            fixture::tap(visual, &format!("live-tool-copy-{key}"));
            visual.update(|_, cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), text)
            });
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
