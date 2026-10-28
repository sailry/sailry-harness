use super::*;
use records::{Operation, Runtime};
use sailry_link::{CancellationToken, Subscription};

async fn subscription(fixture: &Fixture) -> Box<dyn Subscription> {
    let mut subscription = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            fixture.read(None, 100).await,
            Default::default(),
            Default::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert!(matches!(
        subscription.next().await.unwrap(),
        Update::ConversationSnapshot(_)
    ));
    subscription
}

async fn append(fixture: &mut Fixture, turn: TurnId, mut record: Record) {
    record.turn = turn;
    super::super::append(
        &mut fixture.database,
        fixture.session,
        turn,
        record,
        &fixture.events,
    )
    .await
    .unwrap();
}

fn change(message: &str, visible: bool) -> Record {
    let mut record = record(
        "system",
        Kind::Operation(Operation::Visibility {
            message: message.into(),
            user_visible: visible,
            agent_visible: true,
        }),
    );
    record.metadata.runtime = Some(Runtime {
        user_visible: false,
        agent_visible: false,
        ..Default::default()
    });
    record
}

#[tokio::test]
async fn reveals_unpublished_tail() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let mut updates = subscription(&fixture).await;
    let mut hidden = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::Text {
            text: "Recovered answer".into(),
        }],
    );
    hidden.metadata.runtime = Some(Runtime {
        user_visible: false,
        ..Default::default()
    });
    let id = hidden.id.clone();
    append(&mut fixture, turn, hidden).await;
    assert!(fixture.read(None, 100).await.page.entries.is_empty());
    append(&mut fixture, turn, change(&id, true)).await;
    let entry = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match updates.next().await.unwrap() {
                Update::ConversationFrame(frame) => {
                    if let Change::Entry(entry) = frame.change {
                        break entry;
                    }
                }
                Update::ResetRequired => {
                    panic!("an unpublished tail does not require a snapshot reset")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(entry.id, id);
    assert_eq!(fixture.read(None, 100).await.page.entries, vec![entry]);
}

#[tokio::test]
async fn resets_immutable_public_entries() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let mut hidden = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::Text {
            text: "Earlier answer".into(),
        }],
    );
    hidden.metadata.runtime = Some(Runtime {
        user_visible: false,
        ..Default::default()
    });
    let id = hidden.id.clone();
    append(&mut fixture, turn, hidden).await;
    append(
        &mut fixture,
        turn,
        message(
            "assistant",
            Role::Assistant,
            vec![BlockKind::Text {
                text: "Published answer".into(),
            }],
        ),
    )
    .await;
    let mut updates = subscription(&fixture).await;
    append(&mut fixture, turn, change(&id, true)).await;
    assert!(matches!(
        updates.next().await.unwrap(),
        Update::ResetRequired
    ));
    assert_eq!(fixture.read(None, 100).await.page.entries.len(), 2);
    let mut updates = subscription(&fixture).await;
    append(&mut fixture, turn, change(&id, false)).await;
    assert!(matches!(
        updates.next().await.unwrap(),
        Update::ResetRequired
    ));
    let entries = fixture.read(None, 100).await.page.entries;
    assert_eq!(entries.len(), 1);
    assert_ne!(entries[0].id, id);
}
