use super::*;

#[tokio::test]
async fn rebuilds_recognized_storage() {
    let markers = [
        "PRAGMA foreign_keys=OFF; INSERT INTO agent_events(sequence,session,id,turn,body)
         SELECT 1,id,'legacy-event','00000000-0000-4000-8000-000000000001',CAST('{\"author\":\"fixture\",\"content\":{\"parts\":[{\"text\":\"Private legacy answer\"}]}}' AS BLOB) FROM sessions LIMIT 1".into(),
        "INSERT OR REPLACE INTO search_settings(singleton,body) VALUES(1,CAST('{\"settings\":{\"prefer_native\":false}}' AS BLOB))".into(),
        "UPDATE roles SET body=CAST(json_set(body,'$.tools',json('[{\"kind\":\"model\",\"name\":\"web_search\"}]')) AS BLOB) WHERE id=(SELECT id FROM roles LIMIT 1)".into(),
        usage_index(false),
        usage_index(true),
    ];
    for remote in [false, true] {
        for sql in &markers {
            restart(remote, Some(sql)).await;
        }
    }
}

#[tokio::test]
async fn preserves_current_configuration() {
    for remote in [false, true] {
        restart(remote, None).await;
    }
}

#[tokio::test]
async fn backup_failure_preserves_old_storage() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    Node::start(&profile)
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    let path = profile.join("storage/node.turso");
    {
        let (_database, connection) = open(&path, false).await;
        connection.execute_batch("INSERT OR REPLACE INTO search_settings(singleton,body) VALUES(1,CAST('{\"settings\":{\"prefer_native\":true}}' AS BLOB))").await.unwrap();
        checkpoint(&connection).await;
    }
    std::fs::write(profile.join("storage/backups"), b"occupied fixture").unwrap();
    assert!(Node::start(&profile).await.is_err());
    let (_database, connection) = open(&path, true).await;
    assert_eq!(
        rows(
            &connection,
            "SELECT json_extract(body,'$.settings.prefer_native') FROM search_settings"
        )
        .await,
        vec![vec![Value::Integer(1)]]
    );
}

#[tokio::test]
async fn rejects_unrecognized_indexes() {
    assert_rejected(
        "DROP INDEX agent_usage; CREATE INDEX agent_usage ON agent_events(body) WHERE body != '$.usage_metadata'",
        "agent_usage",
    )
    .await;
}

async fn restart(remote: bool, marker: Option<&str>) {
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
    seed(&client, directory.path()).await;
    let Output::Snapshot(before) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let credentials = execute(&client, Command::ListCredentials).await;
    drop(client);
    node.shutdown().await.unwrap();
    let path = profile.join("storage/node.turso");
    let originals;
    {
        let (_database, connection) = open(&path, false).await;
        if let Some(sql) = marker {
            connection.execute_batch(sql).await.unwrap();
        }
        checkpoint(&connection).await;
        originals = contents(&connection).await;
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
    if marker.is_some() {
        assert!(after.projects.is_empty());
        assert!(after.worktrees.is_empty());
        assert!(after.sessions.is_empty());
        assert!(after.providers.is_empty());
        assert!(after.defaults.config.is_none());
        assert_eq!(
            execute(&client, Command::ListCredentials).await,
            Output::Credentials(vec![])
        );
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
    }
    assert_eq!(
        std::fs::read(directory.path().join("source/kept.txt")).unwrap(),
        b"project bytes"
    );
    drop(client);
    node.shutdown().await.unwrap();
    let saved = backups(&profile);
    assert_eq!(saved.len(), usize::from(marker.is_some()));
    if let Some(backup) = saved.first() {
        let (_database, connection) = open(backup, true).await;
        assert_eq!(contents(&connection).await, originals);
        assert!(rows(&connection, "SELECT hex(body) FROM providers").await.iter().any(|row| {
            matches!(&row[0], Value::Text(value) if value.contains(&hex("isolated-rebuild-credential")))
        }));
    }
    Node::start(&profile)
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    assert_eq!(backups(&profile), saved);
    controller.close().await.unwrap();
}

async fn seed(client: &Client, directory: &Path) {
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
    execute(
        client,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(config),
        },
    )
    .await;
}

async fn contents(connection: &Connection) -> Vec<Vec<Vec<Value>>> {
    let mut result = Vec::new();
    for query in [
        "SELECT id,hex(body) FROM providers ORDER BY id",
        "SELECT id,name,path FROM projects ORDER BY id",
        "SELECT session,revision,hex(config),hex(profile),hex(roles) FROM session_revisions ORDER BY session,revision",
        "SELECT hex(body) FROM agent_events ORDER BY body",
        "SELECT singleton,hex(body) FROM search_settings ORDER BY singleton",
        "SELECT id,key,hex(body) FROM roles ORDER BY id",
        "SELECT name,type,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name!='__turso_internal_mvcc_meta' ORDER BY name,type",
    ] {
        result.push(rows(connection, query).await);
    }
    result
}

fn hex(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02X}")).collect()
}

fn usage_index(timed: bool) -> String {
    let (name, prefix) = if timed {
        (
            "agent_usage_time",
            "substr(json_extract(body,'$.timestamp'),1,19),sequence,turn,json_extract(body,'$.timestamp')",
        )
    } else {
        ("agent_usage", "turn")
    };
    format!(
        "DROP INDEX {name}; CREATE INDEX {name} ON agent_events(
        {prefix},
        max(0,json_extract(body,'$.usage_metadata.prompt_token_count')),
        max(0,json_extract(body,'$.usage_metadata.candidates_token_count')),
        max(0,coalesce(json_extract(body,'$.usage_metadata.cache_read_input_token_count'),0)),
        max(0,coalesce(json_extract(body,'$.usage_metadata.thinking_token_count'),0))
    ) WHERE json_type(body,'$.usage_metadata')='object'"
    )
}
