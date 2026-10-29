#![cfg(all(unix, feature = "test-support"))]
use super::*;
use crate::oauth_server as issuer;
use sailry_protocol::{
    plugin::{self, card, connector as authorization},
    tool,
};
use serde_json::json;
use std::sync::atomic::Ordering;

async fn install(fixture: &process::Fixture, endpoint: &str) -> plugin::Info {
    fixture
        .node
        .set_connector_fixture("https://mcp.example.com", endpoint);
    let root = fixture.root.join("card-package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::create_dir_all(root.join("dev.sailry.platform/desktop")).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), r#"
import {card} from 'sailry/cards';
import {status} from 'sailry/connectors';
export async function connection(){
  const props={configured:(await status('service'))?.state==='connected'};
  return {isError:!props.configured,error:props.configured?null:{code:'not_configured',message:'Connect'},
    sailry_content:{version:1,blocks:[card('connection',props,{label:'Connect'})]}};
}
"#).unwrap();
    std::fs::write(root.join("dev.sailry.platform/desktop/card.js"), "import {View,div} from 'gpui-kit'; export default class Card extends View {render(){return div().child('Connect');}};").unwrap();
    std::fs::write(
        root.join("mcp.json"),
        json!({"$schema":plugin::mcp::SCHEMA,"mcpServers":{"input":{"type":"streamable-http","url":"https://mcp.example.com/mcp"}}}).to_string(),
    )
    .unwrap();
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"card-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{
            "api_version":"v1","actions":[],"connectors":[{"id":"service","display":{"label":"Service"},"accounts":"single","origins":["https://mcp.example.com"],"auth":[{"kind":"mcp_oauth","server":"input"}],"inject":[{"server":"input","header":"Authorization","prefix":"Bearer "}]}],
            "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["connection"]},
            "desktop":{"resources":["dev.sailry.platform/desktop/card.js"]},
            "tools":[{"name":"connection","description":"Read connection","contexts":["workspace"],"presentation":"content","handler":{"name":"connection","read_only":true,"parameters":{"type":"object","properties":{},"additionalProperties":false}}}],
            "cards":[{"key":"connection","entry":"dev.sailry.platform/desktop/card.js","tool":"connection","mcp_server":"input",
                "props_schema":{"type":"object","additionalProperties":false,"properties":{"configured":{"type":"boolean"}},"required":["configured"]},
                "capabilities":{"browser":[{"name":"login","origins":["https://example.test"],"source":{"kind":"mcp_authorization"},"modes":["external"]}]}}]
        }}
    }).to_string()).unwrap();
    let Output::Plugin(info) = fixture
        .client
        .execute(fixture.client.prepare(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "card-package".into(),
            name: "card-fixture".into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

async fn summary(
    client: &Client,
    page: &Page,
    calls: &[sailry_client::conversation::tools::Call],
) -> card::Summary {
    for call in calls
        .iter()
        .filter(|call| call.summary(page).is_some_and(|summary| summary.published))
    {
        let call = call.with_detail(
            crate::agent_support::details::read(client, call.detail_reference(page).unwrap())
                .await
                .unwrap(),
        );
        if let Some(card) = call
            .content()
            .into_iter()
            .flat_map(|content| &content.blocks)
            .find_map(|block| {
                if let tool::Block::Card(card) = block {
                    Some(card.clone())
                } else {
                    None
                }
            })
        {
            return card;
        }
    }
    panic!("canonical card expected")
}

fn context(card: &card::Summary, package: plugin::Reference) -> plugin::Context {
    plugin::Context {
        invocation: None,
        turn: None,
        surface: plugin::desktop::Surface::Card(card.receipt),
        package,
        session: Some(card.receipt.session),
        worktree: Some(card.worktree),
    }
}

async fn ready(client: &Client, attempt: &authorization::Attempt) -> authorization::LoginState {
    let mut stream = client.subscribe_connector_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Update::ConnectorLogin(update) = stream.next().await.unwrap() else {
                panic!("login update expected")
            };
            if !matches!(
                update.state,
                authorization::LoginState::Starting | authorization::LoginState::Exchanging
            ) {
                return update.state;
            }
        }
    })
    .await
    .expect("login deadline")
}

