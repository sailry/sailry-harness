use super::*;
use core::prelude::v1::test;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn wait(cx: &mut VisualTestContext, editor: &Entity<Editor>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            let _ = window.draw(cx);
            !editor.read(cx).pending
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "project update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn picker_geometry(cx: &mut VisualTestContext, selected: &str) {
    use sailry_protocol::projects::ICONS;

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let colors = cx.debug_bounds("project-colors").unwrap();
    let icons = cx.debug_bounds("project-icon-row-0").unwrap();
    assert_eq!(colors.left(), icons.left());
    assert_eq!(colors.right(), icons.right());
    for (row, icons_in_row) in ICONS.chunks(6).enumerate() {
        let bounds: Vec<_> = icons_in_row
            .iter()
            .map(|icon| {
                cx.debug_bounds(Box::leak(format!("project-icon-{icon}").into_boxed_str()))
                    .unwrap()
            })
            .collect();
        assert!(bounds.iter().all(|b| b.top() == bounds[0].top()));
        assert!(bounds.windows(2).all(|b| b[0].right() < b[1].left()));
        assert_eq!(bounds[0].left(), icons.left());
        assert_eq!(bounds[5].right(), icons.right());
        if row > 0 {
            assert!(bounds[0].top() > icons.bottom());
        }
    }
    let selected = cx
        .debug_bounds(Box::leak(
            format!("project-color-{selected}").into_boxed_str(),
        ))
        .unwrap();
    let check = cx.debug_bounds("project-color-check").unwrap();
    assert_eq!(check.center(), selected.center());
    assert_eq!(check.size.width, px(10.));
    assert_eq!(check.size.height, px(10.));
    assert!(check.left() > selected.left());
    assert!(check.right() < selected.right());
}

#[gpui::test]
fn request_recovery(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(root.join("node")))
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
    let mut shell = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| Shell::new(window, cx));
        shell = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let editor = visual.update(|window, cx| {
        let editor = shell
            .unwrap()
            .update(cx, |shell, cx| shell.project_editor(0, None, window, cx))
            .unwrap();
        editor.update(cx, |editor, cx| {
            editor.live = Some(node.local());
            editor.inputs[0].update(cx, |input, cx| input.set_value("Clone", window, cx));
            editor.inputs[1].update(cx, |input, cx| {
                input.set_value(root.join("missing").to_str().unwrap(), window, cx)
            });
            editor.save(window, cx);
        });
        editor
    });
    wait(visual, &editor);
    assert!(editor.read_with(visual, |editor, _| editor.request.is_none()));
    let host = visual
        .debug_bounds("project-host")
        .expect("single-host selector");
    visual.simulate_click(host.center(), Modifiers::default());
    visual.run_until_parked();
    visual.simulate_keystrokes("down enter");
    assert!(editor.read_with(visual, |editor, cx| {
        editor.inputs[1].read(cx).value().ends_with("missing")
    }));
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_source(true, window, cx);
            editor.inputs[1].update(cx, |input, cx| {
                input.set_value(root.to_str().unwrap(), window, cx)
            });
            editor.repository.update(cx, |input, cx| {
                input.set_value(
                    root.join("missing-repository").to_str().unwrap(),
                    window,
                    cx,
                )
            });
            editor.save(window, cx);
        })
    });
    wait(visual, &editor);
    assert!(root.join("Clone").is_dir());
    let request = editor.read_with(visual, |editor, _| {
        assert_eq!(editor.error, Some("project_outcome_unknown"));
        editor.request.clone().unwrap()
    });
    visual.update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)));
    wait(visual, &editor);
    assert_eq!(
        editor.read_with(visual, |editor, _| editor.request.clone()),
        Some(request.clone())
    );
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.inputs[0].update(cx, |input, cx| input.set_value("Changed", window, cx));
            editor.save(window, cx);
            assert!(!editor.pending);
            assert_eq!(editor.request, Some(request.clone()));
        })
    });
    assert!(!root.join("Changed").exists());
    visual.update(|window, _| window.remove_window());
    runtime.block_on(node.shutdown()).unwrap();
}

