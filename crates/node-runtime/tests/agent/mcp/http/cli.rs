use super::*;
use crate::{connector_fixture as connectors, github_fixture as source};
use sailry_protocol::plugin::connector::{Input, Kind, State};

#[tokio::test]
async fn expires_after_repeated_post_rejections() {
    for remote in [false, true] {
        let programs = tempfile::tempdir().unwrap();
        let cli = source::Cli::new(programs.path());
        let http = Http::start(Reply::Unauthorized).await;
        let model = Server::turn_tools(Vec::new()).await;
        let fixture = process::Fixture::environment(remote, &model, cli.source()).await;
        fixture.node.set_connector_fixture(
            connectors::ORIGIN,
            http.endpoint.strip_suffix("/mcp").unwrap(),
        );
        package(
            &fixture.root,
            json!({"input":{
                "type":"streamable-http", "url":format!("{}/mcp",connectors::ORIGIN),
                "headers":{"Authorization":""}
            }}),
            "0.1.0",
        );
        let manifest: Value =
            serde_json::from_str(include_str!("../../../../../../plugins/github/plugin.json"))
                .unwrap();
        let auth = manifest["extensions"][plugin::NAMESPACE]["connectors"][0]["auth"][1].clone();
        connectors::configure(
            &fixture.root.join("package"),
            json!({
                "id":"service","display":{"label":"Service"},"accounts":"single",
                "origins":[connectors::ORIGIN],"fields":[],"auth":[auth],
                "inject":[{"server":"input","header":"Authorization","prefix":"Bearer "}]
            }),
        );
        let info = install(&fixture, 0).await;
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        let row = connectors::connected(
            &fixture.client,
            fixture.client.prepare(Command::StartConnectorLogin {
                package: info.summary.reference(),
                connector: "service".into(),
                connection: None,
                expected_revision: None,
                method: Kind::LocalCli,
                input: Input::default(),
            }),
        )
        .await;
        let initial = cli.calls("token");
        let mut rotated = initial;
        for (index, expected) in [State::Connected, State::Expired].into_iter().enumerate() {
            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert_eq!(http.count("server/discover"), index + 1, "{remote}/{index}");
            assert_eq!(http.count("initialize"), 0);
            assert_eq!(http.count("tools/call"), 0);
            assert_eq!(http.requests.lock().unwrap().len(), index + 1);
            let Output::Connections(rows) =
                execute(&fixture.client, Command::ListConnections).await
            else {
                panic!("connections expected")
            };
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, row.id);
            assert_eq!(rows[0].state, expected, "{remote}/{index}");
            assert_eq!(rows[0].revision, row.revision);
            let calls = cli.calls("token");
            if index == 0 {
                assert_eq!(calls, initial + 2, "initial access and one 401 renewal");
                rotated = calls;
            } else {
                assert_eq!(calls, rotated, "the rejected replacement is not reacquired");
            }
            let public = serde_json::to_string(&(page, rows)).unwrap();
            assert!(!public.contains(source::TOKEN));
        }
        {
            let requests = http.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            for (method, headers, body) in requests.iter() {
                assert_eq!(method, "POST");
                assert_eq!(body["method"], "server/discover");
                assert_eq!(
                    headers["authorization"],
                    format!("Bearer {}", source::TOKEN)
                );
            }
        }
        assert!(
            !serde_json::to_string(&*model.requests.lock().unwrap())
                .unwrap()
                .contains(source::TOKEN)
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
