use super::*;
use gpui_kit::test::TestWindowExt as _;

fn staging_ready(visual: &mut VisualTestContext, id: SharedString, checked: bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        wait(visual, |_| true);
        let ready = visual.update(|window, _| {
            window.try_find(id.clone()).is_some_and(|check| {
                check.visible()
                    && check.disabled() != Some(true)
                    && check.checked() == Some(checked)
            })
        });
        if ready {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "staging checkbox readiness deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn protects_counts_and_staging_paths(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        let root = fixture.directory.path().join("project");
        let repository = git2::Repository::open(&root).unwrap();
        let long = format!("nested/long/path/{}.rs", "长文件名".repeat(12));
        let paths: [&str; 4] = [
            &long,
            "only-plus.rs",
            "only-minus.rs",
            "nested/companion.rs",
        ];
        let base = [
            "old\n".repeat(164),
            "base\n".into(),
            "old\n".repeat(164),
            "base\n".into(),
        ];
        let changed = [
            "new\n".repeat(237),
            format!("base\n{}", "new\n".repeat(237)),
            String::new(),
            "base\nnew\n".into(),
        ];
        let mut index = repository.index().unwrap();
        for (path, content) in paths.iter().zip(&base) {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), content).unwrap();
            index.add_path(Path::new(path)).unwrap();
        }
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let parent = repository.head().unwrap().peel_to_commit().unwrap();
        let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repository
            .commit(
                Some("HEAD"),
                &author,
                &author,
                "Counts fixture",
                &tree,
                &[&parent],
            )
            .unwrap();
        for (path, content) in paths.iter().zip(&changed) {
            std::fs::write(root.join(path), content).unwrap();
        }
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let selector = format!("git-tracked-{long}");
        let panel = main(&shell, &fixture, visual, &selector);
        assert!(visual.debug_bounds("git-dir-tracked/nested").is_some());
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("resource-detail-tracked/{long}").into_boxed_str()
                ))
                .is_none()
        );
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1400.), px(900.)));
        let workspace = shell.read_with(visual, |shell, cx| shell.plugin_workspace(cx).unwrap());
        for width in [260., 480.] {
            workspace.update(visual, |workspace, cx| {
                workspace.width = px(width);
                cx.notify();
            });
            wait(visual, |_| true);
            let mut columns = Vec::new();
            for (index, path) in paths.iter().enumerate() {
                let bounds = |visual: &mut VisualTestContext, prefix: &str| {
                    let selector = Box::leak(format!("{prefix}-tracked/{path}").into_boxed_str());
                    visual.debug_bounds(selector).unwrap()
                };
                let label = bounds(visual, "resource-label");
                let counts = bounds(visual, "resource-badges");
                let selector = Box::leak(format!("git-index-tracked-{path}").into_boxed_str());
                let check = visual.debug_bounds(selector).unwrap();
                let viewport = visual.update(|window, _| window.viewport_size());
                assert!(counts.size.width > px(0.) && counts.size.width < px(96.));
                assert!(label.right() <= counts.left());
                assert!(counts.right() < check.left());
                assert!(check.left() >= px(0.) && check.right() <= viewport.width);
                assert!(check.top() >= px(0.) && check.bottom() <= viewport.height);
                assert!(label.size.width > px(20.));
                for badge in 0..if index == 0 { 2 } else { 1 } {
                    let selector = Box::leak(
                        format!("resource-badge-tracked/{path}-{badge}").into_boxed_str(),
                    );
                    let badge = visual.debug_bounds(selector).unwrap();
                    assert!(badge.left() >= counts.left() && badge.right() <= counts.right());
                }
                columns.push((counts.right(), check.left(), check.right()));
            }
            assert!(columns.windows(2).all(|pair| pair[0] == pair[1]));
        }
        let leaf: &'static str = Box::leak(selector.clone().into_boxed_str());
        click(visual, leaf);
        let diff = format!("git-diff-{long}");
        ready(&panel, visual, &diff);
        click(visual, "git-dir-tracked/nested");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            wait(visual, |_| true);
            if visual.debug_bounds(leaf).is_none() {
                break;
            }
            assert!(Instant::now() < deadline, "folder collapse deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            visual
                .debug_bounds(Box::leak(diff.clone().into_boxed_str()))
                .is_some()
        );
        // The folder cursor is independent of the still-selected diff document.
        visual.simulate_keystrokes("enter");
        ready(&panel, visual, &selector);
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("resource-guide-tracked/{long}-2-arm").into_boxed_str()
                ))
                .is_some()
        );
        let group = SharedString::from("git-select-group-dir/tracked/nested");
        for (count, operation) in [
            (1, sailry_protocol::GitIndexChange::Stage),
            (2, sailry_protocol::GitIndexChange::Unstage),
        ] {
            staging_ready(visual, group.clone(), count == 2);
            visual.update(|window, cx| {
                let check = window.find(group.clone());
                assert_eq!(check.checked(), Some(count == 2));
                window.click(group.clone(), cx);
            });
            wait(visual, |_| writes(&fixture).len() == count);
            wait(visual, |_| {
                let mut index = repository.index().unwrap();
                index.read(true).unwrap();
                [0, 3].iter().all(|&position| {
                    let entry = index.get_path(Path::new(paths[position]), 0).unwrap();
                    repository.find_blob(entry.id).unwrap().content()
                        == if count == 1 {
                            changed[position].as_bytes()
                        } else {
                            base[position].as_bytes()
                        }
                })
            });
            let mutations = writes(&fixture);
            let Command::UpdateGitIndex {
                worktree,
                paths: captured,
                operation: actual,
                ..
            } = &mutations[count - 1].command
            else {
                panic!("index update expected")
            };
            assert_eq!(*worktree, fixture.session.worktree);
            assert_eq!(*actual, operation);
            let mut captured = captured.clone();
            captured.sort();
            let mut expected = vec![long.clone(), "nested/companion.rs".into()];
            expected.sort();
            assert_eq!(captured, expected);
            assert!(
                mutations[count - 1]
                    .plugin
                    .as_ref()
                    .is_some_and(|context| context.package.name == "git"
                        && context.worktree == fixture.binding.worktree)
            );
            ready(&panel, visual, &selector);
            assert!(
                visual
                    .debug_bounds(Box::leak(diff.clone().into_boxed_str()))
                    .is_some()
            );
        }
        let admitted = writes(&fixture).len();
        // Selecting the submenu renders a second popup-menu scope. Use Kit's
        // submenu scope to click List without relying on its screen direction.
        click(visual, "git-actions");
        visual.update(|window, cx| {
            let item = window.within("popup-menu").find(0_usize);
            assert_eq!(item.role(), Some(Role::MenuItem));
            assert_eq!(item.label(), Some("View options"));
            window.press("down", cx);
            let mut submenu = window.within("submenu");
            let item = submenu.find(0_usize);
            assert_eq!(item.role(), Some(Role::MenuItem));
            assert_eq!(item.label(), Some("List"));
            assert!(item.visible());
            submenu.click(0_usize, cx);
        });
        let detail = format!("resource-detail-tracked/{long}");
        ready(&panel, visual, &detail);
        assert!(visual.debug_bounds("git-dir-tracked/nested").is_none());
        let label = visual
            .debug_bounds(Box::leak(
                format!("resource-label-tracked/{long}").into_boxed_str(),
            ))
            .unwrap();
        let detail = visual
            .debug_bounds(Box::leak(detail.into_boxed_str()))
            .unwrap();
        let counts = visual
            .debug_bounds(Box::leak(
                format!("resource-badges-tracked/{long}").into_boxed_str(),
            ))
            .unwrap();
        assert!(label.size.width > px(20.));
        assert!(label.right() <= counts.left() && detail.right() <= counts.left());
        click(visual, "git-refresh");
        ready(&panel, visual, &selector);
        assert!(visual.debug_bounds("git-dir-tracked/nested").is_none());
        let check = SharedString::from(format!("git-index-tracked-{long}"));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            wait(visual, |_| true);
            let observed = visual.update(|window, _| window.try_find(check.clone()));
            if observed
                .as_ref()
                .is_some_and(|check| check.visible() && check.disabled() != Some(true))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "checkbox readiness deadline: remote={remote}, check={observed:?}; {}",
                panel.read_with(visual, |_, cx| diagnostics(&panel, cx)),
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        visual.update(|window, cx| {
            let observed = window.find(check.clone());
            assert_eq!(observed.role(), Some(Role::CheckBox));
            assert_eq!(observed.checked(), Some(false));
            window.click(check.clone(), cx);
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mutations = writes(&fixture);
            assert!(
                mutations.len() <= admitted + 1,
                "duplicate staging requests: {mutations:?}"
            );
            if mutations.len() == admitted + 1 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "staging deadline: remote={remote}, check={:?}, mutations={mutations:?}; {}",
                visual.update(|window, _| window.try_find(check.clone())),
                panel.read_with(visual, |_, cx| diagnostics(&panel, cx)),
            );
            wait(visual, |_| true);
            std::thread::sleep(Duration::from_millis(10));
        }
        wait(visual, |_| {
            let mut index = repository.index().unwrap();
            // Node writes the index through a separate repository handle.
            index.read(true).unwrap();
            let entry = index.get_path(Path::new(&long), 0).unwrap();
            repository.find_blob(entry.id).unwrap().content() == changed[0].as_bytes()
        });
        let mutations = writes(&fixture);
        let Command::UpdateGitIndex {
            worktree,
            paths: captured,
            operation,
            ..
        } = &mutations[admitted].command
        else {
            panic!("index update expected")
        };
        assert_eq!(*worktree, fixture.session.worktree);
        assert_eq!(captured, std::slice::from_ref(&long));
        assert_eq!(*operation, sailry_protocol::GitIndexChange::Stage);
        assert!(mutations[admitted].plugin.as_ref().is_some_and(|context| {
            context.package.name == "git" && context.worktree == fixture.binding.worktree
        }));
        let mut index = repository.index().unwrap();
        index.read(true).unwrap();
        for (path, content) in paths[1..].iter().zip(&base[1..]) {
            let entry = index.get_path(Path::new(path), 0).unwrap();
            assert_eq!(
                repository.find_blob(entry.id).unwrap().content(),
                content.as_bytes()
            );
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
