use super::*;
use process::Fixture;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};

pub(super) async fn observe(client: &Client, session: SessionId) -> View {
    let (sender, mut receiver) = watch::channel(View::default());
    let (_history, requests) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let observer = client.watch_conversation(session, sender, stop.clone(), requests);
    let read = async {
        let view = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                receiver.changed().await.unwrap();
                let view = receiver.borrow_and_update().clone();
                if view.connected && view.snapshot.is_some() {
                    break view;
                }
            }
        })
        .await
        .unwrap();
        stop.cancel();
        view
    };
    let (result, view) = tokio::join!(observer, read);
    result.unwrap();
    view
}

fn plan(state: &str) -> Value {
    json!({"title": "Task 中文 🙂", "steps": [
        {"description": "Inspect", "state": "completed"},
        {"description": "Verify", "state": state},
        {"description": "Publish", "state": "skipped"}
    ]})
}

#[tokio::test]
async fn restores_without_replay() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (plugin_tool("progress", "update_plan"), plan("in_progress")),
            (
                plugin_tool("files", "read_file"),
                json!({"path": "source.txt"}),
            ),
            (plugin_tool("progress", "update_plan"), plan("completed")),
        ])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::write(fixture.root.join("source.txt"), "Evidence 中文 🙂").unwrap();
        planning::configure(&mut fixture, WorkMode::Plan).await;
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Inspect and report progress".into(),
        });
        let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.approvals.is_empty());
        let view = observe(&fixture.client, fixture.session.id).await;
        assert_eq!(view.calls.len(), 3);
        let recorded: Vec<_> = view
            .calls
            .iter()
            .filter_map(|call| call.progress.as_ref())
            .collect();
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            serde_json::to_value(recorded[0]).unwrap(),
            plan("in_progress")
        );
        assert_eq!(
            serde_json::to_value(recorded[1]).unwrap(),
            plan("completed")
        );
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        let last = server.requests.lock().unwrap()[3].clone();
        assert!(last["messages"].to_string().contains("Evidence 中文 🙂"));
        assert!(
            last["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == plugin_tool("progress", "update_plan"))
        );
        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: turn.id,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        let inherited = observe(&fixture.client, branch.id).await;
        assert_eq!(inherited.calls, view.calls);
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(observe(&client, fixture.session.id).await.calls, view.calls);
        assert_eq!(observe(&client, branch.id).await.calls, inherited.calls);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_valid_updates() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (plugin_tool("progress", "update_plan"), plan("in_progress")),
            (
                plugin_tool("progress", "update_plan"),
                json!({"title": null, "steps": []}),
            ),
            (plugin_tool("progress", "update_plan"), plan("invented")),
            (plugin_tool("progress", "update_plan"), plan("completed")),
        ])
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.approvals.is_empty());
        let view = observe(&fixture.client, fixture.session.id).await;
        assert_eq!(view.calls.len(), 4);
        assert!(view.calls[0].progress.is_some() && view.calls[3].progress.is_some());
        assert!(
            view.calls[1..3]
                .iter()
                .all(|call| call.progress.is_none() && call.result(&page).is_some())
        );
        assert_eq!(server.requests.lock().unwrap().len(), 5);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
