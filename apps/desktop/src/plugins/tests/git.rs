//! The packaged controller is exercised against the same local and Link Git service.
use super::*;
use crate::{preview::Page, resources::SideResource, shell::Shell};
use sailry_protocol::{Output, Request, plugin::Info};
use std::path::Path;

mod composer;

fn package(fixture: &Fixture) -> Info {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin { name: "git".into() }) else {
        panic!("Git package expected")
    };
    info
}
fn prepare(fixture: &Fixture) -> String {
    let root = fixture.directory.path().join("project");
    let repository = git2::Repository::open(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Fixture").unwrap();
    config
        .set_str("user.email", "fixture@example.invalid")
        .unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    std::fs::write(root.join("notes.txt"), "base\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("notes.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let head = repository
        .commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
        .unwrap();
    repository
        .branch("feature", &repository.find_commit(head).unwrap(), false)
        .unwrap();
    std::fs::write(root.join("notes.txt"), "working\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/git");
    fixture::copy_package(&source, &root.join("git-package"));
    let view = root.join("git-package/dev.sailry.platform/desktop/view.js");
    let script = std::fs::read_to_string(&view)
        .unwrap()
        .replace(
            "div().id('git-commit-controls')",
            "div().id('git-commit-controls').relative().child(Bounds.new('git-commit-controls'))",
        )
        .replace(
            "div().id('git-changes')",
            "div().id('git-changes').relative().child(Bounds.new('git-changes'))",
        )
        .replace(
            "div().id(`${id}-binary`)",
            "div().id(`${id}-binary`).relative().child(Bounds.new(`${id}-binary`))",
        )
        .replace(
            "new Button('git-open-changes')",
            "new Button('git-open-changes').relative().child(Bounds.new('git-open-changes'))",
        );
    std::fs::write(
        view,
        format!("import {{Bounds}} from 'sailry/test';\n{script}"),
    )
    .unwrap();
    std::fs::write(root.join(".git/info/exclude"), "git-package/\n").unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "git-package".into(),
        name: "git".into(),
        expected_revision: package(fixture).summary.revision,
    }) else {
        panic!("Git package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    assert!(info.summary.enabled);
    head.to_string()
}
fn main(
    shell: &Entity<Shell>,
    fixture: &Fixture,
    visual: &mut VisualTestContext,
    selector: &str,
) -> Entity<Panel> {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live_resource_action(
                &crate::live::menus::Dispatch {
                    node: fixture.node.id(),
                    target: crate::live::menus::Target::Project(fixture.binding.project.unwrap()),
                    command: crate::live::menus::Command::Open,
                },
                window,
                cx,
            );
        })
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .extension_entries(cx)
            .iter()
            .any(|entry| entry.package.name == "git")
    });
    let entry_selector = shell.read_with(visual, |shell, cx| {
        shell
            .extension_entries(cx)
            .into_iter()
            .find(|entry| entry.package.name == "git")
            .unwrap()
            .selector()
    });
    click(visual, Box::leak(entry_selector.into_boxed_str()));
    let panel = shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    });
    ready(&panel, visual, selector);
    assert_eq!(
        shell.read_with(visual, |shell, cx| shell
            .plugin_workspace(cx)
            .unwrap()
            .read(cx)
            .width),
        px(320.),
    );
    let actions = visual.debug_bounds("git-actions").unwrap();
    let refresh = visual.debug_bounds("git-refresh").unwrap();
    assert_eq!(actions.right(), refresh.left());
    assert!(visual.debug_bounds("shell-module-header").is_some());
    assert!(visual.debug_bounds("git-tabs-header").is_none());
    assert!(visual.debug_bounds("shell-navigation").is_none());
    assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
    assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
    panel
}
#[track_caller]
fn ready(panel: &Entity<Panel>, visual: &mut VisualTestContext, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let selector: &'static str = Box::leak(text.to_owned().into_boxed_str());
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let (connected, loading) = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let panel = panel.read(cx);
            (panel.connected, panel.loading)
        });
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Git renderer deadline for {text}: connected={connected}, loading={loading}, viewport={:?}, row={:?}",
            visual.debug_bounds("diff-viewport"),
            visual.debug_bounds("diff-line-0")
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn writes(fixture: &Fixture) -> Vec<Request> {
    fixture
        .transport
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| {
            matches!(
                request.command,
                Command::UpdateGitIndex { .. } | Command::CreateGitCommit { .. }
            )
        })
        .cloned()
        .collect()
}

