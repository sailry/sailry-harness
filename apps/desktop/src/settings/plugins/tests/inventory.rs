use super::*;
use crate::settings::Section;
use crate::settings::plugins::test_support::reveal;

fn install(fixture: &Fixture) {
    fixture.package("1.0.0");
    fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 0,
        expected_digest: None,
    });
}

fn select(owner: &Entity<Workspace>, visual: &mut VisualTestContext, section: Section) {
    owner.update(visual, |owner, cx| owner.select(section, cx));
    draw(visual);
}

fn ready(owner: &Entity<Workspace>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        owner
            .read(cx)
            .plugin_catalog
            .metadata
            .as_ref()
            .unwrap()
            .read(cx)
            .settled()
    });
}

fn focus(visual: &mut VisualTestContext, id: &str) {
    use gpui_kit::test::TestWindowExt as _;
    let id: ElementId = SharedString::from(id.to_owned()).into();
    for _ in 0..100 {
        draw(visual);
        assert!(reveal(visual, id.clone()), "focus target is not visible");
        if visual.update(|window, _| {
            window
                .try_find(id.clone())
                .and_then(|target| target.focused())
        }) == Some(true)
        {
            return;
        }
        visual.update(|window, cx| window.press("tab", cx));
    }
    assert_eq!(
        visual.update(|window, _| window.find(id).focused()),
        Some(true)
    );
}