#[gpui::test]
fn creates_on_the_chosen_host(cx: &mut TestAppContext) {
    use sailry_client::Client;
    use sailry_protocol::{Command, Output};
    cx.executor().allow_parking();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(sailry_node_runtime::Node::start(root.join("local")))
        .unwrap();
    let remote = runtime
        .block_on(sailry_node_runtime::Node::start(root.join("remote")))
        .unwrap();
    runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(crate::backend::Services {
            runtime: runtime.clone(),
            link: local.link(),
            local: local.local(),
            relay_enabled: false,
        });
    });
    let mut shell = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| Shell::new(window, cx));
        shell = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let shell = shell.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !shell.read_with(visual, |shell, _| {
        shell
            .live
            .as_ref()
            .unwrap()
            .hosts
            .contains_key(&remote.id())
    }) {
        assert!(Instant::now() < deadline, "host discovery deadline");
        visual.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    for (index, node) in [&remote, &local].into_iter().enumerate() {
        let path = root.join(format!("project-{index}"));
        std::fs::create_dir(&path).unwrap();
        let editor = visual.update(|window, cx| {
            let editor = shell
                .update(cx, |shell, cx| shell.project_editor(0, None, window, cx))
                .unwrap();
            editor.update(cx, |editor, cx| {
                editor.inputs[0].update(cx, |input, cx| input.set_value("Chosen host", window, cx));
                editor.inputs[1].update(cx, |input, cx| {
                    input.set_value("old/local/path", window, cx)
                });
                editor.other_path = "old/clone/path".into();
                editor.repository.update(cx, |input, cx| {
                    input.set_value("https://example.test/repo.git", window, cx)
                });
                editor
                    .branch
                    .update(cx, |input, cx| input.set_value("main", window, cx));
                // Exercise both directions independently of the sidebar selection.
                let opposite = if node.id() == local.id() {
                    remote.id()
                } else {
                    local.id()
                };
                editor.select_host(super::host::Host::Node(opposite), window, cx);
                editor.select_host(super::host::Host::Node(node.id()), window, cx);
                assert!(editor.inputs[1].read(cx).value().is_empty());
                assert!(editor.other_path.is_empty());
                assert_eq!(editor.inputs[0].read(cx).value(), "Chosen host");
                assert_eq!(
                    editor.repository.read(cx).value(),
                    "https://example.test/repo.git"
                );
                assert_eq!(editor.branch.read(cx).value(), "main");
                editor.inputs[1].update(cx, |input, cx| {
                    input.set_value(path.to_str().unwrap(), window, cx)
                });
            });
            editor
        });
        visual.run_until_parked();
        let picker = visual.debug_bounds("project-appearance").unwrap();
        visual.simulate_click(picker.center(), Modifiers::default());
        visual.run_until_parked();
        picker_geometry(visual, "none");
        let color = visual.debug_bounds("project-color-teal").unwrap();
        visual.simulate_click(color.center(), Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.appearance.color.clone()),
            "teal"
        );
        picker_geometry(visual, "teal");
        let style = visual.debug_bounds("project-icon-media").unwrap();
        visual.simulate_click(style.center(), Modifiers::default());
        visual.run_until_parked();
        let appearance = sailry_protocol::projects::Appearance {
            icon: "media".into(),
            color: "teal".into(),
        };
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.appearance.clone()),
            appearance
        );
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        let save = visual.debug_bounds("project-save").unwrap();
        visual.simulate_click(save.center(), Modifiers::default());
        wait(visual, &editor);
        assert!(editor.read_with(visual, |editor, _| editor.error.is_none()));
        let client = Client::new(node.local());
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.projects.len(), 1);
        assert_eq!(snapshot.projects[0].path, path.to_str().unwrap());
        assert_eq!(snapshot.projects[0].appearance, appearance);
        let reopened = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .open_project_editor(0, None, Some(snapshot.projects[0].clone()), window, cx)
                    .unwrap()
            })
        });
        assert_eq!(
            reopened.read_with(visual, |editor, _| editor.appearance.clone()),
            appearance
        );
        visual.update(|window, cx| window.close_dialog(cx));
    }
    visual.update(|window, _| window.remove_window());
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
