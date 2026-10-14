use super::*;
use sailry_client::activity::{Id, Notice};
use sailry_protocol::{ErrorCode, Fault};

#[gpui::test]
fn toasts_show_the_reason_and_keep_details(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("zh-CN");
    let fixture = fixture::Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
        crate::preferences::update(cx, |data| {
            data.notifications[0] = true;
            data.notifications[3] = true;
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    let error = Fault::new(ErrorCode::Unavailable, "context summary timed out");
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let notice = Notice {
            id: Id {
                node,
                cursor: 1000,
                target: Target::Session(fixture.sessions[index].id),
            },
            project: fixture.sessions[index].project,
            worktree: Some(fixture.sessions[index].worktree),
            title: "Original conversation title".into(),
            message: error.message.clone(),
            error: Some(error.clone()),
            presentation: None,
            kind: Kind::Failed,
            read: false,
        };
        let background = visual.update(|_, cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                window.activate_window();
                cx.new(|_| delivery::Background)
            })
            .unwrap()
        });
        visual.run_until_parked();
        let delivered = visual.delivered_system_notifications().len();
        visual.update(|window, cx| {
            assert!(!window.is_window_active());
            shell.update(cx, |shell, cx| {
                shell.receive_notices(node, std::slice::from_ref(&notice), window, cx)
            });
        });
        assert_eq!(visual.delivered_system_notifications().len(), delivered + 1);
        assert_eq!(
            visual
                .delivered_system_notifications()
                .last()
                .unwrap()
                .body
                .as_ref(),
            tr("chat_compact_timeout").as_ref()
        );
        background
            .update(visual, |_, window, _| window.remove_window())
            .unwrap();

        let mut active = notice.clone();
        active.id.cursor += 1;
        visual.update(|window, _| window.activate_window());
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.is_window_active());
            window.clear_notifications(cx);
            shell.update(cx, |shell, cx| {
                shell.receive_notices(node, std::slice::from_ref(&active), window, cx)
            });
        });
        crate::feedback::tests::settle(visual);
        let presented = visual
            .update(|window, cx| crate::feedback::tests::count(window, &error.to_string(), cx));
        visual.update(|window, cx| {
            let action = window.find("notification-details");
            assert_eq!(action.role(), Some(Role::Button));
            assert!(action.visible());
            window.click("notification-details", cx);
            assert_eq!(
                crate::feedback::tests::count(window, &error.to_string(), cx),
                presented + 1,
            );
        });
        crate::feedback::tests::settle(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary).as_ref(),
            error.to_string()
        );
        assert!(shell.read_with(visual, |shell, _| {
            shell
                .activity
                .inbox
                .notices()
                .iter()
                .any(|retained| retained.id == notice.id && retained.error.as_ref() == Some(&error))
        }));
    }
    drop(shell);
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
