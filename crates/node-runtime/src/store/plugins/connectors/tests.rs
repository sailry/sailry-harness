use super::*;
use crate::store::database::Database;
use serde_json::json;
use settings::history;
use std::path::PathBuf;

struct Fixture {
    directory: tempfile::TempDir,
    path: PathBuf,
    database: Option<Database>,
    host: crate::plugins::Host,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let path = root.join("node.turso");
        let database = Database::open(&path, sailry_protocol::NodeId([74; 32]), Some(root.clone()))
            .await
            .unwrap();
        let host = crate::plugins::Host::new(Some(root));
        Self {
            directory,
            path,
            database: Some(database),
            host,
        }
    }
    fn database(&self) -> &Database {
        self.database.as_ref().unwrap()
    }
    async fn legacy(&self, name: &str, cli: bool) -> (Info, Vec<u8>) {
        let mut info = self.host.install_bundled(name).unwrap();
        let declaration = info.extension.as_ref().unwrap().connectors[0].clone();
        info.summary.version = Some("0.0.1".into());
        info.origin = Some(plugin::Origin::Bundled);
        info.extension.as_mut().unwrap().connectors.clear();
        super::super::save(&self.database().connection, &info, true)
            .await
            .unwrap();
        let mut saved = settings::stored(&self.database().connection, &info)
            .await
            .unwrap();
        saved
            .values
            .insert("unrelated".into(), json!("preserve draft"));
        for field in &declaration.fields {
            if field.secret {
                saved.slots.insert(
                    field.name.clone(),
                    Some(Secret::new(format!("private-{}", field.name))),
                );
            } else {
                saved.values.insert(
                    field.name.clone(),
                    json!(field.default.as_deref().unwrap_or("legacy-client")),
                );
            }
        }
        if matches!(name, "google-workspace" | "microsoft-365") {
            saved.oauth = Some(history::Authorized {
                id: sailry_protocol::CredentialId::new(),
                revision: 3,
                grant: Some(history::Grant {
                    declaration: history::Declaration {
                        provider: if name == "microsoft-365" {
                            "microsoft"
                        } else {
                            "google"
                        }
                        .into(),
                        scopes: vec!["read".into()],
                        credentials: vec!["token".into()],
                    },
                    configuration: history::Configuration {
                        client_id: "legacy-client".into(),
                        client_secret: Some(Secret::new("private-client-secret".into())),
                        tenant: (name == "microsoft-365").then(|| "retained-tenant".into()),
                    },
                    access_token: Secret::new("private-access".into()),
                    refresh_token: Some(Secret::new("private-refresh".into())),
                    expires_at_ms: u64::MAX / 2,
                }),
            });
        }
        if matches!(name, "notion" | "slack") {
            let Auth::McpOauth { server, .. } = &declaration.auth[0] else {
                panic!("MCP method expected")
            };
            saved.authorizations.insert(
                server.clone(),
                history::McpGrant {
                    id: sailry_protocol::CredentialId::new(),
                    endpoint: if name == "slack" {
                        "https://mcp.slack.com/mcp"
                    } else {
                        "https://mcp.notion.com/mcp"
                    }
                    .into(),
                    revision: 3,
                    credentials: Some(Secret::new("private-mcp-grant".into())),
                },
            );
        }
        if cli {
            saved.slots.clear();
            saved.github = Some(history::Identity {
                login: "legacy-user".into(),
                source: "keyring".into(),
            });
        }
        settings::persist(&self.database().connection, &mut info, &saved)
            .await
            .unwrap();
        let bytes = encode(&saved).unwrap();
        (info, bytes)
    }
    async fn history(&self, info: &Info) -> Vec<u8> {
        (async {
            let mut statement = self
                .database()
                .connection
                .prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2")
                .await?;
            let row = statement
                .query_row(params![
                    info.summary.name.as_str(),
                    info.summary.settings_revision as i64
                ])
                .await?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
            mapped
        })
        .await
        .unwrap()
    }
    async fn upgrade(&mut self) -> Result<(), crate::Error> {
        let previous = self.database.take().unwrap();
        let node = previous.node;
        previous.close().await?;
        let database = Database::open_with_plugins(
            &self.path,
            node,
            Some(self.directory.path().canonicalize()?),
            Some(&self.host),
        )
        .await?;
        self.database = Some(database);
        Ok(())
    }
}

