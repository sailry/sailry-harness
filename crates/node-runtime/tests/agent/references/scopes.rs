use super::*;
use sailry_protocol::{connection::Resource, database};

async fn create(client: &Client, config: &SessionConfig) -> Session {
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(config.clone()),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

#[tokio::test]
async fn reads_unassigned_history_in_resource_sessions() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
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
        let original = Server::start(false).await;
        let project_session = approvals::prepare_packages(&client, &original, &root, &[]).await;
        let source = create(&client, &project_session.config).await;
        submit(
            &client,
            &source,
            "A prior decision about shared work 中文 🙂".into(),
        )
        .await;
        let deleted = create(&client, &source.config).await;
        client
            .execute(client.prepare(Command::RemoveSession {
                session: deleted.id,
                expected_revision: deleted.revision,
            }))
            .await
            .unwrap();
        let target = create(&client, &source.config).await;
        let path = directory.path().join("data.sqlite3");
        drop(rusqlite::Connection::open(&path).unwrap());
        let Output::DatabaseProfile(database) = client
            .execute(client.prepare(Command::SaveDatabase {
                profile: database::Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    name: "History fixture".into(),
                    connection: database::Connection::Sqlite {
                        path: path.to_str().unwrap().into(),
                    },
                    read_only: true,
                    sharing: None,
                },
                expected_revision: 0,
                password: None,
            }))
            .await
            .unwrap()
        else {
            panic!("database expected")
        };
        let Output::Session(target) = client
            .execute(client.prepare(Command::BindConnectionSession {
                session: target.id,
                expected_revision: target.revision,
                resource: Resource::Database(database.id),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_ne!(source.worktree, target.worktree);
        let reader = Server::tools(vec![
            ("list_sessions".into(), json!({})),
            ("read_session".into(), json!({"session":source.id})),
            (
                "search_session".into(),
                json!({"session":source.id,"text":"prior decision"}),
            ),
            ("read_session".into(), json!({"session":project_session.id})),
            ("read_session".into(), json!({"session":deleted.id})),
            (
                "search_session".into(),
                json!({"session":deleted.id,"text":"decision"}),
            ),
            (
                "read_session".into(),
                json!({"session":source.id,"revision":0}),
            ),
        ])
        .await;
        select_server(&client, &source, &reader).await;
        let turn = submit(
            &client,
            &target,
            Input {
                text: "Continue @Earlier work".into(),
                attachments: vec![],
                references: vec![Reference {
                    target: Target::Session(source.id),
                    label: "Earlier work".into(),
                }],
            },
        )
        .await;
        let results = history(&client, target.id)
            .await
            .entries
            .into_iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(results.len(), 7, "{results:?}");
        assert_eq!(results[0]["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(results[0]["sessions"][0]["session"], source.id.to_string());
        assert!(results[0]["sessions"][0].get("config").is_none());
        assert!(results[1].to_string().contains("prior decision"));
        assert_eq!(results[2]["matches"].as_array().unwrap().len(), 1);
        assert_eq!(results[3]["error"]["code"], "wrong_target");
        assert_eq!(results[4]["error"]["code"], "not_found");
        assert_eq!(results[5]["error"]["code"], "not_found");
        assert_eq!(results[6]["error"]["code"], "revision_conflict");
        let requests = reader.requests.lock().unwrap();
        for name in ["list_sessions", "read_session", "search_session"] {
            let declaration = requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == name)
                .unwrap();
            assert!(
                declaration["function"]["parameters"]["properties"]["session"]
                    .get("enum")
                    .is_none()
            );
        }
        drop(requests);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