#[gpui::test]
#[ignore = "opt-in inventory CPU sample; does not measure native presentation latency"]
fn samples_scroll_work(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use sailry_protocol::plugin::catalog::Source;
    use std::time::Instant;

    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::PluginCatalog(catalog) = fixture.execute(Command::SearchPluginCatalog {
            force: false,
            source: Source::Official,
            query: String::new(),
            page: 1,
        }) else {
            panic!("official catalog expected");
        };
        assert_eq!(catalog.entries.len(), 44);
        let initial = fixture.public_packages(false);
        for entry in &catalog.entries {
            assert!(entry.bundled);
            if initial.iter().any(|package| package.name == entry.name) {
                continue;
            }
            let Output::Plugin(installed) = fixture.execute(Command::InstallBundledPlugin {
                name: entry.name.clone(),
                expected_revision: 0,
            }) else {
                panic!("bundled plugin expected");
            };
            assert_eq!(installed.summary.name, entry.name);
        }
        let packages = fixture.public_packages(false);
        assert_eq!(packages.len(), catalog.entries.len());
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        owner.read_with(visual, |owner, cx| {
            assert_eq!(owner.plugin_catalog.packages, packages);
            assert_eq!(
                owner
                    .plugin_catalog
                    .metadata
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .entries
                    .len(),
                packages.len()
            );
        });
        // Dispatch into the actual finite whole-page list viewport.
        let viewport =
            visual
                .debug_bounds("settings-viewport")
                .unwrap()
                .intersect(&visual.update(|window, _| {
                    Bounds::new(point(px(0.), px(0.)), window.viewport_size())
                }));
        assert!(viewport.size.width > px(0.) && viewport.size.height > px(0.));
        let position = viewport.center();
        let wheel = |visual: &mut VisualTestContext, dy| {
            visual.simulate_event(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                touch_phase: TouchPhase::Moved,
                modifiers: Modifiers::default(),
            });
            draw(visual);
        };
        let top = visual.debug_bounds("plugin-cards").unwrap().top();
        wheel(visual, -40.);
        assert_eq!(
            visual.debug_bounds("plugin-cards").unwrap().top(),
            top - px(40.)
        );
        wheel(visual, 40.);
        assert_eq!(visual.debug_bounds("plugin-cards").unwrap().top(), top);
        for _ in 0..12 {
            wheel(visual, -4.);
            wheel(visual, 4.);
        }
        focus(visual, &format!("plugin-details-{}", packages[0].name));
        let focus = visual.update(|window, cx| window.focused(cx).unwrap());
        let identity = owner.read_with(visual, |owner, _| {
            (
                owner.plugin_catalog.metadata.as_ref().unwrap().entity_id(),
                owner.plugin_catalog.updates.as_ref().unwrap().entity_id(),
            )
        });
        let path = visual.update(|window, _| window.find("plugins-install").path().to_vec());
        let bounds = visual.debug_bounds("plugin-cards").unwrap();
        let requests = fixture.transport.requests.lock().unwrap().len();
        let mut elapsed = Vec::with_capacity(120);
        for frame in 0..120 {
            let started = Instant::now();
            wheel(visual, if frame % 2 == 0 { -4. } else { 4. });
            elapsed.push(started.elapsed());
        }
        elapsed.sort();
        eprintln!(
            "sailry-desktop: plugin inventory scroll sample {}",
            serde_json::json!({
                "remote": remote,
                "packages": packages.len(),
                "cards": catalog.entries.len(),
                "samples": elapsed.len(),
                "dispatch_draw_median_us": elapsed[elapsed.len() / 2].as_micros(),
                "dispatch_draw_p95_us": elapsed[elapsed.len() * 95 / 100].as_micros(),
                "presentation_measured": false
            })
        );
        assert_eq!(visual.debug_bounds("plugin-cards").unwrap(), bounds);
        visual.update(|window, _| {
            assert_eq!(window.find("plugins-install").path(), path);
            assert!(focus.is_focused(window));
        });
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.section, Section::Plugins);
            assert_eq!(owner.plugin_catalog.packages, packages);
            assert_eq!(
                (
                    owner.plugin_catalog.metadata.as_ref().unwrap().entity_id(),
                    owner.plugin_catalog.updates.as_ref().unwrap().entity_id(),
                ),
                identity
            );
        });
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        assert_eq!(fixture.public_packages(false), packages);

        let mut seen = std::collections::BTreeSet::new();
        let state = owner.read_with(visual, |owner, _| {
            owner
                .plugin_catalog
                .inventory
                .as_ref()
                .unwrap()
                .list
                .clone()
        });
        let step = f32::from(viewport.size.height) * 0.75;
        for _ in 0..40 {
            let mut rendered = 0;
            for plugin in &packages {
                let selector = Box::leak(format!("plugin-card-{}", plugin.name).into_boxed_str());
                if let Some(bounds) = visual.debug_bounds(selector) {
                    rendered += 1;
                    if bounds.intersect(&viewport).size.height > px(0.) {
                        seen.insert(plugin.name.clone());
                    }
                }
            }
            assert!(
                rendered < packages.len(),
                "scrolling must not render all 44 cards"
            );
            if state.is_scrolled_to_end() == Some(true) {
                break;
            }
            wheel(visual, -step);
        }
        assert_eq!(state.is_scrolled_to_end(), Some(true));
        assert_eq!(seen.len(), packages.len());
        assert!(visual.update(|window, _| focus.is_focused(window)));
        for _ in 0..40 {
            let offset = state.logical_scroll_top();
            if offset.item_ix == 0 && offset.offset_in_item == px(0.) {
                break;
            }
            wheel(visual, step);
        }
        assert_eq!(state.logical_scroll_top().item_ix, 0);
        assert_eq!(state.logical_scroll_top().offset_in_item, px(0.));
        assert_eq!(visual.debug_bounds("plugin-cards").unwrap(), bounds);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        assert_eq!(fixture.public_packages(false), packages);
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_measured_rows(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let installed = fixture.public_packages(false);
        for name in ["files", "git", "slack"] {
            if installed.iter().any(|plugin| plugin.name == name) {
                continue;
            }
            fixture.execute(Command::InstallBundledPlugin {
                name: name.into(),
                expected_revision: 0,
            });
        }
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(420.)));
        draw(visual);
        let state = owner.read_with(visual, |owner, _| {
            owner
                .plugin_catalog
                .inventory
                .as_ref()
                .unwrap()
                .list
                .clone()
        });
        let viewport = visual.debug_bounds("settings-viewport").unwrap();
        let body = visual.debug_bounds("settings-content").unwrap();
        let grid = visual.debug_bounds("plugin-cards").unwrap();
        let rem = visual.update(|window, _| window.rem_size());
        assert_eq!(body.center().x, viewport.center().x);
        assert_eq!(
            body.size.width,
            viewport.size.width.min(px(crate::preview::CONTENT_WIDTH))
        );
        assert_eq!(grid.center().x, viewport.center().x);
        assert_eq!(grid.size.width, body.size.width - rem * 3.);
        let index = owner.read_with(visual, |owner, _| {
            owner
                .plugin_catalog
                .packages
                .iter()
                .position(|plugin| plugin.name == "example")
                .unwrap()
                / 2
                + 1
        });
        let row = if index == 1 {
            "plugin-cards"
        } else {
            Box::leak(format!("plugin-card-row-{index}").into_boxed_str())
        };
        focus(visual, "plugin-details-example");
        let focus = visual.update(|window, cx| window.focused(cx).unwrap());
        let wheel = |visual: &mut VisualTestContext, dy| {
            visual.simulate_event(ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                touch_phase: TouchPhase::Moved,
                modifiers: Modifiers::default(),
            });
            draw(visual);
        };
        wheel(visual, -40.);
        let offset = state.logical_scroll_top();
        assert!(offset.item_ix > 0 || offset.offset_in_item > px(0.));
        let anchor = visual.debug_bounds(row).unwrap().origin;
        let summary = visual.debug_bounds("plugin-summary-example").unwrap();
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 1,
            enabled: false,
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|plugin| plugin.name == "example" && plugin.revision == 2 && !plugin.enabled)
        });
        ready(&owner, visual);
        assert!(visual.update(|window, _| focus.is_focused(window)));
        assert_eq!(state.logical_scroll_top().item_ix, offset.item_ix);
        assert_eq!(
            state.logical_scroll_top().offset_in_item,
            offset.offset_in_item
        );
        assert_eq!(visual.debug_bounds(row).unwrap().origin, anchor);

        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["description"] = serde_json::Value::String(
            "A description that wraps across two lines while preserving every existing card control and the current scroll anchor".into(),
        );
        std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 2,
            expected_digest: None,
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|plugin| plugin.name == "example" && plugin.revision == 3 && !plugin.enabled)
        });
        ready(&owner, visual);
        assert!(
            visual
                .debug_bounds("plugin-summary-example")
                .unwrap()
                .size
                .height
                > summary.size.height
        );
        assert!(visual.update(|window, _| focus.is_focused(window)));
        assert_eq!(state.logical_scroll_top().item_ix, offset.item_ix);
        assert_eq!(
            state.logical_scroll_top().offset_in_item,
            offset.offset_in_item
        );
        assert_eq!(visual.debug_bounds(row).unwrap().origin, anchor);
        visual.simulate_window_resize(handle, size(px(1040.), px(420.)));
        draw(visual);
        let card = visual.debug_bounds("plugin-card-example").unwrap();
        let text = visual.debug_bounds("plugin-summary-example").unwrap();
        assert!(
            text.left() >= card.left()
                && text.right() <= card.right()
                && text.top() >= card.top()
                && text.bottom() <= card.bottom(),
            "summary exceeds card: card={card:?}, summary={text:?}, row={:?}, viewport={:?}",
            visual.debug_bounds(row).unwrap(),
            visual.debug_bounds("settings-viewport").unwrap()
        );
        assert!(visual.update(|window, _| focus.is_focused(window)));
        assert_eq!(state.logical_scroll_top().item_ix, offset.item_ix);
        assert_eq!(
            state.logical_scroll_top().offset_in_item,
            offset.offset_in_item
        );
        wait(visual, |cx| owner.read(cx).plugin_catalog.python.connected);
        assert!(owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.python.preparations.is_empty()
        }));
        visual.simulate_window_resize(handle, size(px(1040.), px(300.)));
        owner.update(visual, |owner, cx| {
            owner.plugin_catalog.python.connected = false;
            cx.notify();
        });
        draw(visual);
        state.scroll_to(ListOffset {
            item_ix: 1,
            offset_in_item: px(0.),
        });
        draw(visual);
        assert_eq!(state.item_is_above_viewport(0), Some(true));
        let extent = state.max_offset_for_scrollbar().y;
        let first = visual.debug_bounds("plugin-cards").unwrap();
        for connected in [true, false] {
            owner.update(visual, |owner, cx| {
                owner.plugin_catalog.python.connected = connected;
                cx.notify();
            });
            draw(visual);
            assert_eq!(state.item_is_above_viewport(0), Some(true));
            assert_eq!(
                state.max_offset_for_scrollbar().y,
                extent + if connected { rem } else { px(0.) }
            );
            assert_eq!(visual.debug_bounds("plugin-cards").unwrap(), first);
            assert!(visual.update(|window, _| focus.is_focused(window)));
        }
        fixture.close(visual);
    }
}

