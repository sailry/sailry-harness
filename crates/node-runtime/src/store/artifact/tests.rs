use super::*;

#[tokio::test]
async fn paginates_large_revision_metadata() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch("CREATE TABLE artifacts(id TEXT PRIMARY KEY,body BLOB); CREATE TABLE artifact_revisions(artifact TEXT,revision INTEGER,body BLOB)").await.unwrap();
    let id = ArtifactId::new();
    let session = SessionId::new();
    let title = "Project report ".repeat(20_000);
    let summary = "Detailed summary ".repeat(30);
    for revision in 1..=66 {
        let value = Summary {
            id,
            session,
            revision,
            title: title.clone(),
            summary: summary.clone(),
            kind: artifact::Kind::Document,
            turn: TurnId::new(),
            package: plugin::Reference {
                name: "example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            size: 1,
            created_at: 1,
            updated_at: revision.into(),
        };
        assert!(value.valid());
        db.execute(
            "INSERT INTO artifact_revisions VALUES(?1,?2,?3)",
            params![id.to_string(), revision, encode(&value).unwrap()],
        )
        .await
        .unwrap();
        if revision == 66 {
            db.execute(
                "INSERT INTO artifacts VALUES(?1,?2)",
                params![id.to_string(), encode(&value).unwrap()],
            )
            .await
            .unwrap();
        }
    }
    let mut before = None;
    let mut restored = Vec::new();
    loop {
        let page = revisions(&db, id, before, u16::MAX, session).await.unwrap();
        assert!(!page.items.is_empty());
        assert!(page.items.len() <= usize::from(artifact::MAX_PAGE_LIMIT));
        assert!(encode(&page).unwrap().len() <= PAGE_BYTES);
        for value in &page.items {
            assert_eq!(value.title, title);
            assert_eq!(value.summary, summary);
        }
        restored.extend(page.items.iter().map(|value| value.revision));
        let Some(next) = page.next_before else { break };
        assert_eq!(Some(next), page.items.last().map(|value| value.revision));
        before = Some(next);
    }
    assert_eq!(restored, (1..=66).rev().collect::<Vec<_>>());
    assert!(revisions(&db, id, None, 0, session).await.is_err());
    assert_eq!(
        revisions(&db, ArtifactId::new(), None, 1, session)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}
