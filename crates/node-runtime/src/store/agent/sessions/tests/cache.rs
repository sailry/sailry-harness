use super::*;

async fn cached(fixture: &Fixture, turn: TurnId, cache: &mut conversion::Cache) -> Session {
    get_cached(
        &fixture.database.connection,
        turn,
        Query {
            session: fixture.session,
            recent: None,
            after: None,
        },
        cache,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn reapplies_visibility_and_compaction() {
    let (mut fixture, turn) = running().await;
    let message = ModelMessage::assistant()
        .with_generated_id()
        .with_text("Original output");
    let id = message.id.clone().unwrap();
    apply(&mut fixture, turn, vec![message.into()])
        .await
        .unwrap();
    let mut cache = conversion::Cache::default();
    assert!(
        cached(&fixture, turn, &mut cache).await.conversation[0]
            .metadata
            .agent_visible
    );
    apply(
        &mut fixture,
        turn,
        vec![
            ConversationEffect::SetMessageVisibility {
                message_id: id.clone(),
                user_visible: true,
                agent_visible: false,
            }
            .into(),
        ],
    )
    .await
    .unwrap();
    let hidden = cached(&fixture, turn, &mut cache).await;
    assert!(!hidden.conversation[0].metadata.agent_visible);
    assert_eq!(hidden.conversation, load(&fixture, turn).await.conversation);
    apply(
        &mut fixture,
        turn,
        vec![
            ConversationEffect::SetMessageVisibility {
                message_id: id,
                user_visible: true,
                agent_visible: true,
            }
            .into(),
        ],
    )
    .await
    .unwrap();
    assert!(
        cached(&fixture, turn, &mut cache).await.conversation[0]
            .metadata
            .agent_visible
    );
    apply(
        &mut fixture,
        turn,
        vec![Effect::Compaction {
            summary: Box::new(
                ModelMessage::user()
                    .with_generated_id()
                    .with_text("Handoff"),
            ),
            usage: usage(),
            files: Vec::new(),
            tools: Vec::new(),
        }],
    )
    .await
    .unwrap();
    let compacted = cached(&fixture, turn, &mut cache).await;
    assert!(!compacted.conversation[0].metadata.agent_visible);
    assert!(compacted.conversation[1].metadata.agent_visible);
    assert_eq!(
        compacted.conversation,
        load(&fixture, turn).await.conversation
    );
}

#[tokio::test]
async fn avoids_rehydration_when_appending_effects() {
    let (mut fixture, turn) = running().await;
    apply(
        &mut fixture,
        turn,
        vec![
            ModelMessage::user()
                .with_generated_id()
                .with_image("AQID", "image/png")
                .into(),
        ],
    )
    .await
    .unwrap();
    let mut cache = conversion::Cache::default();
    let first = cached(&fixture, turn, &mut cache).await;
    fixture
        .database
        .connection
        .execute("DELETE FROM agent_blobs", ())
        .await
        .unwrap();
    apply(
        &mut fixture,
        turn,
        vec![
            ModelMessage::assistant()
                .with_generated_id()
                .with_text("Next output")
                .into(),
        ],
    )
    .await
    .unwrap();
    let next = cached(&fixture, turn, &mut cache).await;
    assert_eq!(next.conversation[0], first.conversation[0]);
    assert_eq!(next.conversation[1].as_concat_text(), "Next output");
}