#[gpui::test]
fn shows_current_components(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        select(&owner, visual, Section::Skills);
        shown(visual, "skill-card-example:analysis", false);
        shown(visual, "empty-plugins_no_skills", true);
        select(&owner, visual, Section::Mcp);
        shown(visual, "mcp-card-example", true);
        shown(visual, "mcp-add", true);
        assert!(visual.debug_bounds("mcp-plugins-tab").is_none());
        assert!(visual.debug_bounds("mcp-standalone-tab").is_none());
        shown(visual, "plugin-component-example:native", false);
        select(&owner, visual, Section::Plugins);
        tap(visual, "plugin-details-example");
        shown(visual, "plugin-skill-analysis", true);
        shown(visual, "plugin-server-native", true);
        assert!(
            visual
                .debug_bounds("plugin-live-details")
                .unwrap()
                .size
                .width
                <= px(520.)
        );
        visual.simulate_keystrokes("escape");
        shown(visual, "plugin-live-details", false);
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 1,
            enabled: false,
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| package.name == "example" && package.revision == 2)
        });
        ready(&owner, visual);
        assert!(owner.read_with(visual, |owner, cx| {
            owner
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .skills()
                .iter()
                .all(|skill| !skill.starts_with("example:"))
        }));
        select(&owner, visual, Section::Skills);
        shown(visual, "skill-card-example:analysis", false);
        fixture.package("2.0.0");
        fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 2,
            expected_digest: None,
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| package.name == "example" && package.revision == 3)
        });
        ready(&owner, visual);
        fixture.execute(Command::RemovePlugin {
            name: "example".into(),
            expected_revision: 3,
        });
        shown(visual, "skill-card-example:analysis", false);
        assert_eq!(fixture.plugins().len(), 0);
        select(&owner, visual, Section::Plugins);
        shown(visual, "plugin-details-example", false);
        shown(visual, "plugin-details-web-search", true);
        shown(visual, "empty-plugins_none", false);
        shown(visual, "plugin-details-databases", true);
        shown(visual, "computer-permissions", false);
        fixture.close(visual);
    }
}

