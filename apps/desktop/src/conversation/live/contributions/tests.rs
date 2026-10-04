use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, hover, init, leave, open, tap},
    wait,
};
use core::prelude::v1::test;
use sailry_protocol::plugin;
use serde_json::{Value, json};
mod assistants;
mod commands;
mod placement;

fn install(fixture: &mut Fixture) -> plugin::Info {
    install_with(fixture, |_| {})
}

fn install_with(fixture: &mut Fixture, edit: impl FnOnce(&mut Value)) -> plugin::Info {
    let root = fixture.directory.path().join("project/package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/desktop")).unwrap();
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/task-notes");
    for path in [
        "plugin.json",
        "dev.sailry.platform/desktop/main.js",
        "dev.sailry.platform/desktop/locales.js",
        "dev.sailry.platform/desktop/chat.js",
    ] {
        std::fs::copy(source.join(path), root.join(path)).unwrap();
    }
    let path = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut manifest);
    std::fs::write(path, manifest.to_string()).unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "package".into(),
        name: "task-notes".into(),
        expected_revision: 0,
    }) else {
        panic!("plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    for key in ["review", "build"] {
        scoped(
            fixture,
            &info,
            Command::WritePluginValue {
                key: key.into(),
                value: json!({"done":false}),
                expected_revision: 0,
            },
        );
    }
    info
}

fn scoped(fixture: &Fixture, info: &plugin::Info, command: Command) -> Output {
    fixture
        .runtime
        .block_on(
            fixture
                .binding
                .client
                .execute(
                    fixture
                        .binding
                        .client
                        .prepare(command)
                        .with_plugin(plugin::Context {
                            invocation: None,
                            turn: None,
                            surface: plugin::desktop::Surface::Composer,
                            package: info.summary.reference(),
                            worktree: Some(fixture.session.worktree),
                            session: Some(fixture.session.id),
                        }),
                ),
        )
        .unwrap()
}

fn value(view: &Entity<View>, id: &str, cx: &App) -> Option<Value> {
    view.read(cx)
        .contributions
        .read(cx)
        .entries(Slot::Composer, cx)
        .into_iter()
        .chain(
            view.read(cx)
                .contributions
                .read(cx)
                .entries(Slot::Context, cx),
        )
        .chain(
            view.read(cx)
                .contributions
                .read(cx)
                .entries(Slot::Statistics, cx),
        )
        .find(|entry| entry.key.id == id && entry.state.enabled)
        .map(|entry| entry.state.value)
}

fn resize(visual: &mut VisualTestContext, width: f32) {
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(width), px(820.)));
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn shares_controls_across_widths(cx: &mut TestAppContext) {
    init(cx);
    cx.update(crate::plugins::init);
    for remote in [false, true] {
        let mut fixture = Fixture::with_tools(remote, vec![]);
        let info = install(&mut fixture);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        resize(visual, 1100.);
        wait(visual, |cx| value(&view, "notes", cx) == Some(json!(2)));
        assert!(
            visual
                .debug_bounds("plugin-stat-task-notes-notes")
                .is_some()
        );
        tap(visual, "plugin-control-task-notes-focus");
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(true)));
        tap(visual, "plugin-control-task-notes-level");
        visual.simulate_keystrokes("down enter");
        wait(visual, |cx| {
            value(&view, "level", cx) == Some(json!("brief"))
        });
        tap(visual, "plugin-control-task-notes-note");
        assert!(visual.debug_bounds("plugin-contribution-picker").is_some());
        let duration = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            cx.theme().motion_tokens().duration_normal
        });
        visual.executor().advance_clock(duration);
        tap(visual, "command-search");
        visual.simulate_input("review");
        wait(visual, |cx| {
            view.read(cx)
                .contributions
                .read(cx)
                .entries(Slot::Context, cx)
                .iter()
                .any(|entry| entry.key.id == "note" && entry.choices().len() == 1)
        });
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            value(&view, "note", cx) == Some(json!("review"))
        });
        for width in [520., 280.] {
            resize(visual, width);
            tap(visual, "composer-settings");
            assert!(
                visual
                    .debug_bounds("plugin-control-task-notes-focus")
                    .is_some()
            );
            tap(visual, "plugin-control-task-notes-focus");
            wait(visual, |cx| {
                value(&view, "focus", cx) == Some(json!(width == 280.))
            });
            visual.simulate_keystrokes("escape");
            wait(visual, |_| true);
            let control = visual
                .debug_bounds("plugin-control-task-notes-note")
                .unwrap();
            if width < 440. {
                assert!(
                    control.size.width <= px(48.),
                    "context contribution is not compact"
                );
            }
            hover(visual, "composer-stats");
            assert!(
                visual
                    .debug_bounds("statistics-plugin-task-notes-notes")
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds("statistics-plugin-task-notes-notes-0")
                    .is_some()
            );
            leave(visual);
            assert!(visual.debug_bounds("composer-statistics-panel").is_none());
        }
        let Output::PluginValue(saved) = scoped(
            &fixture,
            &info,
            Command::ReadPluginValue {
                key: "preferences".into(),
            },
        ) else {
            panic!("plugin value expected")
        };
        assert_eq!(
            saved.value,
            json!({"focus":true,"level":"brief","selected":"review"})
        );
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn releases_changed_packages(cx: &mut TestAppContext) {
    init(cx);
    cx.update(crate::plugins::init);
    for remote in [false, true] {
        let mut fixture = Fixture::with_tools(remote, vec![]);
        let info = install(&mut fixture);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let unrelated: BTreeMap<_, _> = snapshot
            .plugins
            .iter()
            .filter(|package| package.name != info.summary.name)
            .map(|package| (package.name.clone(), package.reference()))
            .collect();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        resize(visual, 1100.);
        wait(visual, |cx| value(&view, "notes", cx) == Some(json!(2)));
        tap(visual, "plugin-control-task-notes-focus");
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(true)));
        fixture.execute(Command::SetPluginEnabled {
            name: "task-notes".into(),
            expected_revision: info.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| value(&view, "focus", cx).is_none());
        assert!(
            visual
                .debug_bounds("plugin-control-task-notes-focus")
                .is_none()
        );
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["version"] = json!("0.2.0");
        std::fs::write(path, manifest.to_string()).unwrap();
        let Output::Plugin(updated) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: "task-notes".into(),
            expected_revision: 2,
        }) else {
            panic!("plugin expected")
        };
        assert_ne!(updated.summary.digest, info.summary.digest);
        fixture.execute(Command::SetPluginEnabled {
            name: "task-notes".into(),
            expected_revision: updated.summary.revision,
            enabled: true,
        });
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(true)));
        view.read_with(visual, |view, cx| {
            let entries = view.contributions.read(cx).entries(Slot::Composer, cx);
            let changed: Vec<_> = entries
                .iter()
                .filter(|entry| entry.key.package.name == updated.summary.name)
                .collect();
            assert!(!changed.is_empty());
            assert!(
                changed
                    .iter()
                    .all(|entry| { entry.key.package == updated.summary.reference() })
            );
            for entry in entries
                .iter()
                .filter(|entry| entry.key.package.name != updated.summary.name)
            {
                assert_eq!(
                    unrelated.get(&entry.key.package.name),
                    Some(&entry.key.package)
                );
            }
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
