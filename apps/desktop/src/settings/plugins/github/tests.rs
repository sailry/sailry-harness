#![cfg(unix)]
use super::super::super::test_support::{Fixture, draw, init, shown, tap, wait};
use super::*;
use core::prelude::v1::test;
use std::sync::atomic::Ordering;
#[path = "../../../../../../crates/node-runtime/tests/support/github.rs"]
mod cli;

fn open(
    fixture: &Fixture,
    owner: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<Editor> {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: "github".into(),
    }) else {
        panic!("GitHub plugin expected")
    };
    let binding = owner.read_with(cx, |owner, _| {
        owner.provider_link.as_ref().unwrap().binding.clone()
    });
    let editor = cx.update(|window, cx| super::super::mount(binding, info.summary, window, cx));
    wait(cx, |cx| {
        !editor.read(cx).loading
            && editor
                .read(cx)
                .github
                .as_ref()
                .is_some_and(|login| !login.reading)
    });
    assert!(editor.read_with(cx, |editor, _| {
        editor
            .github
            .as_ref()
            .unwrap()
            .status
            .as_ref()
            .unwrap()
            .authenticated
    }));
    draw(cx);
    editor
}

fn state(fixture: &Fixture) -> sailry_protocol::plugin::settings::State {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: "github".into(),
    }) else {
        panic!("plugin expected")
    };
    let Output::PluginSettings(state) = fixture.execute(Command::ReadPluginSettings {
        package: info.summary.reference(),
    }) else {
        panic!("settings expected")
    };
    state
}

#[gpui::test]
fn imports_without_exposing_the_token(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let cli = cli::Cli::new(directory.path());
        let fixture = Fixture::with_environment(remote, cli.source());
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        shown(visual, "github-login-import", true);
        assert_eq!(cli.calls("token"), 0);
        fixture.bind(&owner, visual, true);
        tap(visual, "github-login-import");
        wait(visual, |cx| {
            !editor.read(cx).loading
                && editor
                    .read(cx)
                    .github
                    .as_ref()
                    .is_some_and(|login| !login.pending && !login.reading)
        });
        assert_eq!(state(&fixture).configured, ["token"]);
        assert_eq!(cli.calls("token"), 1);
        let other = sailry_client::Client::new(fixture.other.local());
        let Output::Plugin(info) = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadPlugin {
                name: "github".into(),
            })))
            .unwrap()
        else {
            panic!("other plugin expected")
        };
        let Output::PluginSettings(other) = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadPluginSettings {
                package: info.summary.reference(),
            })))
            .unwrap()
        else {
            panic!("other settings expected")
        };
        assert!(other.configured.is_empty());
        assert!(
            editor.read_with(visual, |editor, cx| editor.fields.iter().all(
                |field| !matches!(
                    field.read(cx).value(cx),
                    Ok(fields::Value::Secret(
                        sailry_protocol::plugin::settings::SecretUpdate::Replace(_)
                    ))
                )
            ))
        );
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("github_login_imported")
        );
        assert!(
            !format!(
                "{:?}",
                editor.read_with(visual, |editor, _| editor
                    .github
                    .as_ref()
                    .unwrap()
                    .status
                    .clone())
            )
            .contains(cli::TOKEN)
        );
        visual.simulate_keystrokes("escape");
        fixture.close(visual);
    }
}

#[gpui::test]
fn resolves_a_lost_reply_without_reimporting(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let cli = cli::Cli::new(directory.path());
        let fixture = Fixture::with_environment(remote, cli.source());
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "github-login-import");
        wait(visual, |cx| {
            !editor.read(cx).loading
                && editor
                    .read(cx)
                    .github
                    .as_ref()
                    .is_some_and(|login| !login.pending && !login.reading)
        });
        assert_eq!(cli.calls("token"), 1);
        assert_eq!(state(&fixture).configured, ["token"]);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 1);
        assert!(editor.read_with(visual, |editor, _| {
            editor.github.as_ref().unwrap().request.is_none()
        }));
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("github_login_imported")
        );
        visual.simulate_keystrokes("escape");
        fixture.close(visual);
    }
}

#[gpui::test]
fn reports_revision_conflicts_as_toasts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let cli = cli::Cli::new(directory.path());
        let fixture = Fixture::with_environment(remote, cli.source());
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        let package = state(&fixture).package;
        fixture.execute(Command::SavePluginSettings {
            package,
            values: Default::default(),
            secrets: std::collections::BTreeMap::from([(
                "token".into(),
                sailry_protocol::plugin::settings::SecretUpdate::Clear,
            )]),
        });
        tap(visual, "github-login-import");
        wait(visual, |cx| editor.read(cx).error.is_some());
        assert_eq!(cli.calls("token"), 0);
        assert!(state(&fixture).configured.is_empty());
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.error),
            Some("plugins_settings_conflict")
        );
        crate::feedback::tests::shown(visual);
        visual.simulate_keystrokes("escape");
        fixture.close(visual);
    }
}
