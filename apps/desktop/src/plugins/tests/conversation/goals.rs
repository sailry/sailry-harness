use crate::{
    agent_fixture as support,
    conversation::live::{
        View,
        tests::{contributions, fixture, wait},
    },
};
use fixture::tap as click;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, gpui};
use sailry_protocol::{Command, Output, conversation::Status, plugin};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn package(fixture: &fixture::Fixture) -> plugin::Reference {
    let root = fixture.directory.path().join("project/goal-package");
    crate::plugins::fixture::copy_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/goals"),
        &root,
    );
    let view = root.join("dev.sailry.platform/desktop/view.js");
    let mut code = std::fs::read_to_string(&view).unwrap();
    for (id, action) in [
        ("goal-stop", "pause"),
        ("goal-resume", "resume"),
        ("goal-clear", "clear"),
    ] {
        code = code
            .replace(
                &format!("new Button('{id}')"),
                &format!("Anchor.new('{id}',{{lastControl:view.lastControl ?? null}}).child(new Button('{id}')"),
            )
            .replace(
                &format!(".on_click((_,cx) => view.perform('{action}',cx))]"),
                &format!(".on_click((_,cx) => {{view.lastControl={{kind:'{action}',revision:view.goal?.revision ?? null,busy:view.busy,loading:view.loading,pending:view.pending?.kind ?? null}};view.perform('{action}',cx);cx.notify();}}))]"),
            );
    }
    std::fs::write(
        view,
        format!("import {{Anchor}} from 'sailry/test';\n{code}"),
    )
    .unwrap();
    let Output::Plugins(packages) = fixture.execute(Command::ListPlugins) else {
        panic!("packages expected")
    };
    for package in packages
        .iter()
        .filter(|package| package.enabled && package.name != "goals")
    {
        fixture.execute(Command::SetPluginEnabled {
            name: package.name.clone(),
            expected_revision: package.revision,
            enabled: false,
        });
    }
    let previous = packages
        .iter()
        .find(|package| package.name == "goals")
        .unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "goal-package".into(),
        name: "goals".into(),
        expected_revision: previous.revision,
    }) else {
        panic!("package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info.summary.reference()
}

fn execute(fixture: &fixture::Fixture, package: &plugin::Reference, command: Command) -> Output {
    let request = fixture
        .binding
        .client
        .prepare(command)
        .with_plugin(plugin::Context {
            package: package.clone(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
            turn: None,
            invocation: None,
            surface: Default::default(),
        });
    fixture
        .runtime
        .block_on(fixture.binding.client.execute(request))
        .unwrap()
}

fn read(fixture: &fixture::Fixture, package: &plugin::Reference) -> Value {
    let Output::PluginResult(value) = execute(
        fixture,
        package,
        Command::CallPlugin {
            handler: "read".into(),
            input: json!({}),
        },
    ) else {
        panic!("result expected")
    };
    value["goal"].clone()
}

fn stored(
    fixture: &fixture::Fixture,
    package: &plugin::Reference,
) -> plugin::storage::ConversationEntry {
    let Output::PluginConversationValue(value) = execute(
        fixture,
        package,
        Command::ReadPluginConversationValue { key: "goal".into() },
    ) else {
        panic!("conversation value expected")
    };
    value
}

fn cancelled(fixture: &fixture::Fixture, visual: &mut VisualTestContext, count: usize) {
    wait(visual, |_| {
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 100,
        }) else {
            return false;
        };
        history.page.runs.len() == count
            && history
                .page
                .runs
                .iter()
                .all(|run| run.status == Status::Cancelled)
    });
    assert_eq!(fixture.task_requests(), count);
}

