use super::*;
use sailry_protocol::plugin::connector::State;

const HTTP_SECRET: &str = "retired-http-private-marker";
const MCP_SECRET: &str = "retired-mcp-private-marker";
const ORIGIN: &str = "https://service.example.com";

fn declare(fixture: &process::Fixture, endpoint: &str) {
    package(
        &fixture.root,
        json!({"remote":{"type":"streamable-http","url":endpoint,"headers":{"Authorization":""}}}),
        "0.1.0",
    );
    let root = fixture.root.join("package");
    fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    let path = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{
        "api_version":"v1","actions":["http.request"],
        "settings_schema":"dev.sailry.platform/settings.json",
        "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["request"]},
        "tools":[{"name":"request","description":"Read the fixture","handler":{
            "name":"request","parameters":{"type":"object","properties":{}},
            "flow":{"operations":["http.request"]}
        }}]
    }});
    fs::write(path, manifest.to_string()).unwrap();
    let origin = url::Url::parse(endpoint)
        .unwrap()
        .origin()
        .ascii_serialization();
    fs::write(root.join("dev.sailry.platform/settings.json"), json!({
        "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
        "properties":{
            "http_token":{"type":"string","x-sailry-secret":{"origin":origin,"header":"X-Key"}},
            "mcp_token":{"type":"string","x-sailry-secret":{"server":"remote","header":"Authorization"}}
        },"required":["http_token","mcp_token"]
    }).to_string()).unwrap();
    let arguments = json!({"url":endpoint,"method":"POST","headers":{"Content-Type":"application/json"},
        "body":json!({"jsonrpc":"2.0","id":7,"method":"tools/list","params":{}}).to_string(),
        "credential":"http_token","timeout_ms":2000});
    fs::write(root.join("dev.sailry.platform/host/main.js"), format!(
        "export function request({{step,outcome}}) {{\nif (step === 0) return {{call:{{operation:'http.request',arguments:{arguments}}}}};\nreturn {{result:outcome,output:0}};\n}}"
    )).unwrap();
}

fn update(fixture: &process::Fixture) {
    let root = fixture.root.join("package");
    let path = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["version"] = json!("0.1.1");
    let extension = manifest["extensions"]["dev.sailry.platform"]
        .as_object_mut()
        .unwrap();
    extension.remove("settings_schema");
    extension.insert("connectors".into(), json!([{
        "id":"service","display":{"label":"Service"},"accounts":"single","origins":[ORIGIN],
        "auth":[{"kind":"secret","fields":["http_token","mcp_token"]}],
        "fields":[{"name":"http_token","title":"HTTP token","secret":true},{"name":"mcp_token","title":"MCP token","secret":true}],
        "inject":[{"origin":ORIGIN,"header":"X-Key","field":"http_token"},{"server":"remote","header":"Authorization","field":"mcp_token"}]
    }]));
    fs::write(path, manifest.to_string()).unwrap();
    fs::write(root.join("mcp.json"), json!({"$schema":plugin::mcp::SCHEMA,
        "mcpServers":{"remote":{"type":"streamable-http","url":format!("{ORIGIN}/mcp"),"headers":{"Authorization":""}}}
    }).to_string()).unwrap();
}