#[gpui::test]
fn shows_supported_sse_servers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        std::fs::write(
            fixture.directory.path().join("project/package/mcp.json"),
            serde_json::json!({
                "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
                "mcpServers":{"legacy":{"type":"sse","url":"https://example.invalid/sse"}}
            })
            .to_string(),
        )
        .unwrap();
        let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
            expected_digest: None,
        }) else {
            panic!("plugin expected")
        };
        assert!(info.issues.is_empty());
        assert_eq!(
            info.mcp[0].transport,
            sailry_protocol::plugin::McpTransport::Sse
        );
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        select(&owner, visual, Section::Mcp);
        shown(visual, "mcp-card-example", true);
        select(&owner, visual, Section::Plugins);
        tap(visual, "plugin-details-example");
        shown(visual, "plugin-server-legacy", true);
        shown(visual, "plugin-authorize-legacy", true);
        shown(visual, "plugin-details-cancel", true);
        shown(visual, "plugin-details-uninstall", true);
        assert!(
            visual
                .debug_bounds("plugin-details-mcp-configure")
                .is_none()
        );
        assert!(visual.debug_bounds("plugin-details-configure").is_none());
        visual.simulate_keystrokes("escape");
        shown(visual, "plugin-live-details", false);
        fixture.close(visual);
    }
}

