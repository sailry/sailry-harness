use super::*;
use sailry_protocol::notification::{Details, Draft, Kind};

fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE notifications(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,body BLOB NOT NULL)").unwrap();
    db
}

fn publish(db: &Connection) -> Notice {
    insert(
        db,
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
    .unwrap()
}

#[test]
fn bulk_actions_preserve_unread_and_new_arrivals() {
    let db = database();
    let read = publish(&db);
    let unread = publish(&db);
    let command = Command::MarkNotificationsRead { ids: vec![read.id] };
    let (_, event) = execute(&db, &command, None).unwrap();
    assert_eq!(event, Some(Event::NotificationsRead { ids: vec![read.id] }));
    assert_eq!(execute(&db, &command, None).unwrap().1, None);
    let incoming = publish(&db);
    let (_, event) = execute(
        &db,
        &Command::DismissReadNotifications {
            ids: vec![read.id, unread.id],
        },
        None,
    )
    .unwrap();
    assert_eq!(
        event,
        Some(Event::NotificationsDismissed { ids: vec![read.id] })
    );
    assert_eq!(list(&db).unwrap(), vec![incoming, unread]);
}

#[test]
fn bulk_commands_bound_the_captured_selection() {
    let db = database();
    for command in [
        Command::MarkNotificationsRead {
            ids: vec![NotificationId::new(); LIMIT + 1],
        },
        Command::DismissReadNotifications {
            ids: vec![NotificationId::new(); LIMIT + 1],
        },
    ] {
        assert_eq!(
            execute(&db, &command, None).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}
