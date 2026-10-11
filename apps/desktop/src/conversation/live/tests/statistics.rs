use super::*;
use gpui_kit::test::TestWindowExt as _;
use sailry_protocol::{
    conversation::{Statistics, Usage},
    plugin::ui::Slot,
};

fn hover_summary(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("composer-stats").label(),
            Some(tr("composer_statistics").as_ref())
        );
        window.hover("composer-stats", cx);
    });
    for _ in 0..12 {
        visual.executor().advance_clock(Duration::from_millis(100));
        wait(visual, |_| true);
    }
}

fn details(entry: &crate::plugins::contributions::Entry, own: &str, child: &str) {
    assert_eq!(entry.state.details.len(), 2);
    for (detail, label, value) in [
        (&entry.state.details[0], "This session", own),
        (&entry.state.details[1], "Subagents", child),
    ] {
        assert_eq!(detail.label.label(&rust_i18n::locale()), label);
        assert_eq!(detail.value, value);
    }
}

#[gpui::test]
fn restores_totals_and_keeps_focus(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        git2::Repository::init(fixture.directory.path().join("project")).unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            let view = view.read(cx);
            view.connected()
                && view.git
                && view.contributions.read(cx).ready(cx)
                && view.registered_statistics(cx).is_empty()
        });
        assert_eq!(
            view.read_with(visual, |view, _| view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .statistics
                .clone()),
            Statistics::default()
        );
        assert!(visual.debug_bounds("composer-stats").is_none());
        click(visual, "live-chat-input");
        visual.simulate_input("Count this response");
        wait(visual, |cx| {
            let view = view.read(cx);
            view.contributions.read(cx).ready(cx) && view.registered_statistics(cx).is_empty()
        });
        assert!(visual.debug_bounds("composer-stats").is_none());
        visual.simulate_keystrokes("enter");
        let mut expected = Statistics {
            turns: 1,
            responses: 1,
            context_tokens: Some(16),
            usage: Some(Usage {
                input: 12,
                output: 4,
                cached_input: 0,
                reasoning: 0,
            }),
            ..Default::default()
        };
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.statistics.usage == expected.usage && snapshot.statistics.turns == 1
                })
        });
        expected.generation = view.read_with(visual, |view, _| {
            view.history
                .snapshot
                .as_ref()
                .unwrap()
                .statistics
                .generation
                .clone()
        });
        let generation = expected
            .generation
            .as_ref()
            .expect("request timing is recorded");
        assert!(generation.elapsed_us > 0);
        assert_eq!(generation.output_tokens, 4);
        assert_eq!(generation.responses, 1);
        wait(visual, |cx| {
            let entries = view
                .read(cx)
                .contributions
                .read(cx)
                .entries(Slot::Statistics, cx);
            [
                ("composer_tokens", "16"),
                ("composer_turns", "1"),
                ("composer_cache", "0%"),
                ("composer_cost", "0.00"),
            ]
            .into_iter()
            .all(|(id, value)| {
                entries.iter().any(|entry| {
                    entry.key.package.name == "statistics"
                        && entry.key.id == id
                        && entry.state.value.as_str() == Some(value)
                })
            })
        });
        view.read_with(visual, |view, cx| {
            let entries = view.contributions.read(cx).entries(Slot::Statistics, cx);
            let metric = |id: &str| {
                entries
                    .iter()
                    .find(|entry| entry.key.package.name == "statistics" && entry.key.id == id)
                    .unwrap()
            };
            for (id, icon) in [
                ("composer_speed", "gauge"),
                ("composer_cost", "dollar-sign"),
                ("composer_turns", "messages-square"),
            ] {
                assert_eq!(metric(id).declaration.icon.as_deref(), Some(icon));
                assert!(metric(id).icon().is_some());
            }
            let rate = metric("composer_speed")
                .state
                .value
                .as_str()
                .unwrap()
                .strip_suffix("/s")
                .unwrap()
                .parse::<f64>()
                .unwrap();
            let expected =
                generation.output_tokens as f64 * 1_000_000. / generation.elapsed_us as f64;
            assert!((rate - expected).abs() < 0.050_001);
            assert!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .statistics
                    .cost
                    .is_none()
            );
            details(metric("composer_tokens"), "16", "0");
            details(metric("composer_cost"), "—", "$0.00");
        });
        visual.simulate_input("Preserved draft 中文 🙂");
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for width in [480., 1000.] {
                let handle = visual.update(|window, cx| {
                    Theme::change(mode, Some(window), cx);
                    window.window_handle()
                });
                visual.simulate_window_resize(handle, size(px(width), px(820.)));
                wait(visual, |cx| {
                    view.read(cx).compact_composer == (width < 680.)
                });
                let context = visual.debug_bounds("composer-context-bar").unwrap();
                assert!(visual.debug_bounds("live-compact").is_none());
                if width < 680. {
                    assert!(
                        visual
                            .debug_bounds("plugin-control-statistics-context")
                            .is_none()
                    );
                    let location = visual
                        .debug_bounds("plugin-control-worktrees-location")
                        .unwrap();
                    let branch = visual.debug_bounds("plugin-control-git-branch").unwrap();
                    let stats = visual.debug_bounds("composer-stats").unwrap();
                    assert!(location.right() <= branch.left());
                    assert!(branch.right() < stats.left());
                    assert_eq!(location.center().y, stats.center().y);
                    assert_eq!(branch.center().y, stats.center().y);
                    hover_summary(visual);
                    assert!(visual.debug_bounds("composer-statistics-panel").is_some());
                    assert!(
                        visual
                            .debug_bounds("statistics-plugin-statistics-composer_tokens-0")
                            .is_some()
                    );
                    assert!(
                        visual
                            .debug_bounds("statistics-plugin-statistics-composer_cost-1")
                            .is_some()
                    );
                    fixture::leave(visual);
                    assert!(visual.debug_bounds("composer-statistics-panel").is_none());
                    continue;
                }
                let ring = visual
                    .debug_bounds("plugin-control-statistics-context")
                    .unwrap();
                let toolbar = visual.debug_bounds("composer-toolbar").unwrap();
                assert!(ring.left() >= toolbar.left() && ring.right() <= toolbar.right());
                assert!(ring.top() >= toolbar.top() && ring.bottom() <= toolbar.bottom());
                let stats = visual.debug_bounds("composer-stats").unwrap();
                assert!(stats.left() >= context.left());
                assert!(stats.right() <= context.right());
                assert!(stats.top() >= context.top());
                assert!(stats.bottom() <= context.bottom());
                let branch = visual.debug_bounds("plugin-control-git-branch").unwrap();
                assert!(
                    branch.right() < stats.left() || branch.bottom() < stats.top(),
                    "worktree and statistics must not overlap at width {width}"
                );
                assert!(branch.size.width >= px(48.));
                for selector in [
                    "plugin-stat-statistics-composer_tokens",
                    "plugin-stat-statistics-composer_cache",
                    "plugin-stat-statistics-composer_turns",
                ] {
                    let metric = visual.debug_bounds(selector).unwrap();
                    assert!(metric.left() >= stats.left() && metric.right() <= stats.right());
                }
                for (metric, selector, count) in [
                    (
                        "composer_tokens",
                        "plugin-stat-statistics-composer_tokens",
                        2,
                    ),
                    ("composer_speed", "plugin-stat-statistics-composer_speed", 2),
                    ("composer_cost", "plugin-stat-statistics-composer_cost", 2),
                    ("composer_cache", "plugin-stat-statistics-composer_cache", 3),
                    ("composer_turns", "plugin-stat-statistics-composer_turns", 1),
                ] {
                    fixture::hover(visual, selector);
                    let tooltip = visual.debug_bounds("composer-stat-details").unwrap();
                    assert!(
                        tooltip.bottom() <= px(820.)
                            && tooltip.left() >= px(0.)
                            && tooltip.right() <= px(width)
                    );
                    assert_eq!(tooltip.size.width, px(220.));
                    let prefix = format!("statistics-plugin-statistics-{metric}");
                    let header = visual
                        .debug_bounds(Box::leak(prefix.clone().into_boxed_str()))
                        .unwrap();
                    assert!(header.top() >= tooltip.top() && header.bottom() < tooltip.bottom());
                    view.read_with(visual, |view, cx| {
                        let entry = view
                            .contributions
                            .read(cx)
                            .entries(Slot::Statistics, cx)
                            .into_iter()
                            .find(|entry| {
                                entry.key.package.name == "statistics" && entry.key.id == metric
                            })
                            .unwrap();
                        assert_eq!(entry.state.details.len(), count);
                        assert_eq!(
                            entry
                                .state
                                .value
                                .as_str()
                                .is_some_and(|value| value.ends_with('%')),
                            metric == "composer_cache",
                            "cache hit rate belongs to the cache tooltip"
                        );
                    });
                    let mut previous = header.bottom();
                    for index in 0..count {
                        let row = visual
                            .debug_bounds(Box::leak(format!("{prefix}-{index}").into_boxed_str()))
                            .unwrap();
                        assert!(row.left() >= tooltip.left() && row.right() <= tooltip.right());
                        assert!(row.top() > previous);
                        previous = row.bottom();
                    }
                    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
                }
            }
        }
        visual.simulate_input(" still focused");
        view.read_with(visual, |view, cx| {
            assert_eq!(
                view.input.read(cx).value().as_str(),
                "Preserved draft 中文 🙂 still focused"
            );
            assert_eq!(view.history.snapshot.as_ref().unwrap().statistics, expected);
        });
        visual.update(|window, _| window.remove_window());
        let (restored, visual) =
            fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            let view = restored.read(cx);
            view.connected()
                && view.contributions.read(cx).ready(cx)
                && view
                    .contributions
                    .read(cx)
                    .entries(Slot::Statistics, cx)
                    .iter()
                    .any(|entry| {
                        entry.key.package.name == "statistics"
                            && entry.key.id == "composer_tokens"
                            && entry.state.value.as_str() == Some("16")
                    })
        });
        assert!(visual.debug_bounds("composer-stats").is_some());
        assert_eq!(
            restored.read_with(visual, |view, _| view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .statistics
                .clone()),
            expected
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn counts_turns_instead_of_responses(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                support::plugin_tool("progress", "update_plan"),
                serde_json::json!({
                    "title": "Count fixture",
                    "steps": [{"description": "Record one turn", "state": "completed"}],
                }),
            )],
        );
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.start();
        wait(visual, |cx| {
            let view = view.read(cx);
            view.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.statistics.turns == 1 && snapshot.statistics.responses == 2
            }) && view
                .contributions
                .read(cx)
                .entries(Slot::Statistics, cx)
                .iter()
                .any(|entry| {
                    entry.key.package.name == "statistics"
                        && entry.key.id == "composer_turns"
                        && entry.state.value.as_str() == Some("1")
                        && entry.state.details[0].value == "2"
                })
        });
        view.read_with(visual, |view, cx| {
            let statistics = &view.history.snapshot.as_ref().unwrap().statistics;
            assert_eq!((statistics.turns, statistics.responses), (1, 2));
            assert!(statistics.cost.is_none());
            let entry = view
                .contributions
                .read(cx)
                .entries(Slot::Statistics, cx)
                .into_iter()
                .find(|entry| {
                    entry.key.package.name == "statistics" && entry.key.id == "composer_cost"
                })
                .unwrap();
            assert_eq!(entry.state.value.as_str(), Some("0.00"));
            details(&entry, "—", "$0.00");
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
