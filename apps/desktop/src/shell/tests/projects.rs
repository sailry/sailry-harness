use super::workspace::{click, dispatch};
use super::*;
use crate::workspace::{Command, Target};

fn fill(cx: &mut VisualTestContext, selector: &'static str, value: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    cx.run_until_parked();
}

#[gpui::test]
fn clone_source_and_appearance(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "project-add");
    click(&mut cx, "project-clone");
    fill(
        &mut cx,
        "project-repository",
        "https://example.test/team/repository.git",
    );
    fill(&mut cx, "project_path-input", "/preview/clones");
    click(&mut cx, "project-appearance");
    click(&mut cx, "project-color-violet");
    click(&mut cx, "project-icon-code");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    click(&mut cx, "project-save");
    cx.update(|_, cx| {
        let project = &shell.read(cx).workspace.projects[&2];
        assert_eq!(project.name, "repository");
        assert_eq!(project.path, "/preview/clones/repository");
        assert_eq!(project.appearance.color, "violet");
        assert_eq!(project.appearance.icon, "code");
        assert_eq!(project.repository, crate::workspace::Repository::Ready);
    });
}

#[gpui::test]
fn source_changes_keep_manual_name(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "project-add");
    fill(&mut cx, "project_name-input", "My workspace");
    fill(&mut cx, "project_path-input", "/preview/local");
    click(&mut cx, "project-clone");
    fill(
        &mut cx,
        "project-repository",
        "https://example.test/team/repository.git",
    );
    click(&mut cx, "project-local");
    click(&mut cx, "project-appearance");
    click(&mut cx, "project-color-blue");
    click(&mut cx, "project-color-none");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    click(&mut cx, "project-save");
    cx.update(|_, cx| {
        let project = &shell.read(cx).workspace.projects[&2];
        assert_eq!(project.name, "My workspace");
        assert_eq!(project.path, "/preview/local");
        assert_eq!(project.appearance.color, "none");
    });
}

#[gpui::test]
fn creation_confirmation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "project-add");
    fill(&mut cx, "project_name-input", "Second project");
    fill(&mut cx, "project_path-input", "/preview/second");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let owner = cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Project);
        assert_eq!(shell.workspace.projects.len(), 3);
        shell.workspace.owner(0)
    });
    click(&mut cx, "project-new-session");
    assert!(cx.has_pending_prompt());
    crate::prompts::tests::answer(&mut cx, "settings_cancel");
    assert!(!cx.update(|_, cx| shell.read(cx).workspace.projects[&owner.project].trusted));
    click(&mut cx, "project-new-session");
    crate::prompts::tests::answer(&mut cx, "project_grant");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(shell.workspace.projects[&owner.project].trusted);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(shell.workspace.sessions[&(0, shell.session)].owner, owner);
        assert_eq!(
            shell
                .workspace
                .sessions
                .values()
                .filter(|session| session.owner.project == owner.project)
                .count(),
            1
        );
    });
}

#[gpui::test]
fn failed_edit_preserves_metadata(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = cx.update(|_, cx| Target::Project(shell.read(cx).workspace.owner(0)));
    dispatch(&shell, &mut cx, target, Command::EditProject);
    assert!(cx.debug_bounds("project-editor").is_some());
    fill(&mut cx, "project_name-input", "Renamed project");
    fill(&mut cx, "project_path-input", "/preview/moved");
    click(&mut cx, "project-save");
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        assert_eq!(window.notifications(cx).len(), 1);
    });
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects[&0].name.clone()),
        tr("project")
    );
    fill(&mut cx, "project_path-input", "/preview/sailry");
    click(&mut cx, "project-save");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects[&0].name.clone()),
        "Renamed project"
    );
}