#[gpui::test]
fn uninstalls_captured_details(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let installed = fixture.public_packages(false);
        let other = fixture.public_packages(true);
        let requests = fixture.transport.requests.lock().unwrap().len();
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        tap(visual, "plugin-details-example");
        shown(visual, "plugin-skill-analysis", true);
        tap(visual, "plugin-details-uninstall");
        let (title, detail) = crate::prompts::tests::wait(visual);
        assert_eq!(title, tr("plugins_uninstall"));
        assert!(detail.contains("example"));
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        crate::prompts::tests::answer(visual, "settings_cancel");
        shown(visual, "plugin-live-details", true);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(!visual.has_pending_prompt());
        assert_eq!(fixture.public_packages(false), installed);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);

        tap(visual, "plugin-details-uninstall");
        crate::prompts::tests::wait(visual);
        fixture.bind(&owner, visual, true);
        ready(&owner, visual);
        owner.read_with(visual, |owner, _| {
            assert_eq!(
                owner
                    .provider_link
                    .as_ref()
                    .unwrap()
                    .binding
                    .client
                    .target(),
                fixture.other.id()
            );
            assert_eq!(owner.plugin_catalog.packages, other);
        });
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        wait(visual, |_| fixture.plugins().is_empty());
        shown(visual, "plugin-live-details", false);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(
            fixture.transport.requests.lock().unwrap().len(),
            requests + 1
        );
        assert_eq!(
            fixture.public_packages(false),
            installed
                .into_iter()
                .filter(|package| package.name != "example")
                .collect::<Vec<_>>()
        );
        assert_eq!(fixture.public_packages(true), other);
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.plugin_catalog.packages.clone()),
            other
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_conflicting_uninstall(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let other = fixture.public_packages(true);
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        tap(visual, "plugin-details-example");
        shown(visual, "plugin-skill-analysis", true);
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 1,
            enabled: false,
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| {
                    package.name == "example" && package.revision == 2 && !package.enabled
                })
        });
        let updated = fixture.public_packages(false);
        let requests = fixture.transport.requests.lock().unwrap().len();
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "plugin-details-uninstall");
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        let (title, detail) = crate::prompts::tests::wait(visual);
        assert_eq!(title, tr("plugins_uninstall"));
        assert!(detail.contains("example"));
        crate::prompts::tests::answer(visual, "settings_cancel");
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("plugins_conflict")
        );
        assert_eq!(
            fixture.transport.requests.lock().unwrap().len(),
            requests + 1
        );
        assert_eq!(fixture.public_packages(false), updated);
        assert_eq!(fixture.public_packages(true), other);
        shown(visual, "plugin-live-details", true);
        shown(visual, "plugin-skill-analysis", true);
        shown(visual, "plugin-details-uninstall", true);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(!visual.has_pending_prompt());
        tap(visual, "plugin-details-cancel");
        shown(visual, "plugin-live-details", false);
        assert_eq!(fixture.public_packages(false), updated);
        fixture.close(visual);
    }
}

#[gpui::test]
fn recovers_uncertain_uninstall(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let other = fixture.public_packages(true);
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        tap(visual, "plugin-details-example");
        shown(visual, "plugin-skill-analysis", true);
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "plugin-details-uninstall");
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        let (title, detail) = crate::prompts::tests::wait(visual);
        assert_eq!(title, tr("plugins_uninstall"));
        assert!(detail.contains("example"));
        assert!(fixture.plugins().is_empty());
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 1);
        crate::prompts::tests::answer(visual, "settings_cancel");
        shown(visual, "plugin-live-details", true);
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("plugins_unknown")
        );
        tap(visual, "plugin-details-uninstall");
        crate::prompts::tests::answer(visual, "plugins_retry");
        shown(visual, "plugin-live-details", false);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(fixture.plugins().is_empty());
        assert_eq!(fixture.public_packages(true), other);
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        drop(requests);
        fixture.close(visual);
    }
}

#[gpui::test]
fn discards_old_host_reads(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        fixture.transport.read_mode.store(3, Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        select(&owner, visual, Section::Skills);
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.read(cx).errors.is_empty())
        });
        shown(visual, "plugin-metadata-retry", false);
        select(&owner, visual, Section::Plugins);
        select(&owner, visual, Section::Skills);
        shown(visual, "skill-card-example:analysis", false);
        ready(&owner, visual);
        assert!(owner.read_with(visual, |owner, cx| {
            owner
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .entries
                .contains_key("example")
        }));
        fixture.transport.read_mode.store(2, Ordering::SeqCst);
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 1,
            enabled: false,
        });
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        fixture.bind(&owner, visual, true);
        fixture.transport.release.cancel();
        shown(visual, "skill-card-example:analysis", false);
        ready(&owner, visual);
        assert!(owner.read_with(visual, |owner, cx| {
            owner
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .skills()
                .iter()
                .all(|skill| !skill.starts_with("example:"))
        }));
        fixture.bind(&owner, visual, false);
        ready(&owner, visual);
        shown(visual, "skill-card-example:analysis", false);
        assert!(owner.read_with(visual, |owner, cx| {
            owner
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .entries
                .contains_key("example")
        }));
        fixture.close(visual);
    }
}

