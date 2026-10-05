use super::*;
use crate::activity::{Feed, Inbox, Kind, Notice, Target};
use sailry_protocol::{NotificationId, notification};

fn notice(sequence: u64, read: bool) -> notification::Notice {
    notification::Notice {
        id: NotificationId::new(),
        sequence,
        package: "example".into(),
        content: notification::Draft {
            title: "Reminder".into(),
            message: "Check the task".into(),
            kind: notification::Kind::Info,
            session: None,
        },
        timestamp_ms: 1000,
        read,
    }
}

#[test]
fn snapshot_recovers_unread() {
    let mut snapshot = snapshot(900);
    snapshot.notifications = vec![notice(2, false), notice(1, true)];
    let mut feed = Feed::default();
    let mut inbox = Inbox::default();
    feed.observe(&snapshot);
    assert_eq!(inbox.merge(snapshot.node, &feed.notices).len(), 1);
    assert_eq!(inbox.notices().len(), 2);
    assert_eq!(inbox.unread(), 1);
    feed.observe(&snapshot);
    assert!(inbox.merge(snapshot.node, &feed.notices).is_empty());
}

#[test]
fn read_and_dismiss_updates_reconcile_existing_entries() {
    let snapshot = snapshot(0);
    let node = snapshot.node;
    let mut projection = Projection::new(node, 1);
    projection.apply(1, Update::Snapshot(snapshot)).unwrap();
    let mut entry = notice(1, false);
    let mut feed = Feed::default();
    let mut inbox = Inbox::default();
    for (cursor, read) in [(1, false), (2, true)] {
        entry.read = read;
        projection
            .apply(
                1,
                Update::Event(EventEnvelope {
                    node,
                    cursor,
                    event: Event::NotificationChanged(entry.clone()),
                }),
            )
            .unwrap();
        feed.observe(projection.snapshot().unwrap());
        assert_eq!(inbox.merge(node, &feed.notices).len(), usize::from(!read));
        assert_eq!(inbox.unread(), usize::from(!read));
    }
    projection
        .apply(
            1,
            Update::Event(EventEnvelope {
                node,
                cursor: 3,
                event: Event::NotificationsDismissed {
                    ids: vec![entry.id],
                },
            }),
        )
        .unwrap();
    feed.observe(projection.snapshot().unwrap());
    assert!(inbox.merge(node, &feed.notices).is_empty());
    assert!(inbox.notices().is_empty());
}

#[test]
fn plugin_and_activity_sequences_remain_independent() {
    let mut snapshot = snapshot(900);
    snapshot.notifications = vec![notice(1, false)];
    let mut feed = Feed::default();
    let mut inbox = Inbox::default();
    let activity = Notice {
        id: crate::activity::Id {
            node: snapshot.node,
            cursor: 800,
            target: Target::Session(sailry_protocol::SessionId::new()),
        },
        project: None,
        worktree: None,
        title: "Task".into(),
        message: String::new(),
        kind: Kind::Completed,
        read: false,
    };
    assert_eq!(
        inbox
            .merge(snapshot.node, std::slice::from_ref(&activity))
            .len(),
        1
    );
    feed.observe(&snapshot);
    feed.notices.push(activity);
    assert_eq!(inbox.merge(snapshot.node, &feed.notices).len(), 1);
    assert_eq!(inbox.unread(), 2);
    inbox.clear_local();
    assert_eq!(inbox.notices().len(), 1);
    assert!(matches!(
        inbox.notices()[0].id.target,
        Target::Notification(_)
    ));
}
