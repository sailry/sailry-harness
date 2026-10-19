use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_policy_after_delisting() {
    use support::execute;
    const KEY: &str = "isolated-partner-key";
    for remote in [false, true] {
        let server=Server::start_with_request(ModelApi::Anthropic,move |request| match request.path.as_str() {
            "/catalog"=>Reply::Json(json!({"version":1,"partners":[{"id":"example","name":"Example partner","routes":[{"protocol":"openai-chat-completions","endpoint":"https://model.example.test"},{"protocol":"openai-responses","endpoint":"https://model.example.test"},{"protocol":"anthropic","endpoint":"https://model.example.test"}],"auth":{"kind":"api_key","data":{"header":"X-Example-Key","prefix":"Token "}},"policies":[{"models":["fixture"],"body":{"preserve_thinking":true}}]}]})),
            "/removed"=>Reply::Json(json!({"version":1,"partners":[]})),
            "/models" | "/v1/models?limit=100"=>{
                assert_eq!(request.headers["x-example-key"],format!("Token {KEY}"));
                assert!(!request.headers.contains_key("authorization"));
                assert!(!request.headers.contains_key("x-api-key"));
                if request.path == "/v1/models?limit=100" { assert_eq!(request.headers["anthropic-version"], "2023-06-01"); }
                Reply::Json(json!({"data":[{"id":"fixture"}],"has_more":false}))
            },
            "/chat/completions"=>{
                assert_eq!(request.headers["x-example-key"],format!("Token {KEY}"));assert!(!request.headers.contains_key("authorization"));
                let body:serde_json::Value=serde_json::from_str(&request.body).unwrap();assert_eq!(body["preserve_thinking"],true);
                let body="data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Preserved policy\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
                Reply::Raw(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()))
            },
            _=>panic!("unexpected partner fixture route: {}", request.path),
        }).await;
        let fixture = Fixture::empty(remote, &server).await;
        fixture
            .node
            .set_model_partners_fixture(&format!("{}/catalog", server.endpoint))
            .unwrap();
        execute(
            &fixture.client,
            Command::ListChannelPresets {
                refresh_partners: true,
            },
        )
        .await;
        let root = fixture.directory.path().join("source/package");
        support::package(&root, "1.0.0");
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"] =
            json!({"dev.sailry.platform":{"api_version":"v1","actions":["models.generate"]}});
        std::fs::write(path, manifest.to_string()).unwrap();
        execute(
            &fixture.client,
            Command::RegisterProject {
                name: "Partner fixture".into(),
                path: root.parent().unwrap().to_str().unwrap().into(),
            },
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let installed = support::info(
            execute(
                &fixture.client,
                support::install(snapshot.worktrees[0].id, 0),
            )
            .await,
        );
        let provider:Provider=serde_json::from_value(json!({"id":ProviderId::new(),"revision":0,"name":"Saved partner","api":"chat_completions","authentication":"api_key","endpoint":server.endpoint,"enabled":true,"models":[{"id":"fixture","context":64000,"output":4096}],"default_model":"fixture","credential":null,"preset":{"source":{"kind":"partner"},"id":"example"}})).unwrap();
        for api in [
            ModelApi::ChatCompletions,
            ModelApi::Responses,
            ModelApi::Anthropic,
        ] {
            let draft = discovery::Draft {
                preset: provider.preset.clone(),
                provider: provider.id,
                api,
                endpoint: provider.endpoint.clone(),
                credential: None,
                secret: Some(Secret::new(KEY.into())),
            };
            let Output::DiscoveredModels(catalog) = execute(
                &fixture.client,
                Command::DiscoverModels(Box::new(discovery::Source::Draft(draft))),
            )
            .await
            else {
                panic!("catalog expected")
            };
            assert_eq!(catalog.models.len(), 1);
            assert_eq!(catalog.models[0].id, "fixture");
        }
        let Output::Provider(mut provider) = execute(
            &fixture.client,
            Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: Some(Secret::new(KEY.into())),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let context = plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: installed.summary.reference(),
            worktree: None,
            session: None,
        };
        let generate = |provider: &Provider| {
            fixture
                .client
                .prepare(Command::GeneratePluginText {
                    prompt: "Preserve the selected policy".into(),
                    model: format!("{}/fixture", provider.id),
                    effort: None,
                })
                .with_plugin(context.clone())
        };
        for removed in [false, true] {
            if removed {
                fixture
                    .node
                    .set_model_partners_fixture(&format!("{}/removed", server.endpoint))
                    .unwrap();
                let Output::ChannelPresets(listing) = execute(
                    &fixture.client,
                    Command::ListChannelPresets {
                        refresh_partners: true,
                    },
                )
                .await
                else {
                    panic!("catalog expected")
                };
                assert!(
                    !listing
                        .entries
                        .iter()
                        .any(|entry| entry.source == presets::Source::Partner)
                );
                let original = provider.binding.clone();
                provider.name = "Edited after delisting".into();
                let Output::Provider(saved) = execute(
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
                provider = saved;
                assert_eq!(provider.binding, original);
            }
            let Output::DiscoveredModels(catalog) = execute(
                &fixture.client,
                Command::DiscoverModels(Box::new(discovery::Source::Saved {
                    provider: provider.id,
                    expected_revision: provider.revision,
                    oauth: None,
                })),
            )
            .await
            else {
                panic!("catalog expected")
            };
            assert_eq!(catalog.models.len(), 1);
            assert_eq!(catalog.models[0].id, "fixture");
            let Output::PluginText(output) =
                fixture.client.execute(generate(&provider)).await.unwrap()
            else {
                panic!("text expected")
            };
            assert_eq!(output.text, "Preserved policy");
            assert!(!serde_json::to_string(&provider).unwrap().contains(KEY));
        }
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(
                    request.path.as_str(),
                    "/models" | "/v1/models?limit=100"
                ))
                .count(),
            5
        );
        fixture.close().await;
    }
}
