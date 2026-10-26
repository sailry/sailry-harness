use super::*;

#[tokio::test]
async fn retains_project_connections_across_turns() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = fixture(remote, &server, None).await;
        install(&fixture).await;
        configure(&mut fixture, Permission::Full).await;
        let private_database =
            save(&fixture.client, &fixture.root.join("private.sqlite3"), None).await;
        rusqlite::Connection::open(fixture.root.join("project.sqlite3"))
            .unwrap()
            .execute_batch("CREATE TABLE items(value INTEGER); INSERT INTO items VALUES (37)")
            .unwrap();
        let Output::SshProfile(private_host) = fixture
            .client
            .execute(fixture.client.prepare(Command::SaveSsh {
                profile: ssh::Profile {
                    id: SshId::new(),
                    revision: 0,
                    name: "Private host".into(),
                    host: "ssh.example.invalid".into(),
                    port: 22,
                    username: "fixture".into(),
                    authentication: ssh::Authentication::Agent,
                    host_key: None,
                    sharing: None,
                },
                expected_revision: 0,
                credential: Some(ssh::Credential::Agent),
            }))
            .await
            .unwrap()
        else {
            panic!("SSH profile expected")
        };
        let server = Server::turn_tools(vec![
            (tool("ssh_save_connection"), json!({"name":"Project host","host":"ssh.example.invalid","port":22,"username":"fixture","authentication":"agent"})),
            (tool("database_save_connection"), json!({"name":"Project database","engine":"sqlite","path":"project.sqlite3","read_only":false})),
        ]).await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        let host = ssh_profiles(&fixture.client)
            .await
            .into_iter()
            .find(|profile| profile.name == "Project host")
            .unwrap();
        let database = databases(&fixture.client)
            .await
            .into_iter()
            .find(|profile| profile.name == "Project database")
            .unwrap();
        let expected = Some(Sharing::Projects(vec![fixture.session.project.unwrap()]));
        assert_eq!(host.sharing, expected);
        assert_eq!(database.sharing, expected);
        let server = Server::turn_tools(vec![
            (tool("ssh_connections"),json!({})), (tool("database_connections"),json!({})),
            (tool("database_query"),json!({"connection":database.id,"sql":"SELECT value FROM items","row_limit":5,"timeout_ms":3000})),
            (tool("database_query"),json!({"connection":database.id,"sql":"SELECT value FROM absent","row_limit":5,"timeout_ms":3000})),
        ]).await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture).await;
        let mut page = finished(&fixture.client, fixture.session.id, turn).await;
        page.entries.retain(|entry| entry.turn == turn);
        let output = results(&fixture.client, &page).await;
        assert_eq!(output.len(), 4, "{output:?}");
        assert_eq!(output[0].as_array().unwrap().len(), 1);
        assert_eq!(output[0][0]["id"], host.id.to_string());
        assert_eq!(output[1].as_array().unwrap().len(), 1);
        assert_eq!(output[1][0]["id"], database.id.to_string());
        assert_eq!(output[2]["data"]["rows"][0][0]["value"], 37, "{output:?}");
        assert!(output[3]["error"]["code"].is_string(), "{output:?}");
        let view =
            super::super::super::progress::observe(&fixture.client, fixture.session.id).await;
        let queries = view
            .calls
            .iter()
            .filter(|call| call.turn == turn && call.name == tool("database_query"))
            .collect::<Vec<_>>();
        assert_eq!(queries.len(), 2);
        for call in queries {
            let call = call.with_detail(
                fixture
                    .client
                    .tool_detail(call.detail_reference(&page).unwrap())
                    .await
                    .unwrap(),
            );
            assert_eq!(
                call.resolved
                    .as_ref()
                    .unwrap()
                    .input
                    .summary
                    .as_ref()
                    .unwrap()
                    .text,
                "Project database"
            );
            assert_eq!(
                call.arguments(&page).unwrap()["connection"],
                database.id.to_string()
            );
        }
        let other_root = fixture.directory.path().join("other-project");
        std::fs::create_dir(&other_root).unwrap();
        let Output::Project(project) = fixture
            .client
            .execute(fixture.client.prepare(Command::RegisterProject {
                name: "Other project".into(),
                path: other_root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(fixture.session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let server = Server::turn_tools(vec![
            (tool("ssh_connections"), json!({})),
            (tool("database_connections"), json!({})),
        ])
        .await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            results(&fixture.client, &page).await,
            vec![json!([]), json!([])]
        );
        assert_eq!(
            ssh_profiles(&fixture.client)
                .await
                .into_iter()
                .find(|profile| profile.id == private_host.id)
                .unwrap(),
            private_host
        );
        assert_eq!(
            databases(&fixture.client)
                .await
                .into_iter()
                .find(|profile| profile.id == private_database.id)
                .unwrap(),
            private_database
        );
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(fixture.session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let server = Server::tools(vec![(tool("ssh_save_connection"),json!({"name":"Standalone host","host":"ssh.example.invalid","port":22,"username":"fixture","authentication":"agent"}))]).await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        let standalone = ssh_profiles(&fixture.client)
            .await
            .into_iter()
            .find(|profile| profile.name == "Standalone host")
            .unwrap();
        assert_eq!(standalone.sharing, None);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
