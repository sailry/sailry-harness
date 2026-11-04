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
fn restores_and_primes_delivery_per_node() {
    for (case_name, run) in [
        (
            "restores_history_without_replaying",
            (|case_name| {
                let node = NodeId([1; 32]);
                let entry = notice(node, 1);
                let mut inbox = Inbox::default();
                assert!(
                    inbox.merge(node, std::slice::from_ref(&entry)).is_empty(),
                    "{case_name}"
                );
                assert_eq!(inbox.notices().len(), 1, "{case_name}");
                assert!(inbox.merge(node, &[]).is_empty(), "{case_name}");
                assert!(inbox.notices().is_empty(), "{case_name}");
                assert!(
                    inbox.merge(node, std::slice::from_ref(&entry)).is_empty(),
                    "{case_name}"
                );
                assert_eq!(inbox.notices().len(), 1, "{case_name}");
                assert_eq!(inbox.notices()[0].id, entry.id, "{case_name}");
                assert_eq!(inbox.unread(), 1, "{case_name}");
                assert!(
                    inbox.merge(node, std::slice::from_ref(&entry)).is_empty(),
                    "{case_name}"
                );
                assert_eq!(inbox.notices().len(), 1, "{case_name}");
                let next = notice(node, 2);
                let added = inbox.merge(node, &[next.clone(), entry]);
                assert_eq!(added.len(), 1, "{case_name}");
                assert_eq!(added[0].id, next.id, "{case_name}");
                assert_eq!(inbox.notices().len(), 2, "{case_name}");
            }) as fn(&str),
        ),
        (
            "restart_restores_unread_without_replaying",
            (|case_name| {
                let node = NodeId([1; 32]);
                let unread = notice(node, 2);
                let mut read = notice(node, 1);
                read.read = true;
                let history = [unread, read];
                for _ in 0..2 {
                    let mut inbox = Inbox::default();
                    assert!(inbox.merge(node, &history).is_empty(), "{case_name}");
                    assert_eq!(inbox.notices().len(), 2, "{case_name}");
                    assert_eq!(inbox.unread(), 1, "{case_name}");
                    assert!(inbox.merge(node, &history).is_empty(), "{case_name}");
                    let next = notice(node, 3);
                    let added = inbox.merge(
                        node,
                        &[next.clone(), history[0].clone(), history[1].clone()],
                    );
                    assert_eq!(added.len(), 1, "{case_name}");
                    assert_eq!(added[0].id, next.id, "{case_name}");
                    assert_eq!(inbox.unread(), 2, "{case_name}");
                }
            }) as fn(&str),
        ),
        (
            "empty_snapshot_primes_live_delivery",
            (|case_name| {
                let node = NodeId([1; 32]);
                let mut inbox = Inbox::default();
                assert!(inbox.merge(node, &[]).is_empty(), "{case_name}");
                let mut entry = notice(node, 1);
                assert_eq!(
                    inbox.merge(node, std::slice::from_ref(&entry)).len(),
                    1,
                    "{case_name}"
                );
                assert!(
                    inbox.merge(node, std::slice::from_ref(&entry)).is_empty(),
                    "{case_name}"
                );
                entry.read = true;
                entry.title = "Updated fixture".into();
                assert!(
                    inbox.merge(node, std::slice::from_ref(&entry)).is_empty(),
                    "{case_name}"
                );
                assert_eq!(inbox.unread(), 0, "{case_name}");
                assert_eq!(inbox.notices()[0].title, entry.title, "{case_name}");
            }) as fn(&str),
        ),
        (
            "primes_each_node_independently",
            (|case_name| {
                let first = NodeId([1; 32]);
                let second = NodeId([2; 32]);
                let mut inbox = Inbox::default();
                inbox.merge(first, &[]);
                let live = notice(first, 1);
                assert_eq!(
                    inbox.merge(first, std::slice::from_ref(&live)).len(),
                    1,
                    "{case_name}"
                );
                let history = notice(second, 10);
                assert!(
                    inbox
                        .merge(second, std::slice::from_ref(&history))
                        .is_empty(),
                    "{case_name}"
                );
                assert_eq!(inbox.notices().len(), 2, "{case_name}");
                inbox.merge(first, &[]);
                assert_eq!(inbox.notices().len(), 1, "{case_name}");
                assert_eq!(inbox.notices()[0].id, history.id, "{case_name}");
                assert!(
                    inbox.merge(first, std::slice::from_ref(&live)).is_empty(),
                    "{case_name}"
                );
                assert_eq!(inbox.notices().len(), 2, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
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
