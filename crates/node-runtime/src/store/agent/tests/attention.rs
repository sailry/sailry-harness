use super::{fixture::Fixture, *};
use crate::store::sessions::attention;

#[tokio::test]
async fn completions_require_fresh_acknowledgements() {
    let mut fixture = Fixture::new().await;
    let id = fixture.session;
    assert_eq!(
        attention::read(&fixture.database.connection, id)
            .await
            .unwrap()
            .revision,
        0
    );
    fixture.completed(&["First result"]).await;
    let first = activity::read(&fixture.database.connection, id)
        .await
        .unwrap()
        .attention;
    assert!(first.unread);
    attention::set(&fixture.database.connection, id, first.revision, true)
        .await
        .unwrap();
    assert!(
        !activity::read(&fixture.database.connection, id)
            .await
            .unwrap()
            .attention
            .unread
    );
    // Concurrent acknowledgements converge once the same completion is read.
    attention::set(&fixture.database.connection, id, first.revision, true)
        .await
        .unwrap();
    fixture.completed(&["Second result"]).await;
    assert_eq!(
        attention::set(&fixture.database.connection, id, first.revision, true)
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let second = attention::read(&fixture.database.connection, id)
        .await
        .unwrap();
    assert!(second.unread);
    assert!(second.revision > first.revision);
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        attention::read(&fixture.database.connection, id)
            .await
            .unwrap(),
        second
    );
    let output = command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRead {
            session: id,
            expected_revision: second.revision,
            read: true,
        },
    )
    .await;
    let Output::Session(session) = output else {
        panic!("session expected")
    };
    assert!(!session.activity.attention.unread);
    assert_eq!(session.revision, 1);
}
