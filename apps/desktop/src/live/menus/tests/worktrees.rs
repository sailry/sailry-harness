use super::*;
use crate::live::file_tests::{click, settle};

mod availability;

pub(super) fn copy_and_checkout(
    visual: &mut VisualTestContext,
    shell: &Entity<Shell>,
    runtime: &tokio::runtime::Runtime,
    action: &Dispatch,
    root: &std::path::Path,
    branch: &str,
) {
    let (client, worktree, page) = shell.read_with(visual, |shell, _| {
        let live = shell.live.as_ref().unwrap();
        (
            live.client(),
            live.selected_worktree().unwrap().id,
            shell.page,
        )
    });
    let Target::Project(project) = action.target else {
        panic!("project expected")
    };
    install(runtime, &client, worktree, root);
    dispatch(visual, shell, action, Command::Branches);
    overlay(visual, shell, project, "git", branch);
    assert_eq!(shell.read_with(visual, |shell, _| shell.page), page);
    choose(visual, branch);
    overlay(visual, shell, project, "git", &tr("git_copy_branch"));
    choose(visual, &tr("git_copy_branch"));
    settle(visual, shell, |_, cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref()
            == Some(branch)
    });
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        branch
    );

    visual.update(|window, cx| shell.update(cx, |shell, cx| shell.navigate(Page::Git, window, cx)));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let state = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            shell
                .read(cx)
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .map(|panel| crate::plugins::diagnostics(panel, cx))
                .unwrap_or_default()
        });
        if state.lines().any(|line| {
            line.contains("IconButton")
                && line.contains("\"git-branch-menu\"")
                && line.contains("(\"disabled\", Bool(false))")
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "Git button deadline: {state}");
        std::thread::sleep(Duration::from_millis(10));
    }
    surface(visual, "git-branch-menu");
    click(visual, "git-branch-menu");
    surface(visual, "0");
    choose(visual, branch);
    settle(visual, shell, |_, _| {
        matches!(runtime.block_on(client.execute(client.prepare(Request::InspectGit {worktree}))),
        Ok(Output::GitStatus(status)) if status.branch.as_deref() == Some(branch))
    });
    assert_eq!(
        git2::Repository::open(root)
            .unwrap()
            .head()
            .unwrap()
            .shorthand()
            .unwrap(),
        branch
    );
}

