use super::*;
use sailry_client::{
    View,
    activity::{Inbox, Kind, Lane, lane},
};
use sailry_link::CancellationToken;
use serde_json::json;
use tokio::sync::watch;

async fn observed(view: &mut watch::Receiver<View>, ready: impl Fn(&View) -> bool) -> View {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let current = view.borrow_and_update().clone();
            if ready(&current) {
                return current;
            }
            view.changed().await.unwrap();
        }
    })
    .await
    .expect("activity update deadline")
}

#[tokio::test]
async fn projects_activity_lifecycle() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(fixture.path().join("node")).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let server = Server::tools(vec![
            (plugin_tool("files", "write_file"), json!({"path":"result.txt","text":"activity fixture","expected_revision":null})),
            ("ask_user".into(), json!({"prompt":"Continue", "input":{"kind":"text","multiline":false,"max_bytes":128}})),
        ]).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let (sender, mut receiver) = watch::channel(View::default());
        let stop = CancellationToken::new();
        let observer = tokio::spawn({
            let client = client.clone();
            let stop = stop.clone();
            async move { client.watch(sender, stop).await }
        });
        observed(&mut receiver, |view| view.connected).await;
        let turn = approvals::submit(&client, session.id).await;
        let current = observed(&mut receiver, |view| {
            view.notifications.iter().any(|n| n.kind == Kind::Approval)
        })
        .await;
        let activity = &current.snapshot.as_ref().unwrap().sessions[0];
        assert_eq!(lane(activity), Lane::Waiting);
        assert_eq!(activity.activity.title, "Write the requested file");
        let (_, approval) = approvals::pending(&client, session.id).await;
        let Output::QueuedTurn(queued) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "Queued next task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        let current = observed(&mut receiver, |view| {
            view.snapshot
                .as_ref()
                .is_some_and(|s| s.sessions[0].activity.queued == 1)
        })
        .await;
        assert_eq!(
            current.snapshot.unwrap().sessions[0]
                .activity
                .run
                .as_ref()
                .unwrap()
                .turn,
            turn
        );
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let current = observed(&mut receiver, |view| {
            view.notifications.iter().any(|n| n.kind == Kind::Input)
        })
        .await;
        assert_eq!(current.notifications.len(), 2);
        let (_, question) = questions::pending(&client, session.id).await;
        client
            .execute(client.prepare(Command::ResolveQuestion {
                session: session.id,
                question: question.id,
                response: question::Response::Answer(question::Answer::Text("Continue".into())),
            }))
            .await
            .unwrap();
        let current = observed(&mut receiver, |view| {
            view.snapshot.as_ref().is_some_and(|s| {
                s.sessions[0]
                    .activity
                    .run
                    .as_ref()
                    .is_some_and(|r| r.turn == queued.id && r.status == Status::Completed)
            })
        })
        .await;
        assert_eq!(
            current
                .notifications
                .iter()
                .filter(|n| n.kind == Kind::Completed)
                .count(),
            2
        );
        assert_eq!(
            current.snapshot.as_ref().unwrap().sessions[0]
                .activity
                .queued,
            0
        );
        assert_eq!(
            current.snapshot.as_ref().unwrap().sessions[0]
                .activity
                .title,
            "Write the requested file"
        );
        let mut inbox = Inbox::default();
        inbox.merge(node.id(), &current.notifications);
        inbox.mark_all_read();
        inbox.merge(node.id(), &current.notifications);
        assert_eq!(inbox.notices().len(), 4);
        assert_eq!(inbox.unread(), 0);
        let before_count = current.notifications.len();
        let Output::Rewound(rewound) = client
            .execute(client.prepare(Command::RewindConversation {
                session: session.id,
                through: Some(turn),
                expected_head: queued.id,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("rewind expected")
        };
        let current = observed(&mut receiver, |view| {
            view.snapshot
                .as_ref()
                .is_some_and(|s| s.sessions.iter().any(|s| s.id == rewound.backup.id))
        })
        .await;
        assert_eq!(
            current.notifications.len(),
            before_count,
            "rewind and backup are not new completions"
        );
        let projected = current
            .snapshot
            .as_ref()
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.id == session.id)
            .unwrap();
        assert_eq!(projected.activity.run.as_ref().unwrap().turn, turn);
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == session.id)
                .unwrap()
                .activity,
            projected.activity
        );
        assert_eq!(
            snapshot.sessions,
            current.snapshot.as_ref().unwrap().sessions
        );
        stop.cancel();
        observer.await.unwrap().unwrap();
        let (sender, mut receiver) = watch::channel(View::default());
        let stop = CancellationToken::new();
        let observer = tokio::spawn({
            let client = client.clone();
            let stop = stop.clone();
            async move { client.watch(sender, stop).await }
        });
        let restored = observed(&mut receiver, |view| view.connected).await;
        assert!(
            restored.notifications.is_empty(),
            "historical completions do not notify on attach"
        );
        stop.cancel();
        observer.await.unwrap().unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        drop(server);
    }
}
