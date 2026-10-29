use super::*;

const CONTENT: &[u8] = b"\0private attachment fixture\xff\n";

#[tokio::test]
async fn rebuilds_unmarked_storage() {
    for remote in [false, true] {
        restart(remote, true).await;
    }
}

#[tokio::test]
async fn preserves_current_configuration() {
    for remote in [false, true] {
        restart(remote, false).await;
    }
}

#[tokio::test]
async fn rejects_invalid_and_future_values() {
    for value in ["0", "-1", "2", "'unknown'"] {
        assert_rejected(
            &format!("UPDATE node SET storage_generation={value} WHERE singleton=1"),
            "node.storage_generation",
        )
        .await;
    }
}

#[tokio::test]
async fn rejects_unrecognized_indexes() {
    assert_rejected(
        "DROP INDEX agent_usage; CREATE INDEX agent_usage ON agent_events(body) WHERE body != '$.usage_metadata'",
        "agent_usage",
    )
    .await;
}

#[tokio::test]
async fn rejects_unowned_unmarked_storage() {
    for (sql, detail) in [
        (
            "ALTER TABLE node DROP COLUMN storage_generation; PRAGMA application_id=1",
            "application_id",
        ),
        (
            "ALTER TABLE node DROP COLUMN storage_generation; UPDATE node SET identity=zeroblob(32)",
            "identity",
        ),
    ] {
        assert_rejected(sql, detail).await;
    }
}

async fn restart(remote: bool, rebuild: bool) {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let identity = node.id();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
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
    let attachment = seed(&client, directory.path()).await;
    let attachment_path = profile.join("attachments").join(attachment.id.to_string());
    assert_eq!(std::fs::read(&attachment_path).unwrap(), CONTENT);
    let Output::Snapshot(before) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let credentials = execute(&client, Command::ListCredentials).await;
    drop(client);
    node.shutdown().await.unwrap();
    let path = profile.join("storage/node.turso");
    let knowledge = profile.join("storage/knowledge.turso");
    let knowledge_body;
    {
        let (_database, connection) = open(&knowledge, false).await;
        connection
            .execute_batch("CREATE TABLE retained_fixture(body BLOB NOT NULL); INSERT INTO retained_fixture VALUES(X'007FFF')")
            .await
            .unwrap();
        checkpoint(&connection).await;
        knowledge_body = rows(&connection, "SELECT body FROM retained_fixture").await;
    }
    {
        let (_database, connection) = open(&path, false).await;
        if rebuild {
            connection
                .execute_batch(
                    "ALTER TABLE node DROP COLUMN storage_generation;
                     UPDATE providers SET body=CAST(json_set(body,
                         '$.provider.api','gemini',
                         '$.provider.models',json('[{\"id\":\"fixture\",\"context\":128000,\"output\":8192,\"reasoning\":true,\"efforts\":[\"minimal\",{\"budget\":-1}],\"default_effort\":\"minimal\"}]'),
                         '$.provider.binding',json('{\"package\":null,\"policies\":[],\"capabilities\":{\"web_search\":true}}')
                     ) AS BLOB) WHERE json_type(body,'$.provider')='object';
                     UPDATE session_revisions SET config=CAST(json_set(config,'$.effort',json('{\"budget\":-1}')) AS BLOB)",
                )
                .await
                .unwrap();
            let provider = connection
                .prepare("SELECT json_extract(body,'$.provider') FROM providers WHERE json_type(body,'$.provider')='object'")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<String>(0)
                .unwrap();
            assert!(serde_json::from_str::<conversation::Provider>(&provider).is_err());
            let mut provider: serde_json::Value = serde_json::from_str(&provider).unwrap();
            provider["binding"]
                .as_object_mut()
                .unwrap()
                .remove("capabilities");
            let provider: conversation::Provider = serde_json::from_value(provider).unwrap();
            assert_eq!(provider.api, conversation::ModelApi::Gemini);
            assert!(provider.models[0].validate_reasoning(provider.api).is_err());
            assert!(Effort::Budget(-1).validate(provider.api, 8192).is_err());
        }
        checkpoint(&connection).await;
    }
    let node = Node::start(&profile).await.unwrap();
    assert_eq!(node.id(), identity);
    // Rebuilding removes the Node's saved controller trust along with other configuration.
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
    let Output::Snapshot(after) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    if rebuild {
        assert!(after.projects.is_empty());
        assert!(after.worktrees.is_empty());
        assert!(after.sessions.is_empty());
        assert!(after.providers.is_empty());
        assert!(after.defaults.config.is_none());
        assert_eq!(
            execute(&client, Command::ListCredentials).await,
            Output::Credentials(vec![])
        );
        assert!(!attachment_path.exists());
    } else {
        assert_eq!(after.projects, before.projects);
        assert_eq!(after.worktrees, before.worktrees);
        assert_eq!(after.sessions, before.sessions);
        assert_eq!(after.providers, before.providers);
        assert_eq!(after.defaults, before.defaults);
        assert_eq!(
            execute(&client, Command::ListCredentials).await,
            credentials
        );
        assert_eq!(std::fs::read(&attachment_path).unwrap(), CONTENT);
    }
    assert_eq!(
        std::fs::read(directory.path().join("source/kept.txt")).unwrap(),
        b"project bytes"
    );
    drop(client);
    node.shutdown().await.unwrap();
    assert!(backups(&profile).is_empty());
    {
        let (_database, connection) = open(&path, true).await;
        assert_eq!(
            rows(&connection, "SELECT storage_generation FROM node").await,
            vec![vec![Value::Integer(1)]]
        );
    }
    {
        let (_database, connection) = open(&knowledge, true).await;
        if rebuild {
            assert!(
                rows(
                    &connection,
                    "SELECT name FROM sqlite_schema WHERE name='retained_fixture'"
                )
                .await
                .is_empty()
            );
        } else {
            assert_eq!(
                rows(&connection, "SELECT body FROM retained_fixture").await,
                knowledge_body
            );
        }
    }
    let node = Node::start(&profile).await.unwrap();
    assert_eq!(node.id(), identity);
    let client = Client::new(if remote {
        controller.handle().remote(node.link().address())
    } else {
        node.local()
    });
    let Output::Snapshot(restarted) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    assert_eq!(restarted.projects, after.projects);
    assert_eq!(restarted.worktrees, after.worktrees);
    assert_eq!(restarted.sessions, after.sessions);
    assert_eq!(restarted.providers, after.providers);
    assert_eq!(restarted.defaults, after.defaults);
    assert_eq!(
        execute(&client, Command::ListCredentials).await,
        if rebuild {
            Output::Credentials(vec![])
        } else {
            credentials
        }
    );
    if !rebuild {
        assert_eq!(std::fs::read(&attachment_path).unwrap(), CONTENT);
    }
    drop(client);
    node.shutdown().await.unwrap();
    assert!(backups(&profile).is_empty());
    controller.close().await.unwrap();
}

