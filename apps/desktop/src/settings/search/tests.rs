use super::*;
use core::prelude::v1::test;
use sailry_protocol::{Command, Output};
pub(super) mod fixture;
use fixture::{Fixture, draw, init, selected, tap, wait};

#[gpui::test]
fn switches_plugin_without_changing_driver_settings(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let settings = fixture.settings();
        let Output::Plugin(initial) = fixture.execute(Command::ReadPlugin {
            name: "web-search".into(),
        }) else {
            panic!("plugin expected")
        };
        let other = sailry_client::Client::new(fixture.other.local());
        let untouched = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadPlugin {
                name: "web-search".into(),
            })))
            .unwrap();
        owner.read_with(visual, |workspace, cx| {
            assert_eq!(workspace.selected_plugin_settings(), Some("web-search"));
            let page = workspace
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .entries["web-search"]
                .extension
                .as_ref()
                .unwrap()
                .settings_page
                .as_ref()
                .unwrap();
            assert_eq!(
                page.native,
                Some(sailry_protocol::plugin::desktop::NativeSettings::Search)
            );
        });
        for enabled in [!initial.summary.enabled, initial.summary.enabled] {
            let Output::Plugin(previous) = fixture.execute(Command::ReadPlugin {
                name: "web-search".into(),
            }) else {
                panic!("plugin expected")
            };
            tap(visual, "plugin-settings-enabled");
            wait(visual, |cx| {
                let workspace = owner.read(cx);
                !workspace.plugin_catalog.busy()
                    && workspace
                        .plugin_catalog
                        .packages
                        .iter()
                        .any(|plugin| plugin.name == "web-search" && plugin.enabled == enabled)
            });
            let requests = fixture.transport.requests.lock().unwrap();
            let request = requests.last().unwrap();
            assert_eq!(request.target, fixture.node.id());
            assert_eq!(
                request.command,
                Command::SetPluginEnabled {
                    name: "web-search".into(),
                    expected_revision: previous.summary.revision,
                    enabled,
                }
            );
        }
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 2);
        assert_eq!(fixture.settings(), settings);
        assert_eq!(
            fixture
                .runtime
                .block_on(other.execute(other.prepare(Command::ReadPlugin {
                    name: "web-search".into(),
                })))
                .unwrap(),
            untouched
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn saves_and_observes_each_host(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        panel.read_with(visual, |panel, cx| {
            assert_eq!(panel.saved.as_ref().unwrap(), &Settings::default());
            assert_eq!(
                panel.picker.read(cx).selected_value(),
                Some(&Some(Selection::Auto))
            );
        });
        tap(visual, "search-prefer-native");
        wait(visual, |cx| {
            !panel.read(cx).pending && !panel.read(cx).saved.as_ref().unwrap().prefer_native
        });
        tap(visual, "search-fallback");
        wait(visual, |cx| {
            !panel.read(cx).pending && !panel.read(cx).saved.as_ref().unwrap().fallback
        });
        for driver in [
            Some(Selection::Auto),
            None,
            Some(Selection::Driver(Driver::Tavily)),
        ] {
            visual.update(|window, cx| {
                let picker = panel.read(cx).picker.clone();
                picker.update(cx, |picker, cx| {
                    picker.set_selected_value(&driver, window, cx);
                    cx.emit(SelectEvent::Confirm(picker.selected_value().cloned()));
                })
            });
            wait(visual, |cx| {
                !panel.read(cx).pending && panel.read(cx).saved.as_ref().unwrap().driver == driver
            });
            assert_eq!(fixture.settings().driver, driver);
        }
        assert!(fixture.settings().providers[&Driver::Tavily].enabled);
        fixture.bind(&owner, visual, true);
        assert_eq!(
            visual.update(|_, cx| selected(&owner, cx)
                .unwrap()
                .read(cx)
                .saved
                .clone()
                .unwrap()),
            Settings::default()
        );
        fixture.bind(&owner, visual, false);
        let mut changed = fixture.settings();
        changed.prefer_native = true;
        fixture.execute(Command::SaveSearchSettings(Update {
            settings: changed.clone(),
            credentials: Default::default(),
        }));
        wait(visual, |cx| {
            panel.read(cx).saved.as_ref().is_some_and(|settings| {
                settings.revision == changed.revision + 1 && settings.prefer_native
            })
        });
        for width in [420., 960.] {
            let handle = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(handle, size(px(width), px(1100.)));
            draw(visual);
            for selector in [
                "search-enabled-exa",
                "search-edit-exa",
                "search-test-exa",
                "search-default-driver",
            ] {
                let bounds = visual.debug_bounds(selector).unwrap();
                assert!(
                    bounds.left() >= px(0.) && bounds.right() <= px(width),
                    "{selector} is outside the viewport"
                );
            }
        }
        fixture.close(visual);
    }
}

#[gpui::test]
fn preview_does_not_open_a_node(cx: &mut TestAppContext) {
    init(cx);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        let panel = workspace.read(cx).search.preview.clone();
        panel.update(cx, |panel, _| panel.active = true);
        owner = Some(workspace.clone());
        Root::new(panel, window, cx)
    });
    tap(visual, "search-prefer-native");
    tap(visual, "search-test-exa");
    owner.unwrap().read_with(visual, |workspace, cx| {
        assert!(workspace.provider_link.is_none());
        assert!(workspace.search.panels.is_empty());
        assert!(
            !workspace
                .search
                .preview
                .read(cx)
                .saved
                .as_ref()
                .unwrap()
                .prefer_native
        );
        assert!(!workspace.search.preview.read(cx).pending);
        assert!(workspace.search.preview.read(cx).error.is_none());
    });
}
