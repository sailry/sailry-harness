use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, hover, init, leave, open, tap},
    wait,
};
use core::prelude::v1::test;
use sailry_protocol::{Permission, WorkMode, plugin::ui};

mod fit;
mod sidebar;

#[gpui::test]
fn narrow_controls(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        git2::Repository::init(fixture.directory.path().join("project")).unwrap();
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        let mut provider = snapshot.providers[0].clone();
        let mut alternate = provider.models[0].clone();
        alternate.id = "alternate-model".into();
        provider.models.push(alternate);
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            let view = view.read(cx);
            view.connected()
                && view.configured()
                && view.git
                && !view.busy()
                && context_ready(view, cx)
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("History");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "History"
        );
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            !view.read(cx).rows.is_empty()
                && view.read(cx).active().is_none()
                && !view.read(cx).busy()
        });
        wait(visual, |cx| {
            let current = view.read(cx);
            current.connected()
                && current.configured()
                && context_ready(current, cx)
                && ["create", "fork"].into_iter().all(|id| {
                    current
                        .contributions
                        .read(cx)
                        .entries(ui::Slot::Composer, cx)
                        .iter()
                        .any(|entry| {
                            entry.key.package.name == "worktrees"
                                && entry.key.id == id
                                && entry.state.enabled
                        })
                })
                && current
                    .contributions
                    .read(cx)
                    .entries(sailry_protocol::plugin::ui::Slot::Statistics, cx)
                    .iter()
                    .filter(|entry| entry.key.package.name == "statistics")
                    .count()
                    == 5
        });
        view.read_with(visual, |view, cx| {
            let entries = view.contributions.read(cx).entries(ui::Slot::Composer, cx);
            for id in ["create", "fork"] {
                let entry = entries
                    .iter()
                    .find(|entry| entry.key.package.name == "worktrees" && entry.key.id == id)
                    .expect("worktree menu entry expected");
                assert_eq!(entry.declaration.slot, ui::Slot::Composer);
                assert_eq!(entry.declaration.overflow, ui::Overflow::Menu);
                assert!(entry.state.enabled);
            }
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep this draft");
        let handle = visual.update(|window, _| window.window_handle());
        for width in [520., 360., 280., 900., 520.] {
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            wait(visual, |cx| {
                view.read(cx).compact_composer == (width < 680.)
                    && view.read(cx).icon_context == (width < 440.)
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let compact = width < 680.;
            assert_eq!(visual.debug_bounds("composer-settings").is_some(), compact);
            assert!(
                visual
                    .debug_bounds("plugin-control-worktrees-location")
                    .is_some()
            );
            assert!(visual.debug_bounds("plugin-control-git-branch").is_some());
            assert_eq!(visual.debug_bounds("live-chat-model").is_some(), !compact);
            assert_eq!(visual.debug_bounds("live-chat-mode").is_some(), !compact);
            let surface = visual.debug_bounds("composer-surface").unwrap();
            for selector in ["live-chat-send", "live-attach"] {
                let bounds = visual.debug_bounds(selector).unwrap();
                assert!(
                    bounds.left() >= surface.left() && bounds.right() <= surface.right(),
                    "{selector} outside composer at {width}"
                );
            }
            if compact {
                let location = visual
                    .debug_bounds("plugin-control-worktrees-location")
                    .unwrap();
                let branch = visual.debug_bounds("plugin-control-git-branch").unwrap();
                let statistics = visual.debug_bounds("composer-stats").unwrap();
                assert_eq!(location.center().y, statistics.center().y);
                assert_eq!(branch.center().y, statistics.center().y);
                assert!(location.right() <= branch.left());
                if width < 440. {
                    assert_eq!(location.size, size(px(24.), px(24.)));
                    assert_eq!(branch.size, size(px(24.), px(24.)));
                } else {
                    assert!(location.size.width > px(24.));
                    assert!(branch.size.width > px(24.));
                }
                assert!(
                    branch.right() < statistics.left(),
                    "branch {branch:?} overlaps statistics {statistics:?} at width {width}"
                );
                hover(visual, "composer-stats");
                let panel = visual.debug_bounds("composer-statistics-panel").unwrap();
                assert!(panel.left() >= px(0.) && panel.right() <= px(width));
                assert!(panel.bottom() < location.top());
                for (metric, details) in [
                    ("composer_tokens", 6),
                    ("composer_speed", 2),
                    ("composer_cost", 5),
                    ("composer_cache", 3),
                    ("composer_turns", 1),
                ] {
                    let prefix = format!("statistics-plugin-statistics-{metric}");
                    for selector in std::iter::once(prefix.clone())
                        .chain((0..details).map(|index| format!("{prefix}-{index}")))
                    {
                        statistics_row(visual, &selector, panel);
                    }
                }
                hover(visual, "composer-statistics-panel");
                assert!(visual.debug_bounds("composer-statistics-panel").is_some());
                leave(visual);
                assert!(visual.debug_bounds("composer-statistics-panel").is_none());
            }
            assert_eq!(
                view.read_with(visual, |view, cx| view.input.read(cx).value()),
                "Keep this draft"
            );
        }
        tap(visual, "plugin-control-worktrees-location");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            wait(visual, |_| true);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if ["create", "fork"]
                .into_iter()
                .all(|id| visual.debug_bounds(id).is_some())
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worktree actions did not appear"
            );
        }
        visual.simulate_keystrokes("escape");
        wait(visual, |_| true);
        tap(visual, "composer-settings");
        for selector in [
            "plugin-control-statistics-context",
            "plugin-control-worktrees-create",
            "plugin-control-worktrees-fork",
        ] {
            assert!(
                visual.debug_bounds(selector).is_none(),
                "duplicate or indicator row: {selector}"
            );
        }
        assert!(view.read_with(visual, |view, cx| {
            view.can_invoke_contribution(ui::Intent::CreateWorktree, cx)
        }));
        assert!(view.read_with(visual, |view, cx| {
            view.can_invoke_contribution(ui::Intent::ForkWorktree, cx)
        }));
        let panel = visual.debug_bounds("composer-settings-panel").unwrap();
        let models = visual.debug_bounds("model-controls").unwrap();
        let separator = visual.debug_bounds("composer-settings-separator").unwrap();
        assert!(models.bottom() <= separator.top());
        for (selector, key) in [
            ("composer-mode-page", "composer_mode_code"),
            ("composer-permission-page", "composer_permission_ask"),
        ] {
            let button = visual.debug_bounds(selector).unwrap();
            assert_eq!(button.left(), panel.left());
            assert_eq!(button.right(), panel.right());
            assert!(button.top() >= separator.bottom());
            let label = Box::leak(format!("{key}-label").into_boxed_str());
            let indicator = Box::leak(format!("{key}-indicator").into_boxed_str());
            let label = visual.debug_bounds(label).unwrap();
            let indicator = visual.debug_bounds(indicator).unwrap();
            assert_eq!(label.left(), button.left() + px(10.));
            assert_eq!(indicator.right(), button.right() - px(10.));
            assert!(label.right() < indicator.left());
        }
        assert!(visual.debug_bounds("plugin-control-goals-goal").is_none());
        tap(visual, "model-controls-choose");
        wait(visual, |_| true);
        tap(visual, "composer-model-option-0-alternate-model");
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().model == "alternate-model"
                && !view.read(cx).pending
        });
        tap(visual, "model-controls-back");
        tap(visual, "composer-permission-page");
        for width in [360., 520.] {
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            wait(visual, |_| true);
            assert_options(
                visual,
                &[
                    "composer_permission_ask",
                    "composer_permission_project",
                    "composer_permission_full",
                ],
            );
        }
        tap(visual, "composer-settings-back");
        assert!(visual.debug_bounds("composer-permission-page").is_some());
        tap(visual, "composer-permission-page");
        tap(visual, "composer_permission_full-option");
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().permission == Permission::Full
                && !view.read(cx).pending
        });
        visual.simulate_keystrokes("escape");
        if visual.debug_bounds("composer-settings-panel").is_none() {
            tap(visual, "composer-settings");
        }
        tap(visual, "composer-mode-page");
        assert_options(visual, &["composer_mode_code", "composer_mode_plan"]);
        tap(visual, "composer_mode_plan-option");
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().mode == WorkMode::Plan && !view.read(cx).pending
        });
        visual.simulate_keystrokes("escape");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Keep this draft"
        );
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        let saved = snapshot
            .sessions
            .iter()
            .find(|session| session.id == fixture.session.id)
            .unwrap();
        assert_eq!(saved.config.model, "alternate-model");
        assert_eq!(saved.config.permission, Permission::Full);
        assert_eq!(saved.config.mode, WorkMode::Plan);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

