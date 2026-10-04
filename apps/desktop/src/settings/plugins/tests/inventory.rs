use super::*;
use crate::settings::Section;

fn install(fixture: &Fixture) {
    fixture.package("1.0.0");
    fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 0,
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
        menu(visual, "plugin-menu-example", 0);
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
        menu(visual, "plugin-menu-example", 0);
        shown(visual, "plugin-server-legacy", true);
        shown(visual, "plugin-authorize-legacy", true);
        visual.simulate_keystrokes("escape");
        shown(visual, "plugin-live-details", false);
        fixture.close(visual);
    }
}

#[gpui::test]
fn discards_old_host_reads(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        fixture.transport.read_mode.store(1, Ordering::SeqCst);
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
                .any(|entry| entry.name == "external-browser")
        }));
        menu(visual, "plugin-menu-external-browser", 0);
        shown(visual, "plugin-settings-enabled", true);
        shown(visual, "plugin-settings-node", true);
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.selected_plugin_settings(), Some("external-browser"));
            let live = owner.provider_link.as_ref().unwrap();
            assert_eq!(live.binding.client.target(), fixture.client.target());
            assert_eq!(live.binding.label.as_ref(), "Fixture Node");
        });

        // Delay completion after admission, then select a different execution Node.
        fixture.transport.mode.store(2, Ordering::SeqCst);
        tap(visual, "plugin-settings-enabled");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert!(package(false).enabled);
        fixture.bind(&owner, visual, true);
        ready(&owner, visual);
        shown(visual, "plugin-settings-enabled", true);
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
            tap(visual, "plugin-settings-enabled");
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
        shown(visual, "plugin-settings-enabled", true);
        tap(visual, "plugin-settings-enabled");
        wait(visual, |_| !package().enabled);
        shown_settings(&owner, visual, "computer-permissions", true);
        select(&owner, visual, Section::Plugins);
        let revision = package().revision;
        updates::ready(&owner, visual, "computer", revision);
        menu(visual, "plugin-menu-computer", 1);
        assert!(!visual.did_prompt_for_paths());
        wait(visual, |_| package().revision > revision);
        ready(&owner, visual);
        assert!(!package().enabled);
        menu(visual, "plugin-menu-computer", 0);
        shown_settings(&owner, visual, "computer-permissions", true);
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
