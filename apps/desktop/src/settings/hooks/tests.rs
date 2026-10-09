use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt, dialog};
use sailry_protocol::Command;
use std::sync::atomic::Ordering;
mod fixture;
use fixture::{init, *};

fn seed(fixture: &Fixture) {
    fixture.execute(Command::SaveHooks {
        worktree: fixture.worktree,
        expected_revision: 0,
        settings: Settings {
            before_tool: Some(Script {
                command: "printf initial > hook.txt".into(),
                enabled: false,
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    fixture.transport.requests.lock().unwrap().clear();
}

fn settled(panel: &Entity<Panel>, cx: &mut VisualTestContext) {
    wait(cx, |cx| {
        !panel.read(cx).pending
            && panel.read(cx).request.is_none()
            && panel.read(cx).saved == panel.read(cx).draft
    });
}

#[gpui::test]
fn autosaves_entries_and_confirms_exact_scripts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        assert!(cx.debug_bounds("hooks-save").is_none());
        assert!(cx.debug_bounds("hooks-reload").is_none());
        for (name, command) in [
            ("Lint", "printf first >> hook.txt"),
            ("Check", "printf second >> hook.txt"),
        ] {
            tap(cx, "hooks-add");
            input(cx, "hooks-name", name);
            input(cx, "hooks-command", command);
            input(cx, "hooks-timeout", "1500");
            tap(cx, "hooks-editor-close");
            settled(&panel, cx);
        }
        let saved = fixture.settings();
        assert_eq!(saved.additional.len(), 2);
        assert_ne!(saved.additional[0].id, saved.additional[1].id);
        assert!(saved.additional.iter().all(|entry| !entry.script.enabled));
        assert!(!saved.enabled);
        assert!(!fixture.root.join("hook.txt").exists());
        let first = saved.additional[0].id.clone();
        let second = saved.additional[1].id.clone();
        tap(cx, &format!("hooks-enabled-{first}"));
        assert!(cx.debug_bounds("hooks-consent-scope").is_some());
        assert!(lookup(cx, &format!("hooks-consent-{first}")).is_some());
        tap(cx, "hooks-consent-cancel");
        assert!(!fixture.settings().additional[0].script.enabled);
        tap(cx, &format!("hooks-enabled-{first}"));
        tap(cx, "hooks-consent-enable");
        settled(&panel, cx);
        let request = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap();
        let Command::SaveHooks {
            settings,
            expected_revision,
            ..
        } = request.command
        else {
            panic!("save expected")
        };
        assert_eq!(expected_revision, saved.revision);
        assert_eq!(settings.additional[0].id, first);
        assert_eq!(
            settings.additional[0].script.command,
            "printf first >> hook.txt"
        );
        assert_eq!(settings.additional[0].script.timeout_ms, 1500);
        assert!(settings.additional[0].script.enabled);
        tap(cx, "hooks-enabled");
        tap(cx, "hooks-consent-enable");
        settled(&panel, cx);
        assert!(fixture.settings().enabled);
        tap(cx, &format!("hooks-edit-{first}"));
        input(cx, "hooks-command", "printf changed >> hook.txt");
        tap(cx, "hooks-editor-close");
        settled(&panel, cx);
        let changed = fixture.settings();
        assert_eq!(changed.additional[0].id, first);
        assert_eq!(changed.additional[0].script.name, "Lint");
        assert!(!changed.additional[0].script.enabled);
        assert_eq!(changed.additional[1].id, second);
        tap(cx, &format!("hooks-remove-{first}"));
        settled(&panel, cx);
        assert_eq!(fixture.settings().additional[0].id, second);
        assert!(!fixture.root.join("hook.txt").exists());
        fixture.close(cx);
    }
}

#[gpui::test]
fn retains_conflicts_and_original_uncertain_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        seed(&fixture);
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        tap(cx, "hooks-edit-before_tool");
        let editor = cx.update(|_, cx| panel.read(cx).editor.as_ref().unwrap().upgrade().unwrap());
        fixture.execute(Command::SaveHooks {
            worktree: fixture.worktree,
            expected_revision: 1,
            settings: Settings {
                revision: 1,
                turn_end: Some(Script {
                    command: "printf external".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        });
        input(cx, "hooks-command", "printf draft >> draft.txt");
        wait(cx, |cx| panel.read(cx).conflict && !panel.read(cx).pending);
        cx.update(|_, cx| {
            assert_eq!(panel.read(cx).error, Some("hooks_conflict"));
            assert_eq!(
                editor.read(cx).command.read(cx).value(),
                "printf draft >> draft.txt"
            );
            assert_eq!(
                panel
                    .read(cx)
                    .draft
                    .as_ref()
                    .unwrap()
                    .before_tool
                    .as_ref()
                    .unwrap()
                    .command,
                "printf draft >> draft.txt"
            );
        });
        assert!(fixture.settings().before_tool.is_none());
        tap(cx, "hooks-editor-close");
        tap(cx, "hooks-recover");
        wait(cx, |cx| !panel.read(cx).loading && !panel.read(cx).conflict);
        tap(cx, "hooks-edit-turn_end");
        fixture.transport.drop_next.store(true, Ordering::SeqCst);
        input(cx, "hooks-command", "printf retry >> once.txt");
        tap(cx, "hooks-editor-close");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_some()
        });
        let original = cx.update(|_, cx| panel.read(cx).request.clone().unwrap());
        let revision = fixture.settings().revision;
        fixture.bind(&owner, cx, false, Some(fixture.second));
        wait(cx, |cx| selected(&owner, cx).read(cx).saved.is_some());
        fixture.bind(&owner, cx, false, Some(fixture.worktree));
        tap(cx, "hooks-retry");
        settled(&panel, cx);
        let requests = fixture.transport.requests.lock().unwrap();
        let repeated = requests
            .iter()
            .filter(|request| request.id == original.id)
            .collect::<Vec<_>>();
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated[0], repeated[1]);
        drop(requests);
        assert_eq!(fixture.settings().revision, revision);
        assert!(!fixture.root.join("once.txt").exists());
        fixture.close(cx);
    }
}

#[gpui::test]
fn serializes_edits_during_pending_autosave(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        seed(&fixture);
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        tap(cx, "hooks-edit-before_tool");
        fixture.transport.hold.store(true, Ordering::SeqCst);
        input(cx, "hooks-command", "printf first >> once.txt");
        wait(cx, |cx| {
            panel.read(cx).pending
                && panel
                    .read(cx)
                    .draft
                    .as_ref()
                    .unwrap()
                    .before_tool
                    .as_ref()
                    .unwrap()
                    .command
                    == "printf first >> once.txt"
        });
        input(cx, "hooks-command", "printf latest >> once.txt");
        wait(cx, |cx| {
            panel
                .read(cx)
                .draft
                .as_ref()
                .unwrap()
                .before_tool
                .as_ref()
                .unwrap()
                .command
                == "printf latest >> once.txt"
        });
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 1);
        fixture.transport.hold.store(false, Ordering::SeqCst);
        fixture.transport.release.add_permits(1);
        settled(&panel, cx);
        tap(cx, "hooks-editor-close");
        let saved = fixture.settings();
        assert_eq!(
            saved.before_tool.unwrap().command,
            "printf latest >> once.txt"
        );
        assert_eq!(saved.revision, 3);
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let revisions = requests
            .iter()
            .map(|request| match &request.command {
                Command::SaveHooks {
                    expected_revision, ..
                } => *expected_revision,
                _ => panic!("save expected"),
            })
            .collect::<Vec<_>>();
        assert_eq!(revisions, [1, 2]);
        drop(requests);
        assert!(!fixture.root.join("once.txt").exists());
        fixture.close(cx);
    }
}

