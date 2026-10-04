use super::*;
use sailry_protocol::plugin::mcp::{Server as McpServer, State};

async fn read(client: &Client, package: plugin::Reference) -> State {
    let Output::PluginMcp(state) = execute(client, Command::ReadPluginMcp { package }).await else {
        panic!("MCP configuration expected")
    };
    state
}

#[tokio::test]
async fn executes_configured_bundled_tools() {
    for remote in [false, true] {
        for name in ["context7", "github"] {
            let original_peer = Http::start(Reply::Normal).await;
            let replacement_peer = Http::start(Reply::Normal).await;
            let model =
                Server::turn_tools(vec![(peer::package_alias(name, name, "read"), json!({}))])
                    .await;
            let fixture = process::Fixture::new(remote, &model).await;
            let Output::Plugin(installed) =
                execute(&fixture.client, Command::ReadPlugin { name: name.into() }).await
            else {
                panic!("plugin expected")
            };
            let Output::Plugin(installed) = execute(
                &fixture.client,
                Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: installed.summary.revision,
                    enabled: true,
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            let initial = read(&fixture.client, installed.summary.reference()).await;
            let mut configuration = initial.configuration;
            let McpServer::Http { url, headers } = configuration.servers.get_mut(name).unwrap()
            else {
                unreachable!()
            };
            let original_endpoint = url.clone();
            *url = original_peer.endpoint.clone();
            let header = if name == "github" {
                "Authorization"
            } else {
                "X-Context7-API-Key"
            };
            let original_secret = if name == "github" {
                "Bearer original-${PLUGIN_ROOT}-token"
            } else {
                "original-${PLUGIN_ROOT}-key"
            };
            headers.insert(header.into(), original_secret.into());
            let request = fixture.client.prepare(Command::SavePluginMcp {
                package: initial.package,
                configuration: configuration.clone(),
            });
            let Output::Plugin(first) = fixture.client.execute(request.clone()).await.unwrap()
            else {
                panic!("plugin expected")
            };
            assert!(original_peer.requests.lock().unwrap().is_empty());
            let (_, queued) = submit(&fixture, true).await;
            assert!(queued.plugins.contains(&first.summary.reference()));
            let McpServer::Http { url, headers } = configuration.servers.get_mut(name).unwrap()
            else {
                unreachable!()
            };
            *url = replacement_peer.endpoint.clone();
            let next_secret = if name == "github" {
                "Bearer replacement-${PLUGIN_DATA}-token"
            } else {
                "replacement-${PLUGIN_DATA}-key"
            };
            headers.insert(header.into(), next_secret.into());
            let Output::Plugin(latest) = execute(
                &fixture.client,
                Command::SavePluginMcp {
                    package: first.summary.reference(),
                    configuration,
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::Plugin(first)
            );
            execute(
                &fixture.client,
                Command::StartQueuedTurn { turn: queued.id },
            )
            .await;
            let page = finished(&fixture.client, fixture.session.id, queued.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert_eq!(original_peer.count("initialize"), 1);
            assert_eq!(original_peer.count("tools/call"), 1);
            assert!(replacement_peer.requests.lock().unwrap().is_empty());
            let (_, next) = submit(&fixture, false).await;
            assert!(next.plugins.contains(&latest.summary.reference()));
            let page = finished(&fixture.client, fixture.session.id, next.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert_eq!(replacement_peer.count("initialize"), 1);
            assert_eq!(replacement_peer.count("tools/call"), 1);
            for (peer, value) in [
                (&original_peer, original_secret),
                (&replacement_peer, next_secret),
            ] {
                for (_, headers, _) in peer.requests.lock().unwrap().iter() {
                    assert_eq!(
                        headers
                            .get(&header.to_ascii_lowercase())
                            .map(String::as_str),
                        Some(value)
                    );
                }
            }
            let projected = serde_json::to_string(&page).unwrap();
            let model_requests = serde_json::to_string(&*model.requests.lock().unwrap()).unwrap();
            for secret in [original_secret, next_secret] {
                assert!(!projected.contains(secret));
                assert!(!model_requests.contains(secret));
            }
            let immutable = fixture
                .node
                .profile()
                .join("plugins/packages")
                .join(&latest.summary.digest)
                .join("mcp.json");
            assert!(
                fs::read_to_string(immutable)
                    .unwrap()
                    .contains(&original_endpoint)
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
