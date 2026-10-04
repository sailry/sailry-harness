use super::*;

fn explorer_layout(
    workspace: &Entity<crate::plugins::workspace::State>,
    visual: &mut VisualTestContext,
) {
    // Kit adjusts sizes after layout and schedules a settling frame. Wait
    // for those sizes to reach the rendered pane before measuring its controls.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let viewport = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.viewport_size().width
        });
        let (sizes, width) = workspace.read_with(visual, |state, cx| {
            (state.panels.read(cx).sizes().clone(), state.width)
        });
        let header = visual.debug_bounds("file-explorer-header");
        if header.is_some_and(|header| {
            sizes.len() == 2
                && (header.left() - px(crate::preview::RAIL_WIDTH) - sizes[0]).abs() <= px(1.)
                && (header.size.width - sizes[1]).abs() <= px(1.)
                && (header.size.width - width).abs() <= px(1.)
                && (header.right() - viewport).abs() <= px(1.)
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "explorer layout deadline: header={header:?}, sizes={sizes:?}, width={width:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn empty_state_fills_explorer(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let directory = fixture.directory.path().join("empty-project");
        std::fs::create_dir(&directory).unwrap();
        let Output::Project(project) = fixture.execute(Command::RegisterProject {
            name: "Empty project".into(),
            path: directory.to_str().unwrap().into(),
        }) else {
            panic!("project expected");
        };
        let (shell, visual) = mount(&fixture, remote, cx);
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.projects.iter().any(|item| item.id == project.id))
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &crate::live::menus::Dispatch {
                        node: fixture.node.id(),
                        target: crate::live::menus::Target::Project(project.id),
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
                .any(|entry| entry.package.name == "files" && entry.worktree.is_some())
        });
        let selector = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "files")
                .unwrap()
                .selector()
        });
        click(visual, Box::leak(selector.into_boxed_str()));
        let panel = shell.read_with(visual, |shell, _| {
            shell.extensions.as_ref().unwrap().panel.clone().unwrap()
        });
        wait(visual, |cx| {
            let rendered = snapshot(&panel, cx);
            panel.read(cx).resource_active()
                && rendered.contains("files_directory_empty")
                && ["file-create-menu", "file-search-toggle"].iter().all(|id| {
                    rendered.lines().any(|line| {
                        line.contains(&format!("\"{id}\""))
                            && line.contains("(\"disabled\", Bool(false))")
                    })
                })
        });
        let workspace = shell.read_with(visual, |shell, cx| shell.plugin_workspace(cx).unwrap());
        assert_eq!(
            workspace.read_with(visual, |state, _| state.width),
            px(320.)
        );
        click(visual, "file-search-toggle");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("file-search-results")
        });
        click(visual, "file-search-toggle");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("files_directory_empty")
        });
        for width in [1440., 1000.] {
            let handle = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(handle, size(px(width), px(900.)));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let document = visual.debug_bounds("empty-file_empty").unwrap();
            let empty = visual.debug_bounds("empty-files_directory_empty").unwrap();
            let header = visual.debug_bounds("file-explorer-header").unwrap();
            let icon = visual
                .debug_bounds("empty-icon-files_directory_empty")
                .unwrap();
            let title = visual
                .debug_bounds("empty-title-files_directory_empty")
                .unwrap();
            assert!(
                empty.left() >= header.left() && empty.right() <= header.right(),
                "empty state must fit the explorer width: {empty:?}, {header:?}"
            );
            assert!((empty.top() - header.bottom()).abs() < px(1.));
            assert!((empty.bottom() - px(900.)).abs() < px(1.));
            assert!(((icon.top() + title.bottom()) / 2. - empty.center().y).abs() < px(1.));
            assert!(
                visual
                    .debug_bounds("empty-card-files_directory_empty")
                    .is_none()
            );
            assert_eq!(icon.size, size(px(48.), px(48.)));
            assert!(title.size.height <= px(24.));
            let document_icon = visual.debug_bounds("empty-icon-file_empty").unwrap();
            let document_title = visual.debug_bounds("empty-title-file_empty").unwrap();
            assert_eq!(document_icon.size, icon.size);
            assert_eq!(document_title.size.height, title.size.height);
            assert!(
                document_icon.left() >= document.left()
                    && document_icon.right() <= document.right()
            );
            assert!(visual.debug_bounds("empty-card-file_empty").is_none());
            assert!(
                !visual
                    .update(|_, cx| snapshot(&panel, cx))
                    .contains("document-path")
            );
        }
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(700.), px(900.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("empty-files_directory_empty").is_none());
        let document = visual.debug_bounds("empty-file_empty").unwrap();
        assert!(document.left() >= px(crate::preview::RAIL_WIDTH) && document.right() <= px(700.));
        assert_eq!(
            workspace.read_with(visual, |state, _| state.width),
            px(320.)
        );
        visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
        explorer_layout(&workspace, visual);
        let header = visual.debug_bounds("file-explorer-header").unwrap();
        let empty = visual.debug_bounds("empty-files_directory_empty").unwrap();
        let divider = point(header.left(), empty.center().y);
        visual.simulate_mouse_move(divider, None, Modifiers::default());
        visual.simulate_mouse_down(divider, MouseButton::Left, Modifiers::default());
        for step in 1..=4 {
            visual.simulate_mouse_move(
                divider - point(px(10. * step as f32), px(0.)),
                MouseButton::Left,
                Modifiers::default(),
            );
            wait(visual, |_| true);
        }
        visual.simulate_mouse_up(
            divider - point(px(40.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        wait(visual, |cx| workspace.read(cx).width > px(320.));
        let width = workspace.read_with(visual, |state, _| state.width);
        explorer_layout(&workspace, visual);
        click(visual, "file-search-toggle");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("file-search-results")
        });
        click(visual, "file-search-toggle");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("files_directory_empty")
        });
        assert_eq!(workspace.read_with(visual, |state, _| state.width), width);
        click(visual, "toggle-details");
        click(visual, "toggle-details");
        assert_eq!(workspace.read_with(visual, |state, _| state.width), width);
        visual.update(|window, _| window.remove_window());
        drop(workspace);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn closing_tabs_focuses_the_next_editor(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        for name in ["first.txt", "second.txt"] {
            std::fs::write(fixture.directory.path().join("project").join(name), name).unwrap();
        }
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        for path in ["notes.txt", "first.txt", "second.txt"] {
            click(
                visual,
                Box::leak(format!("resource-file-{path}").into_boxed_str()),
            );
            let id = document(&panel, visual, path);
            // Opening reveals the resource but does not promise autofocus.
            // Activate its real tab before checking the content focus contract.
            click(visual, Box::leak(format!("file-tab-{id}").into_boxed_str()));
            editor_focused(&controller, visual, path);
        }
        assert_eq!(
            visual.update(|_, cx| documents(&panel, cx)["documents"].as_array().unwrap().len()),
            3
        );
        let first = document(&panel, visual, "first.txt");
        click(
            visual,
            Box::leak(format!("file-tab-{first}").into_boxed_str()),
        );
        editor_focused(&controller, visual, "first.txt");

        // No pointer interaction separates these closes. Each successor must own
        // the native editor focus so the same scoped shortcut can run again.
        for (closed, next) in [("first.txt", "second.txt"), ("second.txt", "notes.txt")] {
            visual.simulate_keystrokes("secondary-w");
            wait(visual, |cx| {
                !documents(&panel, cx)["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|document| document["path"] == closed)
            });
            editor_focused(&controller, visual, next);
            assert!(visual.update(|_, cx| snapshot(&panel, cx)).contains(next));
        }
        visual.simulate_keystrokes("secondary-w");
        wait(visual, |cx| {
            documents(&panel, cx)["documents"]
                .as_array()
                .unwrap()
                .is_empty()
        });
        assert!(panel.read_with(visual, |panel, _| panel.resource_active()));
        assert!(
            visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("file_empty")
        );
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.client.target()),
            fixture.node.id()
        );
        visual.update(|window, _| window.remove_window());
        drop(controller);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