#[gpui::test]
fn closing_tabs_focuses_the_next_surface(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual, "git-tracked-notes.txt");
        click(visual, "git-tracked-notes.txt");
        ready(&panel, visual, "git-diff-notes.txt");
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("document-path")
        }));
        // Diff's actual selectable rows own focus, not the tab strip.
        ready(&panel, visual, "diff-line-0");
        assert!(visual.debug_bounds("diff-viewport").unwrap().size.height > px(100.));
        click(visual, "diff-line-0");
        let first = visual.update(|window, cx| window.focused(cx).unwrap());
        click(visual, "git-untracked-untracked.txt");
        ready(&panel, visual, "git-diff-untracked.txt");
        ready(&panel, visual, "diff-line-0");
        click(visual, "diff-line-0");
        let second = visual.update(|window, cx| window.focused(cx).unwrap());
        assert_ne!(first, second);
        // Kit's Up from an unselected menu selects its last enabled item: Output.
        click(visual, "git-actions");
        visual.simulate_keystrokes("up enter");
        ready(&panel, visual, "git-output-output:");
        assert!(!panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("document-path")
        }));
        click(visual, "git-output-output:");
        let output = visual.update(|window, cx| window.focused(cx).unwrap());
        assert_ne!(output, second);
        visual.simulate_keystrokes("secondary-w");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("git-output-output:")
        });
        surface_focused(visual, &second);
        visual.simulate_keystrokes("secondary-w");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("git-diff-untracked.txt")
        });
        surface_focused(visual, &first);
        visual.simulate_keystrokes("secondary-w");
        wait(visual, |cx| snapshot(&panel, cx).contains("git_diff_empty"));
        assert!(!panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("document-path")
        }));
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.client.target()),
            fixture.node.id()
        );
        assert!(writes(&fixture).is_empty());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn preserves_binary_entries_and_native_disclosure(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        let root = fixture.directory.path().join("project");
        let repository = git2::Repository::open(&root).unwrap();
        std::fs::write(root.join("payload.bin"), [0, 1, 2]).unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("payload.bin")).unwrap();
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let parent = repository.head().unwrap().peel_to_commit().unwrap();
        let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repository
            .commit(Some("HEAD"), &author, &author, "Binary", &tree, &[&parent])
            .unwrap();
        std::fs::write(root.join("payload.bin"), [0, 3, 4]).unwrap();

        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual, "git-tracked-payload.bin");
        click(visual, "git-tracked-payload.bin");
        ready(&panel, visual, "git-diff-payload.bin-binary");
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("Cannot display binary diff")));
        assert!(visual.debug_bounds("diff-viewport").is_none());

        click(visual, "git-open-changes");
        ready(&panel, visual, "git-diff-changes:/payload.bin-binary");
        let header = visual
            .debug_bounds("git-diff-heading-changes:/payload.bin")
            .unwrap();
        let body = visual.debug_bounds("git-diff-body-payload.bin").unwrap();
        assert!(header.bottom() <= body.top());
        assert!(visual.debug_bounds("git-diff-changes:/notes.txt").is_some());
        click(visual, "diff-file-changes:/payload.bin");
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("git-diff-body-payload.bin").is_none());
        assert!(
            visual
                .debug_bounds("git-diff-heading-changes:/payload.bin")
                .is_some()
        );
        assert!(visual.debug_bounds("git-diff-body-notes.txt").is_some());
        click(visual, "diff-file-changes:/payload.bin");
        ready(&panel, visual, "git-diff-changes:/payload.bin-binary");
        click(visual, "diff-file-changes:/notes.txt");
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("git-diff-body-notes.txt").is_none());
        assert!(visual.debug_bounds("git-diff-body-payload.bin").is_some());
        assert!(writes(&fixture).is_empty());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn empty_navigation_fills_height(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        let root = fixture.directory.path().join("project");
        let repository = git2::Repository::open(&root).unwrap();
        repository.set_head("refs/heads/unborn").unwrap();
        std::fs::remove_file(root.join("notes.txt")).unwrap();
        std::fs::remove_file(root.join("untracked.txt")).unwrap();
        let mut index = repository.index().unwrap();
        index.clear().unwrap();
        index.write().unwrap();
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual, "empty-git_no_changes");
        let empty = visual.debug_bounds("empty-git_no_changes").unwrap();
        let header = visual.debug_bounds("git-changes-header").unwrap();
        let footer = visual.debug_bounds("git-commit-controls").unwrap();
        assert_eq!(empty.size.width, header.size.width);
        assert!(empty.top() > header.bottom());
        // The footer bounds start inside its one-pixel top border.
        assert_eq!(empty.bottom() + px(1.), footer.top());
        assert!(visual.debug_bounds("empty-card-git_no_changes").is_none());
        click(visual, "git-views-item-history");
        ready(&panel, visual, "empty-git_history_empty");
        let empty = visual.debug_bounds("empty-git_history_empty").unwrap();
        let navigation = visual.debug_bounds("git-changes").unwrap();
        assert_eq!(empty.size.width, navigation.size.width);
        assert!((empty.bottom() - navigation.bottom()).abs() < px(1.));
        assert!(
            visual
                .debug_bounds("empty-card-git_history_empty")
                .is_none()
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn initializes_from_main_content(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::directory(remote);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual, "git-initialize");
        let main_icon = visual
            .debug_bounds("empty-icon-git_directory_title")
            .unwrap();
        let sidebar_icon = visual
            .debug_bounds("empty-icon-git_directory_title-sidebar")
            .unwrap();
        assert_eq!(main_icon.size, size(px(48.), px(48.)));
        assert_eq!(main_icon.size, sidebar_icon.size);
        let action = visual.debug_bounds("git-initialize").unwrap();
        assert!(action.size.width >= px(280.));
        assert!(action.top() > main_icon.bottom());
        assert!((action.center().x - main_icon.center().x).abs() < px(1.));
        let root = fixture.directory.path().join("project");
        let draft = std::fs::read(root.join("notes.txt")).unwrap();
        assert!(!root.join(".git").exists());
        click(visual, "git-initialize");
        ready(&panel, visual, "git-untracked-notes.txt");
        assert!(visual.debug_bounds("git-initialize").is_none());
        assert_eq!(std::fs::read(root.join("notes.txt")).unwrap(), draft);
        assert!(git2::Repository::open(&root).unwrap().head().is_err());
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(
                    request.command,
                    Command::RunGitAction {
                        action: sailry_protocol::GitAction::Initialize,
                        ..
                    }
                ))
                .count(),
            1
        );
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