fn control(view: &Entity<View>, visual: &mut VisualTestContext, selector: &'static str) {
    use gpui_kit::test::TestWindowExt;

    ready(view, visual, selector);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let clicked = visual.update(|window, cx| {
            window.render_frame(cx);
            let Some(button) = window.try_find(selector) else {
                return false;
            };
            if !button.visible() || button.disabled() == Some(true) {
                return false;
            }
            assert_eq!(button.role(), Some(gpui::Role::Button));
            window.click(selector, cx);
            true
        });
        if clicked {
            return;
        }
        assert!(Instant::now() < deadline, "disabled {selector}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn absence(
    fixture: &fixture::Fixture,
    package: &plugin::Reference,
    view: &Entity<View>,
    visual: &mut VisualTestContext,
    remote: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let actual = read(fixture, package);
        if actual.is_null() {
            return;
        }
        if Instant::now() >= deadline {
            let entry = stored(fixture, package);
            let diagnostics = visual.read(|cx| {
                contributions(view.read(cx))
                    .read(cx)
                    .test_panel("goals")
                    .map(|panel| crate::plugins::diagnostics(&panel, cx))
                    .unwrap_or_default()
            });
            panic!(
                "goal clear deadline: remote={remote}, stored={entry:?}, read={actual}, panel={diagnostics}"
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ready(view: &Entity<View>, visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let diagnostics = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            contributions(view.read(cx))
                .read(cx)
                .test_panel("goals")
                .map(|panel| crate::plugins::diagnostics(&panel, cx))
                .unwrap_or_default()
        });
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "missing {selector}; {diagnostics}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn local_and_remote_commands_share_status(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::start(true))
        });
        let package = package(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && contributions(view.read(cx)).read(cx).ready(cx)
        });
        assert!(visual.debug_bounds("plugin-control-goals-goal").is_none());
        click(visual, "live-chat-input");
        visual.simulate_input("/goal");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).composer_options(cx)["command"]["name"] == "goal"
        });
        view.read_with(visual, |view, cx| {
            let snapshot = view.snapshot_draft(cx).unwrap().unwrap();
            assert_eq!(snapshot.draft.tokens.len(), 1);
            assert_eq!(snapshot.draft.tokens[0].text, "/goal");
            assert_eq!(snapshot.draft.tokens[0].range, 0..5);
        });
        assert_eq!(fixture.task_requests(), 0);
        click(visual, "live-chat-input");
        visual.simulate_input("Verify this task 中文 🙂\nPreserve the draft");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            fixture.task_requests() == 1 && view.read(cx).composer_options(cx)["command"].is_null()
        });
        assert_eq!(
            read(&fixture, &package)["description"],
            "Verify this task 中文 🙂\nPreserve the draft"
        );
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .intent("goal", cx)
                .is_some()
        });
        click(visual, "plugin-control-goals-status");
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .intent("goal", cx)
                .is_some_and(|entry| entry.state.value == true)
        });
        ready(&view, visual, "goal-description");
        let badge = visual.debug_bounds("plugin-control-goals-status").unwrap();
        let strip = visual.debug_bounds("composer-activity").unwrap();
        let input = visual.debug_bounds("composer-surface").unwrap();
        assert!(badge.top() >= strip.top() && badge.bottom() <= strip.bottom());
        assert!(strip.bottom() <= input.top());
        let first = visual.debug_bounds("goal-item-0").unwrap();
        let second = visual.debug_bounds("goal-item-1").unwrap();
        assert_eq!(first.size.height, gpui_kit::px(32.));
        assert_eq!(second.size.height, gpui_kit::px(32.));
        assert_eq!(first.bottom(), second.top());
        ready(&view, visual, "goal-stop");
        let footer = visual.debug_bounds("goal-description-footer").unwrap();
        let stop = visual.debug_bounds("goal-stop").unwrap();
        let clear = visual.debug_bounds("goal-clear").unwrap();
        assert!(stop.left() > footer.left());
        assert!((clear.right() - footer.right()).abs() < gpui_kit::px(1.));
        click(visual, "goal-item-0");
        ready(&view, visual, "goal-item-0-details");
        control(&view, visual, "goal-stop");
        wait(visual, |_| read(&fixture, &package)["state"] == "paused");
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .entries(plugin::ui::Slot::Commands, cx)
                .iter()
                .any(|entry| {
                    entry.key.package.name == "goals"
                        && entry.key.id == "goal"
                        && entry.state.enabled
                })
        });
        crate::feedback::tests::settle(visual);
        cancelled(&fixture, visual, 1);
        ready(&view, visual, "goal-resume");
        control(&view, visual, "goal-resume");
        wait(visual, |_| {
            fixture.task_requests() == 2 && read(&fixture, &package)["state"] == "active"
        });
        crate::feedback::tests::settle(visual);
        control(&view, visual, "goal-stop");
        wait(visual, |_| read(&fixture, &package)["state"] == "paused");
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .entries(plugin::ui::Slot::Commands, cx)
                .iter()
                .any(|entry| {
                    entry.key.package.name == "goals"
                        && entry.key.id == "goal"
                        && entry.state.enabled
                })
        });
        crate::feedback::tests::settle(visual);
        cancelled(&fixture, visual, 2);
        let paused = stored(&fixture, &package);
        assert!(paused.present);
        assert_eq!(paused.value["state"], "paused");
        control(&view, visual, "goal-clear");
        absence(&fixture, &package, &view, visual, remote);
        let cleared = stored(&fixture, &package);
        assert_eq!(cleared.revision, paused.revision + 1);
        assert!(!cleared.present);
        assert!(cleared.value.is_null());
        assert_eq!(fixture.task_requests(), 2);
        crate::feedback::tests::settle(visual);
        click(visual, "live-chat-input");
        visual.simulate_input("/goal");
        visual.simulate_keystrokes("enter");
        assert_eq!(fixture.task_requests(), 2);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .intent("goal", cx)
                .is_some_and(|entry| entry.state.value == true)
                && view.read(cx).draft(cx).is_empty()
        });
        assert_eq!(fixture.task_requests(), 2);
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .intent("goal", cx)
                .is_some_and(|entry| entry.state.value == false)
        });
        assert_eq!(stored(&fixture, &package), cleared);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn releases_command_routes_without_discarding_drafts(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::start(true))
        });
        let original = package(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && contributions(view.read(cx)).read(cx).ready(cx)
        });
        click(visual, "live-chat-input");
        visual.simulate_input("/goal");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).composer_options(cx)["command"]["name"] == "goal"
        });
        let Output::Plugin(current) = fixture.execute(Command::ReadPlugin {
            name: original.name.clone(),
        }) else {
            panic!("package expected")
        };
        let Output::Plugin(disabled) = fixture.execute(Command::SetPluginEnabled {
            name: original.name,
            expected_revision: current.summary.revision,
            enabled: false,
        }) else {
            panic!("package expected")
        };
        wait(visual, |cx| {
            view.read(cx).composer_options(cx)["command"].is_null()
                && contributions(view.read(cx))
                    .read(cx)
                    .intent("goal", cx)
                    .is_none()
        });
        let Output::Plugin(enabled) = fixture.execute(Command::SetPluginEnabled {
            name: "goals".into(),
            expected_revision: disabled.summary.revision,
            enabled: true,
        }) else {
            panic!("package expected")
        };
        wait(visual, |cx| {
            contributions(view.read(cx)).read(cx).ready(cx)
                && contributions(view.read(cx))
                    .read(cx)
                    .intent("goal", cx)
                    .is_some()
        });
        assert_eq!(view.read_with(visual, |view, cx| view.draft(cx)), "/goal ");
        wait(visual, |cx| {
            view.read(cx).composer_options(cx)["command"]["name"] == "goal"
        });
        fixture.execute(Command::RemovePlugin {
            name: "goals".into(),
            expected_revision: enabled.summary.revision,
        });
        wait(visual, |cx| {
            view.read(cx).composer_options(cx)["command"].is_null()
                && contributions(view.read(cx))
                    .read(cx)
                    .intent("goal", cx)
                    .is_none()
        });
        assert_eq!(view.read_with(visual, |view, cx| view.draft(cx)), "/goal ");
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn restores_status_and_requires_manual_resume(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::goal_reasoning(2, "completed"))
        });
        let package = package(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && contributions(view.read(cx)).read(cx).ready(cx)
        });
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "/goal Verify the requested work 中文 🙂".into(),
        });
        wait(visual, |_| read(&fixture, &package)["state"] == "completed");
        wait(visual, |_| {
            let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
                session: fixture.session.id,
                before: None,
                limit: 100,
            }) else {
                panic!("conversation expected")
            };
            history.page.runs.len() == 2
                && history
                    .page
                    .runs
                    .iter()
                    .all(|run| run.status == Status::Completed)
        });
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 100,
        }) else {
            panic!("conversation expected")
        };
        assert_eq!(history.page.runs.len(), 2);
        assert!(
            history
                .page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        let original = read(&fixture, &package);
        let requests = fixture.task_requests();
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this draft");
        fixture.execute(Command::RewindConversation {
            session: fixture.session.id,
            through: Some(history.page.runs[0].turn),
            expected_head: history.page.runs[1].turn,
            expected_revision: history.page.revision,
        });
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .intent("goal", cx)
                .is_some_and(|entry| {
                    entry
                        .state
                        .segments
                        .last()
                        .is_some_and(|segment| segment.tone == plugin::ui::SegmentTone::Warning)
                })
        });
        assert_eq!(read(&fixture, &package)["id"], original["id"]);
        assert_eq!(read(&fixture, &package)["state"], "paused");
        assert_eq!(fixture.task_requests(), requests);
        assert_eq!(visual.read(|cx| view.read(cx).draft(cx)), "Keep this draft");
        click(visual, "plugin-control-goals-status");
        ready(&view, visual, "goal-resume");
        assert!(visual.debug_bounds("goal-stop").is_none());
        control(&view, visual, "goal-resume");
        wait(visual, |_| {
            fixture.task_requests() > requests && read(&fixture, &package)["state"] == "completed"
        });
        assert_eq!(read(&fixture, &package)["id"], original["id"]);
        assert_eq!(visual.read(|cx| view.read(cx).draft(cx)), "Keep this draft");
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 100,
        }) else {
            panic!("conversation expected")
        };
        assert_eq!(history.page.runs.len(), 3);
        fixture.execute(Command::RewindConversation {
            session: fixture.session.id,
            through: None,
            expected_head: history.page.runs.last().unwrap().turn,
            expected_revision: history.page.revision,
        });
        wait(visual, |cx| {
            contributions(view.read(cx))
                .read(cx)
                .intent("goal", cx)
                .is_some_and(|entry| entry.state.segments.len() == 1)
        });
        assert!(read(&fixture, &package).is_null());
        assert_eq!(visual.read(|cx| view.read(cx).draft(cx)), "Keep this draft");
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
