use super::*;
use crate::panes::Target;
use gpui_kit::component::dock::{DockPlacement, DropTarget};

#[gpui::test]
fn header_follows_active_split(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1800.), px(900.)));
    let a = open(&shell, visual, &fixture, 0, &fixture.sessions[0]);
    let chat_a = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    let b = open(&shell, visual, &fixture, 1, &fixture.sessions[1]);
    let chat_b = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    let targets = [
        Target::Session(fixture.nodes[0].id(), fixture.sessions[0].id),
        Target::Session(fixture.nodes[1].id(), fixture.sessions[1].id),
    ];
    visual.update(|window, cx| {
        let splits = shell.read(cx).splits.clone();
        splits.update(cx, |splits, cx| {
            let workspace = splits.workspace(targets[1], cx).unwrap();
            let area = splits.active_area(cx).unwrap();
            let tree = area.read(cx).layout(DockPlacement::Center).unwrap();
            let node = tree.find_panel_node(tree.panels().next().unwrap()).unwrap();
            assert!(splits.drop(
                workspace,
                targets[0],
                DropTarget::new(node, Some(gpui_kit::base::Placement::Left)),
                window,
                cx,
            ));
        });
    });
    wait(visual, |cx| {
        let splits = shell.read(cx).splits.read(cx);
        splits.workspace(targets[0], cx) == splits.workspace(targets[1], cx)
    });
    fn focus(shell: &Entity<Shell>, visual: &mut VisualTestContext, target: Target, key: Key) {
        let selector = Box::leak(format!("pane-{target:?}").into_boxed_str());
        let pane = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(pane.origin + point(px(8.), px(8.)), Modifiers::default());
        wait(visual, |cx| shell.read(cx).session_scope.active == key);
        shell.read_with(visual, |shell, cx| {
            assert_eq!(shell.splits.read(cx).active, Some(target));
            assert_eq!(shell.session_scope.mounted, Some(key));
        });
        let old_control = Box::leak(format!("pane-details-{target:?}").into_boxed_str());
        assert!(visual.debug_bounds(old_control).is_none());
        let button = visual.debug_bounds("toggle-details").unwrap();
        let header = visual.debug_bounds("shell-module-header").unwrap();
        assert!(button.top() >= header.top() && button.bottom() <= header.bottom());
        assert!(button.right() <= header.right());
        assert!(header.right() - button.right() <= px(16.));
    }
    let mut panels = Vec::new();
    for (target, key, chat) in [(targets[0], a, &chat_a), (targets[1], b, &chat_b)] {
        focus(&shell, visual, target, key);
        assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
        click(&shell, visual, "toggle-details".into());
        let panel = shell.read_with(visual, |shell, cx| {
            let Some(SideResource::Launcher(Some(panel))) = &shell.side_resource else {
                panic!("launcher expected");
            };
            assert_eq!(panel.read(cx).source.as_ref(), Some(chat));
            assert!(shell.layout.panel_open[0]);
            panel.entity_id()
        });
        panels.push(panel);
    }
    assert_ne!(panels[0], panels[1]);
    focus(&shell, visual, targets[0], a);
    assert!(shell.read_with(visual, |shell, _| {
        matches!(&shell.side_resource, Some(SideResource::Launcher(Some(panel)))
            if panel.entity_id() == panels[0])
    }));
    click(&shell, visual, "toggle-details".into());
    assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
    focus(&shell, visual, targets[1], b);
    assert!(shell.read_with(visual, |shell, _| {
        shell.layout.panel_open[0]
            && matches!(&shell.side_resource, Some(SideResource::Launcher(Some(panel)))
                if panel.entity_id() == panels[1])
    }));
    focus(&shell, visual, targets[0], a);
    assert!(shell.read_with(visual, |shell, _| {
        !shell.layout.panel_open[0] && shell.side_resource.is_none()
    }));
    fixture.close();
}

#[gpui::test]
fn retains_file_preview_per_session(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let mut panels = Vec::new();
    let mut keys = Vec::new();
    for index in 0..2 {
        let session = &fixture.sessions[index];
        keys.push(open(&shell, visual, &fixture, index, session));
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        let file = sailry_protocol::tool::File {
            path: "report.docx".into(),
            name: None,
            mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into(),
            size: None,
            revision: None,
        };
        visual.update(|_, cx| {
            source.update(cx, |_, cx| {
                cx.emit(crate::conversation::live::Event::Artifact(
                    session.worktree,
                    file.clone(),
                ))
            })
        });
        wait(visual, |cx| {
            matches!(
                shell.read(cx).side_resource,
                Some(SideResource::Artifact(_))
            )
        });
        panels.push(shell.read_with(visual, |shell, cx| {
            let Some(SideResource::Artifact(panel)) = &shell.side_resource else {
                panic!("preview expected");
            };
            assert_eq!(panel.read(cx).source, source);
            panel.entity_id()
        }));
        let panel = visual.debug_bounds("artifact-panel").unwrap();
        visual.update(|window, cx| {
            use gpui_kit::component::notification::{Notification, NotificationDelivery};
            window.push_notification(
                Notification::new()
                    .delivery(NotificationDelivery::InApp)
                    .placement(Anchor::TopCenter)
                    .autohide(false)
                    .content(|_, _, _| {
                        div()
                            .debug_selector(|| "artifact-notice".into())
                            .child("Preview fixture")
                            .into_any_element()
                    }),
                cx,
            );
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let notice = visual.debug_bounds("artifact-notice").unwrap();
        assert!(
            notice.right() <= panel.left(),
            "notifications stay beside the native preview"
        );
        visual.update(|window, cx| window.clear_notifications(cx));
    }
    for (key, expected) in keys.into_iter().zip(panels) {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.activate_session(key, window, cx))
        });
        wait(
            visual,
            |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Artifact(panel)) if panel.entity_id() == expected),
        );
    }
    fixture.close();
}
