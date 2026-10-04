use super::workspace::click;
use super::*;

fn fill(cx: &mut VisualTestContext, selector: &'static str, value: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    cx.run_until_parked();
}

fn go(cx: &mut VisualTestContext, path: &str) {
    fill(cx, "directory-address", path);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

#[gpui::test]
fn selection_updates_form(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "project-add");
    fill(&mut cx, "project_name-input", "Selected directory");
    click(&mut cx, "project-browse");
    assert!(cx.debug_bounds("directory-picker").is_some());
    click(&mut cx, "directory-row-0");
    assert!(cx.debug_bounds("directory-row-1").is_some());
    click(&mut cx, "directory-confirm");
    assert!(cx.debug_bounds("directory-picker").is_none());
    assert!(cx.debug_bounds("project-editor").is_some());
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects.len()),
        2
    );
    click(&mut cx, "project-save");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let project = &shell.workspace.projects[&2];
        assert_eq!(project.path, "/preview/empty");
        assert_eq!(project.name, "Selected directory");
        assert!(!project.trusted);
    });
}

#[gpui::test]
fn cancellation_and_invalid_path(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "project-add");
    fill(&mut cx, "project_name-input", "Retained");
    fill(&mut cx, "project_path-input", "/preview/manual");
    click(&mut cx, "project-browse");
    assert!(cx.debug_bounds("directory-error").is_some());
    click(&mut cx, "directory-confirm");
    assert!(cx.debug_bounds("directory-picker").is_some());
    click(&mut cx, "directory_home");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    click(&mut cx, "project-browse");
    assert!(cx.debug_bounds("directory-error").is_some());
    click(&mut cx, "directory-cancel");
    click(&mut cx, "project-save");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects[&2].path.clone()),
        "/preview/manual"
    );
}

#[gpui::test]
fn keyboard_navigation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "project-add");
    fill(&mut cx, "project_name-input", "Keyboard directory");
    click(&mut cx, "project-browse");
    click(&mut cx, "directory_home");
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("directory-empty").is_some());
    assert!(cx.debug_bounds("directory-picker").is_some());
    click(&mut cx, "directory_back");
    assert!(cx.debug_bounds("directory-row-1").is_some());
    click(&mut cx, "directory_forward");
    assert!(cx.debug_bounds("directory-empty").is_some());
    click(&mut cx, "directory-up");
    go(&mut cx, "/preview/restricted");
    assert!(cx.debug_bounds("directory-error").is_some());
    click(&mut cx, "directory-confirm");
    assert!(cx.debug_bounds("directory-picker").is_some());
    go(&mut cx, "/not-in-preview");
    assert!(cx.debug_bounds("directory-error").is_some());
    go(&mut cx, "/preview/empty");
    assert!(cx.debug_bounds("directory-error").is_none());
    click(&mut cx, "directory-confirm");
    click(&mut cx, "project-save");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects[&2].path.clone()),
        "/preview/empty"
    );
}

#[gpui::test]
fn protects_bound_projects(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.project_editor(1, Some(1), window, cx))
    });
    click(&mut cx, "project-browse");
    go(&mut cx, "/preview/sailry");
    assert!(cx.debug_bounds("directory-error").is_some());
    go(&mut cx, "/preview/remote-scratch");
    click(&mut cx, "directory-confirm");
    let previous = cx.update(|window, cx| window.notifications(cx).len());
    click(&mut cx, "project-save");
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.debug_bounds("project-editor").is_some());
    cx.update(|window, cx| {
        assert_eq!(window.notifications(cx).len(), previous + 1);
        assert_eq!(
            crate::feedback::tests::summary(window, cx),
            tr("project_path_trusted")
        );
    });
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.host, 0);
        assert_eq!(
            shell.workspace.projects[&1].path,
            "/preview/remote-workspace"
        );
        assert!(shell.workspace.projects[&1].trusted);
    });
    click(&mut cx, "project-cancel");
}

#[gpui::test]
fn long_list_and_small_window(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let handle = cx.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            shell.update(cx, |shell, cx| shell.project_editor(0, None, window, cx));
            window.window_handle()
        });
        cx.simulate_window_resize(handle, size(px(760.), px(560.)));
        fill(&mut cx, "project_name-input", "Long selection");
        click(&mut cx, "project-browse");
        go(&mut cx, "/preview/many");
        for _ in 0..64 {
            cx.simulate_keystrokes("down");
        }
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let list = cx.debug_bounds("directory-list").unwrap();
        let tail = cx.debug_bounds("directory-row-63").unwrap();
        let button = cx.debug_bounds("directory-confirm").unwrap();
        assert!(tail.top() >= list.top() && tail.bottom() <= list.bottom());
        assert!(button.bottom() < px(560.));
        assert!(cx.debug_bounds("directory-picker").unwrap().bottom() <= button.top());
        click(&mut cx, "directory-confirm");
        click(&mut cx, "project-cancel");
    }
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects.len()),
        2
    );
}

#[gpui::test]
fn denied_selection_recovery(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "project-add");
    fill(&mut cx, "project_name-input", "Recovered selection");
    click(&mut cx, "project-browse");
    click(&mut cx, "directory-row-2");
    click(&mut cx, "directory-confirm");
    assert!(cx.debug_bounds("directory-error").is_some());
    click(&mut cx, "directory-row-0");
    assert!(cx.debug_bounds("directory-error").is_none());
    let position = cx.debug_bounds("directory-row-0").unwrap().center();
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        click_count: 2,
        ..Default::default()
    });
    cx.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        click_count: 2,
        ..Default::default()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("directory-empty").is_some());
    click(&mut cx, "directory-confirm");
    click(&mut cx, "project-save");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects[&2].path.clone()),
        "/preview/empty"
    );
}
