use super::*;
use serde_json::json;

#[tokio::test]
async fn publishes_one_completion_per_batch() {
    for remote in [false, true] {
        let server = Server::batch(vec![
            (
                plugin_tool("files", "read_file"),
                json!({"path":"first.txt"}),
            ),
            (
                plugin_tool("files", "read_file"),
                json!({"path":"second.txt"}),
            ),
        ])
        .await;
        let mut fixture = Fixture::with_server(remote, server).await;
        let root = fixture.directory.path().join("project");
        std::fs::write(root.join("first.txt"), "First").unwrap();
        std::fs::write(root.join("second.txt"), "Second").unwrap();
        let mut provider = fixture.provider.clone();
        for model in &mut provider.models {
            model.tools = true;
        }
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider,
                expected_revision: fixture.provider.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected");
        };
        fixture.provider = provider;
        let mut stream = fixture.client.subscribe().await.unwrap();
        let mut projection = sailry_client::Projection::new(fixture.client.target(), 1);
        projection.apply(1, stream.next().await.unwrap()).unwrap();
        let turn = fixture.push("Read both files").await;
        let mut batches = 0;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let update = stream.next().await.unwrap();
                if matches!(&update, Update::Event(EventEnvelope {event:Event::ToolsCompleted {session},..}) if *session == fixture.session.id) {
                    batches += 1;
                }
                projection.apply(1, update).unwrap();
                if projection.snapshot().unwrap().sessions.iter().find(|session| session.id == fixture.session.id)
                    .and_then(|session| session.activity.run.as_ref())
                    .is_some_and(|run| run.turn == turn.id && run.status == Status::Completed) { break; }
            }
        }).await.expect("completion invalidation deadline");
        assert_eq!(
            batches, 1,
            "each settled batch invalidates once over both transports"
        );
        let Output::Snapshot(before) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::InspectGit {
                worktree: fixture.session.worktree,
            }))
            .await
            .unwrap();
        let Output::Snapshot(after) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        assert_eq!(
            before.cursor, after.cursor,
            "repository reads do not invalidate themselves"
        );
        fixture.close().await;
    }
}
