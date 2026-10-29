use super::*;
use crate::delegation::fixture::Fixture;
use sailry_protocol::plugin::conversation::Tool;

fn names(request: &Value) -> Vec<&str> {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect()
}

#[tokio::test]
async fn discovers_defaults_and_preserves_explicit_selection() {
    for remote in [false, true] {
        for selected in [false, true] {
            let parent = Server::tools(vec![(
                plugin_tool("delegation", "spawn_agent"),
                json!({"role":"review","task":"List registered worktrees","background":false}),
            )])
            .await;
            let mut calls = Vec::new();
            if !selected {
                calls.push((
                    "tool_search".into(),
                    json!({"query":"list_worktrees","limit":1}),
                ));
            }
            let canonical = plugin_tool("worktrees", "list_worktrees");
            calls.push((canonical.clone(), json!({})));
            let child = Server::tools(calls).await;
            let mut fixture = Fixture::start(remote, parent, child).await;
            if selected {
                let mut role = fixture.role.clone();
                role.tools = Some(vec![Tool::Package {
                    package: "worktrees".into(),
                    name: "list_worktrees".into(),
                }]);
                let Output::Role(role) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::PutRole {
                        expected_revision: role.revision,
                        role,
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("role expected")
                };
                let Output::Session(session) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetSessionRoles {
                        session: fixture.session.id,
                        expected_revision: fixture.session.revision,
                        roles: vec![role.reference()],
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("session expected")
                };
                fixture.session = session;
            }
            let turn = fixture.push("Perform the selected child task").await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
            let child = children(&fixture.client, fixture.session.id, 1)
                .await
                .pop()
                .unwrap();
            let page = history(&fixture.client, child.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            let details = tool_details(&fixture.client, &page).await;
            assert_eq!(
                details
                    .iter()
                    .filter(|(name, _)| name == &canonical)
                    .count(),
                1
            );
            assert!(
                details
                    .iter()
                    .find(|(name, _)| name == &canonical)
                    .unwrap()
                    .1
                    .result
                    .is_some()
            );
            let requests = fixture.child.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), if selected { 2 } else { 3 });
            assert_eq!(names(&requests[0]).contains(&"list_worktrees"), selected);
            assert_eq!(names(&requests[0]).contains(&"tool_search"), !selected);
            assert_eq!(names(&requests[0]).contains(&"read_file"), !selected);
            assert!(names(requests.last().unwrap()).contains(&"list_worktrees"));
            assert!(
                requests
                    .iter()
                    .all(|request| !names(request).contains(&"spawn_agent"))
            );
            if !selected {
                let body = requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|message| message["role"] == "tool")
                    .unwrap()["content"]
                    .as_str()
                    .unwrap();
                let discovered: Value = serde_json::from_str(body).unwrap();
                assert_eq!(discovered["tools"][0]["name"], "list_worktrees");
                let original = details
                    .iter()
                    .find(|(name, _)| name == "tool_search")
                    .unwrap()
                    .1
                    .result
                    .as_ref()
                    .unwrap();
                assert_eq!(original["tools"][0]["name"], canonical);
            }
            fixture.close().await;
        }
    }
}
