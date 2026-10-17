use super::*;

#[tokio::test]
async fn freezes_delegation_policy_across_resume() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            delegation_proactive: true,
            credential: None,
        };
        client
            .execute(client.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: config.clone(),
            }))
            .await
            .unwrap();
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: None,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected");
        };
        assert_eq!(session.config, config);
        let request = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Capture the coordinator policy".into(),
        });
        let Output::QueuedTurn(queued) = client.execute(request.clone()).await.unwrap() else {
            panic!("queued turn expected");
        };
        let mut standard = session.config.clone();
        standard.delegation_proactive = false;
        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: standard.clone(),
            }))
            .await
            .unwrap();
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(queued.clone())
        );
        assert!(queued.config.delegation_proactive);
        let state = snapshot(&client).await;
        assert_eq!(state.sessions[0].config, standard);
        assert_eq!(state.turns[0].config, config);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let resumed = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let state = snapshot(&resumed).await;
        assert_eq!(state.sessions[0].config, standard);
        assert_eq!(state.turns[0].config, config);
        assert_eq!(state.turns[0].id, queued.id);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
