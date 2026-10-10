#![cfg(unix)]
use super::*;
use crate::plugins::fixture::code as network;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt, dialog};
use sailry_protocol::{Command, Output};
use std::sync::atomic::Ordering;

mod fixture;
mod installation;
mod layout;
use fixture::{init, *};

#[gpui::test]
fn scopes_service_switch_and_preserves_checks(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let mut expected = fixture.settings();
        assert!(expected.enabled);
        expected.after_edit = false;
        let Output::CodeSettings(settings) = fixture.execute(Command::SaveCodeSettings {
            expected_revision: expected.revision,
            settings: expected,
        }) else {
            panic!("settings expected")
        };
        expected = settings;
        fixture.transport.requests.lock().unwrap().clear();
        let other = sailry_client::Client::new(fixture.other.local());
        let untouched = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadCodeSettings)))
            .unwrap();
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        assert!(cx.debug_bounds("code-services-enabled").is_none());
        assert!(
            cx.debug_bounds("settings-group-plugins_settings_title")
                .is_none()
        );
        let controls = cx.debug_bounds("settings-group-code_settings").unwrap();
        assert!(cx.debug_bounds("plugin-settings-enabled").is_none());
        assert!(
            controls.contains(
                &cx.debug_bounds("settings-row-code_rules_updated")
                    .unwrap()
                    .center()
            )
        );
        for enabled in [false, true] {
            let Output::Plugin(plugin) = fixture.execute(Command::ReadPlugin {
                name: "code".into(),
            }) else {
                panic!("plugin expected")
            };
            toggle(&owner, cx, enabled);
            expected.enabled = enabled;
            expected.revision += 1;
            assert_eq!(fixture.settings(), expected);
            cx.update(|_, cx| assert_eq!(panel.read(cx).saved.as_ref(), Some(&expected)));
            let requests = fixture.transport.requests.lock().unwrap();
            let request = requests.last().unwrap();
            assert_eq!(request.target, fixture.node.id());
            let Command::SetPluginEnabled {
                name,
                expected_revision,
                enabled: submitted,
            } = &request.command
            else {
                panic!("plugin toggle expected")
            };
            assert_eq!(name, "code");
            assert_eq!(*expected_revision, plugin.summary.revision);
            assert_eq!(*submitted, enabled);
        }
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 2);
        assert_eq!(
            fixture
                .runtime
                .block_on(other.execute(other.prepare(Command::ReadCodeSettings)))
                .unwrap(),
            untouched
        );
        fixture.close(cx);
    }
}

#[gpui::test]
fn scopes_languages_and_saves_controls(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.executable("rust-analyzer");
        let (owner, cx) = fixture.mount(cx);
        tap(cx, "code-tab-installed");
        let panel = cx.update(|_, cx| selected(&owner, cx));
        cx.update(|_, cx| {
            let languages = &panel.read(cx).catalog.as_ref().unwrap();
            assert!(languages.iter().any(|language| language.id == "rust"));
            assert!(languages.iter().any(|language| language.id == "html"));
            assert!(languages.iter().all(|language| {
                language
                    .servers
                    .iter()
                    .all(|candidate| candidate.startup.is_none())
            }));
        });
        toggle(&owner, cx, false);
        assert!(!fixture.settings().enabled);
        assert!(fixture.settings().after_edit);
        toggle(&owner, cx, true);
        assert!(fixture.settings().enabled);
        tap(cx, "code-enabled-rust-analyzer");
        wait(cx, |cx| !panel.read(cx).pending && !panel.read(cx).loading);
        assert!(fixture.settings().servers["rust-analyzer"].disabled);
        tap(cx, "code-edit-rust-analyzer");
        input(cx, "code-command-0", "/fixture/custom-analyzer");
        input(cx, "code-command-1", "[\"--stdio\"]");
        input(cx, "code-command-2", "{\"check\":false}");
        input(cx, "code-command-3", "{\"FIXTURE_SETTING\":\"kept\"}");
        assert!(cx.debug_bounds("code-editor-save").is_none());
        tap(cx, "code-editor-close");
        wait(cx, |cx| !panel.read(cx).pending && !panel.read(cx).loading);
        let saved = fixture.settings();
        let custom = &saved.servers["rust-analyzer"];
        assert_eq!(custom.command.as_deref(), Some("/fixture/custom-analyzer"));
        assert_eq!(custom.args.as_deref(), Some(["--stdio".into()].as_slice()));
        assert_eq!(custom.config, Some(serde_json::json!({"check":false})));
        assert_eq!(
            custom.environment.as_ref().unwrap()["FIXTURE_SETTING"],
            "kept"
        );
        assert!(custom.disabled);
        assert!(saved.languages.is_empty());
        fixture.bind(&owner, cx, false, Some(fixture.second));
        assert_eq!(cx.update(|_, cx| selected(&owner, cx)), panel);
        fixture.bind(&owner, cx, false, None);
        assert_eq!(cx.update(|_, cx| selected(&owner, cx)), panel);
        assert!(cx.debug_bounds("code-settings").is_some());
        fixture.close(cx);
    }
}

