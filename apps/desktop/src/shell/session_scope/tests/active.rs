use super::*;
use sailry_client::activity::{Lane, lane};

fn selector(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[gpui::test]
fn header_and_group_share_local_and_remote_attention(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let session = &fixture.sessions[index];
        open(&shell, visual, &fixture, index, session);
        fixture.submit(index);
        wait(visual, |cx| {
            shell
                .read(cx)
                .active_sessions()
                .iter()
                .any(|(n, s)| *n == node && s.id == session.id && lane(s) == Lane::Waiting)
        });
        let row = selector(format!("header-session-{node:?}-{}", session.id));
        let group_row = selector(format!("active-session-{}", session.id));
        let project_row = selector(format!("live-session-{}", session.id));
        assert!(
            visual.debug_bounds(project_row).is_some(),
            "activity retains the project entry"
        );
        assert!(
            visual.debug_bounds("active-toggle").unwrap().bottom()
                < visual.debug_bounds("projects-label").unwrap().top()
        );
        assert!(visual.debug_bounds(group_row).is_some());
        let branch = sidebar::hover(visual, group_row);
        let title = visual
            .debug_bounds(selector(format!("{group_row}-label")))
            .unwrap();
        assert!(branch.left() > title.right());
        assert!((branch.center().y - title.center().y).abs() < px(1.));
        assert!(
            visual
                .debug_bounds(selector(format!("active-session-{}-waiting", session.id)))
                .is_some()
        );
        click(&shell, visual, "active-toggle".into());
        assert!(visual.debug_bounds(group_row).is_none());
        click(&shell, visual, "active-toggle".into());
        assert!(visual.debug_bounds(group_row).is_some());
        click(&shell, visual, group_row.into());
        wait(visual, |cx| {
            shell
                .read(cx)
                .extensions
                .as_ref()
                .is_some_and(|state| state.metadata.read(cx).settled())
                && shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.node == node && entry.package.name == "progress")
        });
        let activity = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.node == node && entry.package.name == "progress")
                .unwrap()
        });
        click(&shell, visual, activity.selector());
        wait(visual, |cx| {
            shell.read(cx).extensions.as_ref().is_some_and(|state| {
                state.selected.as_ref() == Some(&activity)
                    && state.panel.as_ref().is_some_and(|panel| {
                        panel.read(cx).resource_active()
                            && crate::plugins::diagnostics(panel, cx).contains("activity-board")
                    })
            })
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.page),
            crate::preview::Page::Plugin
        );
        assert!(visual.debug_bounds("shell-navigation").is_none());
        assert!(visual.debug_bounds(group_row).is_none());
        click(&shell, visual, row.into());
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.page),
            crate::preview::Page::Conversation
        );
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(node, session.id)
        );
        assert!(visual.debug_bounds("shell-activity-slot").is_none());
        assert!(visual.debug_bounds("activity-open").is_none());
        assert!(visual.debug_bounds(row).is_some());
        click(&shell, visual, "recent-toggle".into());
        assert!(
            visual
                .debug_bounds(selector(format!("recent-session-{}", session.id)))
                .is_some()
        );
        for suffix in ["waiting", "loading", "unread"] {
            assert!(
                visual
                    .debug_bounds(selector(format!("recent-session-{}-{suffix}", session.id)))
                    .is_none()
            );
        }
        click(&shell, visual, "recent-toggle".into());
        open_background_session(&shell, visual, &fixture, index);
        fixture.answer(index);
        wait(visual, |cx| {
            shell
                .read(cx)
                .active_sessions()
                .iter()
                .any(|(n, s)| *n == node && s.id == session.id && lane(s) == Lane::Completed)
        });
        assert!(
            visual
                .debug_bounds(selector(format!("live-session-{}-unread", session.id)))
                .is_some()
        );
        assert!(
            visual.debug_bounds(row).is_some(),
            "completion retains the header shortcut until it is read"
        );
        assert!(
            visual
                .debug_bounds(selector(format!("active-session-{}-unread", session.id)))
                .is_some()
        );
        // The header routes across nodes and acknowledges the shared attention revision.
        open(
            &shell,
            visual,
            &fixture,
            1 - index,
            &fixture.sessions[1 - index],
        );
        assert_ne!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(node, session.id)
        );
        click(&shell, visual, row.into());
        wait(visual, |cx| {
            !shell.read(cx).session_unread(node, session.id)
        });
        assert!(visual.debug_bounds(group_row).is_none());
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(node, session.id)
        );
        assert!(
            visual.debug_bounds(project_row).is_some(),
            "read completion only removes the activity shortcut"
        );
        assert!(visual.debug_bounds(row).is_none());
        assert!(visual.debug_bounds("activity-open").is_none());
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
