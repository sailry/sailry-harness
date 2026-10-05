use super::*;
use crate::{
    resources::{SideResource, launcher::Destination},
    shell::{Shell, session_scope::Key},
};
use sailry_protocol::{Output, plugin::Info};
mod settings;

fn package(fixture: &Fixture) -> Info {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: "browser".into(),
    }) else {
        panic!("browser package expected")
    };
    info
}

fn enable(fixture: &Fixture, enabled: bool) {
    let info = package(fixture);
    fixture.execute(Command::SetPluginEnabled {
        name: info.summary.name,
        expected_revision: info.summary.revision,
        enabled,
    });
}

#[gpui::test]
fn routes_pages_and_preserves_core_tabs(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        enable(&fixture, true);
        cx.update(|cx| {
            cx.set_global(fixture.services(remote));
            cx.set_global(crate::preferences::Preferences::default());
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .transport_for(fixture.node.id())
                .is_some()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx);
                let chat = shell.chat_view(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                );
                shell
                    .chats
                    .views
                    .insert((fixture.node.id(), fixture.session.id), chat);
                shell.activate_session(
                    Key::Session(fixture.node.id(), fixture.session.id),
                    window,
                    cx,
                );
                shell.open_destination(Destination::Browser, window, cx);
            })
        });
        let panel = shell.read_with(visual, |shell, _| {
            let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
                panic!("plugin page expected")
            };
            panel.clone()
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("browser-new-tab")
        });
        let browser = panel.read_with(visual, |panel, _| panel.browser_view().unwrap());
        assert_eq!(
            panel.read_with(visual, |panel, _| panel
                .selected
                .as_ref()
                .unwrap()
                .name
                .clone()),
            "browser"
        );
        click(visual, "field-1");
        visual.simulate_input("https://");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            browser.read(cx).snapshot(cx)["tabs"][0]["error"] == "browser_invalid_address"
        });
        let cursor = browser.read_with(visual, |browser, cx| {
            let snapshot = browser.snapshot(cx);
            assert_eq!(snapshot["tabs"][0]["loaded"], false);
            snapshot["cursor"].as_str().unwrap().parse::<u64>().unwrap()
        });
        click(visual, "browser-go");
        wait(visual, |cx| {
            browser.read(cx).snapshot(cx)["cursor"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > cursor
        });
        browser.read_with(visual, |browser, cx| {
            let snapshot = browser.snapshot(cx);
            assert_eq!(snapshot["tabs"][0]["error"], "browser_invalid_address");
            assert_eq!(snapshot["tabs"][0]["loaded"], false);
        });
        click(visual, "browser-new-tab");
        wait(visual, |cx| {
            browser.read(cx).snapshot(cx)["tabs"]
                .as_array()
                .unwrap()
                .len()
                == 2
        });
        let script = panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .read(cx)
                .content()
                .clone()
        });
        let script = script
            .downcast::<gpui_shell::ScriptView>()
            .unwrap()
            .downgrade();
        enable(&fixture, false);
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none() && script.upgrade().is_none()
        });
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.browser_view().unwrap()),
            browser
        );
        enable(&fixture, true);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("browser-new-tab")
                && browser.read(cx).snapshot(cx)["tabs"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 2
        });

        let root = fixture.directory.path().join("project/browser-package");
        super::super::fixture::copy_package(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/browser"),
            &root,
        );
        let manifest_path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["version"] = "0.2.0".into();
        std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let install = |revision| {
            fixture.execute(Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "browser-package".into(),
                name: "browser".into(),
                expected_revision: revision,
            })
        };
        let Output::Plugin(updated) = install(package(&fixture).summary.revision) else {
            panic!("updated package expected")
        };
        wait(visual, |cx| {
            panel.read(cx).selected.as_ref() == Some(&updated.summary.reference())
                && snapshot(&panel, cx).contains("browser-new-tab")
        });
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.browser_view().unwrap()),
            browser
        );
        fixture.execute(Command::RemovePlugin {
            name: "browser".into(),
            expected_revision: package(&fixture).summary.revision,
        });
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.browser_view().unwrap()),
            browser
        );
        install(0);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("browser-new-tab")
        });

        // Sidebar selection cannot redirect a captured browser or its settings.
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        });
        click(visual, "browser-settings");
        wait(visual, |cx| {
            shell.read(cx).page == crate::preview::Page::Settings
        });
        assert!(shell.read_with(visual, |shell, cx| {
            shell.settings_target == Some(fixture.node.id())
                && shell.settings.read(cx).selected_plugin_settings() == Some("browser")
        }));
        wait(visual, |cx| {
            shell
                .read(cx)
                .settings
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| snapshot(&panel, cx).contains("browser-settings-page"))
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(crate::preview::Page::Conversation, window, cx)
            })
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("browser-new-tab")
        });
        click(visual, "browser-tab-close-1");
        wait(visual, |cx| {
            browser.read(cx).snapshot(cx)["tabs"]
                .as_array()
                .unwrap()
                .len()
                == 1
        });
        click(visual, "browser-tab-close-0");
        wait(visual, |cx| shell.read(cx).side_resource.is_none());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(browser);
        drop(shell);
        fixture.close();
    }
}
