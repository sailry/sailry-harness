use super::workspace::{click, dispatch};
use super::*;
use crate::workspace::{Command, Owner, Target};

#[gpui::test]
fn project_entry_cancel_and_launch(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "sidebar-project");
    let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Open);
    click(&mut cx, "project-cli");
    assert!(cx.debug_bounds("cli-profile-codex").is_some());
    assert!(cx.debug_bounds("cli-profile-opencode").is_some());
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    click(&mut cx, "cli-profile-opencode");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.terminals.len()),
        2
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.terminals.len()),
        2
    );
    let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Open);
    click(&mut cx, "project-cli");
    click(&mut cx, "cli-profile-claude");
    assert!(cx.debug_bounds("cli-launcher").is_none());
    assert!(cx.debug_bounds("terminal-cli-profile").is_some());
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Terminal);
        let terminal = &shell.workspace.terminals[&(0, 1)];
        assert_eq!(terminal.profile.as_ref().unwrap().id, "claude");
        assert_eq!(terminal.owner, shell.workspace.owner(0));
    });
}

#[gpui::test]
fn binds_original_target(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = Owner {
        host: 1,
        project: 1,
        worktree: 3,
    };
    dispatch(&shell, &mut cx, Target::Project(owner), Command::LaunchCli);
    assert!(cx.debug_bounds("cli-profile-codex").is_none());
    assert!(cx.debug_bounds("cli-profile-kimi").is_some());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_project(0, window, cx);
            shell.conversations[&(0, 0)]
                .input
                .update(cx, |input, cx| input.set_value("Kept draft", window, cx));
        })
    });
    click(&mut cx, "cli-profile-kimi");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.host, 1);
        assert_eq!(shell.workspace.terminals[&(1, 1)].owner, owner);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Kept draft"
        );
        assert!(shell.workspace.terminals[&(0, 0)].profile.is_none());
    });
}

#[gpui::test]
fn rejects_stale_profiles(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for cause in 0..3 {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.workspace = crate::workspace::State::default();
                shell.cli_launcher(shell.workspace.owner(0), window, cx);
            })
        });
        cx.update(|_, cx| {
            shell.update(cx, |shell, _| match cause {
                0 => shell.workspace.cli_profiles.get_mut(&0).unwrap()[0].available = false,
                1 => {
                    shell.workspace.worktrees.get_mut(&0).unwrap().path = "/preview/changed".into()
                }
                _ => shell.workspace.projects.get_mut(&0).unwrap().trusted = false,
            })
        });
        click(&mut cx, "cli-profile-codex");
        crate::feedback::tests::shown(&mut cx);
        assert!(cx.debug_bounds("cli-error").is_none());
        assert_eq!(
            cx.update(|_, cx| shell.read(cx).workspace.terminals.len()),
            2
        );
        click(&mut cx, "cli-cancel");
    }
}

#[gpui::test]
fn trust_and_launch_snapshot(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.workspace.projects.get_mut(&0).unwrap().trusted = false;
            shell.cli_launcher(shell.workspace.owner(0), window, cx);
        })
    });
    assert!(cx.debug_bounds("cli-launcher").is_none());
    crate::prompts::tests::answer(&mut cx, "settings_cancel");
    click(&mut cx, "sidebar-project");
    let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Open);
    click(&mut cx, "project-cli");
    click(&mut cx, "cli-profile-codex");
    crate::prompts::tests::answer(&mut cx, "project_grant");
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.workspace.cli_profiles.get_mut(&0).unwrap()[0].name = "Changed catalog".into();
            shell.workspace.projects.get_mut(&0).unwrap().trusted = false;
        })
    });
    dispatch(&shell, &mut cx, Target::Terminal((0, 1)), Command::Reset);
    crate::prompts::tests::answer(&mut cx, "settings_cancel");
    cx.update(|_, cx| {
        let terminal = &shell.read(cx).workspace.terminals[&(0, 1)];
        assert_eq!(
            terminal.profile.as_ref().unwrap().name,
            crate::tr("cli_codex")
        );
        assert_eq!(terminal.resets, 0);
    });
}

#[gpui::test]
fn profile_list_bounds(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let handle = cx.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            window.window_handle()
        });
        cx.simulate_window_resize(handle, size(px(760.), px(560.)));
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                for profile in shell.workspace.cli_profiles.get_mut(&0).unwrap() {
                    profile.available = true;
                    profile.name = "Long profile name ".repeat(20).into();
                }
                shell.cli_launcher(shell.workspace.owner(0), window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let launcher = cx.debug_bounds("cli-launcher").unwrap();
        let cancel = cx.debug_bounds("cli-cancel").unwrap();
        let row = cx.debug_bounds("cli-profile-codex").unwrap();
        assert!(cancel.bottom() <= px(560.));
        assert!(row.left() >= launcher.left() && row.right() <= launcher.right());
        cx.simulate_event(ScrollWheelEvent {
            position: row.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-800.))),
            ..Default::default()
        });
        cx.run_until_parked();
        click(&mut cx, "cli-profile-kimi");
        assert!(cx.debug_bounds("terminal-cli-profile").is_some());
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                for profile in shell.workspace.cli_profiles.get_mut(&0).unwrap() {
                    profile.available = false;
                }
                shell.cli_launcher(shell.workspace.owner(0), window, cx);
            })
        });
        click(&mut cx, "cli-cancel");
        assert!(cx.debug_bounds("cli-launcher").is_none());
    }
}
