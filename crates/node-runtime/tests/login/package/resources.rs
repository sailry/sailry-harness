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
        let mut provider = configured_channel(&fixture, true, json!({"kind":"token_exchange","token":{"url":"https://auth.example.test/token","method":"POST"}}), Some(&server.endpoint), None).await;
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
        let database = fixture.directory.path().join("node/storage/node.turso");
        let Fixture {
            directory,
            node,
            controller,
            ..
        } = fixture;
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let (connection_handle, connection) =
            crate::database_fixture::open(&(database), true).await;
        for id in ids {
            assert_eq!(
                (async {
                    let mut statement = connection
                        .prepare("SELECT count(*) FROM requests WHERE id=?1")
                        .await?;
                    let row = statement.query_row([id.to_string()]).await?;
                    let mapped: turso::Result<_> = row.get::<i64>(0);
                    mapped
                })
                .await
                .unwrap(),
                0
            );
        }
        crate::database_fixture::close(connection_handle, connection).await;
        drop(directory);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn executes_declared_mutations_once() {
    use support::{execute, info};
    for remote in [false, true] {
        let effects = Arc::new(AtomicUsize::new(0));
        let observed = effects.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| match request
            .path
            .as_str()
        {
            "/token" => tokens(true),
            "/catalog" => {
                Reply::Json(json!({"catalog":[{"id":"translated","context":64000,"output":4096}]}))
            }
            "/claim" => {
                assert_eq!(request.headers["authorization"], format!("Bearer {FRESH}"));
                assert_eq!(request.headers["x-account"], ACCOUNT);
                let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
                assert!(matches!(body["date"].as_str(), Some("today" | "private")));
                observed.fetch_add(1, Ordering::SeqCst);
                if body["date"] == "private" {
                    Reply::Json(json!({"echo":FRESH}))
                } else {
                    Reply::Json(json!({"accepted":true}))
                }
            }
            _ => panic!("unexpected provider mutation fixture route"),
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured_channel(&fixture, true, json!({"kind":"token_exchange","token":{"url":"https://auth.example.test/token","method":"POST"}}), Some(&server.endpoint), None).await;
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
        manifest["extensions"]["dev.sailry.platform"]["model_auth"][0]["mutations"] = json!({"claim":{"url":"https://resource.example.test/claim","cache_ttl_ms":60000,"method":"POST","headers":{"Content-Type":"application/json"},"body":{"kind":"json","value":{"date":"{input.date}"}}}});
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
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut updates).await.state, login::State::Connected);
        let provider = saved(&fixture.client, provider.id).await;
        let context = plugin::Context {
            package: installed.summary.reference(),
            worktree: None,
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        };
        let command = |revision| Command::ExecuteProviderHttp {
            provider: provider.id,
            expected_revision: revision,
            request: "claim".into(),
            input: [("date".into(), "today".into())].into(),
        };
        let read = fixture
            .client
            .prepare(Command::RequestProviderHttp {
                provider: provider.id,
                expected_revision: provider.revision,
                request: "claim".into(),
                input: Default::default(),
            })
            .with_plugin(context.clone());
        assert_eq!(
            fixture.client.execute(read).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        let request = fixture
            .client
            .prepare(command(provider.revision))
            .with_plugin(context.clone());
        assert!(request.command.durable());
        private(&request);
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        assert_eq!(admission.receipt.id, request.id);
        let result = admission.completion.await.unwrap().unwrap();
        let Output::PluginHttp(response) = &result else {
            panic!("provider HTTP response expected")
        };
        private(response);
        assert_eq!(response.status, 200);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response.body).unwrap(),
            json!({"accepted":true})
        );
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            result
        );
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        // A new durable identity is a new operation, even with a cache hint.
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(command(provider.revision))
                        .with_plugin(context.clone())
                )
                .await
                .unwrap(),
            result
        );
        assert_eq!(effects.load(Ordering::SeqCst), 2);
        let stale = fixture
            .client
            .prepare(command(provider.revision + 1))
            .with_plugin(context.clone());
        assert_eq!(
            fixture.client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
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
                        .prepare(command(provider.revision))
                        .with_plugin(foreign_context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(effects.load(Ordering::SeqCst), 2);
        let unsafe_request = fixture
            .client
            .prepare(Command::ExecuteProviderHttp {
                provider: provider.id,
                expected_revision: provider.revision,
                request: "claim".into(),
                input: [("date".into(), "private".into())].into(),
            })
            .with_plugin(request.plugin.as_ref().unwrap().clone());
        let error = fixture
            .client
            .execute(unsafe_request.clone())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        private(&error);
        assert_eq!(
            fixture.client.execute(unsafe_request).await.unwrap_err(),
            error
        );
        assert_eq!(effects.load(Ordering::SeqCst), 3);

        let stored: (Vec<u8>, String) = {
            let sql = ("SELECT body,result FROM requests WHERE id=?1").to_owned();
            let parameters = turso::params![request.id.to_string()];
            fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::database_fixture::fault)?;
                        let value: turso::Result<_> =
                            (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                        value.map_err(crate::database_fixture::fault)
                    })
                })
                .await
        }
        .unwrap();
        private(&String::from_utf8(stored.0).unwrap());
        private(&stored.1);

        fixture.close().await;
    }
}
