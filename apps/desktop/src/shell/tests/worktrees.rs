use super::workspace::{click, dispatch};
use super::*;
use crate::workspace::{Command, Owner, Target};

fn fill(cx: &mut VisualTestContext, selector: &'static str, value: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    cx.run_until_parked();
}

#[gpui::test]
fn preserves_existing_resources(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 0)]
            .input
            .clone()
            .update(cx, |input, cx| {
                input.set_value("Keep this draft", window, cx)
            });
    });
    let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Open);
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Worktrees);
    click(&mut cx, "worktree-row-2");
    let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Worktrees);
    click(&mut cx, "worktree-new");
    fill(&mut cx, "worktree_branch-input", "feature/ui");
    fill(&mut cx, "worktree_path-input", "/preview/new-ui");
    click(&mut cx, "worktree-submit");
    let owner = cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        let shell = shell.read(cx);
        let owner = shell.workspace.owner(0);
        assert_eq!(
            shell.workspace.worktrees[&owner.worktree]
                .base_ref
                .as_deref(),
            Some(tr("composer_branch_preview").as_ref())
        );
        assert_eq!(shell.workspace.sessions[&(0, 0)].owner.worktree, 0);
        assert_eq!(shell.workspace.terminals[&(0, 0)].owner.worktree, 0);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Keep this draft"
        );
        owner
    });
    click(&mut cx, "project-new-terminal");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.workspace.terminals[&shell.workspace.terminal.unwrap()].owner,
            owner
        );
    });
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Open);
    click(&mut cx, "project-new-session");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.workspace.sessions[&(0, shell.session)].owner, owner);
        assert_eq!(
            shell.conversations[&(0, shell.session)].options.choices[1],
            2
        );
    });
    click(&mut cx, "session-0");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.owner(0).worktree),
        0
    );
}

#[gpui::test]
fn validates_bound_target(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let remote = Owner {
        host: 1,
        project: 1,
        worktree: 1,
    };
    dispatch(
        &shell,
        &mut cx,
        Target::Project(remote),
        Command::NewWorktree,
    );
    click(&mut cx, "worktree-submit");
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.debug_bounds("worktree-error").is_none());
    fill(&mut cx, "worktree_branch-input", "main");
    fill(&mut cx, "worktree_path-input", "/preview/new");
    click(&mut cx, "worktree-submit");
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.debug_bounds("worktree-error").is_none());
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.worktrees.len()),
        4
    );
    click(&mut cx, "worktree-cancel");
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(cx.update(|_, cx| shell.read(cx).host), 0);
    dispatch(
        &shell,
        &mut cx,
        Target::Project(remote),
        Command::NewWorktree,
    );
    fill(&mut cx, "worktree_branch-input", "feature/remote");
    fill(&mut cx, "worktree_path-input", "/preview/new");
    click(&mut cx, "worktree-submit");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.host, 1);
        assert_eq!(shell.workspace.owner(1).project, 1);
        assert_eq!(shell.workspace.owner(0).worktree, 0);
        assert_eq!(
            shell.workspace.worktrees[&shell.workspace.owner(1).worktree].branch,
            "feature/remote"
        );
    });
}

#[gpui::test]
fn form_geometry_and_escape(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(560.)));
            let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
            dispatch(
                &shell,
                &mut cx,
                Target::Project(owner),
                Command::NewWorktree,
            );
            click(&mut cx, "worktree-submit");
            let form = cx.debug_bounds("worktree-form").unwrap();
            let footer = cx.debug_bounds("worktree-submit").unwrap();
            assert!(form.left() >= px(0.) && form.right() < px(width));
            assert!(form.bottom() <= footer.top());
            assert!(footer.bottom() < px(560.));
            for name in [
                "worktree_base-input",
                "worktree_branch-input",
                "worktree_path-input",
            ] {
                let field = cx.debug_bounds(name).unwrap();
                assert!(field.left() >= form.left() && field.right() <= form.right());
            }
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        }
    }
}

#[gpui::test]
fn long_branch_stays_inside_composer(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    cx.simulate_window_resize(handle, size(px(760.), px(820.)));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let owner = shell
                .workspace
                .add_worktree(0, "main", &"feature/long-".repeat(20), "/preview/long")
                .unwrap();
            shell.select_owner(owner, window, cx);
            shell.navigate(Page::Project, window, cx);
        });
    });
    let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Worktrees);
    let row = cx.debug_bounds("worktree-row-4").unwrap();
    let check = cx.debug_bounds("worktree-check-4").unwrap();
    assert_eq!(check.size.width, px(20.));
    assert!(check.right() <= row.right() - px(12.));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    click(&mut cx, "project-new-session");
    let branch = cx.debug_bounds("composer-branch").unwrap();
    let context = cx.debug_bounds("composer-context-bar").unwrap();
    assert!(branch.size.width <= px(224.));
    assert!(branch.right() <= context.right());
}

#[gpui::test]
fn enter_validates_before_closing(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    dispatch(
        &shell,
        &mut cx,
        Target::Project(owner),
        Command::NewWorktree,
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    fill(&mut cx, "worktree_branch-input", "feature/keyboard");
    fill(&mut cx, "worktree_path-input", "/preview/keyboard");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.worktrees.len()),
        5
    );
}