async fn seed(client: &Client, directory: &Path) -> attachment::Attachment {
    let source = directory.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("kept.txt"), b"project bytes").unwrap();
    let Output::Project(project) = execute(
        client,
        Command::RegisterProject {
            name: "Rebuild fixture".into(),
            path: source.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let provider: conversation::Provider = serde_json::from_value(serde_json::json!({
        "id":ProviderId::new(), "revision":0, "name":"Rebuild fixture",
        "api":"chat_completions", "authentication":"api_key",
        "endpoint":"http://127.0.0.1:1/v1", "enabled":true,
        "default_model":"fixture", "credential":null
    }))
    .unwrap();
    let Output::Provider(provider) = execute(
        client,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    execute(
        client,
        Command::PutCredential {
            id: CredentialId::new(),
            provider: provider.id,
            expected_revision: 0,
            secret: Secret::new("isolated-rebuild-credential".into()),
            expires_at_ms: None,
        },
    )
    .await;
    let config = SessionConfig {
        provider: provider.id,
        model: "fixture".into(),
        assistant: None,
        resource: None,
        effort: Effort::Default,
        features: Default::default(),
        options: Default::default(),
        mode: WorkMode::Code,
        permission: Permission::Ask,
        credential: None,
    };
    execute(
        client,
        Command::SetDefaults {
            expected_revision: 0,
            config: config.clone(),
        },
    )
    .await;
    let Output::Session(session) = execute(
        client,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let spec = attachment::Spec {
        worktree: session.worktree,
        name: "fixture.bin".into(),
        media_type: "application/octet-stream".into(),
        size: CONTENT.len() as u64,
        revision: blake3::hash(CONTENT).to_hex().to_string(),
    };
    let Output::AttachmentUpload(upload) = execute(client, Command::UploadAttachment(spec)).await
    else {
        panic!("attachment upload expected")
    };
    client
        .upload(
            &upload,
            &mut &CONTENT[..],
            sailry_link::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    let Output::Attachment(attachment) = execute(
        client,
        Command::FinishAttachmentUpload {
            worktree: session.worktree,
            stream: upload.stream,
        },
    )
    .await
    else {
        panic!("attachment expected")
    };
    attachment
}