pub(super) fn check(
    visual: &mut VisualTestContext,
    shell: &Entity<Shell>,
    runtime: &tokio::runtime::Runtime,
    action: &Dispatch,
    root: &std::path::Path,
) {
    let (client, original) = shell.read_with(visual, |shell, _| {
        let live = shell.live.as_ref().unwrap();
        (live.client(), live.selected_worktree().unwrap().id)
    });
    availability::check(visual, shell, runtime, action, root);
    let branch = format!(
        "managed-{}",
        &sailry_protocol::RequestId::new().to_string()[..8]
    );
    let path = root.parent().unwrap().join(&branch);
    let repository = git2::Repository::open(root).unwrap();
    let Target::Project(project) = action.target else {
        panic!("project expected")
    };
    runtime
        .block_on(client.execute(client.prepare(Request::CreateWorktree {
            project,
            path: path.to_str().unwrap().into(),
            branch: branch.clone(),
            commit: repository.head().unwrap().target().unwrap().to_string(),
        })))
        .unwrap();

    dispatch(visual, shell, action, Command::Branches);
    overlay(visual, shell, project, "git", &branch);
    assert!(visual.debug_bounds("entry-0").is_none());
    choose(visual, &branch);
    overlay(visual, shell, project, "git", &tr("git_rename_branch"));
    choose(visual, &tr("git_rename_branch"));
    surface(visual, "git-rename-name");
    let renamed = format!("{branch}-renamed");
    click(visual, "git-rename-name");
    visual.simulate_keystrokes("secondary-a");
    visual.simulate_input(&renamed);
    visual.simulate_keystrokes("enter");
    settle(visual, shell, |_, _| {
        repository
            .find_branch(&renamed, git2::BranchType::Local)
            .is_ok()
            && git2::Repository::open(&path).is_ok_and(|repository| {
                repository
                    .head()
                    .is_ok_and(|head| head.shorthand().is_ok_and(|name| name == renamed))
            })
    });
    assert_eq!(selected(visual, shell), original);
    assert_eq!(
        git2::Repository::open(&path)
            .unwrap()
            .head()
            .unwrap()
            .shorthand()
            .unwrap(),
        renamed.as_str()
    );

    // Project and composer worktree controls share the package while preserving their source.
    open(visual, shell, action);
    overlay(visual, shell, project, "worktrees", &renamed);
    choose(visual, &renamed);
    surface(visual, "action-1");
    choose(visual, &tr("worktree_remove"));
    surface(visual, "worktree-remove-cancel");
    click(visual, "worktree-remove-cancel");
    assert!(path.exists());

    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Project, window, cx);
            shell.new_live_conversation(window, cx);
        })
    });
    settle(visual, shell, |shell, cx| {
        shell
            .current_chat()
            .is_some_and(|view| view.read(cx).connected())
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let ready = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            shell.read(cx).current_chat().is_some_and(|view| {
                let chat = view.read(cx);
                chat.can_invoke_contribution(sailry_protocol::plugin::ui::Intent::Worktrees, cx)
                    && chat.location_state()["git"] == true
            })
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "composer contribution deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    surface(visual, "plugin-control-worktrees-location");
    click(visual, "plugin-control-worktrees-location");
    composer_surface(visual, shell, "create");
    assert!(visual.debug_bounds("composer-location-picker").is_none());
    choose(visual, &tr("location_create"));
    composer_surface(visual, shell, "location-branch-name");
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    surface(visual, "plugin-control-git-branch");
    click(visual, "plugin-control-git-branch");
    surface(visual, "0");
    assert!(visual.debug_bounds("entry-0").is_none());
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    open(visual, shell, action);
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        Page::Conversation
    );
    choose(visual, &renamed);
    surface(visual, "action-1");
    choose(visual, &tr("worktree_remove"));
    surface(visual, "worktree-remove-confirm");
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        Page::Conversation
    );
    click(visual, "worktree-remove-confirm");
    settle(visual, shell, |shell, _| {
        !path.exists()
            && shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .worktrees
                        .iter()
                        .all(|tree| tree.path != path.to_str().unwrap())
                })
    });
    assert!(
        repository
            .find_branch(&renamed, git2::BranchType::Local)
            .is_ok()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.debug_bounds("worktree-remove-confirm").is_none() {
            visual.run_until_parked();
            break;
        }
        assert!(
            Instant::now() < deadline,
            "worktree removal dialog deadline: worktree-remove-confirm"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn install(
    runtime: &tokio::runtime::Runtime,
    client: &Client,
    worktree: WorktreeId,
    root: &std::path::Path,
) {
    for name in ["git", "worktrees"] {
        let path = root.join(format!("fixture-{name}"));
        crate::plugins::fixture::copy_package(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/{name}")),
            &path,
        );
        let view = path.join("dev.sailry.platform/desktop/view.js");
        let source = std::fs::read_to_string(&view).unwrap()
            .replace("return div().id(id).child(control);", "return Anchor.new(id).child(control);")
            .replace("div().id(id).child(TextField", "Anchor.new(id).child(TextField")
            .replace("div().id(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name')", "Anchor.new(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name')")
            .replace("div().id(branch ? `${prefix}-name` : 'git-form-name')", "Anchor.new(branch ? `${prefix}-name` : 'git-form-name')");
        std::fs::write(
            view,
            format!("import {{Anchor}} from 'sailry/test';\n{source}"),
        )
        .unwrap();
        let Output::Plugin(previous) = runtime
            .block_on(client.execute(client.prepare(Request::ReadPlugin { name: name.into() })))
            .unwrap()
        else {
            panic!("package expected")
        };
        let Output::Plugin(package) = runtime
            .block_on(client.execute(client.prepare(Request::InstallPlugin {
                worktree,
                path: format!("fixture-{name}"),
                name: name.into(),
                expected_revision: previous.summary.revision,
            })))
            .unwrap()
        else {
            panic!("package expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        assert!(package.summary.enabled);
    }
}

fn selected(visual: &mut VisualTestContext, shell: &Entity<Shell>) -> WorktreeId {
    shell.read_with(visual, |shell, _| {
        shell.live.as_ref().unwrap().selected_worktree().unwrap().id
    })
}
fn dispatch(
    visual: &mut VisualTestContext,
    shell: &Entity<Shell>,
    action: &Dispatch,
    command: Command,
) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live_resource_action(
                &Dispatch {
                    command,
                    ..action.clone()
                },
                window,
                cx,
            )
        })
    });
    visual.run_until_parked();
}
fn open(visual: &mut VisualTestContext, shell: &Entity<Shell>, action: &Dispatch) {
    dispatch(visual, shell, action, Command::Worktrees);
    surface(visual, "entry-0");
}

fn overlay(
    visual: &mut VisualTestContext,
    shell: &Entity<Shell>,
    project: ProjectId,
    package: &str,
    text: &str,
) {
    let registry = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.project_plugins(project, window, cx).unwrap()
        })
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let state = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            registry
                .read(cx)
                .test_panel(package)
                .map(|panel| crate::plugins::diagnostics(&panel, cx))
                .unwrap_or_default()
        });
        if state.contains(text) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "package overlay deadline: {package}, {text}; {state}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[track_caller]
fn surface(visual: &mut VisualTestContext, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "manager surface deadline: {selector}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[track_caller]
fn composer_surface(visual: &mut VisualTestContext, shell: &Entity<Shell>, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let (state, location) = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let chat = shell.read(cx).current_chat().unwrap();
            let chat = chat.read(cx);
            (
                chat.plugin_panel("worktrees", cx)
                    .map(|panel| crate::plugins::diagnostics(&panel, cx))
                    .unwrap_or_default(),
                chat.location_state(),
            )
        });
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "manager surface deadline: {selector}; location={location}; {state}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn choose(visual: &mut VisualTestContext, query: &str) {
    visual.simulate_keystrokes("secondary-a");
    visual.simulate_input(query);
    visual.run_until_parked();
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}
