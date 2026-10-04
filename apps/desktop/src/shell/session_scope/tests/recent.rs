use super::*;

#[gpui::test]
fn navigation_is_scoped_and_collapsible(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("sidebar-recent-viewport").is_none());
    click(&shell, visual, "recent-toggle".into());
    for index in 0..2 {
        let original = &fixture.sessions[index];
        let node = fixture.nodes[index].id();
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        let Output::Session(second) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: original.project,
                worktree: Some(original.worktree),
                config: Some(original.config.clone()),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        open(&shell, visual, &fixture, index, original);
        open(&shell, visual, &fixture, index, &second);
        open(&shell, visual, &fixture, index, original);
        let first_row: &'static str =
            Box::leak(format!("recent-session-{}", original.id).into_boxed_str());
        let second_row: &'static str =
            Box::leak(format!("recent-session-{}", second.id).into_boxed_str());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds(first_row).unwrap().top()
                < visual.debug_bounds(second_row).unwrap().top()
        );
        sidebar::hover(visual, first_row);
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("recent-session-{}", fixture.sessions[1 - index].id).into_boxed_str()
                ))
                .is_none()
        );
        click(&shell, visual, second_row.into());
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(node, second.id)
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds(second_row).unwrap().top()
                < visual.debug_bounds(first_row).unwrap().top()
        );
        let expanded = visual.debug_bounds("sidebar-projects-viewport").unwrap();
        click(&shell, visual, "recent-toggle".into());
        assert!(visual.debug_bounds(second_row).is_none());
        assert!(
            (visual
                .debug_bounds("sidebar-projects-viewport")
                .unwrap()
                .size
                .height
                - expanded.size.height)
                .abs()
                <= px(1.),
            "short project content must not expand when Recent closes"
        );
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(node, second.id)
        );
        click(&shell, visual, "recent-toggle".into());
        assert!(visual.debug_bounds(second_row).is_some());
        click(&shell, visual, "projects-toggle".into());
        assert!(visual.debug_bounds("sidebar-projects-viewport").is_none());
        assert!(visual.debug_bounds(second_row).is_some());
        click(&shell, visual, "projects-toggle".into());
        // Hidden resources leave history intact but cannot be opened from Recent.
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .view
                    .snapshot
                    .as_mut()
                    .unwrap()
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == second.id)
                    .unwrap()
                    .archived = true;
                cx.notify();
            })
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(second_row).is_none());
    }
}
