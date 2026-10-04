use super::*;
use crate::workspace::{Command, Dispatch, Owner, Target};

pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        // Kit animates dialog geometry with wall time, not the test executor clock.
        std::thread::sleep(std::time::Duration::from_millis(300));
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

pub(super) fn dispatch(
    shell: &Entity<Shell>,
    cx: &mut VisualTestContext,
    target: Target,
    command: Command,
) {
    cx.update(|window, cx| {
        shell.read(cx).focus.clone().focus(window, cx);
        assert!(!shell.read(cx).native_workspace_menu(target).is_empty());
        window.dispatch_action(Box::new(Dispatch { target, command }), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

#[gpui::test]
fn project_record_search(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let (target, path) = cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.workspace.sessions.get_mut(&(0, 0)).unwrap().title = "Search Target".into();
            shell.workspace.sessions.get_mut(&(0, 1)).unwrap().title = "Other session".into();
            shell.workspace.terminals.get_mut(&(0, 0)).unwrap().title = "Console Target".into();
            let owner = shell.workspace.owner(shell.host);
            (
                Target::Project(owner),
                shell.workspace.worktrees[&owner.worktree].path.to_string(),
            )
        })
    });
    dispatch(&shell, &mut cx, target, Command::Open);
    let search = |cx: &mut VisualTestContext, value: &str| {
        click(cx, "project-record-search");
        cx.simulate_keystrokes("secondary-a backspace");
        cx.simulate_input(value);
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    };
    search(&mut cx, "  TARGET  ");
    assert!(cx.debug_bounds("project-record-0-0").is_some());
    assert!(cx.debug_bounds("project-record-0-1").is_none());
    click(&mut cx, "project-record-terminals");
    assert_eq!(cx.update(|_, cx| shell.read(cx).project_tab), 1);
    assert!(cx.debug_bounds("project-record-0-0").is_some());
    search(&mut cx, "missing-record-query");
    assert!(cx.debug_bounds("project-record-0-0").is_none());
    click(&mut cx, "project-record-sessions");
    assert!(cx.debug_bounds("project-record-0-0").is_none());
    search(&mut cx, &path.to_uppercase());
    assert!(cx.debug_bounds("project-record-0-0").is_some());
    search(&mut cx, "");
    assert!(cx.debug_bounds("project-record-0-0").is_some());
    assert!(cx.debug_bounds("project-record-0-1").is_some());
}

#[gpui::test]
fn project_and_host_entries(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    assert!(cx.debug_bounds("navigation-terminal").is_none());
    assert!(cx.debug_bounds("terminal-0").is_some());
    let target =
        cx.update(|_, cx| Target::Project(shell.read(cx).workspace.owner(shell.read(cx).host)));
    dispatch(&shell, &mut cx, target, Command::Open);
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Project);
    assert!(cx.debug_bounds("project-overview").is_some());
    assert!(cx.debug_bounds("session-0").is_some());
    click(&mut cx, "project-disclosure-0");
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Project);
    assert!(cx.debug_bounds("session-0").is_none());
    click(&mut cx, "project-disclosure-0");
    click(&mut cx, "project-new-session");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(
            shell.workspace.sessions[&(0, shell.session)].owner,
            Owner {
                host: 0,
                project: 0,
                worktree: 0
            }
        );
    });
    click(&mut cx, "host-1");
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Host);
    assert!(cx.debug_bounds("host-overview").is_some());
    click(&mut cx, "host-project-1");
    click(&mut cx, "project-new-terminal");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Terminal);
        let key = shell.workspace.terminal.unwrap();
        assert_eq!(
            shell.workspace.terminals[&key].owner,
            Owner {
                host: 1,
                project: 1,
                worktree: 1
            }
        );
    });
    assert!(cx.debug_bounds("terminal-overview").is_some());
}

#[gpui::test]
fn section_visibility(cx: &mut TestAppContext) {
    let (_, mut cx) = setup(cx);
    click(&mut cx, "project-disclosure-0");
    assert!(cx.debug_bounds("session-0").is_none());
    click(&mut cx, "projects-toggle");
    assert!(cx.debug_bounds("sidebar-project").is_none());
    assert!(cx.debug_bounds("host-0").is_some());
    cx.simulate_keystrokes("enter");
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("sidebar-project").is_some());
    assert!(cx.debug_bounds("session-0").is_none());
    click(&mut cx, "hosts-toggle");
    assert!(cx.debug_bounds("host-0").is_none());
    assert!(cx.debug_bounds("sidebar-project").is_some());
    cx.simulate_keystrokes("space");
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("host-0").is_some());
    click(&mut cx, "host-add");
    assert!(cx.debug_bounds("host-0").is_some());
    click(&mut cx, "project-add");
    assert!(cx.debug_bounds("sidebar-project").is_some());
    click(&mut cx, "project-cancel");
    click(&mut cx, "hosts-label");
    assert!(cx.debug_bounds("host-0").is_none());
    click(&mut cx, "hosts-label");
    assert!(cx.debug_bounds("host-0").is_some());
    click(&mut cx, "projects-label");
    assert!(cx.debug_bounds("sidebar-project").is_none());
    click(&mut cx, "projects-label");
    assert!(cx.debug_bounds("sidebar-project").is_some());
}