async fn complete(fixture: &process::Fixture, turn: TurnId) -> Page {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let page = history(&fixture.client, fixture.session.id).await;
            for approval in page
                .approvals
                .iter()
                .filter(|approval| approval.state == ApprovalState::Pending)
            {
                process::decide(&fixture.client, approval, Decision::Approve).await;
            }
            if page
                .runs
                .iter()
                .any(|run| run.turn == turn && run.status == Status::Completed)
            {
                return page;
            }
            assert!(
                !page
                    .runs
                    .iter()
                    .any(|run| run.turn == turn && run.status == Status::Failed),
                "{:?}",
                page.runs
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("captured credential deadline")
}

async fn retained(
    fixture: &process::Fixture,
    package: &plugin::Reference,
    turn: TurnId,
) -> (Vec<u8>, Vec<u8>) {
    let package = package.clone();
    fixture
        .node
        .with_storage(move |db| {
            Box::pin(async move {
                let settings = db
                    .prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2")
                    .await
                    .map_err(crate::database_fixture::fault)?
                    .query_row(turso::params![
                        package.name,
                        package.settings_revision as i64
                    ])
                    .await
                    .map_err(crate::database_fixture::fault)?
                    .get::<Vec<u8>>(0)
                    .map_err(crate::database_fixture::fault)?;
                let capture = db
                    .prepare("SELECT plugins FROM turns WHERE id=?1")
                    .await
                    .map_err(crate::database_fixture::fault)?
                    .query_row([turn.to_string()])
                    .await
                    .map_err(crate::database_fixture::fault)?
                    .get::<Vec<u8>>(0)
                    .map_err(crate::database_fixture::fault)?;
                Ok((settings, capture))
            })
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn fences_retired_protected_slots() {
    for remote in [false, true] {
        let peer = Http::start(Reply::Normal).await;
        let http_tool = plugin_tool("example", "request");
        let model = Server::turn_tools(vec![
            (alias("remote", "read"), json!({})),
            (http_tool.clone(), json!({})),
        ])
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        declare(&fixture, &peer.endpoint);
        let installed = install(&fixture, 0).await;
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let Output::PluginSettings(settings) = execute(
            &fixture.client,
            Command::SavePluginSettings {
                package: installed.summary.reference(),
                values: BTreeMap::new(),
                secrets: BTreeMap::from([
                    (
                        "http_token".into(),
                        plugin::settings::SecretUpdate::Replace(Secret::new(HTTP_SECRET.into())),
                    ),
                    (
                        "mcp_token".into(),
                        plugin::settings::SecretUpdate::Replace(Secret::new(MCP_SECRET.into())),
                    ),
                ]),
            },
        )
        .await
        else {
            panic!("settings expected")
        };
        let (_, first) = submit(&fixture, false).await;
        complete(&fixture, first.id).await;
        assert_eq!(peer.count("initialize"), 1);
        assert_eq!(peer.count("tools/call"), 1);
        {
            let requests = peer.requests.lock().unwrap();
            assert!(requests.iter().any(
                |(_, headers, _)| headers.get("x-key").map(String::as_str) == Some(HTTP_SECRET)
            ));
            assert!(requests.iter().any(|(_, headers, _)| {
                headers.get("authorization").map(String::as_str) == Some(MCP_SECRET)
            }));
        }
        let (_, queued) = submit(&fixture, true).await;
        assert!(queued.plugins.contains(&settings.package));
        let original = retained(&fixture, &settings.package, queued.id).await;
        let old: Value = serde_json::from_slice(&original.0).unwrap();
        assert_eq!(old["slots"]["http_token"], HTTP_SECRET);
        assert_eq!(old["slots"]["mcp_token"], MCP_SECRET);
        let Output::Plugin(previous) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "example".into(),
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        update(&fixture);
        let installed = install(&fixture, previous.summary.revision).await;
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let Output::Connections(connections) =
            execute(&fixture.client, Command::ListConnections).await
        else {
            panic!("connections expected")
        };
        let connection = connections
            .into_iter()
            .find(|row| row.package.name == "example")
            .unwrap();
        assert_eq!(connection.state, State::Connected);
        execute(
            &fixture.client,
            Command::DisconnectConnection {
                connection: connection.id,
                expected_revision: connection.revision,
            },
        )
        .await;
        assert_eq!(
            retained(&fixture, &settings.package, queued.id).await,
            original
        );
        let before = peer.requests.lock().unwrap().len();
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = complete(&fixture, queued.id).await;
        let result = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { name, result, .. } if name == &http_tool => Some(result),
                _ => None,
            })
            .next_back()
            .unwrap();
        assert_eq!(result["isError"], true, "{result}");
        assert_eq!(result["error"]["code"], "not_configured", "{result}");
        let unexpected = peer.requests.lock().unwrap()[before..]
            .iter()
            .map(|(method, _, body)| (method.clone(), body["method"].clone()))
            .collect::<Vec<_>>();
        assert!(
            unexpected.is_empty(),
            "retired credential requests: {unexpected:?}"
        );
        assert_eq!(
            retained(&fixture, &settings.package, queued.id).await,
            original
        );
        for secret in [HTTP_SECRET, MCP_SECRET] {
            assert!(!serde_json::to_string(&page).unwrap().contains(secret));
            assert!(
                !serde_json::to_string(&*model.requests.lock().unwrap())
                    .unwrap()
                    .contains(secret)
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
