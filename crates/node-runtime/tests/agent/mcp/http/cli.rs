use super::*;
use crate::{connector_fixture as connectors, github_fixture as source};
use sailry_protocol::plugin::connector::{Input, Kind, State};

async fn connect(fixture: &process::Fixture) -> sailry_protocol::plugin::connector::Connection {
    package(
        &fixture.root,
        json!({"input":{
            "type":"streamable-http", "url":format!("{}/mcp",connectors::ORIGIN),
            "headers":{"Authorization":""}
        }}),
        "0.1.0",
    );
    let manifest: Value =
        serde_json::from_str(include_str!("../../../../../../plugins/github/plugin.json")).unwrap();
    let auth = manifest["extensions"][plugin::NAMESPACE]["connectors"][0]["auth"][1].clone();
    connectors::configure(
        &fixture.root.join("package"),
        json!({
            "id":"service","display":{"label":"Service"},"accounts":"single",
            "origins":[connectors::ORIGIN],"fields":[],"auth":[auth],
            "inject":[{"server":"input","header":"Authorization","prefix":"Bearer "}]
        }),
    );
    let info = install(fixture, 0).await;
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    connectors::connected(
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
    .await
}

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
        let row = connect(&fixture).await;
        let initial = cli.calls("token");
        let mut rotated = initial;
        let fence = || -> u64 {
            let body: Vec<u8> = rusqlite::Connection::open_with_flags(
                fixture.node.profile().join("storage/node.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap()
            .query_row(
                "SELECT body FROM plugin_connections WHERE id=?1",
                [row.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
            serde_json::from_slice::<Value>(&body).unwrap()["grant_revision"]
                .as_u64()
                .unwrap()
        };
        let connected = fence();
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
            // Token reads never write; only the rotation and the expiry advance the fence.
            assert_eq!(fence(), connected + 1 + index as u64, "{remote}/{index}");
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

#[tokio::test]
async fn distinguishes_responses_from_connection_failures() {
    for remote in [false, true] {
        for intermediate in [0, 403, 503] {
            let programs = tempfile::tempdir().unwrap();
            let cli = source::Cli::new(programs.path());
            let status = Arc::new(std::sync::atomic::AtomicU16::new(401));
            let http = Http::start(Reply::Status(status.clone())).await;
            let model = Server::turn_tools(Vec::new()).await;
            let fixture = process::Fixture::environment(remote, &model, cli.source()).await;
            fixture.node.set_connector_fixture(
                connectors::ORIGIN,
                http.endpoint.strip_suffix("/mcp").unwrap(),
            );
            let row = connect(&fixture).await;
            let mut expected_calls = 0;
            let replies = if intermediate == 0 {
                vec![401, 0, 401]
            } else {
                vec![401, intermediate, 401, 401]
            };
            for (index, reply) in replies.iter().copied().enumerate() {
                status.store(reply, Ordering::SeqCst);
                let (_, turn) = submit(&fixture, false).await;
                let page = finished(&fixture.client, fixture.session.id, turn.id).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert_eq!(http.count("server/discover"), index + 1);
                assert_eq!(http.requests.lock().unwrap().len(), index + 1);
                assert_eq!(http.count("initialize"), 0);
                assert_eq!(http.count("tools/call"), 0);
                let Output::Connections(rows) =
                    execute(&fixture.client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].id, row.id);
                assert_eq!(
                    rows[0].state,
                    if index + 1 == replies.len() {
                        State::Expired
                    } else {
                        State::Connected
                    },
                    "{remote}/{intermediate}/{index}"
                );
                if index == 0 {
                    expected_calls = cli.calls("token");
                } else if intermediate != 0 && index == 2 {
                    expected_calls += 1;
                }
                assert_eq!(cli.calls("token"), expected_calls);
                let public = serde_json::to_string(&(page, rows)).unwrap();
                assert!(!public.contains(source::TOKEN));
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
