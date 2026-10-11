use super::*;
use crate::{
    plugins::{
        Panel,
        fixture::Fixture,
        tests::{click, init, snapshot, wait},
    },
    settings::Workspace,
};
use core::prelude::v1::test;
use gpui_kit::{component::Root, test::TestWindowExt, *};
use sailry_protocol::{Command, Output, plugin::Info};
use std::time::Instant;

fn package(fixture: &Fixture, rows: usize) -> Info {
    fixture.package();
    let root = fixture.directory.path().join("project/package");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
    let extension = &mut manifest["extensions"]["dev.sailry.platform"];
    let entry = "dev.sailry.platform/desktop/continuity.js";
    extension["settings_page"] = serde_json::json!({
        "entry": entry,
        "navigation": {"label": "Continuity"},
        "full_width": true
    });
    extension["desktop"]["resources"]
        .as_array_mut()
        .unwrap()
        .push(entry.into());
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(
        root.join(entry),
        format!("const ROWS = {rows};\n{}", include_str!("continuity.js")),
    )
    .unwrap();
    fixture.install(0)
}

fn open<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, Entity<Panel>, &'a mut VisualTestContext) {
    let mut workspace = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        owner.update(cx, |owner, cx| {
            owner.bind_providers(
                fixture.transport.clone(),
                fixture.runtime.clone(),
                "Continuity fixture".into(),
                cx,
            )
        });
        workspace = Some(owner.clone());
        Root::new(owner, window, cx)
    });
    let workspace = workspace.unwrap();
    wait(visual, |cx| {
        workspace
            .read(cx)
            .plugin_settings_entries(cx)
            .iter()
            .any(|entry| entry.name == "project-summary")
    });
    workspace.update(visual, |owner, cx| {
        owner.open_plugin_settings("project-summary", cx)
    });
    wait(visual, |cx| {
        workspace
            .read(cx)
            .plugin_settings_panel()
            .is_some_and(|panel| snapshot(&panel, cx).contains("continuity-value-true"))
    });
    let panel = workspace.read_with(visual, |owner, _| owner.plugin_settings_panel().unwrap());
    (workspace, panel, visual)
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn row(visual: &mut VisualTestContext) -> Bounds<Pixels> {
    visual.update(|window, _| window.find("continuity-row-1").bounds())
}

fn wheel(visual: &mut VisualTestContext, dy: f32) {
    let position = row(visual).center();
    visual.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        ..Default::default()
    });
    draw(visual);
}

fn edit(visual: &mut VisualTestContext, panel: &Entity<Panel>) -> FocusHandle {
    click(visual, "field-1");
    visual.simulate_keystrokes("end");
    visual.simulate_input(" retained");
    wait(visual, |cx| snapshot(panel, cx).contains("Draft retained"));
    let focus = visual.update(|window, cx| window.focused(cx).unwrap());
    let before = row(visual);
    wheel(visual, -40.);
    assert_eq!(row(visual).top(), before.top() - px(40.));
    assert!(visual.update(|window, _| focus.is_focused(window)));
    focus
}

fn identity(panel: &Entity<Panel>, visual: &mut VisualTestContext) -> EntityId {
    panel.read_with(visual, |panel, _| {
        panel.mounted.as_ref().unwrap().root.entity_id()
    })
}

fn assert_retained(
    panel: &Entity<Panel>,
    root: EntityId,
    focus: &FocusHandle,
    position: Bounds<Pixels>,
    visual: &mut VisualTestContext,
) {
    assert_eq!(
        identity(panel, visual),
        root,
        "settings change remounted the view"
    );
    assert_eq!(
        row(visual),
        position,
        "settings change reset the scroll position"
    );
    assert!(
        visual.update(|window, _| focus.is_focused(window)),
        "settings change moved native input focus"
    );
    assert!(panel.read_with(visual, |_, cx| {
        snapshot(panel, cx).contains("Draft retained")
    }));
}

fn close(
    fixture: Fixture,
    workspace: Entity<Workspace>,
    panel: Entity<Panel>,
    visual: &mut VisualTestContext,
) {
    visual.update(|window, _| window.remove_window());
    drop(panel);
    drop(workspace);
    fixture.close();
}

#[gpui::test]
fn own_save_retains_focus_and_scroll(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = package(&fixture, 32);
        let (workspace, panel, visual) = open(&fixture, cx);
        let focus = edit(visual, &panel);
        let root = identity(&panel, visual);
        let position = row(visual);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("continuity-value-false")
                && panel.read(cx).metadata.read(cx).settled()
                && panel.read(cx).selected.as_ref().is_some_and(|selected| {
                    selected.settings_revision == package.summary.settings_revision + 1
                })
        });
        let Output::PluginSettings(saved) = fixture.execute(Command::ReadPluginSettings {
            package: panel.read_with(visual, |panel, _| panel.selected.clone().unwrap()),
        }) else {
            panic!("saved plugin settings expected")
        };
        assert_eq!(saved.values["include_untracked"], false);
        assert_eq!(saved.package.digest, package.summary.digest);
        assert_retained(&panel, root, &focus, position, visual);
        close(fixture, workspace, panel, visual);
    }
}

#[gpui::test]
fn external_change_retains_focus_and_scroll(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = package(&fixture, 32);
        let (workspace, panel, visual) = open(&fixture, cx);
        let focus = edit(visual, &panel);
        let root = identity(&panel, visual);
        let position = row(visual);
        let updated = fixture.configure(&package, false);
        assert_eq!(updated.summary.digest, package.summary.digest);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("continuity-value-false")
                && panel.read(cx).metadata.read(cx).settled()
        });
        assert_retained(&panel, root, &focus, position, visual);
        close(fixture, workspace, panel, visual);
    }
}

#[gpui::test]
#[ignore = "opt-in CPU scroll sample; does not measure native presentation latency"]
fn samples_scroll_work(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for rows in [32, 256, 1024] {
            let fixture = Fixture::new(remote);
            package(&fixture, rows);
            let (workspace, panel, visual) = open(&fixture, cx);
            let runtime = panel.read_with(visual, |panel, _| {
                panel.mounted.as_ref().unwrap()._runtime.clone()
            });
            for _ in 0..12 {
                wheel(visual, -4.);
                wheel(visual, 4.);
            }
            let before = runtime.read_metrics();
            let root = identity(&panel, visual);
            let position = row(visual);
            let mut elapsed = Vec::new();
            for frame in 0..120 {
                let started = Instant::now();
                wheel(visual, if frame % 2 == 0 { -4. } else { 4. });
                elapsed.push(started.elapsed());
            }
            let metrics = runtime.read_metrics().since(&before);
            elapsed.sort();
            eprintln!(
                "sailry-desktop: settings scroll sample {}",
                serde_json::json!({
                    "remote": remote,
                    "rows": rows,
                    "samples": elapsed.len(),
                    "dispatch_draw_median_us": elapsed[elapsed.len() / 2].as_micros(),
                    "dispatch_draw_p95_us": elapsed[elapsed.len() * 95 / 100].as_micros(),
                    "script_renders": metrics.script_renders(),
                    "script_render_us": metrics.script_render_time().as_micros(),
                    "materializations": metrics.materializations(),
                    "materialize_us": metrics.materialize_time().as_micros(),
                    "frame_script_calls": metrics.frame_script_calls(),
                    "presentation_measured": false
                })
            );
            assert_eq!(identity(&panel, visual), root);
            assert_eq!(row(visual), position);
            close(fixture, workspace, panel, visual);
        }
    }
}