#[gpui::test]
fn section_alignment(cx: &mut TestAppContext) {
    let (_, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let row = cx.debug_bounds("host-0").unwrap();
            for (label, arrow, add) in [
                ("hosts-label", "hosts-disclosure", "host-add"),
                ("projects-label", "projects-disclosure", "project-add"),
            ] {
                let title = cx.debug_bounds(label).unwrap();
                let disclosure = cx.debug_bounds(arrow).unwrap();
                let plus = cx.debug_bounds(add).unwrap();
                assert_eq!(title.left(), row.left() + px(8.));
                assert!(title.right() < plus.left());
                assert_eq!(disclosure.left(), plus.right() + px(4.));
                assert_eq!(disclosure.right(), row.right() - px(8.));
                let project = cx.debug_bounds("project-disclosure-0").unwrap();
                assert_eq!(disclosure.center().x, project.center().x);
                assert_eq!(disclosure.right(), project.right());
                cx.simulate_mouse_move(title.center(), None, Modifiers::default());
                cx.run_until_parked();
                cx.update(|window, cx| window.draw(cx).clear(cx));
                assert_eq!(cx.debug_bounds(label).unwrap(), title);
                assert_eq!(cx.debug_bounds(arrow).unwrap(), disclosure);
            }
        }
    }
}

#[gpui::test]
fn hidden_worktree_ownership(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "session-1");
    let target =
        cx.update(|_, cx| Target::Project(shell.read(cx).workspace.owner(shell.read(cx).host)));
    dispatch(&shell, &mut cx, target, Command::Open);
    click(&mut cx, "project-new-terminal");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let terminal = &shell.workspace.terminals[&shell.workspace.terminal.unwrap()];
        assert_eq!(terminal.owner.worktree, 2);
        assert_eq!(terminal.owner, shell.workspace.sessions[&(0, 1)].owner);
    });
    let target =
        cx.update(|_, cx| Target::Project(shell.read(cx).workspace.owner(shell.read(cx).host)));
    dispatch(&shell, &mut cx, target, Command::Open);
    click(&mut cx, "project-new-session");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.workspace.sessions[&(0, shell.session)].owner.worktree,
            2
        );
        assert_eq!(
            shell.conversations[&(0, shell.session)].options.choices[1],
            1
        );
    });
}

#[gpui::test]
fn native_action_targets(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    dispatch(&shell, &mut cx, Target::Session((1, 0)), Command::Pin);
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(shell.workspace.sessions[&(1, 0)].pinned);
        assert!(!shell.workspace.sessions[&(0, 0)].pinned);
        assert_eq!(shell.host, 0);
    });
    dispatch(&shell, &mut cx, Target::Session((0, 1)), Command::Rename);
    click(&mut cx, "workspace-name-input");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("Renamed preview");
    click(&mut cx, "workspace-rename-save");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.workspace.sessions[&(0, 1)].title, "Renamed preview");
        assert_eq!(shell.workspace.sessions[&(0, 0)].title, tr("session"));
    });
}

#[gpui::test]
fn archive_restore_keeps_draft(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 0)]
            .input
            .clone()
            .update(cx, |input, cx| {
                input.set_value("Preserved draft", window, cx)
            });
    });
    dispatch(&shell, &mut cx, Target::Session((0, 0)), Command::Archive);
    crate::prompts::tests::answer(&mut cx, "workspace_confirm");
    assert!(cx.debug_bounds("session-0").is_none());
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Project);
    click(&mut cx, "project-record-sessions");
    click(&mut cx, "project-record-0-0");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Conversation);
        assert!(!shell.workspace.sessions[&(0, 0)].archived);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Preserved draft"
        );
    });
}

#[gpui::test]
fn terminal_lifecycle_is_scoped(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "terminal-0");
    dispatch(&shell, &mut cx, Target::Terminal((0, 0)), Command::Reset);
    crate::prompts::tests::answer(&mut cx, "workspace_confirm");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.terminals[&(0, 0)].resets),
        1
    );
    dispatch(&shell, &mut cx, Target::Terminal((0, 0)), Command::Close);
    crate::prompts::tests::answer(&mut cx, "workspace_confirm");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(!shell.workspace.terminals.contains_key(&(0, 0)));
        assert!(shell.workspace.terminals.contains_key(&(1, 0)));
        assert!(shell.workspace.sessions.contains_key(&(0, 0)));
        assert_eq!(shell.page, Page::Project);
    });
}