#[gpui::test]
fn external_browser_settings_follow_host(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = |other| {
            fixture
                .public_packages(other)
                .into_iter()
                .find(|package| package.name == "external-browser")
                .unwrap()
        };
        assert!(!package(false).enabled);
        assert!(!package(true).enabled);
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        assert!(owner.read_with(visual, |owner, cx| {
            owner
                .plugin_settings_entries(cx)
                .iter()
                .all(|entry| entry.name != "external-browser")
        }));
        owner.update(visual, |owner, cx| {
            owner.open_plugin_settings("external-browser", cx)
        });
        shown(visual, "plugin-settings-enabled", false);
        shown(visual, "plugin-settings-node", true);
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.selected_plugin_settings(), Some("external-browser"));
            let live = owner.provider_link.as_ref().unwrap();
            assert_eq!(live.binding.client.target(), fixture.client.target());
            assert_eq!(live.binding.label.as_ref(), "Fixture Node");
        });

        // Delay completion after admission, then select a different execution Node.
        fixture.transport.mode.store(2, Ordering::SeqCst);
        select(&owner, visual, Section::Plugins);
        tap(visual, "plugin-toggle-external-browser");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert!(package(false).enabled);
        owner.update(visual, |owner, cx| {
            owner.open_plugin_settings("external-browser", cx)
        });
        fixture.bind(&owner, visual, true);
        ready(&owner, visual);
        shown(visual, "plugin-settings-enabled", false);
        shown(visual, "plugin-settings-node", true);
        fixture.transport.release.cancel();
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.selected_plugin_settings(), Some("external-browser"));
            let live = owner.provider_link.as_ref().unwrap();
            assert_ne!(live.binding.client.target(), fixture.client.target());
            assert_eq!(live.binding.label.as_ref(), "Selected Node");
            let package = owner
                .plugin_catalog
                .packages
                .iter()
                .find(|package| package.name == "external-browser")
                .unwrap();
            assert!(!package.enabled);
            assert_eq!(package.revision, 1);
        });
        for (enabled, revision) in [(true, 2), (false, 3)] {
            select(&owner, visual, Section::Plugins);
            tap(visual, "plugin-toggle-external-browser");
            wait(visual, |cx| {
                owner
                    .read(cx)
                    .plugin_catalog
                    .packages
                    .iter()
                    .any(|package| {
                        package.name == "external-browser"
                            && package.enabled == enabled
                            && package.revision == revision
                    })
            });
            assert_eq!(package(true).enabled, enabled);
            assert!(package(false).enabled);
            owner.update(visual, |owner, cx| {
                owner.open_plugin_settings("external-browser", cx)
            });
            shown(visual, "plugin-settings-enabled", false);
            shown(visual, "plugin-settings-node", true);
        }
        fixture.bind(&owner, visual, false);
        ready(&owner, visual);
        shown(visual, "plugin-settings-node", true);
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.selected_plugin_settings(), Some("external-browser"));
            assert_eq!(
                owner
                    .provider_link
                    .as_ref()
                    .unwrap()
                    .binding
                    .client
                    .target(),
                fixture.client.target()
            );
            let package = owner
                .plugin_catalog
                .packages
                .iter()
                .find(|package| package.name == "external-browser")
                .unwrap();
            assert!(package.enabled);
            assert_eq!(package.revision, 2);
        });
        assert!(!package(true).enabled);
        fixture.close(visual);
    }
}

