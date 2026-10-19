use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reads_declared_resources_without_exposing_grants() {
    use support::{execute, info};
    for remote in [false, true] {
        let mode = Arc::new(AtomicUsize::new(0));
        let selected = mode.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| match request
            .path
            .as_str()
        {
            "/token" => tokens(true),
            "/catalog" => {
                Reply::Json(json!({"catalog":[{"id":"translated","context":64000,"output":4096}]}))
            }
            "/resource" => {
                assert_eq!(request.headers["authorization"], format!("Bearer {FRESH}"));
                assert_eq!(request.headers["x-account"], ACCOUNT);
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&request.body).unwrap(),
                    json!({"after":"current"})
                );
                if selected.load(Ordering::SeqCst) == 0 {
                    Reply::Json(json!({"remaining":12.5,"capacity":100}))
                } else {
                    Reply::Json(json!({"remaining":12.5,"echo":FRESH}))
                }
            }
            _ => panic!("unexpected provider resource fixture route"),
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let mut provider = configured_channel(&fixture, true, json!({"kind":"token_exchange","token":{"url":"https://auth.example.test/token","method":"POST"}}), Some(&server.endpoint)).await;
        let installed = info(
            execute(
                &fixture.client,
                Command::ReadPlugin {
                    name: "example".into(),
                },
            )
            .await,
        );
        let path = fixture.directory.path().join("source/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["version"] = json!("1.0.1");
        manifest["extensions"]["dev.sailry.platform"]["actions"]
            .as_array_mut()
            .unwrap()
            .push(json!("http.request"));
        manifest["extensions"]["dev.sailry.platform"]["model_auth"][0]["requests"] = json!({"status":{"url":"https://resource.example.test/resource","method":"POST","headers":{"Content-Type":"application/json"},"body":{"kind":"json","value":{"after":"{input.after}"}}}});
        std::fs::write(path, manifest.to_string()).unwrap();
        let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let installed = info(
            execute(
                &fixture.client,
                support::install(snapshot.worktrees[0].id, installed.summary.revision),
            )
            .await,
        );
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        provider.consent.as_mut().unwrap().package = installed.summary.reference();
        provider.consent.as_mut().unwrap().origin = None;
        provider.consent.as_mut().unwrap().scopes.messages = false;
        let Output::Provider(updated) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        provider = updated;
        let mut context = plugin::Context {
            package: installed.summary.reference(),
            worktree: None,
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        };
        let command = |provider: &Provider, name: &str| Command::RequestProviderHttp {
            provider: provider.id,
            expected_revision: provider.revision,
            request: name.into(),
            input: [("after".into(), "current".into())].into(),
        };
        let error = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(command(&provider, "status"))
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotConfigured);
        assert!(server.requests.lock().unwrap().is_empty());
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut updates).await.state, login::State::Connected);
        let mut provider = saved(&fixture.client, provider.id).await;
        let mut ids = Vec::new();
        let request = fixture
            .client
            .prepare(command(&provider, "status"))
            .with_plugin(context.clone());
        ids.push(request.id);
        assert!(!request.command.durable());
        let admission = fixture.client.dispatch(request).await.unwrap();
        assert!(!admission.receipt.durable);
        let Output::PluginHttp(response) = admission.completion.await.unwrap().unwrap() else {
            panic!("provider HTTP response expected")
        };
        private(&response);
        assert_eq!(response.status, 200);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response.body).unwrap(),
            json!({"remaining":12.5,"capacity":100})
        );
        // Ordinary code updates preserve the existing account contract, including imports.
        let credential = provider.credential.clone();
        manifest["version"] = json!("1.0.2");
        std::fs::write(
            fixture.directory.path().join("source/package/plugin.json"),
            manifest.to_string(),
        )
        .unwrap();
        let updated = info(
            execute(
                &fixture.client,
                support::install(snapshot.worktrees[0].id, installed.summary.revision),
            )
            .await,
        );
        assert!(updated.issues.is_empty(), "{:?}", updated.issues);
        let old_context = context.clone();
        context.package = updated.summary.reference();
        let error = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(command(&provider, "status"))
                    .with_plugin(old_context),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error.code,
            ErrorCode::PermissionDenied | ErrorCode::RevisionConflict
        ));
        let Output::PluginHttp(response) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(command(&provider, "status"))
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("provider HTTP response expected")
        };
        private(&response);
        assert_eq!(response.status, 200);
        assert_eq!(
            saved(&fixture.client, provider.id).await.credential,
            credential
        );
        let count = server.requests.lock().unwrap().len();
        let mut stale = provider.clone();
        stale.revision += 1;
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(command(&stale, "status"))
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(command(&provider, "missing"))
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(server.requests.lock().unwrap().len(), count);
        let foreign = fixture.directory.path().join("source/foreign");
        std::fs::create_dir_all(foreign.join("dev.sailry.platform")).unwrap();
        let mut other = manifest.clone();
        other["name"] = json!("foreign");
        std::fs::write(foreign.join("plugin.json"), other.to_string()).unwrap();
        for name in ["auth.js", "adapter.js"] {
            std::fs::copy(
                fixture
                    .directory
                    .path()
                    .join("source/package/dev.sailry.platform")
                    .join(name),
                foreign.join("dev.sailry.platform").join(name),
            )
            .unwrap();
        }
        let foreign = info(
            execute(
                &fixture.client,
                Command::InstallPlugin {
                    worktree: snapshot.worktrees[0].id,
                    path: "foreign".into(),
                    name: "foreign".into(),
                    expected_revision: 0,
                    expected_digest: None,
                },
            )
            .await,
        );
        let mut foreign_context = context.clone();
        foreign_context.package = foreign.summary.reference();
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(command(&provider, "status"))
                        .with_plugin(foreign_context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(server.requests.lock().unwrap().len(), count);
        mode.store(1, Ordering::SeqCst);
        let error = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(command(&provider, "status"))
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap_err();
        private(&error);
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        provider.consent.as_mut().unwrap().scopes.credentials = false;
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        mode.store(0, Ordering::SeqCst);
        let Output::PluginHttp(response) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(command(&provider, "status"))
                    .with_plugin(context),
            )
            .await
            .unwrap()
        else {
            panic!("provider HTTP response expected")
        };
        assert_eq!(response.status, 200);
        private(&response);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.path == "/resource")
                .count(),
            4
        );
        assert_eq!(credentials(&fixture.client).await.len(), 1);
        let database = fixture.directory.path().join("node/storage/node.sqlite3");
        let Fixture {
            directory,
            node,
            controller,
            ..
        } = fixture;
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let connection = rusqlite::Connection::open_with_flags(
            database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        for id in ids {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT count(*) FROM requests WHERE id=?1",
                        [id.to_string()],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
        }
        drop(connection);
        drop(directory);
    }
}
