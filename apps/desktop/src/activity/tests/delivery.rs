use super::*;
use sailry_client::Client;
use sailry_protocol::{
    Command, Output,
    notification::{Draft, Kind},
};

pub(super) struct Background;
impl Render for Background {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

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
    publish_with(fixture, index, session, None);
}

fn publish_with(
    fixture: &fixture::Fixture,
    index: usize,
    session: bool,
    presentation: Option<sailry_protocol::notification::Presentation>,
) {
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
                presentation,
            },
        })))
        .unwrap();
}

#[gpui::test]
fn persistent_notices_open_the_publishing_panel(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("zh-CN");
    let fixture = fixture::Fixture::new();
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
    let background = visual.update(|_, cx| {
        cx.open_window(WindowOptions::default(), |window, cx| {
            window.activate_window();
            cx.new(|_| Background)
        })
        .unwrap()
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .activity
            .observers
            .values()
            .filter(|observer| observer.view.connected)
            .count()
            == 2
    });
    for index in 0..2 {
        background
            .update(visual, |_, window, _| window.activate_window())
            .unwrap();
        visual.update(|window, cx| window.clear_notifications(cx));
        let prior = visual.delivered_system_notifications().len();
        publish_with(
            &fixture,
            index,
            false,
            Some(sailry_protocol::notification::Presentation {
                heading: "Reminder".into(),
                locales: std::collections::BTreeMap::from([("zh-CN".into(), "提醒".into())]),
                persistent: true,
            }),
        );
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity
                .inbox
                .notices()
                .iter()
                .any(|notice| notice.id.node == fixture.nodes[index].id() && !notice.read)
        });
        assert_eq!(visual.delivered_system_notifications().len(), prior + 1);
        assert_eq!(
            visual
                .delivered_system_notifications()
                .last()
                .unwrap()
                .title
                .as_ref(),
            "提醒"
        );
        assert_eq!(
            visual
                .delivered_system_notifications()
                .last()
                .unwrap()
                .body
                .as_ref(),
            format!("Reminder {index}\nCheck the task")
        );
        assert_eq!(
            visual.update(|window, cx| window.notifications(cx).len()),
            1
        );
        visual.executor().advance_clock(Duration::from_secs(1000));
        visual.run_until_parked();
        visual.executor().advance_clock(Duration::from_millis(500));
        visual.run_until_parked();
        assert_eq!(
            visual.update(|window, cx| window.notifications(cx).len()),
            1
        );
        assert!(visual.debug_bounds("notification-open").is_none());
        click(visual, "notification-card");
        wait(visual, |cx| {
            shell.read(cx).page == Page::Plugin
                && shell
                    .read(cx)
                    .extensions
                    .as_ref()
                    .and_then(|state| state.selected.as_ref())
                    .is_some_and(|entry| {
                        entry.node == fixture.nodes[index].id() && entry.package.name == "progress"
                    })
                && shell
                    .read(cx)
                    .activity
                    .inbox
                    .notices()
                    .iter()
                    .filter(|notice| notice.id.node == fixture.nodes[index].id())
                    .all(|notice| notice.read)
        });
        assert!(snapshot(&fixture, index).notifications[0].read);
        // Kit keeps the dismissed toast mounted through its exit transition.
        visual.executor().advance_clock(Duration::from_millis(250));
        visual.run_until_parked();
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
    }
    background
        .update(visual, |_, window, _| window.remove_window())
        .unwrap();
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}

#[gpui::test]
fn offline_notices_keep_ownership_and_durable_reads(cx: &mut TestAppContext) {
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
