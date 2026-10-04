use super::workspace::{click, dispatch};
use super::*;
use crate::workspace::{Command, Owner, Target};

fn empty(shell: &Entity<Shell>, cx: &mut VisualTestContext, project: usize) -> Owner {
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell
                .workspace
                .add_worktree(project, "main", "feature/remove", "/preview/remove")
                .unwrap()
        })
    })
}

fn open(shell: &Entity<Shell>, cx: &mut VisualTestContext, owner: Owner) {
    dispatch(shell, cx, Target::Project(owner), Command::RemoveWorktree);
}

#[gpui::test]
fn row_action_cancel_and_confirm(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 0);
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 0)]
            .input
            .clone()
            .update(cx, |input, cx| {
                input.set_value("Keep the draft", window, cx)
            })
    });
    click(&mut cx, "sidebar-project");
    let selected = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(selected), Command::Open);
    dispatch(
        &shell,
        &mut cx,
        Target::Project(selected),
        Command::Worktrees,
    );
    click(&mut cx, "worktree-remove-0");
    assert!(cx.debug_bounds("worktree-row-0").is_some());
    click(&mut cx, "worktree-remove-4");
    assert!(cx.has_pending_prompt());
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.owner(0).worktree),
        0
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(owner)));
    crate::prompts::tests::answer(&mut cx, "settings_cancel");
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    let selected = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(selected), Command::Open);
    dispatch(
        &shell,
        &mut cx,
        Target::Project(selected),
        Command::Worktrees,
    );
    click(&mut cx, "worktree-remove-4");
    crate::prompts::tests::answer(&mut cx, "worktree_remove_preview");
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        let shell = shell.read(cx);
        assert!(!shell.workspace.contains(owner));
        assert_eq!(shell.page, Page::Project);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Keep the draft"
        );
        assert_eq!(shell.workspace.sessions[&(0, 0)].owner.worktree, 0);
    });
}

#[gpui::test]
fn admission_rechecks_new_resources(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 0);
    open(&shell, &mut cx, owner);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let key = shell.workspace.create_session(owner);
            shell.workspace.sessions.get_mut(&key).unwrap().archived = true;
        })
    });
    crate::prompts::tests::answer(&mut cx, "worktree_remove_preview");
    crate::feedback::tests::shown(&mut cx);
    assert!(!cx.has_pending_prompt());
    assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(owner)));
    assert!(!cx.has_pending_prompt());
    cx.update(|window, cx| window.clear_notifications(cx));
    cx.run_until_parked();
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn revalidates_confirmation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 0);
    open(&shell, &mut cx, owner);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell
                .workspace
                .worktrees
                .get_mut(&owner.worktree)
                .unwrap()
                .path = "/preview/replaced".into();
        })
    });
    crate::prompts::tests::answer(&mut cx, "worktree_remove_preview");
    crate::feedback::tests::shown(&mut cx);
    assert!(!cx.has_pending_prompt());
    cx.update(|window, cx| window.clear_notifications(cx));
    open(&shell, &mut cx, owner);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.workspace.projects.get_mut(&0).unwrap().trusted = false
        })
    });
    crate::prompts::tests::answer(&mut cx, "worktree_remove_preview");
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(owner)));
}

#[gpui::test]
fn binds_remote_target(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 1);
    open(&shell, &mut cx, owner);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_session((0, 1), window, cx)));
    let current = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    let message = cx.update(|_, cx| shell.read(cx).git.message.clone());
    crate::prompts::tests::answer(&mut cx, "worktree_remove_preview");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(!shell.workspace.contains(owner));
        assert_eq!(shell.host, 0);
        assert_eq!(shell.session, 1);
        assert_eq!(shell.workspace.owner(0), current);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(shell.git.message, message);
    });
}

#[gpui::test]
fn retires_selected_editors(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 0);
    let message = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_owner(owner, window, cx);
            shell.navigate(Page::Git, window, cx);
            shell.git.tabs.open.push(1);
            shell.files.tabs.open.push(1);
            shell.git.message.update(cx, |input, cx| {
                input.set_value("Old tree draft", window, cx)
            });
            shell.git.message.clone()
        })
    });
    open(&shell, &mut cx, owner);
    crate::prompts::tests::answer(&mut cx, "worktree_remove_preview");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.workspace.owner(0).worktree, 0);
        assert_eq!(shell.page, Page::Git);
        assert_ne!(shell.git.message, message);
        assert_eq!(shell.git.message.read(cx).value(), "");
        assert_eq!(shell.files.tabs.open.len(), 1);
        assert_eq!(shell.git.tabs.open.len(), 1);
    });
}

#[gpui::test]
fn geometry_and_protection(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 0);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let tree = shell.workspace.worktrees.get_mut(&owner.worktree).unwrap();
            tree.branch = "feature/long-name/".repeat(50).into();
            tree.path = format!("/preview/{}", "long-directory/".repeat(50)).into();
        })
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(560.)));
            open(&shell, &mut cx, owner);
            // Allow the Kit dialog animation to finish before checking geometry.
            std::thread::sleep(std::time::Duration::from_millis(300));
            cx.update(|window, cx| {
                _ = window.draw(cx);
            });
            let (title, detail) = crate::prompts::tests::wait(&mut cx);
            assert_eq!(title, tr("worktree_remove").as_ref());
            assert!(detail.contains("/preview/"));
            crate::prompts::tests::answer(&mut cx, "settings_cancel");
        }
    }
    open(
        &shell,
        &mut cx,
        Owner {
            worktree: 0,
            ..owner
        },
    );
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.update(|_, cx| shell.read(cx).workspace.worktrees.contains_key(&0)));
}

#[gpui::test]
fn long_manager_scrolling(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, cx| {
        shell.update(cx, |shell, _| {
            for index in 0..24 {
                shell
                    .workspace
                    .add_worktree(
                        0,
                        "main",
                        &format!("feature/{index}"),
                        &format!("/preview/tree-{index}"),
                    )
                    .unwrap();
            }
        });
        window.window_handle()
    });
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    click(&mut cx, "sidebar-project");
    let selected = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(selected), Command::Open);
    dispatch(
        &shell,
        &mut cx,
        Target::Project(selected),
        Command::Worktrees,
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let viewport = cx.debug_bounds("worktree-list").unwrap();
    cx.simulate_event(ScrollWheelEvent {
        position: viewport.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-5000.))),
        ..Default::default()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let last = cx.debug_bounds("worktree-remove-27").unwrap();
    assert!(last.top() >= viewport.top() && last.bottom() <= viewport.bottom());
    click(&mut cx, "worktree-remove-27");
    assert!(cx.has_pending_prompt());
    crate::prompts::tests::answer(&mut cx, "worktree_remove_preview");
    assert!(!cx.update(|_, cx| shell.read(cx).workspace.worktrees.contains_key(&27)));
}