#[gpui::test]
fn revocation_preserves_other_work(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            for key in [(0, 0), (0, 2), (1, 2)] {
                shell.select_host(key.0, window, cx);
                shell.session = key.1;
                shell.navigate(Page::Conversation, window, cx);
                let input = shell.conversations[&key].input.clone();
                input.update(cx, |input, cx| {
                    input.set_value("Preview prompt", window, cx)
                });
                shell.send_preview(key, window, cx);
                input.update(cx, |input, cx| {
                    input.set_value("Keep next draft", window, cx)
                });
            }
        })
    });
    let target = cx.update(|_, cx| Target::Project(shell.read(cx).workspace.owner(0)));
    dispatch(&shell, &mut cx, target, Command::Trust);
    crate::prompts::tests::answer(&mut cx, "project_revoke");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.host, 1);
        assert!(!shell.workspace.projects[&0].trusted);
        assert!(shell.workspace.projects[&1].trusted);
        for key in [(0, 0), (0, 2)] {
            let conversation = &shell.conversations[&key];
            assert_eq!(
                conversation.turns.last().unwrap().status,
                crate::conversation::turn::Status::Cancelled
            );
            assert!(conversation.preview_task.is_none());
            assert_eq!(conversation.input.read(cx).value(), "Keep next draft");
        }
        assert!(
            shell.conversations[&(1, 2)]
                .turns
                .last()
                .unwrap()
                .status
                .active()
        );
    });
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_session((0, 2), window, cx)));
    click(&mut cx, "composer-send");
    crate::prompts::tests::answer(&mut cx, "settings_cancel");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns.len()),
        1
    );
    click(&mut cx, "composer-send");
    crate::prompts::tests::answer(&mut cx, "project_grant");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns.len()),
        2
    );
}

#[gpui::test]
fn trust_binds_original_path(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = cx.update(|_, cx| Target::Project(shell.read(cx).workspace.owner(0)));
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.workspace.projects.get_mut(&0).unwrap().trusted = false
        })
    });
    dispatch(&shell, &mut cx, target, Command::Trust);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.workspace.projects.get_mut(&0).unwrap().path = "/preview/replaced".into()
        })
    });
    crate::prompts::tests::answer(&mut cx, "project_grant");
    assert!(!cx.update(|_, cx| shell.read(cx).workspace.projects[&0].trusted));
}

#[gpui::test]
fn project_forms_fit_and_cancel(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(560.)));
            click(&mut cx, "project-add");
            click(&mut cx, "project-save");
            let form = cx.debug_bounds("project-editor").unwrap();
            let footer = cx.debug_bounds("project-save").unwrap();
            assert!(form.right() <= px(width));
            assert!(form.bottom() < footer.top());
            assert!(footer.bottom() < px(560.));
            click(&mut cx, "project-cancel");
            assert_eq!(
                cx.update(|_, cx| shell.read(cx).workspace.projects.len()),
                2
            );
            let target = cx.update(|_, cx| Target::Project(shell.read(cx).workspace.owner(0)));
            dispatch(&shell, &mut cx, target, Command::Trust);
            let (title, detail) = crate::prompts::tests::wait(&mut cx);
            assert_eq!(title, tr("project_revoke_title").as_ref());
            assert!(!detail.is_empty());
            cx.simulate_keystrokes("enter");
            assert!(cx.update(|_, cx| shell.read(cx).workspace.projects[&0].trusted));
            crate::prompts::tests::answer(&mut cx, "settings_cancel");
        }
    }
}

#[gpui::test]
fn nested_sidebar_width(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| Theme::change(mode, Some(window), cx));
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let project = cx.debug_bounds("sidebar-project").unwrap();
        let session = cx.debug_bounds("session-0").unwrap();
        assert_eq!(project.left(), session.left());
        assert_eq!(project.right(), session.right());
        let guide = cx.debug_bounds("session-0-guide").unwrap();
        assert!(
            guide.left() > session.left() && guide.right() < session.left() + px(32.),
            "guide: {guide:?}, row: {session:?}"
        );
        assert_eq!(guide.top(), session.top());
        assert_eq!(guide.bottom(), session.bottom());
        cx.simulate_mouse_move(project.center(), None, Modifiers::default());
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(cx.update(
            |_, cx| shell.read(cx).sidebar.hovered == Some(crate::sidebar::Row::Project(0))
        ));
        assert!(cx.debug_bounds("project-more-0").is_none());
        let disclosure = cx.debug_bounds("project-disclosure-0").unwrap();
        assert!(disclosure.left() > project.center().x && disclosure.right() <= project.right());
        cx.simulate_mouse_move(session.center(), None, Modifiers::default());
        cx.run_until_parked();
        assert!(cx.update(
            |_, cx| shell.read(cx).sidebar.hovered == Some(crate::sidebar::Row::Session(0, 0))
        ));
    }
}
