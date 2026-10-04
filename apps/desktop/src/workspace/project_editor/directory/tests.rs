use super::*;
use core::prelude::v1::test;
use std::time::{Duration, Instant};

fn wait(cx: &mut VisualTestContext, picker: &Entity<Picker>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            let _ = window.draw(cx);
            !picker.read(cx).busy
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "picker update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn opens_folders(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::new(remote);
        cx.update(|cx| {
            let mut services = fixture.services(remote);
            services.local = fixture.transport.clone();
            cx.set_global(services);
        });
        let path = fixture.directory.path().join("opened-folder");
        std::fs::create_dir(&path).unwrap();
        let path = path.canonicalize().unwrap();
        std::fs::write(path.join("draft.txt"), "Keep this draft\n").unwrap();
        let mut shell = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let entity = cx.new(|cx| Shell::new(window, cx));
            shell = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let shell = shell.unwrap();
        for attempt in 0..2 {
            let picker = visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let form = shell
                        .build_project_editor(0, None, None, window, cx)
                        .unwrap();
                    open_project(form, window, cx)
                })
            });
            wait(visual, &picker);
            visual.update(|window, cx| {
                picker.update(cx, |picker, cx| {
                    picker.navigate(Some(path.to_str().unwrap().into()), Visit::Push, window, cx)
                });
            });
            wait(visual, &picker);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("directory-picker").is_some());
            assert!(visual.debug_bounds("project-editor").is_none());
            if remote && attempt == 0 {
                fixture
                    .transport
                    .mode
                    .store(17, std::sync::atomic::Ordering::SeqCst);
            }
            click(visual, "directory-confirm");
            if remote && attempt == 0 {
                let editor =
                    picker.read_with(visual, |picker, _| picker.registration.clone().unwrap());
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    visual.run_until_parked();
                    if editor.read_with(visual, |editor, _| !editor.pending) {
                        break;
                    }
                    assert!(Instant::now() < deadline, "registration receipt deadline");
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(editor.read_with(visual, |editor, _| editor.request.is_some()));
                assert_eq!(
                    editor.read_with(visual, |editor, _| editor.error),
                    Some("project_outcome_unknown")
                );
                assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
                visual.update(|window, cx| {
                    assert_eq!(
                        crate::feedback::tests::summary(window, cx),
                        tr("project_outcome_unknown")
                    );
                });
                click(visual, "directory-confirm");
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                visual.run_until_parked();
                if visual.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    !window.has_active_dialog(cx)
                        && shell
                            .read(cx)
                            .live
                            .as_ref()
                            .unwrap()
                            .view
                            .snapshot
                            .as_ref()
                            .is_some_and(|snapshot| {
                                snapshot
                                    .projects
                                    .iter()
                                    .any(|project| project.path == path.to_str().unwrap())
                            })
                }) {
                    break;
                }
                assert!(Instant::now() < deadline, "folder registration deadline");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                shell.read_with(visual, |shell, _| shell.page),
                Page::Project
            );
            let requests = fixture.transport.requests.lock().unwrap();
            let creations: Vec<_> = requests
                .iter()
                .filter(|request| {
                    matches!(request.command, sailry_protocol::Command::CreateProject(_))
                })
                .collect();
            assert_eq!(
                creations.len(),
                if remote { 2 } else { 1 },
                "attempt {attempt}"
            );
            if remote {
                assert_eq!(creations[0], creations[1]);
            }
        }
        assert_eq!(
            std::fs::read(path.join("draft.txt")).unwrap(),
            b"Keep this draft\n"
        );
        assert!(!path.join(".git").exists());
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn host_operations(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(root.join("node")))
        .unwrap();
    let controller = runtime
        .block_on(sailry_link::Link::controller(
            root.join("controller"),
            sailry_link::NetworkScope::default(),
        ))
        .unwrap();
    let address = runtime
        .block_on(
            controller
                .handle()
                .pair(node.link().invite().unwrap().ticket()),
        )
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(crate::backend::Services {
            runtime: runtime.clone(),
            link: node.link(),
            local: node.local(),
            relay_enabled: false,
        });
    });
    for (index, transport) in [node.local(), controller.handle().remote(address)]
        .into_iter()
        .enumerate()
    {
        let path = root.join(format!("files-{index}"));
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("destination")).unwrap();
        for name in ["first.txt", "second.txt"] {
            std::fs::write(path.join(name), name).unwrap();
        }
        let mut shell = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let entity = cx.new(|cx| Shell::new(window, cx));
            shell = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let shell = shell.unwrap();
        let (editor, picker) = visual.update(|window, cx| {
            let editor = shell
                .update(cx, |shell, cx| shell.project_editor(0, None, window, cx))
                .unwrap();
            editor.update(cx, |editor, cx| {
                editor.live = Some(transport);
                editor.inputs[1].update(cx, |input, cx| {
                    input.set_value(path.to_str().unwrap(), window, cx)
                });
            });
            let picker = open(editor.clone(), window, cx);
            (editor, picker)
        });
        wait(visual, &picker);
        visual.simulate_resize(size(px(760.), px(560.)));
        wait(visual, &picker);
        let locations = visual.debug_bounds("directory-locations").unwrap();
        let grid = visual.debug_bounds("directory-list").unwrap();
        assert_eq!(locations.top(), grid.top());
        assert_eq!(locations.bottom(), grid.bottom());
        assert_eq!(
            picker.read_with(visual, |picker, _| picker.entries().len()),
            3
        );
        click(visual, "directory-row-1");
        let second = visual.debug_bounds("directory-row-2").unwrap().center();
        visual.simulate_click(
            second,
            Modifiers {
                platform: true,
                ..Default::default()
            },
        );
        assert_eq!(
            picker.read_with(visual, |picker, _| picker.selected.len()),
            2
        );
        visual.simulate_keystrokes("secondary-x");
        visual.update(|window, cx| {
            picker.update(cx, |picker, cx| {
                picker.navigate(
                    Some(path.join("destination").to_str().unwrap().into()),
                    Visit::Push,
                    window,
                    cx,
                )
            })
        });
        wait(visual, &picker);
        visual.simulate_keystrokes("secondary-v");
        wait(visual, &picker);
        assert!(!path.join("first.txt").exists());
        assert!(path.join("destination/first.txt").exists());
        assert!(picker.read_with(visual, |picker, _| picker.clipboard.is_none()));
        click(visual, "directory-row-0");
        visual.update(|window, cx| {
            window.dispatch_action(Box::new(actions::Dispatch(actions::Operation::Rename)), cx)
        });
        visual.run_until_parked();
        click(visual, "directory-name");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("renamed.txt");
        visual.simulate_keystrokes("enter");
        wait(visual, &picker);
        assert!(path.join("destination/renamed.txt").exists());
        visual.update(|window, cx| {
            window.dispatch_action(Box::new(actions::Dispatch(actions::Operation::Create)), cx)
        });
        visual.run_until_parked();
        click(visual, "directory-name");
        visual.simulate_input("new-folder");
        visual.simulate_keystrokes("enter");
        wait(visual, &picker);
        assert!(path.join("destination/new-folder").is_dir());
        click(visual, "directory-row-0");
        visual.update(|window, cx| {
            window.dispatch_action(Box::new(actions::Dispatch(actions::Operation::Trash)), cx)
        });
        visual.run_until_parked();
        assert!(visual.has_pending_prompt());
        crate::prompts::tests::answer(visual, "files_trash");
        wait(visual, &picker);
        assert!(!path.join("destination/new-folder").exists());
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.inputs[0].update(cx, |input, cx| {
                    input.set_value(if index == 0 { "" } else { "Custom" }, window, cx)
                });
            });
        });
        click(visual, "directory-confirm");
        assert_eq!(
            editor.read_with(visual, |editor, cx| editor.inputs[1].read(cx).value()),
            path.join("destination").to_str().unwrap()
        );
        assert_eq!(
            editor.read_with(visual, |editor, cx| editor.inputs[0].read(cx).value()),
            if index == 0 { "destination" } else { "Custom" }
        );
        visual.update(|window, _| window.remove_window());
    }
    runtime.block_on(controller.close()).unwrap();
    runtime.block_on(node.shutdown()).unwrap();
}