fn context_ready(view: &View, cx: &App) -> bool {
    let registry = view.contributions.read(cx);
    let entries = registry.entries(sailry_protocol::plugin::ui::Slot::Context, cx);
    registry.ready(cx)
        && [("worktrees", "location"), ("git", "branch")]
            .into_iter()
            .all(|(package, id)| {
                entries.iter().any(|entry| {
                    entry.key.package.name == package && entry.key.id == id && entry.state.enabled
                })
            })
}

fn statistics_row(visual: &mut VisualTestContext, selector: &str, panel: Bounds<Pixels>) {
    let selector = Box::leak(selector.to_owned().into_boxed_str());
    let mut row = visual.debug_bounds(selector).unwrap();
    let delta = if row.top() < panel.top() {
        panel.top() - row.top() + px(8.)
    } else if row.bottom() > panel.bottom() {
        panel.bottom() - row.bottom() - px(8.)
    } else {
        px(0.)
    };
    if delta != px(0.) {
        visual.simulate_mouse_move(panel.center(), None, Modifiers::default());
        visual.simulate_event(ScrollWheelEvent {
            position: panel.center(),
            delta: ScrollDelta::Pixels(point(px(0.), delta)),
            ..Default::default()
        });
        wait(visual, |_| true);
        assert_eq!(
            visual.debug_bounds("composer-statistics-panel").unwrap(),
            panel
        );
        let visible = visual.debug_bounds(selector).unwrap();
        assert_ne!(visible.top(), row.top(), "{selector} did not scroll");
        row = visible;
    }
    assert!(row.left() >= panel.left() && row.right() <= panel.right());
    assert!(
        row.top() >= panel.top() && row.bottom() <= panel.bottom(),
        "{selector}: {row:?} outside {panel:?}"
    );
}

