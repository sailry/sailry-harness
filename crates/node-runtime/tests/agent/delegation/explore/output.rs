use super::*;

const PACKAGE: &str = "explore-output";

fn result(server: &Server, name: &str) -> Value {
    server
        .requests
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|request| {
            let messages = request["messages"].as_array()?;
            let mut called = std::collections::HashMap::new();
            let mut result = None;
            for message in messages {
                for call in message["tool_calls"].as_array().into_iter().flatten() {
                    called.insert(call["id"].as_str()?, call["function"]["name"].as_str()?);
                }
                if message["role"] == "tool"
                    && called.get(message["tool_call_id"].as_str()?) == Some(&name)
                {
                    result = message["content"]
                        .as_str()
                        .and_then(|text| serde_json::from_str(text).ok());
                }
            }
            result
        })
        .expect("captured tool result")
}

#[tokio::test]
async fn follows_raw_output_with_read_authority() {
    for remote in [false, true] {
        let parent = Server::scripted_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"explore","task":"Read source.txt"}),
        )])
        .await;
        let child = Server::scripted_tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        )])
        .await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        let root = fixture.root().join("output");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("plugin.json"), json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":PACKAGE,"version":"0.1.0","extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":["process.read"],
                "tools":[{"name":"read","description":"Read an original tool output","operation":"process.read"}]
            }}
        }).to_string()).unwrap();
        let Output::Plugin(package) = execute(
            &fixture.client,
            Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "output".into(),
                name: PACKAGE.into(),
                expected_revision: 0,
                expected_digest: None,
            },
        )
        .await
        else {
            panic!("output reader expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let mut role = select(&mut fixture, false, false).await;
        role.model = fixture.role.model.clone();
        role.tools = Some(vec![
            plugin::conversation::Tool::Package {
                package: "files".into(),
                name: "read_file".into(),
            },
            plugin::conversation::Tool::Package {
                package: PACKAGE.into(),
                name: "read".into(),
            },
        ]);
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
            panic!("readonly role expected")
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
        let text = (0..120)
            .map(|line| format!("Retained exploration evidence {line}\n"))
            .collect::<String>();
        std::fs::write(fixture.root().join("source.txt"), text).unwrap();
        let first = fixture.push("Inspect the source").await;
        let page = finished(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let child_page = history(&fixture.client, child.id).await;
        let owner = child_page.runs[0].turn;
        assert_eq!(child_page.runs[0].status, Status::Completed);
        assert_eq!(child.config.mode, WorkMode::Plan);
        let (entry, index, call) = child_page
            .entries
            .iter()
            .find_map(|entry| {
                entry.parts.iter().enumerate().find_map(|(index, part)| {
                    if let Part::ToolResult {
                        id: Some(id), name, ..
                    } = part
                        && name == &plugin_tool("files", "read_file")
                    {
                        Some((entry, index, id))
                    } else {
                        None
                    }
                })
            })
            .unwrap();
        let retained = tool_detail(&fixture.client, &child_page, entry, index)
            .await
            .result
            .unwrap();
        let output = json!({"call":call,"revision":blake3::hash(retained.to_string().as_bytes()).to_hex().to_string()});
        let projected = result(&fixture.child, &plugin_tool("files", "read_file"));
        assert_eq!(projected, retained);
        fixture.child.replace_tools(vec![(
            plugin_tool(PACKAGE, "read"),
            json!({"output":output,"offset":0,"limit":32768}),
        )]);
        fixture.parent.replace_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"session":child.id,"task":"Read the original retained output"}),
        )]);
        let next = fixture.push("Read the original evidence").await;
        let page = finished(&fixture.client, fixture.session.id, next.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let raw = result(&fixture.child, &plugin_tool(PACKAGE, "read"));
        assert_eq!(raw["turn"], json!(owner));
        assert_eq!(raw["output"], output);
        assert_eq!(raw["offset"], 0);
        assert!(raw["next_offset"].is_null());
        let text = raw["text"].as_str().unwrap();
        assert_eq!(raw["total_bytes"], text.len());
        let original: Value = serde_json::from_str(text).unwrap();
        assert_eq!(original, retained);
        assert_eq!(
            raw["output"]["revision"],
            blake3::hash(text.as_bytes()).to_hex().to_string()
        );
        assert!(
            original
                .to_string()
                .contains("Retained exploration evidence 0")
        );
        for request in fixture.child.requests.lock().unwrap().iter() {
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect();
            assert!(names.contains(&plugin_tool(PACKAGE, "read").as_str()));
            assert!(!names.contains(&plugin_tool("commands", "run_command").as_str()));
        }
        assert!(
            history(&fixture.client, child.id)
                .await
                .approvals
                .is_empty()
        );
        fixture.close().await;
    }
}