fn editor_focused(
    controller: &Entity<crate::plugins::documents::Controller>,
    visual: &mut VisualTestContext,
    path: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        if visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            controller
                .read(cx)
                .editor(path)
                .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "editor focus deadline: {path}; {:?}",
            visual.update(|window, cx| {
                (
                    window.focused(cx),
                    controller
                        .read(cx)
                        .editor(path)
                        .map(|input| input.read(cx).focus_handle(cx)),
                )
            })
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn saves_drafts_and_retains_conflicts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        click(visual, "resource-file-notes.txt");
        let id = document(&panel, visual, "notes.txt");
        let heading = visual.debug_bounds("shell-module-header").unwrap();
        assert!(visual.debug_bounds("file-tabs-header").is_none());
        assert!(visual.debug_bounds("shell-navigation").is_none());
        assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
        let tree = visual.debug_bounds("files-tree").unwrap();
        assert!(heading.right() <= tree.left());
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.worktree),
            fixture.binding.worktree
        );
        edit(&panel, visual, &id, "Saved through the package 中文 🙂\n");
        visual.simulate_keystrokes("secondary-s");
        wait(visual, |cx| {
            value(&panel, &id, cx)["dirty"] == false && value(&panel, &id, cx)["saving"] == false
        });
        let path = fixture.directory.path().join("project/notes.txt");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "Saved through the package 中文 🙂\n"
        );

        edit(&panel, visual, &id, "Retained draft\n");
        std::fs::write(&path, "External edit\n").unwrap();
        visual.simulate_keystrokes("secondary-s");
        wait(visual, |cx| !value(&panel, &id, cx)["error"].is_null());
        assert_eq!(
            visual.update(|_, cx| value(&panel, &id, cx)["dirty"].clone()),
            true
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "External edit\n");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        enable(&fixture, false);
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        enable(&fixture, true);
        ready(&panel, visual);
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.documents.clone().unwrap()),
            controller
        );
        assert_eq!(document(&panel, visual, "notes.txt"), id);
        assert_eq!(
            visual.update(|_, cx| value(&panel, &id, cx)["dirty"].clone()),
            true
        );
        crate::feedback::tests::shown(visual);
        crate::feedback::tests::settle(visual);
        assert_eq!(
            visual.update(|window, cx| window.notifications(cx).len()),
            1
        );
        click(visual, "notification-close");
        crate::feedback::tests::settle(visual);
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
        click(
            visual,
            Box::leak(format!("file-tab-close-{id}").into_boxed_str()),
        );
        let (title, detail) = crate::prompts::tests::wait(visual);
        assert_eq!(title, crate::tr("files_discard_title").as_ref());
        assert!(detail.contains("notes.txt"));
        assert!(detail.contains(crate::tr("files_discard_description").as_ref()));
        assert!(
            !visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("files-discard-dialog")
        );
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(document(&panel, visual, "notes.txt"), id);
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(
                    request.command,
                    Command::WriteFile { .. } | Command::FinishFileUpload { .. }
                ) && request.plugin.as_ref().is_some_and(|context| context
                    .package
                    .name
                    == "files"
                    && context.worktree == fixture.binding.worktree))
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn menu_editing_targets_the_tree(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        let root = fixture.directory.path().join("project");
        for cut in [false, true] {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let row = visual.debug_bounds("resource-file-notes.txt").unwrap();
            visual.simulate_mouse_down(row.center(), MouseButton::Left, Modifiers::default());
            visual.run_until_parked();
            if cut {
                visual.dispatch_action(gpui_kit::component::input::Cut);
            } else {
                visual.dispatch_action(gpui_kit::component::input::SelectAll);
                visual.dispatch_action(gpui_kit::component::input::Copy);
            }
            visual.run_until_parked();
            visual.dispatch_action(gpui_kit::component::input::Paste);
            wait_for(&panel, visual, "file-transfer-dialog");
            visual.simulate_keystrokes("escape");
            wait(visual, |cx| {
                !snapshot(&panel, cx).contains("file-transfer-dialog")
            });
            assert!(root.join("notes.txt").exists());
        }
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(
                    &request.command,
                    Command::CopyEntryTo { .. } | Command::MoveEntryTo { .. }
                ))
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn tree_actions_create_rename_copy_and_search(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        click(visual, "file-create-menu");
        visual.simulate_keystrokes("down enter");
        wait_for(&panel, visual, "file-create-name");
        click(visual, "field-1");
        visual.simulate_input("created.txt");
        visual.simulate_keystrokes("enter");
        let id = document(&panel, visual, "created.txt");
        edit(&panel, visual, &id, "package needle\n");
        visual.simulate_keystrokes("secondary-s");
        wait(visual, |cx| value(&panel, &id, cx)["dirty"] == false);
        wait(visual, |cx| snapshot(&panel, cx).contains("created.txt"));
        click(visual, "resource-file-created.txt");
        visual.simulate_keystrokes("f2");
        wait_for(&panel, visual, "file-rename-name");
        click(visual, "field-2");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("renamed.txt");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| value(&panel, &id, cx)["path"] == "renamed.txt");
        let root = fixture.directory.path().join("project");
        assert!(!root.join("created.txt").exists());
        wait(visual, |cx| snapshot(&panel, cx).contains("renamed.txt"));
        click(visual, "resource-file-renamed.txt");
        visual.simulate_keystrokes("secondary-c secondary-v");
        wait_for(&panel, visual, "file-transfer-dialog");
        click(visual, "field-3");
        let retained = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.focused_input(cx)
        });
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("copied.txt");
        visual.run_until_parked();
        let (same_input, matches, retained_len, current_len, focus) =
            visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                let current = window.focused_input(cx);
                let retained_value = retained.as_ref().map(|input| input.value(cx));
                let current_value = current.as_ref().map(|input| input.value(cx));
                (
                    retained.is_some() && retained == current,
                    retained_value.as_deref() == Some("copied.txt"),
                    retained_value.as_ref().map(|value| value.chars().count()),
                    current_value.as_ref().map(|value| value.chars().count()),
                    window.focused(cx),
                )
            });
        assert!(
            same_input && matches,
            "transfer input mismatch: remote={remote}, bounds={:?}, same_input={same_input}, matches={matches}, retained_len={retained_len:?}, current_len={current_len:?}, focus={focus:?}",
            visual.debug_bounds("field-3"),
        );
        visual.simulate_keystrokes("enter");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.executor().advance_clock(Duration::from_millis(10));
            visual.run_until_parked();
            let rendered = visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                snapshot(&panel, cx)
            });
            // Copy completion omits a status line. The published file can
            // precede the UI reply, so wait for the finished dialog before Escape.
            if root.join("copied.txt").exists()
                && rendered.contains("file-transfer-dialog")
                && visual.debug_bounds("file-transfer-close").is_some()
                && visual.debug_bounds("file-transfer-confirm").is_none()
                && !rendered.contains("Progress ")
                && !rendered.contains("file-transfer-status-")
            {
                break;
            }
            if Instant::now() >= deadline {
                let paths: Vec<_> = fixture
                    .transport
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter_map(|request| match &request.command {
                        Command::CopyEntryTo { from, to, .. } => Some((from.clone(), to.clone())),
                        _ => None,
                    })
                    .collect();
                panic!(
                    "transfer completion deadline: remote={remote}, copy_paths={paths:?}; rendered={rendered}"
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(root.join("copied.txt")).unwrap(),
            "package needle\n"
        );
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("file-transfer-dialog")
        });
        click(visual, "file-search-toggle");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("file-search-results")
        });
        for selector in ["file-search-case", "file-search-regex"] {
            click(visual, selector);
            wait(visual, |cx| {
                snapshot(&panel, cx).lines().any(|line| {
                    line.contains(&format!("Toggle \"{selector}\""))
                        && line.contains("(\"checked\", Bool(true))")
                })
            });
        }
        click(visual, "field-4");
        visual.simulate_input("package needle");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| {
            fixture.transport.requests.lock().unwrap().iter().any(|request|
            matches!(&request.command, Command::SearchFiles {worktree,options} if *worktree == fixture.session.worktree && options.query == "package needle" && options.case_sensitive && options.regex)
                && request.plugin.as_ref().is_some_and(|context| context.package.name == "files"))
        });
        wait(visual, |cx| !snapshot(&panel, cx).contains("Searching"));
        wait_for(&panel, visual, "file-search-result-0");
        click(visual, "file-search-result-0");
        let copied = document(&panel, visual, "copied.txt");
        wait(visual, |cx| {
            documents(&panel, cx)["reveal"]["document"] == copied
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn embedded_renderer_uses_the_session_scope(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let chat = shell.chat_view(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                );
                shell
                    .chats
                    .views
                    .insert((fixture.node.id(), fixture.session.id), chat);
                shell.activate_session(
                    Key::Session(fixture.node.id(), fixture.session.id),
                    window,
                    cx,
                );
                shell.open_documents(
                    (fixture.node.id(), fixture.session.worktree),
                    None,
                    window,
                    cx,
                );
            })
        });
        let panel = shell.read_with(visual, |shell, _| match &shell.side_resource {
            Some(SideResource::Plugin(panel)) => panel.clone(),
            _ => panic!("ordinary Files renderer expected"),
        });
        ready(&panel, visual);
        assert!(visual.debug_bounds("file-tabs-header").is_none());
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.session),
            Some(fixture.session.id)
        );
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        click(visual, "resource-file-notes.txt");
        let id = document(&panel, visual, "notes.txt");
        let heading = visual.debug_bounds("file-tabs-header").unwrap();
        let tree = visual.debug_bounds("files-tree").unwrap();
        assert!(heading.right() <= tree.left());
        edit(&panel, visual, &id, "Embedded save\n");
        click(visual, "files_save");
        wait(visual, |cx| value(&panel, &id, cx)["dirty"] == false);
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/notes.txt")).unwrap(),
            "Embedded save\n"
        );
        visual.update(|_, cx| shell.update(cx, |shell, cx| shell.close_resource_panel(cx)));
        assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
