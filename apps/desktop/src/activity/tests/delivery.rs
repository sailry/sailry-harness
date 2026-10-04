use super::*;
use sailry_client::Client;
use sailry_protocol::{
    Command, Output,
    notification::{Draft, Kind},
};

pub(super) fn snapshot(fixture: &fixture::Fixture, index: usize) -> sailry_protocol::Snapshot {
    let client = Client::new(fixture.nodes[index].local());
    let Output::Snapshot(snapshot) = fixture
        .runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
}

pub(super) fn publish(fixture: &fixture::Fixture, index: usize, session: bool) {
    let package = snapshot(fixture, index)
        .plugins
        .into_iter()
        .find(|plugin| plugin.name == "progress")
        .unwrap()
        .reference();
    let client = Client::new(fixture.nodes[index].local());
    fixture
        .runtime
        .block_on(client.execute(client.prepare(Command::PublishNotification {
            package,
            content: Draft {
                title: format!("Reminder {index}"),
                message: "Check the task".into(),
                kind: Kind::Info,
                session: session.then_some(fixture.sessions[index].id),
            },
        })))
        .unwrap();
}

#[gpui::test]
fn offline_notices_open_captured_hosts_and_clear_durably(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = fixture::Fixture::new();
    publish(&fixture, 0, false);
    publish(&fixture, 1, true);
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    wait(visual, |cx| shell.read(cx).activity.inbox.unread() == 2);
    visual.update(|window, cx| window.clear_notifications(cx));
    click(visual, "notifications-open");
    let index = shell.read_with(visual, |shell, _| {
        shell
            .activity
            .inbox
            .notices()
            .iter()
            .position(|notice| notice.id.node == fixture.nodes[1].id())
            .unwrap()
    });
    click(
        visual,
        Box::leak(format!("notification-{index}").into_boxed_str()),
    );
    wait(visual, |cx| {
        shell
            .read(cx)
            .current_chat()
            .is_some_and(|chat| chat.read(cx).session() == Some(fixture.sessions[1].id))
            && shell.read(cx).activity.inbox.unread() == 1
    });
    assert!(snapshot(&fixture, 1).notifications[0].read);
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
        fixture.nodes[1].id()
    );
    click(visual, "notifications-open");
    let index = shell.read_with(visual, |shell, _| {
        shell
            .activity
            .inbox
            .notices()
            .iter()
            .position(|notice| notice.id.node == fixture.nodes[0].id())
            .unwrap()
    });
    click(
        visual,
        Box::leak(format!("notification-{index}").into_boxed_str()),
    );
    wait(visual, |cx| {
        shell.read(cx).page == Page::Plugin
            && shell
                .read(cx)
                .extensions
                .as_ref()
                .and_then(|state| state.selected.as_ref())
                .is_some_and(|entry| {
                    entry.node == fixture.nodes[0].id() && entry.package.name == "progress"
                })
            && shell.read(cx).activity.inbox.unread() == 0
    });
    assert!(snapshot(&fixture, 0).notifications[0].read);
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
        fixture.nodes[0].id()
    );
    click(visual, "notifications-open");
    click(visual, "notifications-clear");
    wait(visual, |cx| {
        shell.read(cx).activity.inbox.notices().is_empty()
    });
    assert!(snapshot(&fixture, 0).notifications.is_empty());
    assert!(snapshot(&fixture, 1).notifications.is_empty());
    drop(shell);
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
