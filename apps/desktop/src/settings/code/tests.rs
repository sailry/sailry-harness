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
fn scopes_languages_and_saves_controls(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        cx.update(|_, cx| {
            let languages = &panel.read(cx).workspace.as_ref().unwrap().languages;
            assert_eq!(
                languages
                    .iter()
                    .map(|language| language.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["rust", "toml"]
            );
        });
        tap(cx, "code-after-edit");
        wait(cx, |cx| !panel.read(cx).pending && !panel.read(cx).loading);
        assert!(!fixture.settings().after_edit);
        tap(cx, "code-enabled-0-0");
        wait(cx, |cx| !panel.read(cx).pending && !panel.read(cx).loading);
        assert!(fixture.settings().servers["rust-analyzer"].disabled);
        tap(cx, "code-edit-0-0");
        input(cx, "code-command-0", "/fixture/custom-analyzer");
        input(cx, "code-command-1", "[\"--stdio\"]");
        input(cx, "code-command-2", "{\"check\":false}");
        input(cx, "code-command-3", "{\"FIXTURE_SETTING\":\"kept\"}");
        tap(cx, "code-editor-save");
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
        wait(cx, |cx| selected(&owner, cx).read(cx).workspace.is_some());
        cx.update(|_, cx| {
            assert!(
                selected(&owner, cx)
                    .read(cx)
                    .workspace
                    .as_ref()
                    .unwrap()
                    .languages
                    .is_empty()
            )
        });
        fixture.bind(&owner, cx, false, None);
        draw(cx);
        assert!(cx.debug_bounds("empty-code_project_required").is_some());
        assert!(cx.debug_bounds("code-after-edit").is_none());
        fixture.close(cx);
    }
}

#[gpui::test]
fn preserves_conflicting_draft_and_original_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        let mut settings = fixture.settings();
        let revision = settings.revision;
        settings.after_edit = false;
        fixture.execute(Command::SaveCodeSettings {
            settings,
            expected_revision: revision,
        });
        tap(cx, "code-enabled-0-0");
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
        tap(cx, "code-after-edit");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_some()
        });
        let original = cx.update(|_, cx| panel.read(cx).request.clone().unwrap());
        assert!(fixture.settings().after_edit);
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
        tap(cx, "code-install-0-0");
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        fixture.bind(&owner, cx, false, Some(fixture.second));
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
        fixture.bind(&owner, cx, false, Some(fixture.worktree));
        wait(cx, |cx| !selected(&owner, cx).read(cx).loading);
        tap(cx, "code-install-0-0");
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
        assert!(cx.debug_bounds("empty-code_project_required").is_some());
        fixture.close(cx);
    }
}

#[gpui::test]
fn follows_shell_project_selection(cx: &mut TestAppContext) {
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
        tap(visual, "settings_code");
        assert!(visual.debug_bounds("empty-code_project_required").is_some());
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
            settings.read(cx).code.selected == Some((fixture.node.id(), fixture.worktree))
                && selected(&settings, cx).read(cx).saved.is_some()
        });
        assert!(visual.debug_bounds("code-language-0").is_some());
        tap(visual, "code-after-edit");
        wait(visual, |cx| !selected(&settings, cx).read(cx).pending);
        assert!(!fixture.settings().after_edit);
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
            settings.read(cx).code.selected == Some((fixture.node.id(), fixture.second))
                && selected(&settings, cx).read(cx).workspace.is_some()
        });
        assert!(visual.debug_bounds("empty-code_languages_empty").is_some());
        fixture.close(visual);
        visual
            .cx
            .update(|cx| cx.remove_global::<crate::backend::Services>());
    }
}