#[gpui::test]
fn preserves_conflicting_draft_and_original_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.executable("rust-analyzer");
        let (owner, cx) = fixture.mount(cx);
        tap(cx, "code-tab-installed");
        let panel = cx.update(|_, cx| selected(&owner, cx));
        let mut settings = fixture.settings();
        let revision = settings.revision;
        settings.after_edit = false;
        fixture.execute(Command::SaveCodeSettings {
            settings,
            expected_revision: revision,
        });
        tap(cx, "code-enabled-rust-analyzer");
        wait(cx, |cx| !panel.read(cx).pending);
        cx.update(|_, cx| {
            assert_eq!(panel.read(cx).error, Some("code_conflict"));
            assert!(panel.read(cx).draft.as_ref().unwrap().servers["rust-analyzer"].disabled);
        });
        assert!(fixture.settings().servers.is_empty());
        tap(cx, "code-reload");
        wait(cx, |cx| !panel.read(cx).loading);
        cx.update(|_, cx| assert!(!panel.read(cx).draft.as_ref().unwrap().after_edit));
        fixture.transport.drop_next.store(true, Ordering::SeqCst);
        tap(cx, "code-enabled-rust-analyzer");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_some()
        });
        let original = cx.update(|_, cx| panel.read(cx).request.clone().unwrap());
        assert!(fixture.settings().servers["rust-analyzer"].disabled);
        fixture.bind(&owner, cx, false, Some(fixture.second));
        wait(cx, |cx| !selected(&owner, cx).read(cx).loading);
        fixture.bind(&owner, cx, false, Some(fixture.worktree));
        tap(cx, "code-retry");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_none()
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let retries: Vec<_> = requests
            .iter()
            .filter(|request| request.id == original.id)
            .collect();
        assert_eq!(retries.len(), 2);
        assert_eq!(retries[0], retries[1]);
        drop(requests);
        fixture.close(cx);
    }
}

#[gpui::test]
fn refuses_stale_consent_targets(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, cx) = fixture.mount(cx);
        tap(cx, "code-install-rust-analyzer");
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        fixture.bind(&owner, cx, true, None);
        tap(cx, "code-consent-submit");
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request.command, Command::InstallCodeServer { .. }))
        );
        assert!(cx.update(|_, cx| owner.read(cx).code.selected == Some(fixture.other.id())));
        fixture.close(cx);
    }
}

#[gpui::test]
fn manages_services_without_a_selected_project(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        cx.update(|cx| {
            cx.set_global(crate::backend::Services {
                profile: fixture.node.profile().to_path_buf(),
                runtime: fixture.runtime.clone(),
                link: fixture.node.link(),
                local: fixture.transport.clone(),
                relay_enabled: false,
            })
        });
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
            shell.update(cx, |shell, cx| {
                shell.navigate(crate::preview::Page::Settings, window, cx);
            });
            entity = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = entity.unwrap();
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .settings
                .read(cx)
                .plugin_settings_entries(cx)
                .iter()
                .any(|entry| entry.name == "code")
        });
        assert!(visual.debug_bounds("settings_code").is_none());
        assert!(visual.debug_bounds("settings_search").is_none());
        tap(visual, "plugin-settings-code");
        let settings = visual.update(|_, cx| shell.read(cx).settings.clone());
        wait(visual, |cx| {
            settings.read(cx).code.selected == Some(fixture.node.id())
                && selected(&settings, cx).read(cx).catalog.is_some()
        });
        let panel = visual.update(|_, cx| selected(&settings, cx));
        assert!(visual.debug_bounds("code-settings").is_some());
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == fixture.worktree)
            .unwrap()
            .project
            .unwrap();
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select_project_worktree(project, fixture.worktree);
                cx.notify();
            })
        });
        let settings = visual.update(|_, cx| shell.read(cx).settings.clone());
        wait(visual, |cx| {
            settings.read(cx).code.selected == Some(fixture.node.id())
                && selected(&settings, cx).read(cx).saved.is_some()
        });
        assert!(visual.debug_bounds("code-server-rust-analyzer").is_some());
        toggle(&settings, visual, false);
        assert!(!fixture.settings().enabled);
        std::fs::write(
            fixture.root.join("index.html"),
            "<p>Reentered worktree</p>\n",
        )
        .unwrap();
        // Plugin enablement opens the inventory, so return through the real settings rail.
        tap(visual, "sidebar-settings");
        assert_eq!(
            visual.update(|_, cx| shell.read(cx).page),
            crate::preview::Page::Settings
        );
        tap(visual, "settings_appearance");
        tap(visual, "plugin-settings-code");
        wait(visual, |cx| {
            selected(&settings, cx)
                .read(cx)
                .catalog
                .as_ref()
                .is_some_and(|workspace| workspace.iter().any(|language| language.id == "html"))
                && !selected(&settings, cx).read(cx).loading
        });
        assert!(
            visual
                .debug_bounds("code-server-vscode-html-language-server")
                .is_some()
        );
        assert!(visual.debug_bounds("code-refresh").is_none());
        let other = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == fixture.second)
            .unwrap()
            .project
            .unwrap();
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select_project_worktree(other, fixture.second);
                cx.notify();
            })
        });
        wait(visual, |cx| {
            settings.read(cx).code.selected == Some(fixture.node.id())
                && selected(&settings, cx).read(cx).catalog.is_some()
        });
        assert_eq!(visual.update(|_, cx| selected(&settings, cx)), panel);
        assert!(visual.update(|_, cx| {
            selected(&settings, cx)
                .read(cx)
                .rows()
                .iter()
                .any(|row| row.server == "rust-analyzer")
        }));
        fixture.close(visual);
        visual
            .cx
            .update(|cx| cx.remove_global::<crate::backend::Services>());
    }
}

