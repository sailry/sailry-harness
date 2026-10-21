use super::*;
use sailry_protocol::plugin::connector::{Input, Kind, LoginState};
use std::collections::BTreeMap;

const TOKEN: &str = "flow-private-token-marker";

async fn connect(fixture: &process::Fixture, package: &plugin::Info) {
    let Output::ConnectorLogin(attempt) = fixture
        .client
        .execute(fixture.client.prepare(Command::StartConnectorLogin {
            package: package.summary.reference(),
            connector: "figma".into(),
            connection: None,
            expected_revision: None,
            method: Kind::Secret,
            input: Input {
                secrets: BTreeMap::from([("token".into(), Secret::new(TOKEN.into()))]),
                ..Default::default()
            },
        }))
        .await
        .unwrap()
    else {
        panic!("connector login expected")
    };
    let mut updates = fixture
        .client
        .subscribe_connector_login(attempt.id)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Update::ConnectorLogin(update) = updates.next().await.unwrap() else {
                panic!("connector update expected")
            };
            match update.state {
                LoginState::Connected(_) => break,
                LoginState::Failed(error) => panic!("{error:?}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn keeps_context_on_the_protected_result_path() {
    for remote in [false, true] {
        for echo in [false, true] {
            let peer = super::super::http::server::Server::respond(
                Some(json!({"title":if echo {TOKEN} else {"Public response"}}).to_string()),
                None,
            )
            .await;
            let server = Server::tools(vec![(plugin_tool(PACKAGE, "explain"), json!({}))]).await;
            let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
            let original=install(&fixture,r#"
export function explain({step,outcome,state}) {
  if (step===0) return {call:{operation:'http.request',arguments:{method:'GET',url:'https://api.figma.com/echo',headers:{},body:null,connector:'figma',timeout_ms:30000}}};
  if (step===1) return {call:{operation:'storage.transaction',arguments:{operations:[{kind:'write',data:{key:'never',value:true,expected_revision:99}}]}},state:{response:outcome}};
  return {result:{isError:false,saved:true,context:{status:'changed',saved:false,previous:state.response}}};
}
"#,&[("explain",&["http.request","storage.transaction"])],None).await;
            let path = fixture.root.join("package/plugin.json");
            let mut manifest: Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            manifest["version"] = json!("1.0.1");
            let extension = &mut manifest["extensions"]["dev.sailry.platform"];
            let figma: Value =
                serde_json::from_str(include_str!("../../../../../../plugins/figma/plugin.json"))
                    .unwrap();
            extension["connectors"] =
                figma["extensions"]["dev.sailry.platform"]["connectors"].clone();
            std::fs::write(path, manifest.to_string()).unwrap();
            let Output::Plugin(package) = fixture
                .client
                .execute(fixture.client.prepare(Command::InstallPlugin {
                    worktree: fixture.session.worktree,
                    path: "package".into(),
                    name: PACKAGE.into(),
                    expected_revision: original.summary.revision,
                    expected_digest: None,
                }))
                .await
                .unwrap()
            else {
                panic!("package expected")
            };
            assert!(package.issues.is_empty(), "{:?}", package.issues);
            fixture
                .node
                .set_connector_fixture("https://api.figma.com", &peer.origin);
            connect(&fixture, &package).await;
            planning::configure(&mut fixture, WorkMode::Code).await;
            let turn = submit(&fixture, "Read the protected resource".into()).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            let output = results(&page);
            assert_eq!(output.len(), 1, "{page:?}");
            assert_eq!(output[0]["isError"], true);
            assert!(output[0].get("saved").is_none());
            if echo {
                assert_eq!(output[0]["error"]["code"], "permission_denied");
                assert!(output[0].get("context").is_none());
            } else {
                assert_eq!(
                    output[0]["error"]["details"],
                    json!({"kind":"plugin_value_conflict"})
                );
                assert_eq!(output[0]["context"]["saved"], false);
                assert!(
                    output[0]["context"]["previous"]["data"]["body"]
                        .as_str()
                        .unwrap()
                        .contains("Public response")
                );
            }
            assert_eq!(peer.requests.lock().unwrap().len(), 1);
            assert!(peer.requests.lock().unwrap()[0].0.contains(TOKEN));
            assert!(!serde_json::to_string(&page).unwrap().contains(TOKEN));
            assert!(
                !server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| request.to_string().contains(TOKEN))
            );
            assert!(!value(&fixture, &package, "never").await.present);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
