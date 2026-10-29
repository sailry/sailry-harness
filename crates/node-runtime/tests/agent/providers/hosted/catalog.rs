use super::*;
use sailry_protocol::conversation::{
    discovery,
    presets::{Preset, PresetRef, Region, Source},
};
use serde_json::json;

#[tokio::test]
async fn retains_declared_routes_and_captured_policy() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::ChatCompletions, Reply::Text).await;
        let mut fixture = Fixture::empty(remote, ModelApi::ChatCompletions, &server.endpoint).await;
        let preset: Preset =
            serde_json::from_slice(include_bytes!("../../../fixtures/model-vendor.json")).unwrap();
        let mut latest = preset.clone();
        latest.policies[0]
            .body
            .insert("thinking".into(), json!({"clear_thinking":true}));
        let first = json!({"version":1,"partners":[preset.clone()]});
        let second = json!({"version":1,"partners":[latest.clone()]});
        let catalog = crate::discovery_support::Server::start(ModelApi::Responses, move |path| {
            crate::discovery_support::Reply::Json(match path {
                "/v1" => first.clone(),
                "/v1/updated" => second.clone(),
                _ => panic!("unexpected catalog fixture route"),
            })
        })
        .await;
        fixture
            .node
            .set_model_partners_fixture(&catalog.endpoint)
            .unwrap();
        let Output::ChannelPresets(listing) = execute(
            &fixture.client,
            Command::ListChannelPresets {
                refresh_partners: true,
            },
        )
        .await
        else {
            panic!("channel directory expected")
        };
        let entry = listing
            .entries
            .iter()
            .find(|entry| entry.source == Source::Partner && entry.preset.id == preset.id)
            .unwrap();
        assert_eq!(entry.preset, preset);
        assert_eq!(
            entry.preset.products[0]
                .display
                .as_ref()
                .unwrap()
                .label("zh-CN"),
            "示例套餐"
        );
        let route = &entry.preset.routes[2];
        assert_eq!(route.protocol.api(), ModelApi::ChatCompletions);
        assert_eq!(route.region, Some(Region::Global));
        assert_eq!(route.product, Some(Product::Coding));
        let reference = PresetRef {
            source: entry.source.clone(),
            id: entry.preset.id.clone(),
            route: Some(route.key()),
        };
        let Output::DiscoveredModels(models) = execute(
            &fixture.client,
            Command::DiscoverModels(Box::new(discovery::Source::Draft(discovery::Draft {
                provider: fixture.provider.id,
                api: route.protocol.api(),
                endpoint: server.endpoint.clone(),
                credential: fixture.provider.credential.clone(),
                secret: None,
                preset: Some(reference.clone()),
            }))),
        )
        .await
        else {
            panic!("declared models expected")
        };
        assert_eq!(models.models.len(), 1);
        assert_eq!(models.models[0].id, "fixture-a");
        assert!(server.requests.lock().unwrap().is_empty());
        fixture.provider.preset = Some(reference);
        fixture.provider.options = Some(Options::Hosted {
            product: Product::Coding,
        });
        let original = fixture.provider.clone();
        let Output::Provider(saved) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: original.revision,
                provider: original.clone(),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        assert_eq!(saved.endpoint, original.endpoint);
        assert_eq!(saved.models, original.models);
        assert_eq!(saved.default_model, original.default_model);
        assert_eq!(saved.credential, original.credential);
        assert_eq!(saved.preset, original.preset);
        let policy = saved.binding.as_ref().unwrap().policies.last().unwrap();
        assert_eq!(policy, &preset.policies[0]);
        let Output::QueuedTurn(turn) = execute(
            &fixture.client,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Use the admitted directory route".into(),
            },
        )
        .await
        else {
            panic!("queued turn expected")
        };
        fixture
            .node
            .set_model_partners_fixture(&format!("{}/updated", catalog.endpoint))
            .unwrap();
        let Output::ChannelPresets(refreshed) = execute(
            &fixture.client,
            Command::ListChannelPresets {
                refresh_partners: true,
            },
        )
        .await
        else {
            panic!("refreshed directory expected")
        };
        assert_eq!(
            refreshed
                .entries
                .iter()
                .find(|entry| entry.source == Source::Partner && entry.preset.id == preset.id)
                .unwrap()
                .preset,
            latest
        );
        let mut edited = saved;
        edited.endpoint = format!("{}/edited", server.endpoint);
        edited.models[0].id = "edited-model".into();
        edited.default_model = "edited-model".into();
        let Output::Provider(updated) = execute(
            &fixture.client,
            Command::SaveProvider {
                expected_revision: edited.revision,
                provider: edited.clone(),
                secret: Some(Secret::new("edited-fixture-key".into())),
            },
        )
        .await
        else {
            panic!("updated provider expected")
        };
        assert_eq!(updated.endpoint, edited.endpoint);
        assert_eq!(updated.models, edited.models);
        assert_eq!(updated.default_model, edited.default_model);
        assert_eq!(updated.preset, edited.preset);
        assert_ne!(updated.credential, original.credential);
        assert_eq!(
            updated.binding.as_ref().unwrap().policies.last().unwrap(),
            &latest.policies[0]
        );
        assert_eq!(
            fixture
                .client
                .read_turn_capture(fixture.session.id, turn.id)
                .await
                .unwrap(),
            turn
        );
        execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/chat/completions");
        assert_eq!(
            requests[0].headers["authorization"],
            format!("Bearer {}", server::KEY)
        );
        assert_eq!(requests[0].body["model"], "fixture-a");
        assert_eq!(requests[0].body["reasoning_effort"], "high");
        assert_eq!(
            requests[0].body["thinking"],
            json!({"type":"enabled","clear_thinking":false})
        );
        let requests = catalog.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request.method == "GET"
            && !request.headers.contains_key("authorization")
            && !request.headers.contains_key("cookie")));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
