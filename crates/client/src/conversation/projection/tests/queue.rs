use super::*;
use sailry_protocol::conversation::{Pending, Queue};

fn pending() -> Queue {
    Queue {
        revision: 1,
        paused: true,
        items: vec![Pending {
            kind: sailry_protocol::conversation::RunKind::Task,
            attachments: vec![],
            turn: TurnId::new(),
            revision: 1,
            config_revision: 3,
            ready: true,
            preview: "中文🙂".into(),
            truncated: false,
        }],
    }
}

#[test]
fn orders_and_recovers() {
    let (mut projection, snapshot, _, draft) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut queue = pending();
    let update = frame(&snapshot, 11, Change::Queue(queue.clone()));
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    let page = projection.snapshot().unwrap().page.clone();
    projection
        .apply(1, frame(&snapshot, 12, Change::Delta(draft)))
        .unwrap();
    assert!(Arc::ptr_eq(&page, &projection.snapshot().unwrap().page));
    queue.revision = 2;
    queue.paused = false;
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 14, Change::Queue(queue.clone())))
            .unwrap(),
        Apply::Recover
    );
    assert_eq!(projection.snapshot().unwrap().page.queue, page.queue);
    projection.reconnect(2).unwrap();
    let mut resumed = snapshot.clone();
    resumed.sequence = 0;
    Arc::make_mut(&mut resumed.page).queue = queue.clone();
    projection
        .apply(2, Update::ConversationSnapshot(resumed))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().page.queue, queue);
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 15, Change::Queue(page.queue.clone())))
            .unwrap(),
        Apply::Ignored
    );
}

#[test]
fn rejects_inconsistent_revisions() {
    let (mut projection, mut snapshot, _, _) = fixture();
    Arc::make_mut(&mut snapshot.page).queue = pending();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let original = snapshot.page.queue.clone();
    let mut stale = original.clone();
    stale.revision = 0;
    let mut duplicate = original.clone();
    duplicate.revision = 2;
    duplicate.items.push(duplicate.items[0].clone());
    let mut changed = original.clone();
    changed.paused = false;
    let mut content = original.clone();
    content.revision = 2;
    content.items[0].preview = "unversioned edit".into();
    let mut config = original.clone();
    config.revision = 2;
    config.items[0].config_revision = 4;
    let mut kind = original.clone();
    kind.revision = 2;
    kind.items[0].revision = 2;
    kind.items[0].kind = sailry_protocol::conversation::RunKind::Compaction;
    for invalid in [stale, duplicate, changed, content, config, kind] {
        assert!(
            projection
                .apply(1, frame(&snapshot, 11, Change::Queue(invalid)))
                .is_err()
        );
        assert_eq!(projection.snapshot().unwrap().page.queue, original);
    }
    let mut edited = original.clone();
    edited.revision = 2;
    edited.items[0].revision = 2;
    edited.items[0].preview = "edited".into();
    projection
        .apply(1, frame(&snapshot, 11, Change::Queue(edited.clone())))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().page.queue, edited);
}