#[gpui::test]
fn serializes_pending_configuration_edits(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.executable("rust-analyzer");
        let (owner, cx) = fixture.mount(cx);
        tap(cx, "code-tab-installed");
        let panel = cx.update(|_, cx| selected(&owner, cx));
        tap(cx, "code-edit-rust-analyzer");
        fixture.transport.hold.store(true, Ordering::SeqCst);
        input(cx, "code-command-0", "/fixture/first-analyzer");
        wait(cx, |cx| panel.read(cx).pending);
        input(cx, "code-command-0", "/fixture/latest-analyzer");
        wait(cx, |cx| {
            panel.read(cx).draft.as_ref().unwrap().servers["rust-analyzer"]
                .command
                .as_deref()
                == Some("/fixture/latest-analyzer")
        });
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 1);
        fixture.transport.hold.store(false, Ordering::SeqCst);
        fixture.transport.release.add_permits(1);
        wait(cx, |cx| {
            !panel.read(cx).pending && !panel.read(cx).loading && !panel.read(cx).dirty()
        });
        tap(cx, "code-editor-close");
        let saved = fixture.settings();
        assert_eq!(saved.revision, 2);
        assert_eq!(
            saved.servers["rust-analyzer"].command.as_deref(),
            Some("/fixture/latest-analyzer")
        );
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests
                .iter()
                .map(|request| match &request.command {
                    Command::SaveCodeSettings {
                        expected_revision, ..
                    } => *expected_revision,
                    _ => panic!("settings expected"),
                })
                .collect::<Vec<_>>(),
            [0, 1]
        );
        drop(requests);
        fixture.close(cx);
    }
}

#[gpui::test]
fn retains_configuration_conflicts_without_replaying(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.executable("rust-analyzer");
        let (owner, cx) = fixture.mount(cx);
        tap(cx, "code-tab-installed");
        let panel = cx.update(|_, cx| selected(&owner, cx));
        tap(cx, "code-edit-rust-analyzer");
        let editor = cx.update(|_, cx| panel.read(cx).editor.as_ref().unwrap().upgrade().unwrap());
        let mut settings = fixture.settings();
        settings.after_edit = false;
        fixture.execute(Command::SaveCodeSettings {
            expected_revision: settings.revision,
            settings,
        });
        input(cx, "code-command-0", "/fixture/draft-analyzer");
        wait(cx, |cx| {
            panel.read(cx).error == Some("code_conflict") && !panel.read(cx).pending
        });
        cx.update(|_, cx| {
            assert_eq!(
                editor.read(cx).fields[0].1.read(cx).value(),
                "/fixture/draft-analyzer"
            );
            assert_eq!(
                panel.read(cx).draft.as_ref().unwrap().servers["rust-analyzer"]
                    .command
                    .as_deref(),
                Some("/fixture/draft-analyzer")
            );
        });
        assert!(fixture.settings().servers.is_empty());
        let count = fixture.transport.requests.lock().unwrap().len();
        input(cx, "code-command-0", "/fixture/revised-draft-analyzer");
        tap(cx, "code-editor-close");
        cx.update(|_, cx| {
            assert_eq!(
                panel.read(cx).draft.as_ref().unwrap().servers["rust-analyzer"]
                    .command
                    .as_deref(),
                Some("/fixture/revised-draft-analyzer")
            )
        });
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), count);
        fixture.close(cx);
    }
}

#[gpui::test]
fn closes_configurations_after_target_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for escape in [false, true] {
            let fixture = Fixture::new(remote);
            fixture.executable("rust-analyzer");
            let (owner, cx) = fixture.mount(cx);
            tap(cx, "code-tab-installed");
            tap(cx, "code-edit-rust-analyzer");
            fixture.bind(&owner, cx, true, None);
            input(cx, "code-command-0", "/fixture/stale-analyzer");
            if escape {
                cx.simulate_keystrokes("escape");
                draw(cx);
            } else {
                tap(cx, "code-editor-close");
            }
            draw(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(fixture.transport.requests.lock().unwrap().is_empty());
            assert_eq!(fixture.settings().revision, 0);
            fixture.close(cx);
        }
    }
}
