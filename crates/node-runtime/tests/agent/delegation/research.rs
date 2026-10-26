use super::{fixture::Fixture, *};

#[tokio::test]
async fn loads_the_skill_without_expanding_authority() {
    for remote in [false, true] {
        let parent = Server::parallel(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"research","task":"Investigate the assigned public sources within the given budget"}),
        )]).await;
        let unused = Server::start(false).await;
        let mut fixture = Fixture::start(remote, parent, unused).await;
        super::super::approvals::install(&fixture.client, &["web-search"]).await;
        let snapshot = fixture.snapshot().await;
        let role = snapshot
            .roles
            .into_iter()
            .find(|role| role.key == "research")
            .unwrap();
        assert!(role.read_only && role.model.is_none());
        assert_eq!(role.max_turns, Some(8));
        assert_eq!(role.skills, ["web-search:research"]);
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "web-search".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetPluginEnabled {
                name: "web-search".into(),
                expected_revision: package.summary.revision,
                enabled: true,
            }))
            .await
            .unwrap();
        let mut config = fixture.session.config.clone();
        config.mode = WorkMode::Code;
        config.permission = Permission::Full;
        config.options.insert(
            "delegation".into(),
            [
                ("proactive".into(), json!(true)),
                ("mode".into(), json!("multitask")),
            ]
            .into(),
        );
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionRoles {
                session: session.id,
                expected_revision: session.revision,
                roles: vec![role.reference()],
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = fixture.push("Delegate the research").await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(
            results(&fixture.client, &page).await[0]["status"],
            "completed"
        );
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let mut expected = turn.config;
        expected.mode = WorkMode::Plan;
        assert_eq!(child.config, expected);
        assert_eq!(child.config.options["delegation"]["proactive"], json!(true));
        assert_eq!(
            child.config.options["delegation"]["mode"],
            json!("multitask")
        );
        assert_eq!(child.delegation.as_ref().unwrap().role, Some(role.id));
        let requests = fixture.parent.requests.lock().unwrap().clone();
        let request = requests
            .iter()
            .find(|request| {
                request["tools"].as_array().unwrap().iter().all(|tool| {
                    tool["function"]["name"] != plugin_tool("delegation", "spawn_agent")
                })
            })
            .expect("delegated model request");
        let parent_request = requests
            .iter()
            .find(|request| {
                request["tools"].as_array().unwrap().iter().any(|tool| {
                    tool["function"]["name"] == plugin_tool("delegation", "spawn_agent")
                })
            })
            .expect("coordinator model request");
        assert!(
            parent_request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|message| message["content"].as_str())
                .any(|text| text.contains("enables autonomous delegation"))
        );
        let instructions = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .collect::<String>();
        assert!(!instructions.contains("enables autonomous delegation"));
        assert!(instructions.contains("Selected skill web-search:research"));
        assert!(instructions.contains("Search snippets identify candidates"));
        assert!(instructions.contains("six queries and eight distinct fetched sources"));
        assert!(instructions.contains("not additional user authorization"));
        for name in ["web_search", "web_fetch"] {
            assert!(
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == plugin_tool("web-search", name)),
                "missing research tool {name}"
            );
        }
        assert!(fixture.child.requests.lock().unwrap().is_empty());
        let mut changed = child.config.clone();
        changed.mode = WorkMode::Code;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: child.id,
                    expected_revision: child.revision,
                    config: changed,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        fixture.close().await;
    }
}
