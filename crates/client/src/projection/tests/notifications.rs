use super::*;
use crate::activity::{Feed, Inbox, Kind, Notice, Target};
use sailry_protocol::{
    Effort, NotificationId, Permission, ProviderId, SessionConfig, SessionId, TurnId, WorkMode,
    WorktreeId, notification,
};

#[test]
fn retains_canonical_failure_details() {
    use sailry_protocol::conversation::{Run, RunKind, Status};
    let mut state = snapshot(0);
    let session = sailry_protocol::Session {
        id: SessionId::new(),
        archived: false,
        activity: Default::default(),
        project: None,
        worktree: WorktreeId::new(),
        revision: 1,
        config: SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            credential: None,
        },
        roles: Default::default(),
        profile: None,
        fork: None,
        delegation: None,
    };
    state.sessions.push(session);
    let session = &mut state.sessions[0];
    session.activity.title = "Original conversation title".into();
    session.activity.run = Some(Run {
        worktree: session.worktree,
        turn: TurnId::new(),
        kind: RunKind::Compaction,
        session: session.id,
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        started_ms: Some(1000),
        finished_ms: None,
        origin: None,
        continuation: None,
        input: None,
    });
    let mut feed = Feed::default();
    feed.observe(&state);
    assert!(feed.notices.is_empty());

    let fault = Fault::new(ErrorCode::Unavailable, "context summary timed out");
    let run = state.sessions[0].activity.run.as_mut().unwrap();
    run.status = Status::Failed;
    run.error = Some(fault.clone());
    run.finished_ms = Some(61000);
    state.cursor = 1;
    feed.observe(&state);
    assert_eq!(feed.notices.len(), 1);
    let notice = feed.notices[0].clone();
    assert_eq!(notice.kind, Kind::Failed);
    assert_eq!(notice.message, fault.message);
    assert_eq!(notice.error.as_ref(), Some(&fault));
    assert_eq!(notice.title, "Original conversation title");

    let mut inbox = Inbox::default();
    assert_eq!(inbox.merge(state.node, &feed.notices).len(), 1);
    feed.observe(&state);
    assert!(inbox.merge(state.node, &feed.notices).is_empty());
    let run = state.sessions[0].activity.run.as_mut().unwrap();
    run.turn = TurnId::new();
    run.sequence = 2;
    run.status = Status::Completed;
    run.error = None;
    state.cursor = 2;
    feed.observe(&state);
    inbox.merge(state.node, &feed.notices);
    assert!(
        inbox.notices().iter().any(|retained| {
            retained.id == notice.id && retained.error.as_ref() == Some(&fault)
        })
    );
}

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
            presentation: None,
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
        error: None,
        presentation: None,
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
