use super::*;

#[gpui::test]
fn entry_point_selection(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            local: local.local(),
            link: local.link(),
            relay_enabled: false,
        });
    });
    for node in [&local, &remote] {
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            entity = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = entity.unwrap();
        wait(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|view| view.read(cx).connected())
                && shell
                    .read(cx)
                    .live
                    .as_ref()
                    .unwrap()
                    .hosts
                    .contains_key(&node.id())
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let draft = shell.current_chat().unwrap().clone();
                shell.select_draft_host(&draft, node.id(), window, cx);
            })
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|view| {
                view.read(cx).connected() && view.read(cx).binding().client.target() == node.id()
            })
        });
        assert!(shell.read_with(visual, |shell, cx| {
            shell
                .current_chat()
                .unwrap()
                .read(cx)
                .binding()
                .project
                .is_none()
        }));
        click(visual, "project-add");
        assert!(visual.debug_bounds("project-editor").is_some());
        click(visual, "project-cancel");
        let client = Client::new(node.local());
        for index in 0..2 {
            let path = directory
                .path()
                .join(format!("{}-{index}", short_id(node.id())));
            std::fs::create_dir(&path).unwrap();
            runtime
                .block_on(client.execute(client.prepare(Command::RegisterProject {
                    name: format!("Project {index}"),
                    path: path.to_str().unwrap().into(),
                })))
                .unwrap();
        }
        wait(visual, |cx| {
            let shell = shell.read(cx);
            let Some(snapshot) = &shell.live.as_ref().unwrap().view.snapshot else {
                return false;
            };
            snapshot.projects.len() == 2
                && shell.current_chat().is_some_and(|view| {
                    view.read(cx).connected() && view.read(cx).binding().project.is_none()
                })
        });
        let (first, second) = shell.read_with(visual, |shell, _| {
            let projects = &shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .projects;
            assert!(shell.live.as_ref().unwrap().project.is_none());
            (projects[0].id, projects[1].id)
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this draft 中文");
        let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_project(second, window, cx);
            })
        });
        assert!(shell.read_with(visual, |shell, _| {
            shell.session_scope.panels.contains_key(&Key::Draft)
        }));
        click(visual, "live-new-conversation");
        wait(visual, |cx| {
            draft.read(cx).connected() && draft.read(cx).binding().project == Some(second)
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone()),
            draft
        );
        assert_eq!(
            draft.read_with(visual, |view, cx| view.draft(cx)),
            "Keep this draft 中文"
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &crate::live::menus::Dispatch {
                        node: node.id(),
                        target: crate::live::menus::Target::Project(first),
                        command: crate::live::menus::Command::NewSession,
                    },
                    window,
                    cx,
                );
            })
        });
        wait(visual, |cx| {
            draft.read(cx).connected() && draft.read(cx).binding().project == Some(first)
        });
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(snapshot.sessions.is_empty());
        visual.update(|window, _| window.remove_window());
    }
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
