use super::*;

#[gpui::test]
fn targeted_pages_retry_without_showing_catalog(cx: &mut TestAppContext) {
    init(cx);
    use sailry_protocol::plugin::desktop::Surface;
    for (remote, surface, content_sized) in [false, true].into_iter().flat_map(|remote| {
        [
            (Surface::Workspace, false),
            (Surface::Settings, false),
            (Surface::Settings, true),
        ]
        .map(move |(surface, content_sized)| (remote, surface, content_sized))
    }) {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let extension = &mut manifest["extensions"]["dev.sailry.platform"];
        extension.as_object_mut().unwrap().remove("host");
        extension.as_object_mut().unwrap().remove("settings_schema");
        extension["actions"] = serde_json::json!([]);
        extension["desktop"]["resources"] =
            serde_json::json!(["dev.sailry.platform/desktop/main.js"]);
        extension["settings_page"]["entry"] = "dev.sailry.platform/desktop/main.js".into();
        std::fs::write(path, manifest.to_string()).unwrap();
        std::fs::write(
            fixture
                .directory
                .path()
                .join("project/package/dev.sailry.platform/desktop/main.js"),
            r#"
import { View, div } from 'gpui-kit';
import { context } from 'sailry';
export default class NodePage extends View {
  init() { this.scope = JSON.parse(context()); }
  render() {
    return div().size_full().min_h_64().id(`target-page-${this.scope.surface}`)
      .child(this.scope.worktree === null ? this.scope.package.name : 'Unexpected worktree');
  }
}
"#,
        )
        .unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let mut binding = fixture.binding.clone();
            binding.worktree = None;
            let panel = cx.new(|cx| {
                let mut panel = if surface == Surface::Settings {
                    Panel::settings(binding, content_sized, cx)
                } else {
                    Panel::standalone(binding, cx)
                };
                panel = panel.with_target(package.summary.name.clone());
                panel
            });
            owner = Some(panel.clone());
            Panel::observe_notifications(&panel, window, cx);
            Root::new(panel, window, cx)
        });
        let panel = owner.unwrap();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = visual.debug_bounds("plugin-panel").unwrap();
        assert!(visual.debug_bounds("plugin-panel-loading").is_some());
        assert!(visual.debug_bounds("plugin-panel-spinner").is_some());
        assert!(visual.debug_bounds("plugin-open-0").is_none());
        assert!(
            visual
                .debug_bounds("empty-card-plugins_view_empty")
                .is_none()
        );
        wait(visual, |cx| {
            panel.read(cx).connected && panel.read(cx).metadata.read(cx).settled()
        });
        *fixture.transport.view_target.lock().unwrap() = Some(package.summary.reference());
        fixture.transport.mode.store(18, Ordering::SeqCst);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            panel.read(cx).error.is_some() && !panel.read(cx).loading
        });
        let failed_bounds = visual.debug_bounds("plugin-panel-unavailable").unwrap();
        assert_eq!(bounds, visual.debug_bounds("plugin-panel").unwrap());
        assert!(visual.debug_bounds("plugin-panel-loading").is_none());
        assert!(visual.debug_bounds("plugin-panel-spinner").is_none());
        assert!(visual.debug_bounds("plugin-view-retry").is_some());
        assert!(visual.debug_bounds("plugin-open-0").is_none());
        assert!(
            visual
                .debug_bounds("empty-card-plugins_view_empty")
                .is_none()
        );
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 1);
        fixture.transport.mode.store(2, Ordering::SeqCst);
        click(visual, "plugin-view-retry");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert_eq!(bounds, visual.debug_bounds("plugin-panel").unwrap());
        assert_eq!(
            Some(failed_bounds),
            visual.debug_bounds("plugin-panel-loading")
        );
        assert!(visual.debug_bounds("plugin-panel-unavailable").is_none());
        assert!(visual.debug_bounds("plugin-view-retry").is_none());
        assert!(visual.debug_bounds("plugin-panel-spinner").is_some());
        assert!(visual.debug_bounds("plugin-open-0").is_none());
        assert!(
            visual
                .debug_bounds("empty-card-plugins_view_empty")
                .is_none()
        );
        assert!(panel.read_with(visual, |panel, _| panel.mounted.is_none()));
        fixture.transport.release.cancel();
        let marker = format!(
            "target-page-{}",
            if surface == Surface::Settings {
                "settings"
            } else {
                "workspace"
            }
        );
        wait(visual, |cx| snapshot(&panel, cx).contains(&marker));
        panel.read_with(visual, |state, cx| {
            assert_eq!(state.selected, Some(package.summary.reference()));
            assert!(state.resource_active());
            let content = snapshot(&panel, cx);
            assert!(content.contains(&package.summary.name));
            assert!(!content.contains("Unexpected worktree"));
        });
        assert_eq!(bounds, visual.debug_bounds("plugin-panel").unwrap());
        assert!(visual.debug_bounds("plugin-panel-loading").is_none());
        assert!(visual.debug_bounds("plugin-open-0").is_none());
        assert!(visual.debug_bounds("plugin-view-retry").is_none());
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn failed_read_recovers_on_entry(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        *fixture.transport.view_target.lock().unwrap() = Some(package.summary.reference());
        fixture.transport.mode.store(18, Ordering::SeqCst);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| {
            panel.read(cx).error.is_some() && !panel.read(cx).loading
        });
        assert!(panel.read_with(visual, |panel, _| panel.mounted.is_none()));
        assert!(visual.debug_bounds("plugin-view-retry").is_some());
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 1);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.enter(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        assert!(panel.read_with(visual, |panel, _| panel.error.is_none()));
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn pending_entry_keeps_its_reference(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        std::fs::write(
            fixture
                .directory
                .path()
                .join("project/package/dev.sailry.platform/desktop/main.js"),
            include_str!("navigation/entry.js"),
        )
        .unwrap();
        let package = fixture.install(0);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let source = cx.new(|cx| {
                crate::conversation::live::View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            let panel =
                cx.new(|cx| Panel::new(source, cx).with_target(package.summary.name.clone()));
            panel.update(cx, |panel, cx| {
                assert!(!panel.connected);
                assert!(panel.selected.is_none());
                panel.enter(package.summary.reference(), window, cx);
            });
            owner = Some(panel.clone());
            Root::new(panel, window, cx)
        });
        let panel = owner.unwrap();
        let updated = fixture.configure(&package, false);
        assert_ne!(updated.summary.reference(), package.summary.reference());
        wait(visual, |cx| {
            panel.read(cx).ready_for(&updated.summary.reference(), cx)
        });
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(updated.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("entry-count-0"));
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.enter(updated.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("entry-count-1"));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn waits_for_read_capacity(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        *fixture.transport.view_target.lock().unwrap() = Some(package.summary.reference());
        fixture.transport.mode.store(15, Ordering::SeqCst);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        let requests = fixture.transport.views.lock().unwrap();
        assert!(requests.len() >= 2);
        assert!(requests.iter().all(|request| *request == requests[0]));
        assert!(!requests[0].command.durable());
        assert!(fixture.transport.entered.is_cancelled());
        assert_eq!(fixture.transport.mode.load(Ordering::SeqCst), 0);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn closing_cancels_capacity_wait(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        *fixture.transport.view_target.lock().unwrap() = Some(package.summary.reference());
        fixture.transport.mode.store(16, Ordering::SeqCst);
        open_package(visual, &panel, "project-summary");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        panel.update(visual, |panel, cx| panel.back(cx));
        let attempts = fixture.transport.views.lock().unwrap().len();
        assert!(attempts > 0);
        fixture.runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(150)).await;
        });
        visual.run_until_parked();
        assert_eq!(fixture.transport.views.lock().unwrap().len(), attempts);
        panel.read_with(visual, |panel, _| {
            assert!(!panel.loading);
            assert!(panel.selected.is_none());
            assert!(panel.mounted.is_none());
            assert!(panel.error.is_none());
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