#[tokio::test]
async fn keeps_login_private_and_does_not_replay() {
    for remote in [false, true] {
        let input = super::super::mcp::input::Server::with_transport(
            super::super::mcp::input::form_schema(),
            false,
        )
        .await;
        let issuer = issuer::Server::start(&input.endpoint).await;
        let model =
            Server::tools(vec![(plugin_tool("card-fixture", "connection"), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        let package = install(&fixture, &issuer.endpoint).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
        let card = summary(&fixture.client, &page, &view.calls).await;
        assert_eq!(card.receipt.turn, turn);
        assert_eq!(card.receipt.session, fixture.session.id);
        assert_eq!(card.node, fixture.node.id());
        assert_eq!(card.package, package.summary.reference());
        assert_eq!(card.props, json!({"configured":false}));
        let alias = plugin_tool("card-fixture", "connection");
        let prompt = model.requests.lock().unwrap()[0]["messages"].to_string();
        assert!(prompt.contains("- connection: MCP authorization"));
        assert!(!prompt.contains(&format!("- {alias}: MCP authorization")));
        assert_eq!(
            issuer.registrations.load(Ordering::SeqCst),
            0,
            "tool output must not start login"
        );
        let call = view.calls.iter().find(|call| call.name == alias).unwrap();
        let call = call.with_detail(
            crate::agent_support::details::read(
                &fixture.client,
                call.detail_reference(&page).unwrap(),
            )
            .await
            .unwrap(),
        );
        let result = call.result(&page).unwrap();
        assert!(result["isError"].as_bool().unwrap());
        let count = model.requests.lock().unwrap().len();
        let Output::PluginView(view) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPluginView {
                package: card.package.clone(),
                surface: plugin::desktop::Surface::Card(card.receipt),
            }))
            .await
            .unwrap()
        else {
            panic!("card view expected")
        };
        assert_eq!(view.entry, "dev.sailry.platform/desktop/card.js");
        let scope = context(&card, card.package.clone());
        let status = fixture
            .client
            .prepare(Command::ReadPluginConnection {
                package: card.package.clone(),
                card: "connection".into(),
            })
            .with_plugin(scope.clone());
        assert!(matches!(
            fixture.client.execute(status).await.unwrap(),
            Output::PluginConnection(plugin::connection::Status {
                configuration: plugin::connection::Configuration::Ready,
                authorization: plugin::connection::Authorization::Missing,
                credential: plugin::connection::Credential::NotDeclared,
                ..
            })
        ));
        let request = fixture
            .client
            .prepare(Command::StartConnectorLogin {
                package: card.package.clone(),
                connector: "service".into(),
                connection: None,
                expected_revision: None,
                method: authorization::Kind::McpOauth,
                input: authorization::Input {
                    redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                    ..Default::default()
                },
            })
            .with_plugin(scope.clone());
        let Output::ConnectorLogin(attempt) =
            fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("attempt expected")
        };
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            Output::ConnectorLogin(attempt.clone())
        );
        let authorization::LoginState::Browser { url, .. } = ready(&fixture.client, &attempt).await
        else {
            panic!("pending login expected")
        };
        let callback = issuer::Server::callback(&url).await;
        let complete = fixture
            .client
            .prepare(Command::SubmitConnectorInput {
                attempt: attempt.id,
                input: sailry_protocol::Secret::new(callback),
            })
            .with_plugin(scope);
        fixture.client.execute(complete.clone()).await.unwrap();
        fixture.client.execute(complete).await.unwrap();
        let authorization::LoginState::Connected(connection) =
            ready(&fixture.client, &attempt).await
        else {
            panic!("connected login expected")
        };
        let current = connection.package;
        assert_eq!(issuer.registrations.load(Ordering::SeqCst), 1);
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(
            model.requests.lock().unwrap().len(),
            count,
            "login must not replay the blocked model turn"
        );
        let status = fixture
            .client
            .prepare(Command::ReadPluginConnection {
                package: current.clone(),
                card: "connection".into(),
            })
            .with_plugin(context(&card, current));
        assert!(matches!(
            fixture.client.execute(status).await.unwrap(),
            Output::PluginConnection(plugin::connection::Status {
                authorization: plugin::connection::Authorization::Configured,
                ..
            })
        ));
        let restored = history(&fixture.client, fixture.session.id).await;
        let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
        assert_eq!(summary(&fixture.client, &restored, &view.calls).await, card);
        let history = serde_json::to_string(&restored).unwrap();
        for private in [
            &url,
            "access-initial",
            "refresh-initial",
            "fixture-code",
            "code_verifier",
            "code_challenge",
        ] {
            assert!(
                !history.contains(private),
                "private OAuth material escaped canonical history"
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_stale_and_foreign_scopes() {
    for remote in [false, true] {
        let input = super::super::mcp::input::Server::with_transport(
            super::super::mcp::input::form_schema(),
            false,
        )
        .await;
        let issuer = issuer::Server::start(&input.endpoint).await;
        let model =
            Server::tools(vec![(plugin_tool("card-fixture", "connection"), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        let package = install(&fixture, &issuer.endpoint).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
        let card = summary(&fixture.client, &page, &view.calls).await;
        for change in 0..6 {
            let mut scope = context(&card, card.package.clone());
            let mut command = Command::ReadPluginConnection {
                package: card.package.clone(),
                card: "connection".into(),
            };
            match change {
                0 => scope.session = Some(SessionId::new()),
                1 => scope.worktree = Some(WorktreeId::new()),
                2 => scope.turn = Some(turn),
                3 => scope.invocation = Some(sailry_protocol::RequestId::new()),
                4 => scope.package.digest = "b".repeat(64),
                5 => {
                    command = Command::ReadPluginConnection {
                        package: card.package.clone(),
                        card: "other".into(),
                    }
                }
                _ => unreachable!(),
            }
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(command).with_plugin(scope))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let mut forged = card.receipt;
        forged.call = sailry_protocol::RequestId::new();
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadPluginView {
                    package: card.package.clone(),
                    surface: plugin::desktop::Surface::Card(forged)
                }))
                .await
                .is_err()
        );
        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: turn,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        let mut forked = card.receipt;
        forked.session = branch.id;
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadPluginView {
                    package: card.package.clone(),
                    surface: plugin::desktop::Surface::Card(forked)
                }))
                .await
                .is_err()
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetPluginEnabled {
                name: package.summary.name.clone(),
                expected_revision: package.summary.revision,
                enabled: false,
            }))
            .await
            .unwrap();
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadPluginView {
                    package: card.package.clone(),
                    surface: plugin::desktop::Surface::Card(card.receipt)
                }))
                .await
                .is_err()
        );
        assert_eq!(issuer.registrations.load(Ordering::SeqCst), 0);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
