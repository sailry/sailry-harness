use super::*;
use crate::settings::{
    Section,
    plugins::test_support::{Fixture, draw, init, input, shown, tap, wait},
};
use core::prelude::v1::test;
use sailry_protocol::plugin::mcp::{Configuration, SCHEMA, Server};
use std::{collections::BTreeMap, sync::atomic::Ordering};

fn info(fixture: &Fixture, name: &str) -> Info {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin { name: name.into() }) else {
        panic!("plugin information expected")
    };
    info
}

fn state(fixture: &Fixture, name: &str) -> Configuration {
    let Output::PluginMcp(state) = fixture.execute(Command::ReadPluginMcp {
        package: info(fixture, name).summary.reference(),
    }) else {
        panic!("MCP configuration expected")
    };
    state.configuration
}

fn package(fixture: &Fixture) {
    fixture.package("1.0.0");
    let configuration = Configuration {
        schema: SCHEMA.into(),
        servers: BTreeMap::from([
            (
                "native".into(),
                Server::Stdio {
                    command: "sailry_isolated_missing_executable".into(),
                    args: vec!["an argument".into()],
                    env: BTreeMap::from([("DEFAULT".into(), "package-default".into())]),
                    cwd: "${PLUGIN_ROOT}".into(),
                },
            ),
            (
                "remote".into(),
                Server::Http {
                    url: "https://example.invalid/mcp".into(),
                    headers: BTreeMap::from([("X-Package".into(), "package-header".into())]),
                },
            ),
            (
                "events".into(),
                Server::Sse {
                    url: "https://example.invalid/events".into(),
                    headers: BTreeMap::new(),
                },
            ),
        ]),
    };
    std::fs::write(
        fixture.directory.path().join("project/package/mcp.json"),
        serde_json::to_string(&configuration).unwrap(),
    )
    .unwrap();
    fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 0,
    });
}

fn mount(owner: &Entity<Workspace>, info: Info, cx: &mut VisualTestContext) -> Entity<Editor> {
    let binding = owner.read_with(cx, |owner, _| {
        owner.provider_link.as_ref().unwrap().binding.clone()
    });
    let editor = cx.update(|window, cx| open(binding, info, window, cx));
    wait(cx, |cx| editor.read(cx).loaded && !editor.read(cx).loading);
    draw(cx);
    editor
}

fn source(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, cx| {
        editor.document.read(cx).value().to_string()
    })
}

fn edit(cx: &mut VisualTestContext, configuration: &Configuration) {
    input(
        cx,
        "mcp-configuration-document",
        &serde_json::to_string_pretty(configuration).unwrap(),
    );
}

#[gpui::test]
fn bundled_cards_and_details_open_configuration(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        owner.update(visual, |owner, cx| owner.select(Section::Mcp, cx));
        shown(visual, "mcp-card-context7", true);
        shown(visual, "mcp-card-github", true);
        tap(visual, "mcp-edit-context7");
        shown(visual, "mcp-configuration-document", true);
        shown(visual, "mcp-editor", false);
        tap(visual, "mcp-configuration-cancel");
        owner.update(visual, |owner, cx| owner.select(Section::Plugins, cx));
        tap(visual, "plugin-details-github");
        shown(visual, "plugin-details-configure", true);
        tap(visual, "plugin-details-configure");
        shown(visual, "mcp-configuration-document", true);
        tap(visual, "mcp-configuration-cancel");
        visual.simulate_keystrokes("escape");
        fixture.close(visual);
    }
}

