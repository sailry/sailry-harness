use super::*;
use plugin::connector::{Input, Kind, State};
use std::collections::BTreeMap;

const FIRST: &str = "first-private-${PLUGIN_ROOT}-credential";
const SECOND: &str = "second-private-${PLUGIN_DATA}-credential";

#[tokio::test]
async fn binds_distinct_environment_fields() {
    for remote in [false, true] {
        let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        isolate(&fixture.client).await;
        let mut server = peer::config("mcp::peer::stdio_peer", "credentials");
        server["env"]["SAILRY_MCP_TOKEN"] = json!("");
        server["env"]["SAILRY_MCP_SECOND"] = json!("");
        package(&fixture.root, json!({"native":server}), "0.1.0");
        crate::connector_fixture::configure(
            &fixture.root.join("package"),
            json!({
                "id":"service","display":{"label":"Service"},"accounts":"single",
                "origins":[crate::connector_fixture::ORIGIN],
                "auth":[{"kind":"secret","fields":["alpha","beta"]}],
                "fields":[{"name":"alpha","title":"Alpha","secret":true},
                    {"name":"beta","title":"Beta","secret":true}],
                "inject":[{"server":"native","env":"SAILRY_MCP_TOKEN","field":"beta"},
                    {"server":"native","env":"SAILRY_MCP_SECOND","field":"alpha"}]
            }),
        );
        let installed = install(&fixture, 0).await;
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let row = crate::connector_fixture::connected(
            &fixture.client,
            fixture.client.prepare(Command::StartConnectorLogin {
                package: installed.summary.reference(),
                connector: "service".into(),
                connection: None,
                expected_revision: None,
                method: Kind::Secret,
                input: Input {
                    secrets: BTreeMap::from([
                        ("alpha".into(), Secret::new(FIRST.into())),
                        ("beta".into(), Secret::new(SECOND.into())),
                    ]),
                    ..Default::default()
                },
            }),
        )
        .await;
        assert_eq!(row.state, State::Connected);
        let data = fixture.node.profile().join("plugins/data/example");
        for _ in 0..2 {
            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert_eq!(
                results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["hashes"],
                json!([
                    blake3::hash(SECOND.as_bytes()).as_bytes(),
                    blake3::hash(FIRST.as_bytes()).as_bytes()
                ])
            );
            for secret in [FIRST, SECOND] {
                assert!(!serde_json::to_string(&page).unwrap().contains(secret));
                assert!(
                    !serde_json::to_string(&*model.requests.lock().unwrap())
                        .unwrap()
                        .contains(secret)
                );
            }
        }
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        let calls = || {
            fs::read_to_string(data.join("requests.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .filter(|value| value["method"] == "tools/call")
                .count()
        };
        assert_eq!(calls(), 2);
        let (_, queued) = submit(&fixture, true).await;
        execute(
            &fixture.client,
            Command::DisconnectConnection {
                connection: row.id,
                expected_revision: row.revision,
            },
        )
        .await;
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(calls(), 2);
        for secret in [FIRST, SECOND] {
            assert!(!serde_json::to_string(&page).unwrap().contains(secret));
        }
        fixture.node.shutdown().await.unwrap();
        reaped(&data);
        fixture.controller.close().await.unwrap();
    }
}
