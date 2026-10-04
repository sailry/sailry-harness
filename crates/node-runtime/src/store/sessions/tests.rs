use super::*;
use sailry_client::Client;

mod schema;

#[tokio::test]
async fn restores_config_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile");
    let node = crate::Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Session fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let config = SessionConfig {
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: "fixture".into(),
        effort: Effort::High,
        mode: sailry_protocol::WorkMode::Code,
        permission: sailry_protocol::Permission::Ask,
        credential: None,
    };
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(config),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    node.shutdown().await.unwrap();
    let db = Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(db);
    let node = crate::Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert_eq!(snapshot.sessions, vec![session]);
    node.shutdown().await.unwrap();
}