#[gpui::test]
fn refuses_stale_node_and_worktree_dialogs(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        seed(&fixture);
        let (owner, cx) = fixture.mount(cx);
        tap(cx, "hooks-enabled-before_tool");
        fixture.bind(&owner, cx, false, Some(fixture.second));
        tap(cx, "hooks-consent-enable");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert!(!fixture.settings().enabled);
        fixture.bind(&owner, cx, false, Some(fixture.worktree));
        wait(cx, |cx| !selected(&owner, cx).read(cx).loading);
        tap(cx, "hooks-enabled-before_tool");
        fixture.bind(&owner, cx, true, Some(fixture.worktree));
        tap(cx, "hooks-consent-enable");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert_eq!(fixture.settings().revision, 1);
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
        seed(&fixture);
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
        tap(visual, "hooks-enabled-before_tool");
        tap(visual, "hooks-consent-enable");
        wait(visual, |cx| !selected(&settings, cx).read(cx).pending);
        tap(visual, "hooks-enabled");
        tap(visual, "hooks-consent-enable");
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
        visual
            .update(|_, cx| assert!(!selected(&settings, cx).read(cx).current().unwrap().enabled));
        fixture.close(visual);
        visual
            .cx
            .update(|cx| cx.remove_global::<crate::backend::Services>());
    }
}