#[gpui::test]
fn computer_routes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        shown(visual, "plugin-details-computer", true);
        let package = || {
            fixture
                .public_packages(false)
                .into_iter()
                .find(|entry| entry.name == "computer")
                .unwrap()
        };
        assert!(!package().enabled);
        tap(visual, "plugin-toggle-computer");
        wait(visual, |_| package().enabled);
        assert!(fixture.plugins().is_empty());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        let entries = owner.read_with(visual, |owner, cx| owner.plugin_settings_entries(cx));
        assert!(entries.iter().any(|entry| entry.name == "computer"));
        owner.update(visual, |owner, cx| {
            owner.open_plugin_settings("computer", cx)
        });
        shown_settings(&owner, visual, "computer-permissions", true);
        shown(visual, "plugin-settings-enabled", false);
        select(&owner, visual, Section::Plugins);
        tap(visual, "plugin-toggle-computer");
        wait(visual, |_| !package().enabled);
        owner.update(visual, |owner, cx| {
            owner.open_plugin_settings("computer", cx)
        });
        shown_settings(&owner, visual, "computer-permissions", true);
        select(&owner, visual, Section::Plugins);
        let revision = package().revision;
        updates::ready(&owner, visual, "computer", revision);
        menu(visual, "plugin-menu-computer", 1);
        assert!(!visual.did_prompt_for_paths());
        wait(visual, |cx| {
            !owner
                .read(cx)
                .plugin_catalog
                .updates
                .as_ref()
                .unwrap()
                .read(cx)
                .checking()
        });
        assert_eq!(package().revision, revision);
        ready(&owner, visual);
        assert!(!package().enabled);
        menu(visual, "plugin-menu-computer", 0);
        shown_settings(&owner, visual, "computer-permissions", true);
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.section),
            Section::Plugin
        );
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        select(&owner, visual, Section::Plugins);
        menu(visual, "plugin-menu-computer", 2);
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        wait(visual, |cx| {
            !owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|entry| entry.name == "computer")
        });
        shown(visual, "plugin-details-computer", false);
        let entries = owner.read_with(visual, |owner, cx| owner.plugin_settings_entries(cx));
        assert!(!entries.iter().any(|entry| entry.name == "computer"));
        fixture.close(visual);
    }
}

#[gpui::test]
fn registered_settings(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        select(&owner, visual, Section::Skills);
        shown(visual, "skills-install", true);
        owner.read_with(visual, |owner, cx| {
            assert_eq!(owner.section, Section::Skills);
            assert_eq!(owner.selected_plugin_settings(), None);
            assert!(
                !owner
                    .plugin_catalog
                    .packages
                    .iter()
                    .any(|package| package.name == "sailry.builtin.skills")
            );
            assert!(
                !owner
                    .plugin_settings_entries(cx)
                    .iter()
                    .any(|entry| entry.name == "sailry.builtin.skills")
            );
        });
        owner.read_with(visual, |owner, cx| {
            assert_eq!(owner.section, Section::Skills);
            assert_eq!(owner.selected_plugin_settings(), None);
            assert!(
                !owner
                    .plugin_settings_entries(cx)
                    .iter()
                    .any(|entry| entry.name == "statistics")
            );
        });
        let Output::Plugin(statistics) = fixture.execute(Command::ReadPlugin {
            name: "statistics".into(),
        }) else {
            panic!("plugin expected")
        };
        let extension = statistics.extension.unwrap();
        assert!(extension.settings_page.is_none());
        assert_eq!(
            extension.desktop.unwrap().navigation_options.surface,
            sailry_protocol::plugin::desktop::Surface::Workspace
        );
        select(&owner, visual, Section::General);
        let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: "files".into(),
        }) else {
            panic!("plugin expected")
        };
        assert!(info.extension.as_ref().unwrap().settings_page.is_none());
        assert_ne!(crate::plugins::metadata::title(&info), "files");
        fixture.close(visual);
    }
}

#[gpui::test]
fn scripted_configuration_opens_panel(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual);
        for (name, selector) in [
            ("web-search", "search-prefer-native"),
            ("code", "code-settings"),
        ] {
            visual.update(|window, cx| {
                owner.update(cx, |owner, cx| {
                    owner.configure_plugin(name, window, cx);
                })
            });
            crate::settings::plugins::test_support::shown_settings(&owner, visual, selector, true);
            assert_eq!(
                owner.read_with(visual, |owner, _| owner.section),
                Section::Plugin
            );
            visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        }
        fixture.close(visual);
    }
}