#[gpui::test]
fn saves_literal_environment_and_headers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        package(&fixture);
        let (owner, visual) = fixture.mount(cx);
        let editor = mount(&owner, info(&fixture, "example"), visual);
        let declared = state(&fixture, "example");
        assert_eq!(
            serde_json::from_str::<Configuration>(&source(&editor, visual)).unwrap(),
            declared
        );
        let mut configuration = declared;
        let Server::Stdio {
            command,
            args,
            env,
            cwd,
        } = configuration.servers.get_mut("native").unwrap()
        else {
            panic!("stdio server expected")
        };
        *command = "sailry_updated_missing_executable".into();
        *args = vec!["an updated argument".into(), "--fixture".into()];
        *cwd = "${PLUGIN_ROOT}/skills".into();
        env.insert("TOKEN".into(), "fixture-env-value".into());
        let Server::Http { url, headers } = configuration.servers.get_mut("remote").unwrap() else {
            panic!("HTTP server expected")
        };
        *url = "https://changed.example.invalid/mcp".into();
        headers.insert("Authorization".into(), "Bearer fixture-http-value".into());
        let Server::Sse { headers, .. } = configuration.servers.get_mut("events").unwrap() else {
            panic!("SSE server expected")
        };
        headers.insert("X-Access-Key".into(), "fixture-sse-value".into());
        edit(visual, &configuration);
        assert_eq!(
            serde_json::from_str::<Configuration>(&source(&editor, visual)).unwrap(),
            configuration
        );
        let command = editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap());
        assert!(!format!("{command:?}").contains("fixture-http-value"));
        tap(visual, "mcp-configuration-save");
        shown(visual, "mcp-configuration", false);
        assert_eq!(state(&fixture, "example"), configuration);
        assert!(source(&editor, visual).is_empty());
        let public = serde_json::to_string(&info(&fixture, "example")).unwrap();
        for marker in [
            "fixture-env-value",
            "fixture-http-value",
            "fixture-sse-value",
        ] {
            assert!(!public.contains(marker));
        }
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("mcp_configuration_saved")
        );
        let editor = mount(&owner, info(&fixture, "example"), visual);
        assert_eq!(
            serde_json::from_str::<Configuration>(&source(&editor, visual)).unwrap(),
            configuration
        );
        tap(visual, "mcp-configuration-cancel");
        assert!(source(&editor, visual).is_empty());
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_invalid_and_conflicting_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        package(&fixture);
        let (owner, visual) = fixture.mount(cx);
        let editor = mount(&owner, info(&fixture, "example"), visual);
        let declared = state(&fixture, "example");
        input(visual, "mcp-configuration-document", "{invalid MCP draft");
        let draft = source(&editor, visual);
        tap(visual, "mcp-configuration-save");
        wait(visual, |cx| {
            editor.read(cx).error == Some("mcp_configuration_invalid")
        });
        assert_eq!(source(&editor, visual), draft);
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("mcp_configuration_invalid")
        );
        let mut configuration = declared.clone();
        let Server::Stdio { command, .. } = configuration.servers.get_mut("native").unwrap() else {
            panic!("stdio server expected")
        };
        *command = "invalid command with spaces".into();
        edit(visual, &configuration);
        let draft = source(&editor, visual);
        tap(visual, "mcp-configuration-save");
        wait(visual, |cx| {
            !editor.read(cx).pending && editor.read(cx).request.is_none()
        });
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.error),
            Some("mcp_configuration_invalid")
        );
        assert_eq!(source(&editor, visual), draft);
        assert_eq!(state(&fixture, "example"), declared);
        edit(visual, &declared);
        let draft = source(&editor, visual);
        let summary = info(&fixture, "example").summary;
        fixture.execute(Command::SavePluginMcp {
            package: summary.reference(),
            configuration: declared.clone(),
        });
        tap(visual, "mcp-configuration-save");
        wait(visual, |cx| {
            editor.read(cx).error == Some("plugins_settings_conflict")
        });
        assert_eq!(source(&editor, visual), draft);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("plugins_settings_conflict")
        );
        tap(visual, "mcp-configuration-cancel");
        fixture.close(visual);
    }
}

#[gpui::test]
fn retries_uncertain_save_on_captured_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let editor = mount(&owner, info(&fixture, "github"), visual);
        let mut configuration = state(&fixture, "github");
        let Server::Http { headers, .. } = configuration.servers.get_mut("github").unwrap() else {
            panic!("HTTP server expected")
        };
        headers.insert(
            "Authorization".into(),
            "Bearer fixture-captured-token".into(),
        );
        edit(visual, &configuration);
        let draft = source(&editor, visual);
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "mcp-configuration-save");
        wait(visual, |cx| {
            !editor.read(cx).pending && editor.read(cx).request.is_some()
        });
        let id = editor.read_with(visual, |editor, _| editor.request.as_ref().unwrap().id);
        assert!(editor.read_with(visual, |editor, _| editor.locked()));
        input(
            visual,
            "mcp-configuration-document",
            "replacement must be rejected",
        );
        assert_eq!(source(&editor, visual), draft);
        fixture.bind(&owner, visual, true);
        tap(visual, "mcp-configuration-save");
        shown(visual, "mcp-configuration", false);
        assert_eq!(*fixture.transport.requests.lock().unwrap(), [id, id]);
        assert_eq!(state(&fixture, "github"), configuration);
        let client = sailry_client::Client::new(fixture.other.local());
        let Output::Plugin(info) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadPlugin {
                name: "github".into(),
            })))
            .unwrap()
        else {
            panic!("plugin information expected")
        };
        let Output::PluginMcp(state) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadPluginMcp {
                package: info.summary.reference(),
            })))
            .unwrap()
        else {
            panic!("MCP configuration expected")
        };
        assert_ne!(state.configuration, configuration);
        fixture.close(visual);
    }
}