#[gpui::test]
fn preserves_identity_after_external_removal(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveHooks {
            worktree: fixture.worktree,
            expected_revision: 0,
            settings: Settings {
                additional: vec![
                    Entry {
                        id: "first".into(),
                        phase: Phase::BeforeTool,
                        script: Script {
                            name: "First".into(),
                            command: "printf first".into(),
                            enabled: false,
                            ..Default::default()
                        },
                    },
                    Entry {
                        id: "second".into(),
                        phase: Phase::TurnEnd,
                        script: Script {
                            name: "Second".into(),
                            command: "printf second".into(),
                            enabled: false,
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            },
        });
        fixture.transport.requests.lock().unwrap().clear();
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        tap(cx, "hooks-edit-second");
        let editor = cx.update(|_, cx| panel.read(cx).editor.as_ref().unwrap().upgrade().unwrap());
        let mut settings = fixture.settings();
        settings.additional.remove(0);
        fixture.execute(Command::SaveHooks {
            worktree: fixture.worktree,
            expected_revision: settings.revision,
            settings,
        });
        input(cx, "hooks-command", "printf second-draft");
        wait(cx, |cx| panel.read(cx).conflict && !panel.read(cx).pending);
        let saved = fixture.settings();
        assert_eq!(saved.additional.len(), 1);
        assert_eq!(saved.additional[0].id, "second");
        assert_eq!(saved.additional[0].script.command, "printf second");
        cx.update(|_, cx| {
            assert_eq!(
                editor.read(cx).command.read(cx).value(),
                "printf second-draft"
            );
            assert_eq!(
                panel.read(cx).entry("first").unwrap().1.command,
                "printf first"
            );
            assert_eq!(
                panel.read(cx).entry("second").unwrap().1.command,
                "printf second-draft"
            );
        });
        tap(cx, "hooks-editor-close");
        fixture.close(cx);
    }
}

#[gpui::test]
fn closes_editors_after_target_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for escape in [false, true] {
            let fixture = Fixture::new(remote);
            seed(&fixture);
            let (owner, cx) = fixture.mount(cx);
            tap(cx, "hooks-edit-before_tool");
            fixture.bind(&owner, cx, false, Some(fixture.second));
            input(cx, "hooks-command", "printf stale > stale.txt");
            if escape {
                cx.simulate_keystrokes("escape");
                draw(cx);
            } else {
                tap(cx, "hooks-editor-close");
            }
            wait(cx, |cx| selected(&owner, cx).read(cx).saved.is_some());
            draw(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(fixture.transport.requests.lock().unwrap().is_empty());
            assert_eq!(fixture.settings().revision, 1);
            assert!(!fixture.root.join("stale.txt").exists());
            fixture.close(cx);
        }
    }
}
