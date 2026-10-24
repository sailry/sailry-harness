use super::*;
use sailry_protocol::notification::{Details, Draft, Kind};

async fn database() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch("CREATE TABLE counters(namespace TEXT PRIMARY KEY,value INTEGER NOT NULL CHECK(value>0)); CREATE TABLE notifications(sequence INTEGER PRIMARY KEY,id TEXT NOT NULL UNIQUE,body BLOB NOT NULL)").await.unwrap();
    db
}

async fn publish(db: &Connection) -> Notice {
    let transaction = db.unchecked_transaction().await.unwrap();
    let notice = insert(
        &transaction,
        "fixture".into(),
        Draft {
            title: "Completed task".into(),
            message: "Result available".into(),
            kind: Kind::Success,
            session: None,
            presentation: None,
            details: Some(Details::Text {
                body: "Three items processed".into(),
            }),
        },
        1000,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    notice
}

#[tokio::test]
async fn bulk_actions_preserve_unread_and_new_arrivals() {
    let db = database().await;
    let read = publish(&db).await;
    let unread = publish(&db).await;
    let command = Command::MarkNotificationsRead { ids: vec![read.id] };
    let (_, event) = execute(&db, &command, None).await.unwrap();
    assert_eq!(event, Some(Event::NotificationsRead { ids: vec![read.id] }));
    assert_eq!(execute(&db, &command, None).await.unwrap().1, None);
    let incoming = publish(&db).await;
    let (_, event) = execute(
        &db,
        &Command::DismissReadNotifications {
            ids: vec![read.id, unread.id],
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        event,
        Some(Event::NotificationsDismissed { ids: vec![read.id] })
    );
    assert_eq!(list(&db).await.unwrap(), vec![incoming, unread]);
}

#[tokio::test]
async fn bulk_commands_bound_the_captured_selection() {
    let db = database().await;
    for command in [
        Command::MarkNotificationsRead {
            ids: vec![NotificationId::new(); LIMIT + 1],
        },
        Command::DismissReadNotifications {
            ids: vec![NotificationId::new(); LIMIT + 1],
        },
    ] {
        assert_eq!(
            execute(&db, &command, None).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}
