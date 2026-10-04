use super::workspace::{click, dispatch};
use super::*;
use crate::workspace::{Command, Owner, Target};

fn empty(shell: &Entity<Shell>, cx: &mut VisualTestContext, host: usize) -> Owner {
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let id = shell
                .workspace
                .save_project(host, None, "Temporary", "/preview/temporary")
                .unwrap();
            shell.workspace.project_owner(id).unwrap()
        })
    })
}

fn open(shell: &Entity<Shell>, cx: &mut VisualTestContext, owner: Owner) {
    dispatch(shell, cx, Target::Project(owner), Command::RemoveProject);
}

#[gpui::test]
fn confirmation_and_fallback(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 0);
    let message = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_project(owner.project, window, cx);
            shell.files.tabs.open.push(1);
            shell.git.tabs.open.push(1);
            shell.git.message.update(cx, |input, cx| {
                input.set_value("Temporary commit", window, cx)
            });
            shell.conversations[&(0, 0)]
                .input
                .update(cx, |input, cx| input.set_value("Keep draft", window, cx));
            shell.git.message.clone()
        })
    });
    open(&shell, &mut cx, owner);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(owner)));
    crate::prompts::tests::answer(&mut cx, "settings_cancel");
    assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(owner)));
    open(&shell, &mut cx, owner);
    crate::prompts::tests::answer(&mut cx, "project_remove_preview");
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        let shell = shell.read(cx);
        assert!(!shell.workspace.contains(owner));
        assert_eq!(shell.workspace.owner(0).project, 0);
        assert_eq!(shell.page, Page::Project);
        assert_ne!(shell.git.message, message);
        assert_eq!(shell.git.message.read(cx).value(), "");
        assert_eq!(shell.files.tabs.open.len(), 1);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Keep draft"
        );
        assert!(
            shell
                .workspace
                .sessions
                .values()
                .all(|s| s.owner.project != owner.project)
        );
    });
}

#[gpui::test]
fn protects_new_resources(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let protected = cx.update(|_, cx| shell.read(cx).workspace.owner(0));
    open(&shell, &mut cx, protected);
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(protected)));
    assert!(!cx.has_pending_prompt());
    cx.update(|window, cx| window.clear_notifications(cx));
    for terminal in [false, true] {
        let owner = empty(&shell, &mut cx, usize::from(terminal));
        open(&shell, &mut cx, owner);
        cx.update(|_, cx| {
            shell.update(cx, |shell, _| {
                if terminal {
                    shell.workspace.create_terminal(owner);
                } else {
                    let key = shell.workspace.create_session(owner);
                    shell.workspace.sessions.get_mut(&key).unwrap().archived = true;
                }
            })
        });
        crate::prompts::tests::answer(&mut cx, "project_remove_preview");
        crate::feedback::tests::shown(&mut cx);
        assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(owner)));
        assert!(!cx.has_pending_prompt());
        cx.update(|window, cx| window.clear_notifications(cx));
    }
}

#[gpui::test]
fn rejects_stale_targets(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 1);
    open(&shell, &mut cx, owner);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell
                .workspace
                .projects
                .get_mut(&owner.project)
                .unwrap()
                .path = "/preview/changed".into();
        })
    });
    crate::prompts::tests::answer(&mut cx, "project_remove_preview");
    crate::feedback::tests::shown(&mut cx);
    assert!(!cx.has_pending_prompt());
    cx.update(|window, cx| window.clear_notifications(cx));
    open(&shell, &mut cx, owner);
    let message = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_session((0, 1), window, cx);
            shell.git.message.clone()
        })
    });
    crate::prompts::tests::answer(&mut cx, "project_remove_preview");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(!shell.workspace.contains(owner));
        assert_eq!(shell.host, 0);
        assert_eq!(shell.session, 1);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(shell.git.message, message);
    });
}

#[gpui::test]
fn empty_host_navigation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            // Fixture retirement only; production removal must never cascade these resources.
            shell.workspace.sessions.retain(|key, _| key.0 != 1);
            shell.workspace.terminals.retain(|key, _| key.0 != 1);
            shell.select_host(1, window, cx);
            shell.workspace.owner(1)
        })
    });
    open(&shell, &mut cx, owner);
    crate::prompts::tests::answer(&mut cx, "project_remove_preview");
    assert!(cx.debug_bounds("host-no-projects").is_some());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert_eq!(shell.workspace.selected_owner(1), None);
            for page in [Page::Conversation, Page::Project, Page::Terminal] {
                shell.navigate(page, window, cx);
                assert_eq!(shell.page, Page::Host);
                assert!(shell.workspace.sessions.keys().all(|key| key.0 != 1));
            }
            for page in [Page::Files, Page::Git] {
                shell.navigate(page, window, cx);
                assert_eq!(shell.page, page);
                assert!(shell.needs_project());
                assert!(shell.workspace.sessions.keys().all(|key| key.0 != 1));
            }
            shell.navigate(Page::Host, window, cx);
            shell.navigate(Page::Settings, window, cx);
            shell.navigate(Page::Host, window, cx);
            assert_eq!(shell.page, Page::Host);
        })
    });
    click(&mut cx, "new-conversation");
    assert!(cx.debug_bounds("project-editor").is_some());
    click(&mut cx, "project-cancel");
    click(&mut cx, "host-add-project");
    click(&mut cx, "project_name-input");
    cx.simulate_input("Replacement");
    click(&mut cx, "project_path-input");
    cx.simulate_input("/preview/replacement");
    click(&mut cx, "project-save");
    assert!(
        cx.debug_bounds("project-resources-2")
            .is_none_or(|bounds| bounds.size.height == px(0.))
    );
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Project);
        let owner = shell.workspace.owner(1);
        assert!(owner.project > 1);
        assert!(shell.workspace.sessions.keys().all(|key| key.0 != 1));
        assert_eq!(shell.workspace.projects[&owner.project].name, "Replacement");
    });
}

#[gpui::test]
fn long_confirmation_bounds(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = empty(&shell, &mut cx, 0);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let project = shell.workspace.projects.get_mut(&owner.project).unwrap();
            project.name = "Long project name ".repeat(50).into();
            project.path = format!("/preview/{}", "long-directory/".repeat(50)).into();
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
            std::thread::sleep(std::time::Duration::from_millis(300));
            cx.update(|window, cx| {
                _ = window.draw(cx);
            });
            let (title, detail) = crate::prompts::tests::wait(&mut cx);
            assert_eq!(title, tr("project_remove").as_ref());
            assert!(detail.contains("/preview/"));
            crate::prompts::tests::answer(&mut cx, "settings_cancel");
        }
    }
    assert!(cx.update(|_, cx| shell.read(cx).workspace.contains(owner)));
}