fn assert_options(visual: &mut VisualTestContext, keys: &[&str]) {
    let panel = visual.debug_bounds("composer-settings-panel").unwrap();
    let back = visual.debug_bounds("composer-settings-back").unwrap();
    assert_eq!(back.size.height, px(32.));
    let mut previous = back.bottom();
    for key in keys {
        let bounds = |visual: &mut VisualTestContext, part: &str| {
            visual
                .debug_bounds(Box::leak(format!("{key}-{part}").into_boxed_str()))
                .unwrap()
        };
        let option = bounds(visual, "option");
        let icon = bounds(visual, "icon");
        let label = bounds(visual, "label");
        let description = bounds(visual, "description");
        let indicator = bounds(visual, "indicator");
        assert_eq!(option.left(), panel.left());
        assert_eq!(option.right(), panel.right());
        assert_eq!(option.top() - previous, px(4.));
        assert!(option.size.height >= px(48.));
        assert_eq!(icon.left(), option.left() + px(10.));
        assert_eq!(indicator.right(), option.right() - px(10.));
        assert!(icon.right() < label.left());
        assert_eq!(label.left(), description.left());
        assert!(description.right() < indicator.left());
        assert!(description.top() > label.bottom());
        assert!(description.bottom() <= option.bottom() - px(8.));
        previous = option.bottom();
    }
}

#[gpui::test]
fn raw_source_and_typed_markers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("/");
        wait(visual, |cx| view.read(cx).contributions.read(cx).ready(cx));
        assert!(visual.debug_bounds("live-reference-picker").is_some());
        visual.simulate_keystrokes("escape cmd-a");
        let source = "# 标题\n\n**bold** and `code` @文件";
        visual.simulate_input(source);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("markdown-document").is_none());
        view.read_with(visual, |view, cx| {
            let input = view.input.read(cx);
            assert_eq!(input.value(), source);
            assert!(input.tokens().is_empty());
            assert!(view.active_references(cx).is_empty());
        });
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            let view = view.read(cx);
            view.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .last()
                    .is_some_and(|run| run.status == Status::Completed)
            }) && !view.busy()
        });
        assert!(
            fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request["messages"]
                    .to_string()
                    .contains(&serde_json::to_string(source).unwrap()))
        );
        assert!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().is_empty()
                && view.input.read(cx).tokens().is_empty())
        );
        view.read_with(visual, |view, _| {
            assert!(
                !view
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, sailry_protocol::conversation::Part::Reference(_)))
            );
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
