use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt, dialog};
use sailry_protocol::Command;
use std::sync::atomic::Ordering;
mod fixture;
use fixture::{init, *};

#[gpui::test]
fn saves_only_after_exact_script_consent(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        input(cx, "hooks-command-0", "printf before >> hooks.txt");
        input(cx, "hooks-command-1", "printf after >> hooks.txt");
        input(cx, "hooks-command-2", "printf end >> hooks.txt");
        input(cx, "hooks-timeout-0", "1500");
        tap(cx, "hooks-enabled");
        assert!(!fixture.settings().enabled);
        assert!(!fixture.root.join("hooks.txt").exists());
        let captured = cx.update(|_, cx| panel.read(cx).draft(cx).unwrap());
        tap(cx, "hooks-save");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        for selector in [
            "hooks-consent-scope",
            "hooks-consent-before_tool",
            "hooks-consent-after_tool",
            "hooks-consent-turn_end",
        ] {
            assert!(cx.debug_bounds(selector).is_some());
        }
        tap(cx, "hooks-consent-cancel");
        assert!(!fixture.settings().enabled);
        tap(cx, "hooks-save");
        tap(cx, "hooks-consent-save");
        wait(cx, |cx| !panel.read(cx).pending);
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].command,
            Command::SaveHooks {
                worktree: fixture.worktree,
                expected_revision: 0,
                settings: captured
            }
        );
        drop(requests);
        let saved = fixture.settings();
        assert!(saved.enabled);
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.before_tool.unwrap().timeout_ms, 1500);
        tap(cx, "hooks-enabled");
        tap(cx, "hooks-save");
        tap(cx, "hooks-consent-save");
        wait(cx, |cx| !panel.read(cx).pending);
        assert!(!fixture.settings().enabled);
        fixture.close(cx);
    }
}

#[gpui::test]
fn retains_conflicts_and_original_uncertain_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        input(cx, "hooks-command-0", "printf draft >> draft.txt");
        tap(cx, "hooks-enabled");
        fixture.execute(Command::SaveHooks {
            worktree: fixture.worktree,
            expected_revision: 0,
            settings: Settings {
                turn_end: Some(Script {
                    command: "printf external".into(),
                    timeout_ms: 500,
                }),
                ..Default::default()
            },
        });
        tap(cx, "hooks-save");
        tap(cx, "hooks-consent-save");
        wait(cx, |cx| !panel.read(cx).pending);
        cx.update(|_, cx| {
            assert_eq!(panel.read(cx).error, Some("hooks_conflict"));
            assert_eq!(
                panel.read(cx).fields[0].1.read(cx).value(),
                "printf draft >> draft.txt"
            );
        });
        assert!(!fixture.settings().enabled);
        tap(cx, "hooks-reload");
        wait(cx, |cx| !panel.read(cx).loading);
        input(cx, "hooks-command-0", "printf retry >> once.txt");
        tap(cx, "hooks-enabled");
        fixture.transport.drop_next.store(true, Ordering::SeqCst);
        tap(cx, "hooks-save");
        tap(cx, "hooks-consent-save");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_some()
        });
        let original = cx.update(|_, cx| panel.read(cx).request.clone().unwrap());
        let revision = fixture.settings().revision;
        fixture.bind(&owner, cx, false, Some(fixture.second));
        wait(cx, |cx| selected(&owner, cx).read(cx).saved.is_some());
        fixture.bind(&owner, cx, false, Some(fixture.worktree));
        tap(cx, "hooks-retry");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_none()
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let repeated: Vec<_> = requests
            .iter()
            .filter(|request| request.id == original.id)
            .collect();
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated[0], repeated[1]);
        drop(requests);
        assert_eq!(fixture.settings().revision, revision);
        assert!(!fixture.root.join("once.txt").exists());
        fixture.close(cx);
    }
}

#[gpui::test]
fn refuses_stale_node_and_worktree_dialogs(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, cx) = fixture.mount(cx);
        input(cx, "hooks-command-0", "printf captured");
        tap(cx, "hooks-enabled");
        tap(cx, "hooks-save");
        fixture.bind(&owner, cx, false, Some(fixture.second));
        tap(cx, "hooks-consent-save");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert!(!fixture.settings().enabled);
        fixture.bind(&owner, cx, false, Some(fixture.worktree));
        tap(cx, "hooks-save");
        fixture.bind(&owner, cx, true, Some(fixture.worktree));
        tap(cx, "hooks-consent-save");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert_eq!(fixture.settings().revision, 0);
        fixture.bind(&owner, cx, false, None);
        draw(cx);
        assert!(cx.debug_bounds("empty-hooks_project_required").is_some());
        assert!(cx.debug_bounds("hooks-enabled").is_none());
        fixture.close(cx);
    }
}

#[gpui::test]
fn follows_selected_project_in_shell(cx: &mut TestAppContext) {
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
                shell.navigate(crate::preview::Page::Settings, window, cx)
            });
            entity = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = entity.unwrap();
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        });
        tap(visual, "hooks_settings");
        assert!(
            visual
                .debug_bounds("empty-hooks_project_required")
                .is_some()
        );
        let sailry_protocol::Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
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
            settings.read(cx).hooks.selected == Some((fixture.node.id(), fixture.worktree))
                && selected(&settings, cx).read(cx).saved.is_some()
        });
        input(visual, "hooks-command-0", "printf shell");
        tap(visual, "hooks-enabled");
        tap(visual, "hooks-save");
        tap(visual, "hooks-consent-save");
        wait(visual, |cx| !selected(&settings, cx).read(cx).pending);
        assert!(fixture.settings().enabled);
        let project = snapshot
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
                    .select_project_worktree(project, fixture.second);
                cx.notify();
            })
        });
        wait(visual, |cx| {
            settings.read(cx).hooks.selected == Some((fixture.node.id(), fixture.second))
                && selected(&settings, cx).read(cx).saved.is_some()
        });
        visual.update(|_, cx| assert!(!selected(&settings, cx).read(cx).enabled));
        fixture.close(visual);
        visual
            .cx
            .update(|cx| cx.remove_global::<crate::backend::Services>());
    }
}
