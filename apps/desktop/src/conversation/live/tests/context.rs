use super::*;
mod package;

#[gpui::test]
fn selection_preserves_drafts(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, Vec::new());
        package::install(&fixture);
        let mut binding = fixture.binding.clone();
        binding.project = None;
        binding.worktree = None;
        let (_host, view, visual) = package::open(&fixture, cx, binding, None);
        wait(visual, |cx| {
            view.read(cx).connected()
                && view.read(cx).contribution_enabled(
                    "worktrees",
                    "location",
                    sailry_protocol::plugin::ui::Slot::Context,
                    cx,
                )
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this draft 中文");
        choose(visual, "Approval fixture", None);
        wait(visual, |cx| {
            view.read(cx).connected()
                && view.read(cx).binding.project == fixture.binding.project
                && view.read(cx).contribution_enabled(
                    "worktrees",
                    "location",
                    sailry_protocol::plugin::ui::Slot::Context,
                    cx,
                )
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx)),
            "Keep this draft 中文"
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.binding.worktree),
            fixture.binding.worktree
        );
        assert!(
            visual
                .debug_bounds("plugin-control-worktrees-location")
                .is_some()
        );
        assert!(visual.debug_bounds("composer-project").is_none());
        assert!(visual.debug_bounds("plugin-control-git-branch").is_none());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn creates_moves_and_forks(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, Vec::new());
        repository(&fixture);
        package::install(&fixture);
        let path = fixture.directory.path().join("project/sample.txt");
        std::fs::write(&path, "Uncommitted edits\n").unwrap();
        std::fs::write(path.with_file_name("build notes.md"), "New file\n").unwrap();
        fixture.start();
        let (_host, view, visual) = package::open(
            &fixture,
            cx,
            fixture.binding.clone(),
            Some(fixture.session.clone()),
        );
        wait(visual, |cx| {
            view.read(cx).git
                && !view.read(cx).rows.is_empty()
                && view.read(cx).can_move()
                && view.read(cx).can_invoke_contribution(
                    sailry_protocol::plugin::ui::Intent::CreateWorktree,
                    cx,
                )
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.binding.branch.clone()),
            "main"
        );
        click(visual, "plugin-control-git-branch");
        until(visual, |visual| {
            visual.debug_bounds("git-picker-1").is_some()
        });
        assert!(visual.debug_bounds("composer-location-picker").is_none());
        visual.simulate_keystrokes("escape");
        until(visual, |visual| {
            visual.debug_bounds("git-picker-1").is_none()
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.binding.worktree),
            fixture.binding.worktree
        );
        let turn = view.read_with(visual, |view, _| view.rows[0]);
        click(visual, "live-chat-input");
        visual.simulate_input("Continue here");
        view.update(visual, |view, cx| {
            view.scroller
                .update(cx, |scroller, cx| scroller.scroll_to_item(0, cx));
        });
        fixture::tap(visual, &format!("live-turn-more-{turn}"));
        visual.simulate_keystrokes("up enter");
        create(&view, visual, "isolated", true, false);
        wait(visual, |cx| {
            view.read(cx).binding.branch == "isolated" && view.read(cx).can_move()
        });
        assert!(visual.debug_bounds("plugin-control-git-branch").is_some());
        let moved = view.read_with(visual, |view, cx| {
            assert_eq!(view.draft(cx), "Continue here");
            assert!(view.rows.contains(&turn));
            assert_eq!(view.turn_worktree(turn), fixture.binding.worktree);
            view.session.clone().unwrap()
        });
        assert_eq!(moved.id, fixture.session.id);
        assert_ne!(moved.worktree, fixture.session.worktree);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == moved.worktree)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(std::path::Path::new(&tree.path).join("sample.txt")).unwrap(),
            "Uncommitted edits\n"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "Uncommitted edits\n"
        );
        select(&fixture, &view, visual, "main");
        wait(visual, |cx| {
            view.read(cx).binding.worktree == fixture.binding.worktree
                && view.read(cx).binding.branch == "main"
                && view.read(cx).git
                && view.read(cx).can_move()
        });
        assert!(visual.debug_bounds("plugin-control-git-branch").is_some());
        let forked = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen = forked.clone();
        view.update(visual, |_, cx| {
            cx.subscribe(&view, move |_, _, event, _| {
                if let Event::Forked(session) = event {
                    *seen.lock().unwrap() = Some(*session.clone());
                }
            })
            .detach()
        });
        wait(visual, |cx| {
            view.read(cx)
                .can_invoke_contribution(sailry_protocol::plugin::ui::Intent::ForkWorktree, cx)
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.fork_worktree(window, cx));
        });
        create(&view, visual, "forked", true, true);
        wait(visual, |_| forked.lock().unwrap().is_some());
        let fork = forked.lock().unwrap().clone().unwrap();
        assert_ne!(fork.id, moved.id);
        assert_ne!(fork.worktree, moved.worktree);
        assert_eq!(fork.fork.as_ref().unwrap().through, turn);
        assert_eq!(
            view.read_with(visual, |view, _| view.binding.worktree),
            fixture.binding.worktree
        );
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == fork.worktree)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(std::path::Path::new(&tree.path).join("sample.txt")).unwrap(),
            "Uncommitted edits\n"
        );
        assert_eq!(
            std::fs::read_to_string(std::path::Path::new(&tree.path).join("build notes.md"))
                .unwrap(),
            "New file\n"
        );
        visual.update(|window, _| window.remove_window());
        let mut binding = fixture.binding.clone();
        binding.worktree = Some(fork.worktree);
        let (_restored_host, restored, visual) =
            package::open(&fixture, cx, binding, Some(fork.clone()));
        wait(visual, |cx| {
            restored.read(cx).binding.branch == "forked" && restored.read(cx).can_move()
        });
        assert!(restored.read_with(visual, |view, _| view.rows.contains(&turn)));
        select(&fixture, &restored, visual, "isolated");
        wait(visual, |cx| {
            restored.read(cx).binding.worktree == Some(moved.worktree)
                && restored.read(cx).can_move()
        });
        assert_eq!(
            restored.read_with(visual, |view, _| view.session.as_ref().unwrap().id),
            fork.id
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

fn repository(fixture: &fixture::Fixture) {
    let root = fixture.directory.path().join("project");
    let repository = git2::Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("sample.txt"), "base\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(std::path::Path::new("sample.txt")).unwrap();
    index.write().unwrap();
    let tree = index.write_tree().unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Initial",
            &repository.find_tree(tree).unwrap(),
            &[],
        )
        .unwrap();
}

