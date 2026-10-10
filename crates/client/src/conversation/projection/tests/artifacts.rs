use super::*;
use sailry_protocol::artifact;

fn artifact(snapshot: &Snapshot, run: &Run) -> artifact::Summary {
    artifact::Summary {
        id: sailry_protocol::ArtifactId::new(),
        session: snapshot.page.session,
        turn: run.turn,
        package: sailry_protocol::plugin::Reference {
            name: "document-fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        title: "Options".into(),
        summary: "Compare the approaches".into(),
        revision: 1,
        size: 12,
        created_at: 10,
        updated_at: 10,
    }
}

#[test]
fn separates_commits_from_text_deltas() {
    let (mut projection, snapshot, run, draft) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let first = projection.artifact_cursor();
    projection
        .apply(1, frame(&snapshot, 11, Change::Delta(draft)))
        .unwrap();
    assert_eq!(projection.artifact_cursor(), first);
    let mut value = artifact(&snapshot, &run);
    let commit = frame(&snapshot, 12, Change::Artifact(value.clone()));
    projection.apply(1, commit.clone()).unwrap();
    assert_eq!(projection.artifact_cursor(), first + 1);
    assert_eq!(
        projection.snapshot().unwrap().artifacts.items,
        [value.clone()]
    );
    assert_eq!(projection.apply(1, commit).unwrap(), Apply::Ignored);
    assert_eq!(projection.artifact_cursor(), first + 1);
    value.revision = 2;
    value.updated_at = 20;
    projection
        .apply(1, frame(&snapshot, 13, Change::Artifact(value.clone())))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().artifacts.items, [value]);
    assert_eq!(projection.artifact_cursor(), first + 2);
    assert!(projection.snapshot().unwrap().page.entries.is_empty());
}

#[test]
fn preserves_honest_head_and_outside_updates() {
    let (mut projection, mut snapshot, run, _) = fixture();
    snapshot.artifacts.items = (0..artifact::MAX_PAGE_LIMIT)
        .map(|_| artifact(&snapshot, &run))
        .collect();
    let boundary = snapshot.artifacts.items.last().unwrap().id;
    snapshot.artifacts.next_after = Some(boundary);
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut outside = artifact(&snapshot, &run);
    outside.revision = 4;
    projection
        .apply(1, frame(&snapshot, 11, Change::Artifact(outside)))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().artifacts, snapshot.artifacts);
    assert_eq!(projection.artifact_cursor(), 2);
    let fresh = artifact(&snapshot, &run);
    projection
        .apply(1, frame(&snapshot, 12, Change::Artifact(fresh.clone())))
        .unwrap();
    let head = &projection.snapshot().unwrap().artifacts;
    assert_eq!(head.items.len(), usize::from(artifact::MAX_PAGE_LIMIT));
    assert_eq!(head.items[0], fresh);
    assert_eq!(head.next_after, head.items.last().map(|value| value.id));
    projection.reconnect(2).unwrap();
    snapshot.sequence = 0;
    projection
        .apply(2, Update::ConversationSnapshot(snapshot))
        .unwrap();
    assert_eq!(projection.artifact_cursor(), 4);
}

#[test]
fn rejects_cross_session_and_stale_revisions() {
    let (mut projection, mut snapshot, run, _) = fixture();
    let mut value = artifact(&snapshot, &run);
    snapshot.artifacts.items.push(value.clone());
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    assert!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Artifact(value.clone())))
            .is_err()
    );
    value.session = SessionId::new();
    assert!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Artifact(value.clone())))
            .is_err()
    );
    snapshot.artifacts.items.push(value);
    assert!(
        projection
            .apply(1, Update::ConversationSnapshot(snapshot))
            .is_err()
    );
    assert_eq!(projection.artifact_cursor(), 1);
}
