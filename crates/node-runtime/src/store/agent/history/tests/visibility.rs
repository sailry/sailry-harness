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
                    if let Change::Revealed(entry) = frame.change {
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

/// Next visibility frame, skipping unrelated statistics and task frames.
async fn toggled(updates: &mut Box<dyn Subscription>) -> Change {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match updates.next().await.unwrap() {
                Update::ConversationFrame(frame)
                    if matches!(frame.change, Change::Revealed(_) | Change::Hidden { .. }) =>
                {
                    break frame.change;
                }
                Update::ResetRequired => panic!("visibility changes stay incremental"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn publishes_earlier_changes_incrementally() {
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
    let Change::Revealed(revealed) = toggled(&mut updates).await else {
        panic!("revealed entry expected");
    };
    assert_eq!(revealed.id, id);
    let entries = fixture.read(None, 100).await.page.entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], revealed);
    append(&mut fixture, turn, change(&id, false)).await;
    assert_eq!(
        toggled(&mut updates).await,
        Change::Hidden {
            turn,
            id: id.clone()
        }
    );
    let entries = fixture.read(None, 100).await.page.entries;
    assert_eq!(entries.len(), 1);
    assert_ne!(entries[0].id, id);
    // An unchanged visibility value publishes nothing.
    append(&mut fixture, turn, change(&id, false)).await;
    append(&mut fixture, turn, change(&id, true)).await;
    assert!(matches!(toggled(&mut updates).await, Change::Revealed(entry) if entry.id == id));
}
