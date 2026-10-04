use super::workspace::{click, dispatch};
use super::*;
use crate::workspace::{Command, Owner, Repository, Target};

fn directory(shell: &Entity<Shell>, cx: &mut VisualTestContext, host: usize) -> Owner {
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let id = shell
                .workspace
                .save_project(host, None, "Directory", "/preview/directory")
                .unwrap();
            shell.workspace.project_owner(id).unwrap()
        })
    })
}

#[gpui::test]
fn initialization_lifecycle(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = directory(&shell, &mut cx, 0);
    let terminal = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_project(owner.project, window, cx);
            shell.conversations[&(0, 0)]
                .input
                .update(cx, |input, cx| input.set_value("Keep draft", window, cx));
            shell.workspace.create_terminal(owner)
        })
    });
    click(&mut cx, "project-initialize");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).repository()),
        Repository::Directory
    );
    crate::prompts::tests::answer(&mut cx, "settings_cancel");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).repository()),
        Repository::Directory
    );
    click(&mut cx, "project-initialize");
    crate::prompts::tests::answer(&mut cx, "git_initialize_preview");
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Project);
        assert_eq!(shell.repository(), Repository::Unborn);
        assert!(!shell.workspace.projects[&owner.project].trusted);
        assert_eq!(shell.workspace.terminals[&terminal].owner, owner);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Keep draft"
        );
    });
    dispatch(&shell, &mut cx, Target::Project(owner), Command::Worktrees);
    click(&mut cx, "worktree-new");
    assert!(cx.debug_bounds("worktree-list").is_some());
    assert!(cx.debug_bounds("worktree-form").is_none());
}

#[gpui::test]
fn independent_git_state(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = directory(&shell, &mut cx, 0);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_owner(owner, window, cx);
            shell.navigate(Page::Git, window, cx);
        })
    });
    click(&mut cx, "git-initialize");
    assert!(cx.debug_bounds("document-tab-0").is_none());
    assert!(cx.debug_bounds("resource-change-0").is_none());
    assert!(cx.debug_bounds("git-commit-controls").is_none());
    crate::prompts::tests::answer(&mut cx, "git_initialize_preview");
    assert!(cx.debug_bounds("git-empty").is_some());
    assert!(cx.debug_bounds("document-tab-0").is_none());
    assert!(cx.debug_bounds("resource-change-0").is_none());
    assert!(cx.debug_bounds("git-commit-controls").is_some());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_project(0, window, cx);
            shell.navigate(Page::Git, window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("document-tab-0").is_some());
    assert!(cx.debug_bounds("resource-change-0").is_some());
}

#[gpui::test]
fn embedded_git_lifecycle(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = directory(&shell, &mut cx, 0);
    let (key, input) = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let key = shell.workspace.create_session(owner);
            shell.select_session(key, window, cx);
            let input = shell.conversations[&key].input.clone();
            input.update(cx, |input, cx| {
                input.set_value("Keep directory draft", window, cx)
            });
            (key, input)
        })
    });
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_resource_panel(Page::Git, window, cx)
        })
    });
    cx.run_until_parked();
    click(&mut cx, "git-initialize");
    crate::prompts::tests::answer(&mut cx, "git_initialize_preview");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(shell.workspace.sessions[&key].owner, owner);
        assert_eq!(input.read(cx).value(), "Keep directory draft");
        assert!(matches!(
            shell.side_resource,
            Some(crate::resources::SideResource::PreviewGit(_))
        ));
        assert!(shell.git_state(true).tabs.open.is_empty());
    });
    click(&mut cx, "toggle-details");
    assert!(cx.update(|_, cx| shell.read(cx).side_resource.is_none()));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_resource_panel(Page::Git, window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("git-empty").is_some());
    assert!(cx.debug_bounds("document-tab-0").is_none());
}

#[gpui::test]
fn revalidates_bound_path(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = directory(&shell, &mut cx, 1);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.initialize_git(owner.project, window, cx)
        })
    });
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell
                .workspace
                .projects
                .get_mut(&owner.project)
                .unwrap()
                .path = "/preview/replaced".into();
        })
    });
    crate::prompts::tests::answer(&mut cx, "git_initialize_preview");
    crate::feedback::tests::shown(&mut cx);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.projects[&owner.project].repository),
        Repository::Directory
    );
    assert!(!cx.has_pending_prompt());
    cx.update(|window, cx| window.clear_notifications(cx));
    let message = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.initialize_git(owner.project, window, cx)
        });
        shell.read(cx).git.message.clone()
    });
    crate::prompts::tests::answer(&mut cx, "git_initialize_preview");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.workspace.projects[&owner.project].repository,
            Repository::Unborn
        );
        assert_eq!(shell.host, 0);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(shell.repository(), Repository::Ready);
        assert_eq!(shell.git.message, message);
    });
}

#[gpui::test]
fn preserves_commit_draft(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = directory(&shell, &mut cx, 0);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_project(owner.project, window, cx);
            shell.initialize_git(owner.project, window, cx);
            let expected = shell.workspace.projects[&owner.project].clone();
            shell
                .workspace
                .initialize_repository(owner.project, &expected)
                .unwrap();
            shell.git.message.update(cx, |input, cx| {
                input.set_value("First commit draft", window, cx)
            });
        })
    });
    crate::prompts::tests::answer(&mut cx, "git_initialize_preview");
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert_eq!(
            shell.read(cx).git.message.read(cx).value(),
            "First commit draft"
        );
    });
}

#[gpui::test]
fn small_window_bounds(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let owner = directory(&shell, &mut cx, 0);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let project = shell.workspace.projects.get_mut(&owner.project).unwrap();
            project.name = "Directory name ".repeat(50).into();
            project.path = format!("/preview/{}", "long-directory/".repeat(50)).into();
        })
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                shell.update(cx, |shell, cx| {
                    shell.select_owner(owner, window, cx);
                    shell.navigate(Page::Git, window, cx);
                });
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(560.)));
            click(&mut cx, "git-initialize");
            std::thread::sleep(std::time::Duration::from_millis(300));
            cx.update(|window, cx| {
                _ = window.draw(cx);
            });
            let (title, detail) = crate::prompts::tests::wait(&mut cx);
            assert_eq!(title, tr("git_initialize").as_ref());
            assert!(detail.contains("/preview/"));
            crate::prompts::tests::answer(&mut cx, "settings_cancel");
            let empty = cx.debug_bounds("git-empty").unwrap();
            let initialize = cx.debug_bounds("git-initialize").unwrap();
            assert!(
                empty.contains(&initialize.origin) && empty.contains(&initialize.bottom_right())
            );
        }
    }
}
