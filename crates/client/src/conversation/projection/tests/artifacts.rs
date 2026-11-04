use super::*;
use sailry_protocol::artifact;

fn artifact(snapshot: &Snapshot, run: &Run) -> artifact::Summary {
    artifact::Summary {
        kind: artifact::Kind::Document,
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
fn validates_scope_and_canonical_revision() {
    for (case_name, run) in [
        (
            "separates_commits_from_text_deltas",
            (|case_name| {
                let (mut projection, snapshot, run, draft) = fixture();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let first = projection.artifact_cursor();
                projection
                    .apply(1, frame(&snapshot, 11, Change::Delta(draft)))
                    .unwrap();
                assert_eq!(projection.artifact_cursor(), first, "{case_name}");
                let mut value = artifact(&snapshot, &run);
                let commit = frame(&snapshot, 12, Change::Artifact(value.clone()));
                projection.apply(1, commit.clone()).unwrap();
                assert_eq!(projection.artifact_cursor(), first + 1, "{case_name}");
                assert_eq!(
                    projection.snapshot().unwrap().artifacts.items,
                    [value.clone()],
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, commit).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(projection.artifact_cursor(), first + 1, "{case_name}");
                value.revision = 2;
                value.updated_at = 20;
                projection
                    .apply(1, frame(&snapshot, 13, Change::Artifact(value.clone())))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().artifacts.items,
                    [value],
                    "{case_name}"
                );
                assert_eq!(projection.artifact_cursor(), first + 2, "{case_name}");
                assert!(
                    projection.snapshot().unwrap().page.entries.is_empty(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "preserves_honest_head_and_outside_updates",
            (|case_name| {
                let (mut projection, mut snapshot, run, _) = fixture();
                snapshot.artifacts.items = (0..artifact::MAX_PAGE_LIMIT)
                    .map(|_| artifact(&snapshot, &run))
                    .collect();
                let boundary = snapshot.artifacts.items.last().unwrap().id;
                snapshot.artifacts.next_after = Some(boundary);
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut outside = artifact(&snapshot, &run);
                outside.revision = 4;
                projection
                    .apply(1, frame(&snapshot, 11, Change::Artifact(outside)))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().artifacts,
                    snapshot.artifacts,
                    "{case_name}"
                );
                assert_eq!(projection.artifact_cursor(), 2, "{case_name}");
                let fresh = artifact(&snapshot, &run);
                projection
                    .apply(1, frame(&snapshot, 12, Change::Artifact(fresh.clone())))
                    .unwrap();
                let head = &projection.snapshot().unwrap().artifacts;
                assert_eq!(
                    head.items.len(),
                    usize::from(artifact::MAX_PAGE_LIMIT),
                    "{case_name}"
                );
                assert_eq!(head.items[0], fresh, "{case_name}");
                assert_eq!(
                    head.next_after,
                    head.items.last().map(|value| value.id),
                    "{case_name}"
                );
                projection.reconnect(2).unwrap();
                snapshot.sequence = 0;
                projection
                    .apply(2, Update::ConversationSnapshot(Box::new(snapshot)))
                    .unwrap();
                assert_eq!(projection.artifact_cursor(), 4, "{case_name}");
            }) as fn(&str),
        ),
        (
            "rejects_cross_session_and_stale_revisions",
            (|case_name| {
                let (mut projection, mut snapshot, run, _) = fixture();
                let mut value = artifact(&snapshot, &run);
                snapshot.artifacts.items.push(value.clone());
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 11, Change::Artifact(value.clone())))
                        .is_err(),
                    "{case_name}"
                );
                value.session = SessionId::new();
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 11, Change::Artifact(value.clone())))
                        .is_err(),
                    "{case_name}"
                );
                snapshot.artifacts.items.push(value);
                assert!(
                    projection
                        .apply(1, Update::ConversationSnapshot(Box::new(snapshot)))
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(projection.artifact_cursor(), 1, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
