use super::*;

#[tokio::test]
async fn continues_after_command_denial() {
    for remote in [false, true] {
        let command = plugin_tool("commands", "run_command");
        let read = plugin_tool("files", "read_file");
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"explore","task":"Inspect source.txt without changing the project"}),
        )])
        .await;
        let child = Server::turn_tools(vec![
            (
                command.clone(),
                json!({"access":"unrestricted","command":"printf forbidden > forbidden.txt"}),
            ),
            (read.clone(), json!({"path":"source.txt"})),
        ])
        .await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        std::fs::write(fixture.root().join("source.txt"), "Read after denial").unwrap();
        let mut role = select(&mut fixture, true, false).await;
        role.tools
            .as_mut()
            .unwrap()
            .push(plugin::conversation::Tool::Package {
                package: "commands".into(),
                name: "run_command".into(),
            });
        let revision = role.revision;
        let Output::Role(role) = execute(
            &fixture.client,
            Command::PutRole {
                role,
                expected_revision: revision,
            },
        )
        .await
        else {
            panic!("role expected")
        };
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionRoles {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                roles: vec![role.reference()],
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = fixture.push("Explore the project").await;
        let parent = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(parent.runs[0].status, Status::Completed);
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        assert_eq!(child.config.mode, WorkMode::Plan);
        assert_eq!(child.config.permission, Permission::Full);
        let page = history(&fixture.client, child.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.runs[0].error.is_none());
        assert!(page.approvals.is_empty());
        assert!(page.questions.is_empty());
        assert!(!fixture.root().join("forbidden.txt").exists());
        let details = tool_details(&fixture.client, &page).await;
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].0, command);
        let denied = details[0].1.result.as_ref().unwrap();
        assert_eq!(denied["isError"], true);
        assert_eq!(
            denied["content"][0]["text"],
            "Tool execution was denied. Reason: planning turns cannot authorize tools with side effects"
        );
        assert_eq!(details[1].0, read);
        assert_eq!(
            details[1].1.result.as_ref().unwrap()["data"]["lines"][0]["text"],
            "Read after denial"
        );
        {
            let requests = fixture.child.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(
                requests[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| { tool["function"]["name"] == "run_command" })
            );
            assert!(
                requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| {
                        message["role"] == "tool"
                            && message["content"]
                                .as_str()
                                .is_some_and(|text| text.contains("planning turns"))
                    })
            );
        }
        fixture.close().await;
    }
}