fn choose(visual: &mut VisualTestContext, query: &str, selector: Option<&'static str>) {
    until(visual, |visual| {
        visual
            .debug_bounds("plugin-control-worktrees-location")
            .is_some()
    });
    click(visual, "plugin-control-worktrees-location");
    until(visual, |visual| {
        visual.debug_bounds("composer-location-picker").is_some()
    });
    if !query.is_empty() {
        visual.simulate_input(query);
    }
    if let Some(selector) = selector {
        until(visual, |visual| visual.debug_bounds(selector).is_some());
        click(visual, selector);
    } else {
        visual.simulate_keystrokes("enter");
    }
}

#[track_caller]
fn create(
    view: &Entity<View>,
    visual: &mut VisualTestContext,
    branch: &str,
    include_changes: bool,
    keyboard: bool,
) {
    until(visual, |visual| {
        assert_eq!(
            view.read_with(visual, |view, _| view.error),
            None,
            "opening {branch}"
        );
        visual.debug_bounds("location-branch-name").is_some()
    });
    click(visual, "location-branch-name");
    visual.simulate_keystrokes("cmd-a");
    visual.simulate_input(branch);
    if !include_changes {
        click(visual, "location-copy-changes");
    }
    if keyboard {
        visual.simulate_keystrokes("enter");
    } else {
        click(visual, "location-create-submit");
    }
    until(visual, |visual| {
        visual.debug_bounds("location-branch-name").is_none()
    });
}

#[track_caller]
fn until(visual: &mut VisualTestContext, predicate: impl Fn(&mut VisualTestContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if predicate(visual) {
            return;
        }
        assert!(Instant::now() < deadline, "location surface deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[track_caller]
fn select(
    fixture: &fixture::Fixture,
    view: &Entity<View>,
    visual: &mut VisualTestContext,
    branch: &str,
) {
    wait(visual, |cx| {
        view.read(cx)
            .can_invoke_contribution(sailry_protocol::plugin::ui::Intent::Worktrees, cx)
    });
    let Output::GitWorktrees(found) = fixture.execute(Command::ListWorktrees {
        worktree: fixture.session.worktree,
    }) else {
        panic!("worktrees expected")
    };
    let tree = found
        .entries
        .iter()
        .find(|tree| tree.branch.as_deref() == Some(branch))
        .unwrap();
    let index = found
        .entries
        .iter()
        .position(|entry| entry.path == tree.path)
        .unwrap();
    let completed = Arc::new(std::sync::Mutex::new(None));
    let result = completed.clone();
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            let request = view.contribution_intent(
                sailry_protocol::plugin::ui::Intent::Worktrees,
                serde_json::Value::Null,
                window,
                cx,
            );
            cx.spawn(async move |_, _| {
                let value = request.await;
                *result.lock().unwrap() = Some(value);
            })
            .detach();
        });
    });
    wait(visual, |_| completed.lock().unwrap().is_some());
    assert_eq!(
        completed.lock().unwrap().take(),
        Some(Ok(serde_json::Value::Bool(true)))
    );
    let selector = Box::leak(format!("entry-{index}").into_boxed_str());
    until(visual, |visual| visual.debug_bounds(selector).is_some());
    click(visual, selector);
    until(visual, |visual| visual.debug_bounds("action-0").is_some());
    click(visual, "action-0");
}
