use super::*;
use crate::mcp::peer;
use sailry_protocol::{connection::Resource, database, ssh};

async fn package(fixture: &process::Fixture) -> plugin::Info {
    let info = install(fixture, 0, false).await;
    let root = fixture.root.join("assistant");
    let path = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let declarations =
        &mut manifest["extensions"]["dev.sailry.platform"]["desktop"]["conversations"];
    *declarations = json!(["database", "ssh"].map(|resource| json!({
        "id":resource, "resource":resource, "context":CONTEXT,
        "tools":[
            {"kind":"package", "package":"files", "name":"read_file"},
            {"kind":"plugin", "name":"read_note"},
            {"kind":"plugin", "server":"notes", "name":"read"}
        ]
    })));
    std::fs::write(path, manifest.to_string()).unwrap();
    let server = peer::config("mcp::peer::stdio_peer", "normal");
    std::fs::write(
        root.join("mcp.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers":{"notes":server, "unused":server}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(root.join("version.txt"), "Resource assistant").unwrap();
    load(fixture, info.summary.revision).await
}

async fn connection(fixture: &process::Fixture, kind: &str) -> Resource {
    let command = match kind {
        "database" => {
            let path = fixture.directory.path().join("assistant.sqlite3");
            drop(rusqlite::Connection::open(&path).unwrap());
            Command::SaveDatabase {
                profile: database::Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    name: "Assistant database".into(),
                    connection: database::Connection::Sqlite {
                        path: path.to_str().unwrap().into(),
                    },
                    read_only: true,
                    sharing: None,
                },
                expected_revision: 0,
                password: None,
            }
        }
        "ssh" => Command::SaveSsh {
            profile: ssh::Profile {
                id: SshId::new(),
                revision: 0,
                name: "Assistant SSH".into(),
                host: "127.0.0.1".into(),
                port: 22,
                username: "fixture".into(),
                authentication: ssh::Authentication::Password,
                host_key: None,
                sharing: None,
            },
            expected_revision: 0,
            credential: Some(ssh::Credential::Password {
                password: Secret::new("fixture".into()),
            }),
        },
        _ => unreachable!("unknown fixture resource"),
    };
    match execute(&fixture.client, command).await {
        Output::DatabaseProfile(profile) => Resource::Database(profile.id),
        Output::SshProfile(profile) => Resource::Ssh(profile.id),
        _ => panic!("connection expected"),
    }
}

#[tokio::test]
async fn tools() {
    for remote in [false, true] {
        for kind in ["database", "ssh"] {
            let mcp = peer::package_alias(PACKAGE, "notes", "read");
            let server = Server::tools(vec![
                (alias("read_note"), json!({"key":"review"})),
                (mcp.clone(), json!({"value":"Resource read"})),
            ])
            .await;
            let fixture = process::Fixture::new(remote, &server).await;
            let info = package(&fixture).await;
            let resource = connection(&fixture, kind).await;
            let mut config = fixture.session.config.clone();
            config.resource = Some(resource);
            config.assistant = Some(Binding {
                package: info.summary.reference(),
                id: kind.into(),
            });
            config.permission = Permission::Full;
            let Output::Session(session) = execute(
                &fixture.client,
                Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(config),
                },
            )
            .await
            else {
                panic!("session expected")
            };
            assert_eq!(session.config.resource, Some(resource));
            assert_eq!(
                session.config.assistant.as_ref().unwrap().package,
                info.summary.reference()
            );
            seed(&fixture, &session, &info).await;
            let turn = submit(&fixture.client, &session, false).await;
            let page = finished(&fixture.client, session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
            note(&page);
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| {
                        matches!(part, Part::ToolResult { name, result, .. } if name == &mcp
                    && result["output"]["version"] == "Resource assistant"
                    && result["output"]["value"] == "Resource read")
                    })
            );
            let request = server.requests.lock().unwrap()[0].clone();
            let tools: BTreeSet<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(
                tools,
                BTreeSet::from([
                    alias("read_note"),
                    mcp,
                    "ask_user".into(),
                    "set_session_title".into(),
                    "compact_context".into(),
                ])
            );
            let data = fixture.node.profile().join("plugins/data").join(PACKAGE);
            assert_eq!(std::fs::read_to_string(data.join("launches")).unwrap(), "x");
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
