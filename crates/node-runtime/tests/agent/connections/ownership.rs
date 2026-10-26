use super::*;

#[tokio::test]
async fn survives_without_projects() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile_path = directory.path().join("node");
        let node = Node::start_empty(&profile_path).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let personal = "Personal preferences apply to standalone connections";
        let unrelated = "Unrelated controller instructions";
        std::fs::write(node.profile().join("instructions.md"), personal).unwrap();
        std::fs::write(
            directory.path().join("controller/instructions.md"),
            unrelated,
        )
        .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        approvals::install(&client, &["databases", "ssh"]).await;
        let database = save(&client, &directory.path().join("database.sqlite3"), None).await;
        let ssh = SshId::new();
        client
            .execute(client.prepare(Command::SaveSsh {
                profile: sailry_protocol::ssh::Profile {
                    id: ssh,
                    revision: 0,
                    name: "Standalone SSH".into(),
                    host: "127.0.0.1".into(),
                    port: 22,
                    username: "fixture".into(),
                    authentication: sailry_protocol::ssh::Authentication::Password,
                    host_key: None,
                    sharing: None,
                },
                expected_revision: 0,
                credential: Some(sailry_protocol::ssh::Credential::Password {
                    password: Secret::new("fixture-password".into()),
                }),
            }))
            .await
            .unwrap();
        let server = Server::start(false).await;
        let provider = Provider {
            request_max_retries: None,
            stream_max_retries: None,
            preset: None,
            binding: None,
            consent: None,
            options: None,
            oauth: None,
            id: ProviderId::new(),
            revision: 0,
            name: "Connection model".into(),
            api: ModelApi::ChatCompletions,
            authentication: Authentication::ApiKey,
            endpoint: server.endpoint.clone(),
            enabled: true,
            credential: None,
            default_model: "fixture-a".into(),
            models: vec![Model {
                id: "fixture-a".into(),
                display_name: None,
                context: 16000,
                output: 2048,
                vision: false,
                tools: false,
                reasoning: false,
                web_search: false,
                generates: vec![],
                efforts: vec![],
                capabilities: Some(discovery::Capabilities {
                    tools: Some(false),
                    ..Default::default()
                }),
                custom_efforts: false,
                compaction_tokens: None,
                default_effort: Effort::Default,
            }],
        };
        let Output::Provider(saved) = client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected");
        };
        assert!(!saved.models[0].tools);
        let mut sessions = Vec::new();
        for resource in [Resource::Database(database.id), Resource::Ssh(ssh)] {
            let (name, assistant) = match resource {
                Resource::Database(_) => ("databases", "database"),
                Resource::Ssh(_) => ("ssh", "ssh"),
            };
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
                .await
                .unwrap()
            else {
                panic!("package expected");
            };
            let Output::Session(session) = client
                .execute(client.prepare(Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(SessionConfig {
                        features: Default::default(),
                        assistant: Some(sailry_protocol::plugin::conversation::Binding {
                            package: package.summary.reference(),
                            id: assistant.into(),
                        }),
                        resource: Some(resource),
                        provider: provider.id,
                        model: "fixture-a".into(),
                        effort: Effort::Default,
                        mode: WorkMode::Code,
                        options: Default::default(),
                        permission: Permission::Ask,
                        credential: None,
                    }),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            assert_eq!(session.project, None);
            assert_eq!(session.config.resource, Some(resource));
            let turn = approvals::submit(&client, session.id).await;
            let page = finished(&client, session.id, turn).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
            let requests = server.requests.lock().unwrap().clone();
            let request = requests.last().unwrap();
            let messages = request["messages"].as_array().unwrap();
            assert_eq!(messages[0]["role"], "system");
            let instructions = messages
                .iter()
                .filter(|message| message["role"] == "system")
                .map(|message| message["content"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            let base = include_str!("../../../src/agent/prompt.txt").trim();
            assert_eq!(instructions.matches(base).count(), 1);
            let (plugin, guidance) = match resource {
                Resource::Database(_) => ("databases", "You are the database workspace assistant"),
                Resource::Ssh(_) => ("ssh", "You are the SSH workspace assistant"),
            };
            assert!(instructions.contains(guidance), "{instructions}");
            assert_eq!(instructions.matches(personal).count(), 1);
            assert!(!instructions.contains(unrelated));
            assert!(
                messages
                    .iter()
                    .filter(|message| message["role"] == "user")
                    .all(|message| {
                        let content = message["content"].to_string();
                        !content.contains(personal) && !content.contains(unrelated)
                    })
            );
            assert!(!instructions.contains("Write the requested file"));
            assert_eq!(
                messages
                    .iter()
                    .filter(|message| message["role"] == "user")
                    .filter(|message| message["content"] == "Write the requested file")
                    .count(),
                1
            );
            assert!(
                request["tools"].is_null()
                    || request["tools"].as_array().is_some_and(Vec::is_empty),
                "unexpected tools: {}",
                request["tools"]
            );
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: plugin.into(),
                    expected_revision: package.summary.revision,
                    enabled: false,
                }))
                .await
                .unwrap();
            let blocked = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Inspect the saved connection".into(),
                }))
                .await
                .unwrap_err();
            assert_eq!(blocked.code, ErrorCode::NotConfigured);
            assert_eq!(server.requests.lock().unwrap().len(), requests.len());
            sessions.push(session);
        }
        let snapshot = configuration::snapshot(&client).await;
        assert!(snapshot.projects.is_empty());
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.worktrees.len(), 2);
        assert_ne!(sessions[0].worktree, sessions[1].worktree);
        for tree in &snapshot.worktrees {
            assert_eq!(tree.project, None);
            assert!(std::path::Path::new(&tree.path).starts_with(node.profile()));
        }
        let now = chrono::Utc::now().timestamp_millis();
        let Output::Usage(report) = client
            .execute(
                client.prepare(Command::ReadUsage(sailry_protocol::usage::Query {
                    start_ms: now - 86_400_000,
                    end_ms: now + 86_400_000,
                    dimension: sailry_protocol::usage::Dimension::Project,
                    utc_offset_minutes: 0,
                    projects: vec![],
                    worktrees: vec![],
                    providers: vec![],
                    models: vec![],
                    before: None,
                })),
            )
            .await
            .unwrap()
        else {
            panic!("usage expected")
        };
        assert!(report.totals.responses >= 2);
        assert!(
            report
                .groups
                .iter()
                .all(|group| matches!(group.key, sailry_protocol::usage::Key::Connection(_)))
        );
        assert!(
            report
                .requests
                .items
                .iter()
                .all(|request| request.project.is_none() && !request.scope_name.is_empty())
        );
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile_path).await.unwrap();
        let restored = configuration::snapshot(&Client::new(node.local())).await;
        assert!(restored.projects.is_empty());
        for session in sessions {
            let saved = restored
                .sessions
                .iter()
                .find(|entry| entry.id == session.id)
                .unwrap();
            assert_eq!(saved.project, None);
            assert_eq!(saved.config.resource, session.config.resource);
            assert_eq!(saved.worktree, session.worktree);
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