#[tokio::test]
async fn imports_credentials_and_preserves_captures() {
    for cli in [false, true] {
        let mut fixture = Fixture::new().await;
        let mut originals = Vec::new();
        for name in [
            "figma",
            "dingtalk",
            "google-workspace",
            "notion",
            "github",
            "microsoft-365",
            "slack",
            "feishu",
        ] {
            originals.push(fixture.legacy(name, cli && name == "github").await);
        }
        fixture.upgrade().await.unwrap();
        let records = records(&fixture.database().connection).await.unwrap();
        assert_eq!(records.len(), 8);
        for (original, bytes) in &originals {
            assert_eq!(fixture.history(original).await, *bytes);
            let current = required(&fixture.database().connection, &original.summary.name)
                .await
                .unwrap();
            let settings = settings::stored(&fixture.database().connection, &current)
                .await
                .unwrap();
            assert_eq!(settings.values["unrelated"], "preserve draft");
            let row = records
                .iter()
                .find(|row| row.plugin == original.summary.name)
                .unwrap();
            assert_eq!(row.state, State::Connected);
            let projected = project(&fixture.database().connection, fixture.host.profile(), row)
                .await
                .unwrap();
            assert!(projected.consent.is_none());
            assert!(
                !serde_json::to_string(&projected)
                    .unwrap()
                    .contains("private-")
            );
            if row.method == Kind::LocalCli {
                assert!(row.grant.is_none() && row.secrets.is_empty());
                assert_eq!(row.identity.as_ref().unwrap().id, "legacy-user");
            }
            if row.plugin == "microsoft-365" {
                assert_eq!(row.fields["tenant"], "retained-tenant");
            }
            if row.plugin == "slack" {
                assert_eq!(row.method, Kind::McpOauth);
                assert_eq!(row.fields["client_id"], "legacy-client");
                assert_eq!(
                    row.secrets["client_secret"].expose(),
                    "private-client_secret"
                );
            }
            if row.plugin == "feishu" {
                assert_eq!(row.method, Kind::Application);
                assert_eq!(row.fields["app_id"], "legacy-client");
                assert_eq!(row.secrets["app_secret"].expose(), "private-app_secret");
            }
        }
        let backup = std::fs::read_dir(fixture.directory.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(backup.len(), 1);
        let backup_handle = crate::store::database::engine(&backup[0].join("node.turso"))
            .await
            .unwrap();
        let previous = crate::store::database::connect(&backup_handle)
            .await
            .unwrap();
        previous.pragma_update("query_only", 1).await.unwrap();
        for (original, bytes) in originals {
            let retained: Vec<u8> = (async {
                let mut statement = previous
                    .prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2")
                    .await?;
                let row = statement
                    .query_row(params![
                        original.summary.name.as_str(),
                        original.summary.settings_revision as i64
                    ])
                    .await?;
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                mapped
            })
            .await
            .unwrap();
            assert_eq!(retained, bytes);
        }
        drop(previous);
        drop(backup_handle);
        fixture.upgrade().await.unwrap();
        assert_eq!(
            std::fs::read_dir(fixture.directory.path().join("backups"))
                .unwrap()
                .filter(|entry| entry.as_ref().unwrap().path().is_dir())
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn skips_draft_defaults_without_saved_configuration() {
    let fixture = Fixture::new().await;
    for name in [
        "notion",
        "linear",
        "figma",
        "slack",
        "feishu",
        "tencent-docs",
        "google-workspace",
        "microsoft-365",
        "wecom",
        "dingtalk",
        "yuque",
        "github",
        "context7",
    ] {
        let mut info = fixture.host.install_bundled(name).unwrap();
        super::super::save(&fixture.database().connection, &info, true)
            .await
            .unwrap();
        assert!(
            !needs_migration(&fixture.database().connection, &info)
                .await
                .unwrap()
        );
        assert!(
            !migrate(
                &fixture.database().connection,
                fixture.host.profile(),
                &mut info
            )
            .await
            .unwrap()
        );
        assert_eq!(info.summary.settings_revision, 0);
    }
    assert!(
        records(&fixture.database().connection)
            .await
            .unwrap()
            .is_empty()
    );
    let mut info = required(&fixture.database().connection, "microsoft-365")
        .await
        .unwrap();
    let mut saved = settings::stored(&fixture.database().connection, &info)
        .await
        .unwrap();
    saved.values.insert("unrelated".into(), json!("retained"));
    settings::persist(&fixture.database().connection, &mut info, &saved)
        .await
        .unwrap();
    assert!(
        !needs_migration(&fixture.database().connection, &info)
            .await
            .unwrap()
    );
    assert!(
        !migrate(
            &fixture.database().connection,
            fixture.host.profile(),
            &mut info
        )
        .await
        .unwrap()
    );
    assert!(
        records(&fixture.database().connection)
            .await
            .unwrap()
            .is_empty()
    );
    saved.values.insert("tenant".into(), json!("common"));
    settings::persist(&fixture.database().connection, &mut info, &saved)
        .await
        .unwrap();
    assert!(
        needs_migration(&fixture.database().connection, &info)
            .await
            .unwrap()
    );
    assert!(
        migrate(
            &fixture.database().connection,
            fixture.host.profile(),
            &mut info
        )
        .await
        .unwrap()
    );
    let rows = records(&fixture.database().connection).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].fields["tenant"], "common");
}

#[tokio::test]
async fn revokes_changed_relogin_before_persisting() {
    let mut fixture = Fixture::new().await;
    fixture.legacy("google-workspace", false).await;
    fixture.upgrade().await.unwrap();
    let mut previous = records(&fixture.database().connection)
        .await
        .unwrap()
        .remove(0);
    assert!(previous.grant.is_some());
    previous.revision = 10;
    save(&fixture.database().connection, &previous)
        .await
        .unwrap();
    let info = required(&fixture.database().connection, &previous.plugin)
        .await
        .unwrap();
    for edit in ["unchanged", "client", "secret"] {
        let (mut record, _) = create(
            &fixture.database().connection,
            fixture.host.profile(),
            &Command::StartConnectorLogin {
                package: info.summary.reference(),
                connector: previous.connector.clone(),
                connection: Some(previous.id),
                expected_revision: Some(previous.revision),
                method: previous.method,
                input: Input {
                    fields: if edit == "client" {
                        BTreeMap::from([("client_id".into(), "different-client".into())])
                    } else {
                        BTreeMap::new()
                    },
                    secrets: if edit == "secret" {
                        BTreeMap::from([(
                            "client_secret".into(),
                            Secret::new("private-new-client".into()),
                        )])
                    } else {
                        BTreeMap::new()
                    },
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();
        next(&mut record).unwrap();
        validate(&record).unwrap();
        assert_eq!(record.revision, previous.revision + 1);
        assert_eq!(record.grant.is_none(), edit != "unchanged");
        assert_eq!(
            record.auth_revision,
            if edit == "unchanged" {
                previous.auth_revision
            } else {
                record.revision
            }
        );
    }
    assert!(
        encode(
            &read(&fixture.database().connection, previous.id)
                .await
                .unwrap()
        )
        .unwrap()
            == encode(&previous).unwrap()
    );
}

#[tokio::test]
async fn rejects_upgrade_without_writing() {
    let mut fixture = Fixture::new().await;
    let (mut info, _) = fixture.legacy("notion", false).await;
    let mut saved = settings::stored(&fixture.database().connection, &info)
        .await
        .unwrap();
    saved.mcp = Some(plugin::mcp::Configuration {
        schema: plugin::mcp::SCHEMA.into(),
        servers: BTreeMap::from([(
            "notion".into(),
            plugin::mcp::Server::Http {
                exclusive: false,
                keep_images: None,
                reuse: true,
                url: "https://outside.example.com/mcp".into(),
                headers: BTreeMap::new(),
            },
        )]),
    });
    settings::persist(&fixture.database().connection, &mut info, &saved)
        .await
        .unwrap();
    let before = fixture.history(&info).await;
    let inventory = encode(
        &required(&fixture.database().connection, "notion")
            .await
            .unwrap(),
    )
    .unwrap();
    fixture
        .database()
        .connection
        .execute_batch("DROP TABLE turn_connections;DROP TABLE plugin_connections;")
        .await
        .unwrap();
    assert!(fixture.upgrade().await.is_err());
    assert!(fixture.database.is_none());
    let handle = crate::store::database::engine(&fixture.path).await.unwrap();
    let db = crate::store::database::connect(&handle).await.unwrap();
    assert_eq!((async { let mut statement = db.prepare("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('plugin_connections','turn_connections')").await?;
let row = statement.query_row(()).await?;
let mapped: turso::Result<_> = (|row: &turso::Row|row.get::<i64>(0))(&row);
mapped }).await.unwrap(),0);
    assert_eq!(
        db.prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2")
            .await
            .unwrap()
            .query_row(params![
                info.summary.name.as_str(),
                info.summary.settings_revision as i64
            ])
            .await
            .unwrap()
            .get::<Vec<u8>>(0)
            .unwrap(),
        before
    );
    assert_eq!(
        encode(&required(&db, "notion").await.unwrap()).unwrap(),
        inventory
    );

    assert!(!fixture.directory.path().join("backups").exists());
}

#[tokio::test]
async fn suspends_changed_mcp_paths_and_queries() {
    for url in [
        "https://mcp.notion.com/other",
        "https://mcp.notion.com/mcp?account=other",
    ] {
        let mut fixture = Fixture::new().await;
        fixture.legacy("notion", false).await;
        fixture.upgrade().await.unwrap();
        let row = records(&fixture.database().connection)
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.plugin == "notion")
            .unwrap();
        let info = required(&fixture.database().connection, "notion")
            .await
            .unwrap();
        let configuration = plugin::mcp::Configuration {
            schema: plugin::mcp::SCHEMA.into(),
            servers: BTreeMap::from([(
                "notion".into(),
                plugin::mcp::Server::Http {
                    exclusive: false,
                    keep_images: None,
                    reuse: true,
                    url: url.into(),
                    headers: BTreeMap::new(),
                },
            )]),
        };
        super::super::mcp::save(
            &fixture.database().connection,
            fixture.host.profile(),
            &info.summary.reference(),
            &configuration,
        )
        .await
        .unwrap();
        let projected = project(&fixture.database().connection, fixture.host.profile(), &row)
            .await
            .unwrap();
        assert_eq!(projected.state, State::NeedsConfirmation);
        let public = serde_json::to_string(&projected.consent).unwrap();
        assert!(public.contains("notion"));
        assert!(!public.contains(url));
        assert!(
            current_target(
                &fixture.database().connection,
                fixture.host.profile(),
                row.id,
                row.revision,
                false
            )
            .await
            .is_err()
        );
        execute(
            &fixture.database().connection,
            fixture.host.profile(),
            &sailry_protocol::Request::new(
                fixture.database().node,
                Command::ConfirmConnectorConsent {
                    connection: row.id,
                    expected_revision: row.revision,
                    accept: true,
                },
            ),
        )
        .await
        .unwrap();
        assert!(
            read(&fixture.database().connection, row.id)
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }
}

#[tokio::test]
async fn suspends_changed_oauth_permissions() {
    for change in ["origins", "scopes", "client", "flow", "targets", "source"] {
        let mut fixture = Fixture::new().await;
        fixture.legacy("google-workspace", false).await;
        fixture.upgrade().await.unwrap();
        let row = records(&fixture.database().connection)
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.plugin == "google-workspace")
            .unwrap();
        let mut info = required(&fixture.database().connection, "google-workspace")
            .await
            .unwrap();
        let declaration = &mut info.extension.as_mut().unwrap().connectors[0];
        match change {
            "origins" => declaration.origins.push("https://other.example.com".into()),
            "scopes" => {
                let Auth::Oauth2 { scopes, .. } = &mut declaration.auth[0] else {
                    unreachable!()
                };
                scopes.push("additional.read".into());
            }
            "client" => {
                let Auth::Oauth2 { client, .. } = &mut declaration.auth[0] else {
                    unreachable!()
                };
                *client = sailry_protocol::plugin::connector::Client::Bundled {
                    bundled: sailry_protocol::plugin::connector::PublicClient {
                        client_id: "different-client".into(),
                    },
                };
            }
            "flow" => {
                let Auth::Oauth2 { token, .. } = &mut declaration.auth[0] else {
                    unreachable!()
                };
                token.push_str("/changed");
            }
            "targets" => declaration.inject[0].prefix = "Token ".into(),
            "source" => info.origin = Some(plugin::Origin::Archive),
            _ => unreachable!(),
        }
        super::super::save(&fixture.database().connection, &info, true)
            .await
            .unwrap();
        let projection = project(&fixture.database().connection, fixture.host.profile(), &row)
            .await
            .unwrap();
        assert_eq!(projection.state, State::NeedsConfirmation, "{change}");
        let diff = serde_json::to_value(&projection.consent).unwrap();
        assert!(
            diff["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value["kind"] == change),
            "{change}: {diff}"
        );
        assert!(
            current_target(
                &fixture.database().connection,
                fixture.host.profile(),
                row.id,
                row.revision,
                false
            )
            .await
            .is_err()
        );
        execute(
            &fixture.database().connection,
            fixture.host.profile(),
            &sailry_protocol::Request::new(
                fixture.database().node,
                Command::ConfirmConnectorConsent {
                    connection: row.id,
                    expected_revision: row.revision,
                    accept: true,
                },
            ),
        )
        .await
        .unwrap();
        assert!(
            read(&fixture.database().connection, row.id)
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }
}

#[tokio::test]
async fn preserves_reduced_permissions_and_rejects_stale_captures() {
    let mut fixture = Fixture::new().await;
    fixture.legacy("google-workspace", false).await;
    fixture.upgrade().await.unwrap();
    let db = &fixture.database().connection;
    let row = records(db)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.plugin == "google-workspace")
        .unwrap();
    let mut info = required(db, &row.plugin).await.unwrap();
    let context = plugin::Context {
        package: info.summary.reference(),
        invocation: Some(RequestId::new()),
        turn: None,
        worktree: None,
        session: None,
        surface: Default::default(),
    };
    assert!(
        resolve(
            db,
            fixture.host.profile(),
            &context,
            &row.connector,
            Some(row.id)
        )
        .await
        .is_ok()
    );
    let origin = info.origin.clone();
    info.origin = Some(plugin::Origin::Archive);
    super::super::save(db, &info, true).await.unwrap();
    assert!(
        resolve(
            db,
            fixture.host.profile(),
            &context,
            &row.connector,
            Some(row.id)
        )
        .await
        .is_err()
    );
    info.origin = origin;
    let declaration = &mut info.extension.as_mut().unwrap().connectors[0];
    let Auth::Oauth2 { scopes, .. } = &mut declaration.auth[0] else {
        unreachable!()
    };
    assert!(scopes.len() > 1);
    scopes.pop();
    assert!(declaration.inject.len() > 1);
    declaration.inject.pop();
    assert!(declaration.valid());
    super::super::save(db, &info, true).await.unwrap();
    assert_eq!(
        project(db, fixture.host.profile(), &row)
            .await
            .unwrap()
            .state,
        State::NeedsConfirmation
    );
    execute(
        db,
        fixture.host.profile(),
        &sailry_protocol::Request::new(
            fixture.database().node,
            Command::ConfirmConnectorConsent {
                connection: row.id,
                expected_revision: row.revision,
                accept: true,
            },
        ),
    )
    .await
    .unwrap();
    let saved = read(db, row.id).await.unwrap();
    assert_eq!(
        saved.grant.as_ref().unwrap().access,
        row.grant.as_ref().unwrap().access
    );
    assert_eq!(
        saved.bound,
        bound(
            db,
            fixture.host.profile(),
            &info,
            &info.extension.as_ref().unwrap().connectors[0],
            saved.method
        )
        .await
        .unwrap()
    );
    assert!(
        resolve(
            db,
            fixture.host.profile(),
            &plugin::Context {
                invocation: None,
                ..context.clone()
            },
            &row.connector,
            Some(row.id)
        )
        .await
        .is_ok()
    );
    assert!(
        resolve(
            db,
            fixture.host.profile(),
            &context,
            &row.connector,
            Some(row.id)
        )
        .await
        .is_err()
    );
    assert!(
        current_target(db, fixture.host.profile(), row.id, row.revision, false)
            .await
            .is_err()
    );
}

#[test]
fn reduces_only_existing_mcp_targets() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../../plugins/notion/plugin.json")).unwrap();
    let declaration: Declaration = serde_json::from_value(
        manifest["extensions"]["dev.sailry.platform"]["connectors"][0].clone(),
    )
    .unwrap();
    let mut old = Bound {
        origin: Some(plugin::Origin::Bundled),
        auth: declaration.auth[0].clone(),
        origins: declaration.origins,
        inject: declaration.inject,
        targets: BTreeMap::new(),
    };
    let server = match &old.auth {
        Auth::McpOauth { server, .. } => server.clone(),
        _ => unreachable!(),
    };
    let target = plugin::mcp::Definition::StreamableHttp {
        url: "https://mcp.notion.com/mcp?scope=current".into(),
        headers: vec!["x-request-mode".into()],
    };
    old.targets.insert(server.clone(), target.clone());
    old.targets.insert("archive".into(), target);
    let mut injection = old.inject[0].clone();
    injection.server = Some("archive".into());
    old.inject.push(injection);
    let mut reduced = old.clone();
    reduced.targets.remove("archive");
    reduced.inject.pop();
    assert!(consent_reduced("notion", &old, &reduced));
    for change in ["path", "query", "header", "new"] {
        let mut changed = reduced.clone();
        match change {
            "new" => {
                changed
                    .targets
                    .insert("other".into(), old.targets[&server].clone());
            }
            "path" | "query" | "header" => {
                let plugin::mcp::Definition::StreamableHttp { url, headers } =
                    changed.targets.get_mut(&server).unwrap()
                else {
                    unreachable!()
                };
                match change {
                    "path" => *url = "https://mcp.notion.com/changed?scope=current".into(),
                    "query" => *url = "https://mcp.notion.com/mcp?scope=changed".into(),
                    _ => headers.push("other".into()),
                }
            }
            _ => unreachable!(),
        }
        assert!(!consent_reduced("notion", &old, &changed), "{change}");
    }
}
