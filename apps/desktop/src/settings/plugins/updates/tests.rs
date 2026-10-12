use super::*;
use crate::settings::plugins::test_support::{Fixture, draw, init, shown, tap, wait};
use core::prelude::v1::test;
use std::sync::atomic::Ordering;

mod retry;

fn start(fixture: &Fixture, owner: &Entity<Workspace>, visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        let packages = owner.read(cx).plugin_catalog.packages.clone();
        connected(
            fixture.transport.clone(),
            fixture.runtime.clone(),
            "Fixture Node".into(),
            &packages,
            window,
            cx,
        );
    });
    shown(visual, "plugins-update-dialog", true);
}

fn install(fixture: &Fixture, path: &str, name: &str) {
    fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: path.into(),
        name: name.into(),
        expected_revision: 0,
        expected_digest: None,
    });
}

fn package(fixture: &Fixture, name: &str, version: &str) {
    let directory = fixture.directory.path().join("project").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("plugin.json"),
        serde_json::json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": name, "version": version,
        })
        .to_string(),
    )
    .unwrap();
}

#[gpui::test]
fn selects_updates_and_prompts_once(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        install(&fixture, "package", "example");
        fixture.package("2.0.0");
        let skill = fixture
            .directory
            .path()
            .join("project/package/skills/draft");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: draft\ndescription: Draft changes\n---\nDraft changes\n",
        )
        .unwrap();
        let (owner, visual) = fixture.mount(cx);
        start(&fixture, &owner, visual);
        let state = owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.updates.clone().unwrap()
        });
        assert!(state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["example"].selected
        }));
        tap(visual, "plugin-update-select-example");
        assert!(!state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["example"].selected
        }));
        tap(visual, "plugins-updates-apply");
        assert_eq!(fixture.plugins()[0].revision, 1);
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        tap(visual, "plugin-update-expand-example");
        shown(visual, "plugins_update_skills", true);
        assert!(state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["example"].expanded
        }));
        tap(visual, "plugin-update-select-example");
        fixture.transport.mode.store(2, Ordering::SeqCst);
        tap(visual, "plugins-updates-apply");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert!(state.read_with(visual, |state, _| state.dialog.as_ref().unwrap().locked));
        assert!(state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["example"].status == view::Status::Updating
        }));
        tap(visual, "plugin-update-select-example");
        assert!(state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["example"].selected
        }));
        fixture.transport.release.cancel();
        shown(visual, "plugins-update-dialog", false);
        assert_eq!(fixture.plugins()[0].version.as_deref(), Some("2.0.0"));
        visual.update(|window, cx| {
            let packages = fixture.public_packages(false);
            connected(
                fixture.transport.clone(),
                fixture.runtime.clone(),
                "Fixture Node".into(),
                &packages,
                window,
                cx,
            );
        });
        for _ in 0..10 {
            draw(visual);
        }
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 1);
        fixture.close(visual);
    }
}

