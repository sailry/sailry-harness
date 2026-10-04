use super::*;

#[tokio::test]
async fn stops_only_the_owned_admission_and_replays_its_receipt() {
    for remote in [false, true] {
        let start = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let server = model_fixture::Server::held(start.clone(), finish.clone()).await;
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, server.endpoint.clone()).await;
        let task = ok(&client, &context, "save", draft(Some(configuration))).await;
        let job = ok(&client, &context, "run", json!({"id":task["id"]})).await;
        server.wait_count(1).await;
        let running = wait(&client, &context, &job["id"], "running").await;
        let id = serde_json::from_value(job["id"].clone()).unwrap();
        let foreign = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "reminders".into(),
                },
            )
            .await,
        );
        let mut other = context.clone();
        other.package = foreign.summary.reference();
        let stop = Command::StopDispatchTurn {
            package: other.package.clone(),
            job: id,
        };
        assert_eq!(
            client
                .execute(client.prepare(stop).with_plugin(other))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut scoped = context.clone();
        scoped.worktree = Some(tree);
        let stop = Command::StopDispatchTurn {
            package: context.package.clone(),
            job: id,
        };
        assert_eq!(
            client
                .execute(client.prepare(stop).with_plugin(scoped))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let request = request(&client, &context, "stop", json!({"id":id}));
        let output = client.execute(request.clone()).await.unwrap();
        assert!(
            matches!(&output, Output::PluginResult(result) if result.get("Ok").is_some()),
            "{output:?}"
        );
        assert_eq!(client.execute(request).await.unwrap(), output);
        wait(&client, &context, &job["id"], "cancelled").await;
        let session = serde_json::from_value(running["session"].clone()).unwrap();
        let Output::Conversation(history) = execute(
            &client,
            Command::ReadConversation {
                session,
                before: None,
                limit: 10,
            },
        )
        .await
        else {
            panic!("history expected")
        };
        assert_eq!(history.page.runs[0].status, conversation::Status::Cancelled);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        start.notify_one();
        finish.notify_one();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
