use super::*;
use sailry_client::View;
use sailry_link::CancellationToken;
use tokio::sync::watch;

async fn observed(receiver: &mut watch::Receiver<View>, unread: bool, revision: u64) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if receiver.borrow().snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.sessions.first().is_some_and(|session| {
                    session.activity.attention.unread == unread
                        && session.activity.attention.revision == revision
                })
            }) {
                return;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn synchronizes_and_persists_read_state() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let local = Client::new(node.local());
    let remote = Client::new(controller.handle().remote(address));
    let Output::Project(project) = local
        .execute(local.prepare(Command::RegisterProject {
            name: "Attention fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Session(session) = local
        .execute(local.prepare(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                credential: None,
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let stop = CancellationToken::new();
    let (sender, mut receiver) = watch::channel(View::default());
    let observer = Client::new(node.local());
    let cancellation = stop.clone();
    let task = tokio::spawn(async move { observer.watch(sender, cancellation).await });
    observed(&mut receiver, false, 0).await;
    let set = |revision, read| Command::SetSessionRead {
        session: session.id,
        expected_revision: revision,
        read,
    };
    remote.execute(remote.prepare(set(0, false))).await.unwrap();
    observed(&mut receiver, true, 1).await;
    local.execute(local.prepare(set(1, true))).await.unwrap();
    assert!(
        !snapshot(&remote).await.sessions[0]
            .activity
            .attention
            .unread
    );
    remote.execute(remote.prepare(set(2, false))).await.unwrap();
    observed(&mut receiver, true, 3).await;
    assert_eq!(
        remote
            .execute(remote.prepare(set(1, true)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    stop.cancel();
    task.await.unwrap().unwrap();
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let attention = snapshot(&client).await.sessions[0].activity.attention;
    assert!(attention.unread);
    assert_eq!(attention.revision, 3);
    client.execute(client.prepare(set(3, true))).await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    assert!(
        !snapshot(&Client::new(node.local())).await.sessions[0]
            .activity
            .attention
            .unread
    );
    node.shutdown().await.unwrap();
}