#[gpui::test]
fn scrolls_all_candidates(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let names = [
            "candidate-a",
            "candidate-b",
            "candidate-c",
            "candidate-d",
            "candidate-e",
            "candidate-f",
            "candidate-g",
            "candidate-h",
            "candidate-i",
        ];
        for name in names {
            package(&fixture, name, "1.0.0");
            install(&fixture, name, name);
            package(&fixture, name, "2.0.0");
        }
        let skill = fixture
            .directory
            .path()
            .join("project/candidate-a/skills/draft");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: draft\ndescription: Draft changes\n---\nDraft changes\n",
        )
        .unwrap();
        let (owner, visual) = fixture.mount(cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(640.)));
        start(&fixture, &owner, visual);
        let state = owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.updates.clone().unwrap()
        });
        assert_eq!(
            state.read_with(visual, |state, _| state.dialog.as_ref().unwrap().rows.len()),
            names.len()
        );
        tap(visual, "plugin-update-select-candidate-a");
        let first = visual
            .debug_bounds("plugin-update-select-candidate-a")
            .unwrap();
        let last = visual
            .debug_bounds("plugin-update-select-candidate-i")
            .unwrap();
        let dialog = visual.debug_bounds("dialog-0").unwrap();
        let footer = ["plugins-updates-later", "plugins-updates-apply"]
            .map(|selector| visual.debug_bounds(selector).unwrap());
        let bottom = footer[0].top().min(footer[1].top());
        assert!(last.bottom() > bottom);
        for button in footer {
            assert!(button.top() >= dialog.top() && button.bottom() <= dialog.bottom());
        }
        visual.simulate_event(ScrollWheelEvent {
            position: first.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        draw(visual);
        let reached = visual
            .debug_bounds("plugin-update-select-candidate-i")
            .unwrap();
        assert!(
            reached.top() < last.top() - px(100.),
            "the candidate list did not scroll: first={first:?}, last={last:?}, reached={reached:?}, dialog={dialog:?}, footer={footer:?}"
        );
        assert!(reached.top() >= first.top() && reached.bottom() <= bottom);
        tap(visual, "plugin-update-select-candidate-i");
        assert!(state.read_with(visual, |state, _| {
            let rows = &state.dialog.as_ref().unwrap().rows;
            !rows["candidate-a"].selected
                && !rows["candidate-i"].selected
                && rows.values().filter(|row| row.selected).count() == 7
        }));
        assert_eq!(
            ["plugins-updates-later", "plugins-updates-apply"]
                .map(|selector| visual.debug_bounds(selector).unwrap()),
            footer
        );
        visual.simulate_event(ScrollWheelEvent {
            position: first.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(2000.))),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        draw(visual);
        assert_eq!(
            visual
                .debug_bounds("plugin-update-select-candidate-a")
                .unwrap(),
            first
        );
        let card = visual
            .debug_bounds("plugin-update-card-candidate-a")
            .unwrap();
        tap(visual, "plugin-update-expand-candidate-a");
        shown(visual, "plugins_update_skills", true);
        assert!(
            visual
                .debug_bounds("plugin-update-card-candidate-a")
                .unwrap()
                .size
                .height
                > card.size.height
        );
        assert!(state.read_with(visual, |state, _| {
            let rows = &state.dialog.as_ref().unwrap().rows;
            rows["candidate-a"].expanded
                && !rows["candidate-a"].selected
                && !rows["candidate-i"].selected
        }));
        assert_eq!(
            ["plugins-updates-later", "plugins-updates-apply"]
                .map(|selector| visual.debug_bounds(selector).unwrap()),
            footer
        );
        let requests = fixture.transport.requests.lock().unwrap().len();
        tap(visual, "plugins-updates-later");
        shown(visual, "plugins-update-dialog", false);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        assert!(
            fixture
                .plugins()
                .iter()
                .all(|package| package.version.as_deref() == Some("1.0.0"))
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn retains_failed_rows_and_retries(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let second = fixture.directory.path().join("project/second");
        std::fs::create_dir(&second).unwrap();
        let write = |version: &str| {
            std::fs::write(
                second.join("plugin.json"),
                serde_json::json!({
                    "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                    "name": "second", "version": version,
                })
                .to_string(),
            )
            .unwrap()
        };
        fixture.package("1.0.0");
        write("1.0.0");
        install(&fixture, "package", "example");
        install(&fixture, "second", "second");
        fixture.package("2.0.0");
        write("2.0.0");
        let (owner, visual) = fixture.mount(cx);
        start(&fixture, &owner, visual);
        fixture.package("3.0.0");
        *fixture.transport.lose_update.lock().unwrap() = Some("second".into());
        tap(visual, "plugins-updates-apply");
        let state = owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.updates.clone().unwrap()
        });
        wait(visual, |cx| state.read(cx).retryable());
        assert!(state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["example"].status == view::Status::Failed
        }));
        assert!(state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["second"].status == view::Status::Failed
        }));
        let requests = fixture.transport.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        tap(visual, "plugin-update-retry-example");
        assert_eq!(*fixture.transport.requests.lock().unwrap(), requests);
        tap(visual, "plugin-update-retry-second");
        wait(visual, |cx| !state.read(cx).busy());
        assert!(state.read_with(visual, |state, _| {
            state.dialog.as_ref().unwrap().rows["second"].status == view::Status::Succeeded
        }));
        assert_eq!(
            *fixture.transport.requests.lock().unwrap(),
            [requests[0], requests[1], requests[1]]
        );
        let packages = fixture.plugins();
        assert_eq!(
            packages
                .iter()
                .find(|entry| entry.name == "example")
                .unwrap()
                .version
                .as_deref(),
            Some("1.0.0")
        );
        assert_eq!(
            packages
                .iter()
                .find(|entry| entry.name == "second")
                .unwrap()
                .version
                .as_deref(),
            Some("2.0.0")
        );
        fixture.package("2.0.0");
        tap(visual, "plugin-update-retry-example");
        shown(visual, "plugins-update-dialog", false);
        assert_eq!(
            fixture
                .plugins()
                .iter()
                .find(|entry| entry.name == "example")
                .unwrap()
                .version
                .as_deref(),
            Some("2.0.0")
        );
        let final_requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(final_requests.len(), 4);
        assert_ne!(final_requests[0], final_requests[3]);
        drop(final_requests);
        fixture.close(visual);
    }
}

#[gpui::test]
fn applies_only_ready_rows(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        for name in ["example", "second", "third"] {
            package(&fixture, name, "1.0.0");
            install(&fixture, name, name);
            package(&fixture, name, "2.0.0");
        }
        let (owner, visual) = fixture.mount(cx);
        start(&fixture, &owner, visual);
        let state = owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.updates.clone().unwrap()
        });
        tap(visual, "plugin-update-select-third");
        package(&fixture, "example", "3.0.0");
        tap(visual, "plugins-updates-apply");
        wait(visual, |cx| !state.read(cx).busy());
        assert!(state.read_with(visual, |state, _| {
            let rows = &state.dialog.as_ref().unwrap().rows;
            rows["example"].status == view::Status::Failed
                && rows["second"].status == view::Status::Succeeded
                && rows["third"].status == view::Status::Ready
        }));
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 2);
        package(&fixture, "example", "2.0.0");
        tap(visual, "plugin-update-select-third");
        tap(visual, "plugins-updates-apply");
        wait(visual, |cx| !state.read(cx).busy());
        assert!(state.read_with(visual, |state, _| {
            let rows = &state.dialog.as_ref().unwrap().rows;
            rows["example"].status == view::Status::Failed
                && rows["third"].status == view::Status::Succeeded
        }));
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 3);
        let installed = fixture.plugins();
        assert_eq!(
            installed
                .iter()
                .find(|entry| entry.name == "example")
                .unwrap()
                .version
                .as_deref(),
            Some("1.0.0")
        );
        tap(visual, "plugin-update-retry-example");
        shown(visual, "plugins-update-dialog", false);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 4);
        fixture.close(visual);
    }
}
