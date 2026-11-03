use super::*;

fn notice(node: NodeId, cursor: u64) -> Notice {
    Notice {
        id: Id {
            node,
            cursor,
            target: Target::Notification(NotificationId::new()),
        },
        project: None,
        worktree: None,
        title: "Fixture".into(),
        message: String::new(),
        error: None,
        presentation: None,
        kind: Kind::Notification(sailry_protocol::notification::Kind::Info),
        read: false,
        package: String::new(),
        session: None,
        timestamp_ms: Some(1000),
        details: None,
    }
}

#[test]
fn restores_history_without_replaying() {
    let node = NodeId([1; 32]);
    let entry = notice(node, 1);
    let mut inbox = Inbox::default();
    assert!(inbox.merge(node, std::slice::from_ref(&entry)).is_empty());
    assert_eq!(inbox.notices().len(), 1);
    assert!(inbox.merge(node, &[]).is_empty());
    assert!(inbox.notices().is_empty());
    assert!(inbox.merge(node, std::slice::from_ref(&entry)).is_empty());
    assert_eq!(inbox.notices().len(), 1);
    assert_eq!(inbox.notices()[0].id, entry.id);
    assert_eq!(inbox.unread(), 1);
    assert!(inbox.merge(node, std::slice::from_ref(&entry)).is_empty());
    assert_eq!(inbox.notices().len(), 1);
    let next = notice(node, 2);
    let added = inbox.merge(node, &[next.clone(), entry]);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].id, next.id);
    assert_eq!(inbox.notices().len(), 2);
}

#[test]
fn restart_restores_unread_without_replaying() {
    let node = NodeId([1; 32]);
    let unread = notice(node, 2);
    let mut read = notice(node, 1);
    read.read = true;
    let history = [unread, read];
    for _ in 0..2 {
        let mut inbox = Inbox::default();
        assert!(inbox.merge(node, &history).is_empty());
        assert_eq!(inbox.notices().len(), 2);
        assert_eq!(inbox.unread(), 1);
        assert!(inbox.merge(node, &history).is_empty());
        let next = notice(node, 3);
        let added = inbox.merge(
            node,
            &[next.clone(), history[0].clone(), history[1].clone()],
        );
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].id, next.id);
        assert_eq!(inbox.unread(), 2);
    }
}

#[test]
fn empty_snapshot_primes_live_delivery() {
    let node = NodeId([1; 32]);
    let mut inbox = Inbox::default();
    assert!(inbox.merge(node, &[]).is_empty());
    let mut entry = notice(node, 1);
    assert_eq!(inbox.merge(node, std::slice::from_ref(&entry)).len(), 1);
    assert!(inbox.merge(node, std::slice::from_ref(&entry)).is_empty());
    entry.read = true;
    entry.title = "Updated fixture".into();
    assert!(inbox.merge(node, std::slice::from_ref(&entry)).is_empty());
    assert_eq!(inbox.unread(), 0);
    assert_eq!(inbox.notices()[0].title, entry.title);
}

#[test]
fn primes_each_node_independently() {
    let first = NodeId([1; 32]);
    let second = NodeId([2; 32]);
    let mut inbox = Inbox::default();
    inbox.merge(first, &[]);
    let live = notice(first, 1);
    assert_eq!(inbox.merge(first, std::slice::from_ref(&live)).len(), 1);
    let history = notice(second, 10);
    assert!(
        inbox
            .merge(second, std::slice::from_ref(&history))
            .is_empty()
    );
    assert_eq!(inbox.notices().len(), 2);
    inbox.merge(first, &[]);
    assert_eq!(inbox.notices().len(), 1);
    assert_eq!(inbox.notices()[0].id, history.id);
    assert!(inbox.merge(first, std::slice::from_ref(&live)).is_empty());
    assert_eq!(inbox.notices().len(), 2);
}

#[test]
fn clearing_preserves_delivery_cursor() {
    let node = NodeId([1; 32]);
    let mut entry = notice(node, 1);
    entry.id.target = Target::Session(SessionId::new());
    entry.kind = Kind::Input;
    let mut inbox = Inbox::default();
    assert_eq!(inbox.merge(node, std::slice::from_ref(&entry)).len(), 1);
    inbox.clear_local();
    assert_eq!(inbox.unread(), 0);
    assert!(inbox.merge(node, std::slice::from_ref(&entry)).is_empty());
    assert!(inbox.notices().is_empty());
    entry.id.cursor += 1;
    assert_eq!(inbox.merge(node, &[entry]).len(), 1);
    assert_eq!(inbox.unread(), 1);
}