fn surface_focused(visual: &mut VisualTestContext, handle: &FocusHandle) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        if visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            handle.is_focused(window)
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "successor surface focus deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn stages_commits_and_keeps_captured_resource_scope(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let initial = prepare(&fixture);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual, "git-tracked-notes.txt");
        wait(visual, |cx| {
            snapshot(&panel, cx).lines().any(|line| {
                line.contains("module_component sailry/ui.IconButton")
                    && line.contains("\"git-branch-menu\"")
                    && line.contains("(\"disabled\", Bool(false))")
            })
        });
        click(visual, "git-branch-menu");
        ready(&panel, visual, "git-picker-1");
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !snapshot(&panel, cx).contains("git-picker-1"));
        assert!(visual.debug_bounds("git-picker-1").is_none());
        click(visual, "git-tracked-notes.txt");
        ready(&panel, visual, "git-diff-notes.txt");
        assert!(visual.debug_bounds("git-tab-notes.txt").is_some());
        click(visual, "git-index-tracked-notes.txt");
        wait(visual, |_| {
            writes(&fixture)
                .iter()
                .any(|request| matches!(request.command, Command::UpdateGitIndex { .. }))
        });
        wait(visual, |_| {
            let repository =
                git2::Repository::open(fixture.directory.path().join("project")).unwrap();
            let index = repository.index().unwrap();
            let entry = index.get_path(Path::new("notes.txt"), 0).unwrap();
            repository.find_blob(entry.id).unwrap().content() == b"working\n"
        });
        // The existing native text input and split action submit the package's frozen draft.
        click(visual, "field-1");
        visual.simulate_input("Packaged commit");
        visual.run_until_parked();
        visual.simulate_keystrokes("secondary-enter");
        wait(visual, |_| {
            let repository =
                git2::Repository::open(fixture.directory.path().join("project")).unwrap();
            repository.head().unwrap().target().unwrap().to_string() != initial
        });
        let mutations = writes(&fixture);
        assert_eq!(mutations.len(), 2);
        assert!(mutations.iter().all(|request| {
            request.plugin.as_ref().is_some_and(|context| {
                context.package.name == "git" && context.worktree == fixture.binding.worktree
            })
        }));
        let repository = git2::Repository::open(fixture.directory.path().join("project")).unwrap();
        assert_eq!(
            repository
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .message()
                .unwrap(),
            "Packaged commit\n"
        );
        let head = repository.head().unwrap().target().unwrap();

        ready(&panel, visual, "git-views-item-history");
        click(visual, "git-views-item-history");
        let row = format!("git-commit-{initial}");
        ready(&panel, visual, &row);
        click(visual, Box::leak(row.clone().into_boxed_str()));
        ready(&panel, visual, &format!("git-tab-commit:{initial}"));
        ready(
            &panel,
            visual,
            &format!("git-diff-commit:{initial}/notes.txt"),
        );
        let selected = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(selected.lines().any(|line| {
            line.contains("module_component sailry/ui.SelectableRow")
                && line.contains(&format!("\"{row}\""))
                && line.contains("(\"selected\", Bool(true))")
        }));
        let reference = package(&fixture).summary.reference();
        {
            let observed = fixture.transport.requests.lock().unwrap();
            let reads = observed.iter().filter(|request| {
                matches!(&request.command, Command::ReadGitCommit { commit, .. } if commit == &initial)
            }).collect::<Vec<_>>();
            assert!(!reads.is_empty());
            assert!(reads.iter().all(|request| {
                !request.command.durable()
                    && matches!(request.command, Command::ReadGitCommit { worktree, .. } if worktree == fixture.session.worktree)
                    && request.plugin.as_ref().is_some_and(|context| {
                        context.package == reference && context.worktree == fixture.binding.worktree
                    })
            }));
        }
        assert_eq!(repository.head().unwrap().target().unwrap(), head);

        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx);
            })
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).binding().client.target() == fixture.node.id()
                    && chat.read(cx).session() == Some(fixture.session.id)
                    && chat
                        .read(cx)
                        .renderer_available(sailry_protocol::plugin::desktop::ResourceKind::Git, cx)
            })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_git_resource(
                    (fixture.node.id(), fixture.session.worktree),
                    Some(Some("untracked.txt".into())),
                    window,
                    cx,
                );
            })
        });
        let embedded = shell.read_with(visual, |shell, _| match &shell.side_resource {
            Some(SideResource::Plugin(panel)) => panel.clone(),
            _ => panic!("ordinary Git renderer expected"),
        });
        ready(&embedded, visual, "git-diff-untracked.txt");
        let heading = visual.debug_bounds("git-tabs-header").unwrap();
        let controls = visual.debug_bounds("git-views-item-changes").unwrap();
        assert!(heading.right() <= controls.left());
        assert_eq!(
            embedded.read_with(visual, |panel, cx| panel.resource_scope(cx)),
            Some((fixture.node.id(), fixture.session.worktree))
        );
        assert_ne!(panel, embedded);
        let info = package(&fixture);
        fixture.execute(Command::SetPluginEnabled {
            name: "git".into(),
            expected_revision: info.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            !embedded.read(cx).resource_active() && embedded.read(cx).mounted.is_none()
        });
        wait(visual, |cx| {
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&gpui_shell::action::ShellAction::new("git-commit-action"))
                .next()
                .is_none()
                && cx
                    .key_bindings()
                    .borrow()
                    .bindings_for_action(&crate::resources::shortcuts::OpenResource(
                        sailry_protocol::plugin::desktop::ResourceKind::Git,
                    ))
                    .next()
                    .is_none()
        });
        assert!(
            repository
                .find_branch("feature", git2::BranchType::Local)
                .is_ok()
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn composer_changes_follow_package_lifecycle(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).session() == Some(fixture.session.id)
                    && chat
                        .read(cx)
                        .renderer_available(sailry_protocol::plugin::desktop::ResourceKind::Git, cx)
            })
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.executor().advance_clock(Duration::from_millis(10));
            visual.run_until_parked();
            let enabled = visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                shell.read(cx).current_chat().is_some_and(|chat| {
                    chat.read(cx).contribution_enabled(
                        "git",
                        "changes",
                        sailry_protocol::plugin::ui::Slot::Status,
                        cx,
                    )
                })
            });
            if enabled && visual.debug_bounds("plugin-control-git-changes").is_some() {
                break;
            }
            let detail = visual.update(|_, cx| {
                shell
                    .read(cx)
                    .current_chat()
                    .map(|chat| {
                        let chat = chat.read(cx);
                        chat.plugin_panel("git", cx)
                            .map(|panel| diagnostics(&panel, cx))
                            .unwrap_or_else(|| "Git controller missing".into())
                    })
                    .unwrap_or_else(|| "Conversation missing".into())
            });
            assert!(
                Instant::now() < deadline,
                "Git contribution deadline: {detail}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        click(visual, "plugin-control-git-changes");
        wait(visual, |cx| {
            matches!(shell.read(cx).side_resource, Some(SideResource::Plugin(_)))
        });
        let embedded = shell.read_with(visual, |shell, _| match &shell.side_resource {
            Some(SideResource::Plugin(panel)) => panel.clone(),
            _ => panic!("Git renderer expected"),
        });
        ready(&embedded, visual, "git-diff-changes:/notes.txt");
        let info = package(&fixture);
        fixture.execute(Command::SetPluginEnabled {
            name: "git".into(),
            expected_revision: info.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| !embedded.read(cx).resource_active());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("plugin-control-git-changes").is_none());
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .current_chat()
                .unwrap()
                .read(cx)
                .binding()
                .branch
                .to_string()),
            "main"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
