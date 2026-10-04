use super::*;
use crate::{activity::fixture::Fixture, live::file_tests::settle};

#[gpui::test]
fn opens_before_session_creation(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = Fixture::new();
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    wait(visual, &shell, fixture.nodes[0].id());
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        visual.update(|window, cx| {
            window.dispatch_action(
                Box::new(Dispatch {
                    node,
                    target: Target::Host,
                    command: Command::Open,
                }),
                cx,
            );
        });
        wait(visual, &shell, node);
        let client = shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().client());
        let path = directory.path().join(format!("project-{index}"));
        std::fs::create_dir(&path).unwrap();
        let Output::Project(project) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Request::RegisterProject {
                name: "Unopened project".into(),
                path: path.to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        settle(visual, &shell, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.projects.iter().any(|entry| entry.id == project.id)
                })
        });
        assert!(shell.read_with(visual, |shell, _| {
            let live = shell.live.as_ref().unwrap();
            let snapshot = live.view.snapshot.as_ref().unwrap();
            let trees: Vec<_> = snapshot
                .worktrees
                .iter()
                .filter(|tree| tree.project == Some(project.id))
                .collect();
            live.project != Some(project.id)
                && live.selected_worktree().is_none()
                && trees.len() == 1
                && trees[0].main
                && trees[0].path == project.path
                && snapshot
                    .sessions
                    .iter()
                    .all(|session| session.project != Some(project.id))
        }));
        let selector = Box::leak(format!("live-project-{}", project.id).into_boxed_str());
        let position = visual.debug_bounds(selector).unwrap().center();
        visual.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        visual.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            if visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                project::opened(window, cx, node, project.id)
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "project native menu deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        let actions = visual.update(project::captured);
        assert_eq!(
            actions
                .iter()
                .map(|action| action.command)
                .collect::<Vec<_>>(),
            items(Target::Project(project.id), index == 0)
                .into_iter()
                .map(|(command, _)| command)
                .collect::<Vec<_>>()
        );
        assert!(
            actions.iter().all(|action| {
                action.node == node && action.target == Target::Project(project.id)
            })
        );
        visual.update(|window, cx| project::choose(Command::Copy, window, cx));
        visual.run_until_parked();
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            project.path
        );
        assert!(visual.debug_bounds("popup-menu").is_none());

        // The OS dispatch must retain its original Node after a controller selection changes.
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_host(fixture.nodes[1 - index].id(), window, cx)
            });
            cx.write_to_clipboard(ClipboardItem::new_string("Unchanged".into()));
            project::choose(Command::Copy, window, cx);
        });
        visual.run_until_parked();
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Unchanged"
        );
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
