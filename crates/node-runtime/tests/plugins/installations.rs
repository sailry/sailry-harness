use super::*;
use sailry_protocol::installation::{Resource, State, Task};

async fn tasks(client: &Client) -> Vec<Task> {
    let Update::Installations { node, tasks } = client
        .subscribe_installations()
        .await
        .unwrap()
        .next()
        .await
        .unwrap()
    else {
        panic!("installation snapshot expected")
    };
    assert_eq!(node, client.target());
    tasks
}

fn changed(worktree: WorktreeId) -> Command {
    Command::InstallPlugin {
        worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 0,
        expected_digest: Some("f".repeat(64)),
    }
}

#[tokio::test]
async fn retains_and_dismisses_without_replaying() {
    for remote in [false, true] {
        let (_directory, node, controller, client, worktree) = fixture(remote).await;
        let request = client.prepare(changed(worktree));
        let error = client.execute(request.clone()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        let retained = tasks(&client).await;
        let task = retained
            .iter()
            .find(|task| task.request == request.id)
            .unwrap();
        assert_eq!(
            task.resource,
            Resource::Plugin {
                name: "example".into()
            }
        );
        assert_eq!(
            task.state,
            State::Failed {
                error: error.clone()
            }
        );
        // Reconnect recovers observation, never another installation.
        assert_eq!(tasks(&client).await, retained);
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Err(error.clone())))
        );
        let dismiss = client.prepare(Command::DismissInstallation {
            request: request.id,
        });
        let admission = client.dispatch(dismiss).await.unwrap();
        assert!(!admission.receipt.durable);
        assert_eq!(
            admission.completion.await.unwrap().unwrap(),
            Output::InstallationDismissed {
                request: request.id
            }
        );
        assert!(
            !tasks(&client)
                .await
                .iter()
                .any(|task| task.request == request.id)
        );
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err(),
            error.clone()
        );
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Err(error)))
        );
        assert!(
            !tasks(&client)
                .await
                .iter()
                .any(|task| task.request == request.id)
        );
        // A domain retry prepares a fresh attempt with current revision.
        let retry = client.prepare(install(worktree, 0));
        assert_ne!(retry.id, request.id);
        let installed = info(client.execute(retry.clone()).await.unwrap());
        assert_eq!(installed.summary.revision, 1);
        assert_eq!(
            client.outcome(&retry).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(Output::Plugin(installed))))
        );
        assert!(
            !tasks(&client)
                .await
                .iter()
                .any(|task| task.request == retry.id)
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn isolates_authenticated_callers_and_nodes() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let request = client.prepare(changed(worktree));
        client.execute(request.clone()).await.unwrap_err();
        let retained = tasks(&client).await;
        assert!(retained.iter().any(|task| task.request == request.id));
        let peer = Link::controller(
            directory.path().join("other-controller"),
            NetworkScope::default(),
        )
        .await
        .unwrap();
        let address = peer
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let foreign = Client::new(peer.handle().remote(address));
        assert!(tasks(&foreign).await.is_empty());
        assert_eq!(
            execute(
                &foreign,
                Command::DismissInstallation {
                    request: request.id
                }
            )
            .await,
            Output::InstallationDismissed {
                request: request.id
            }
        );
        assert_eq!(tasks(&client).await, retained);
        let (_other_directory, other_node, other_controller, other, _) = fixture(remote).await;
        assert_ne!(other.target(), client.target());
        assert!(tasks(&other).await.is_empty());
        assert_eq!(tasks(&client).await, retained);
        other_node.shutdown().await.unwrap();
        other_controller.close().await.unwrap();
        peer.close().await.unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
