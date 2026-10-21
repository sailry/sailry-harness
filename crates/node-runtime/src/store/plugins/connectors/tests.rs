use super::*;
use crate::store::database::Database;
use serde_json::json;
use settings::history;
use std::path::PathBuf;

struct Fixture {
    directory: tempfile::TempDir,
    path: PathBuf,
    database: Database,
    host: crate::plugins::Host,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let path = root.join("node.sqlite3");
        let database =
            Database::open(&path, sailry_protocol::NodeId([74; 32]), Some(root.clone())).unwrap();
        let host = crate::plugins::Host::new(Some(root));
        Self {
            directory,
            path,
            database,
            host,
        }
    }
    fn legacy(&self, name: &str, cli: bool) -> (Info, Vec<u8>) {
        let mut info = self.host.install_bundled(name).unwrap();
        let declaration = info.extension.as_ref().unwrap().connectors[0].clone();
        info.summary.version = Some("0.0.1".into());
        info.origin = Some(plugin::Origin::Bundled);
        info.extension.as_mut().unwrap().connectors.clear();
        super::super::save(&self.database.connection, &info, true).unwrap();
        let mut saved = settings::stored(&self.database.connection, &info).unwrap();
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
        if name == "google-workspace" {
            saved.oauth = Some(history::Authorized {
                id: sailry_protocol::CredentialId::new(),
                revision: 3,
                grant: Some(history::Grant {
                    declaration: history::Declaration {
                        provider: "google".into(),
                        scopes: vec!["read".into()],
                        credentials: vec!["token".into()],
                    },
                    configuration: history::Configuration {
                        client_id: "legacy-client".into(),
                        client_secret: Some(Secret::new("private-client-secret".into())),
                        tenant: None,
                    },
                    access_token: Secret::new("private-access".into()),
                    refresh_token: Some(Secret::new("private-refresh".into())),
                    expires_at_ms: u64::MAX / 2,
                }),
            });
        }
        if name == "notion" {
            let Auth::McpOauth { server, .. } = &declaration.auth[0] else {
                panic!("MCP method expected")
            };
            saved.authorizations.insert(
                server.clone(),
                history::McpGrant {
                    id: sailry_protocol::CredentialId::new(),
                    endpoint: "https://mcp.notion.com/mcp".into(),
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
        settings::persist(&self.database.connection, &mut info, &saved).unwrap();
        let bytes = encode(&saved).unwrap();
        (info, bytes)
    }
    fn history(&self, info: &Info) -> Vec<u8> {
        self.database
            .connection
            .query_row(
                "SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2",
                params![info.summary.name, info.summary.settings_revision as i64],
                |row| row.get(0),
            )
            .unwrap()
    }
    fn upgrade(&mut self) -> Result<(), crate::Error> {
        Database::open_with_plugins(
            &self.path,
            self.database.node,
            Some(self.directory.path().canonicalize()?),
            Some(&self.host),
        )?
        .close()
    }
}

#[test]
fn imports_credentials_and_preserves_captures() {
    for cli in [false, true] {
        let mut fixture = Fixture::new();
        let originals = ["figma", "dingtalk", "google-workspace", "notion", "github"]
            .into_iter()
            .map(|name| fixture.legacy(name, cli && name == "github"))
            .collect::<Vec<_>>();
        fixture.upgrade().unwrap();
        let records = records(&fixture.database.connection).unwrap();
        assert_eq!(records.len(), 5);
        for (original, bytes) in &originals {
            assert_eq!(fixture.history(original), *bytes);
            let current = required(&fixture.database.connection, &original.summary.name).unwrap();
            let settings = settings::stored(&fixture.database.connection, &current).unwrap();
            assert_eq!(settings.values["unrelated"], "preserve draft");
            let row = records
                .iter()
                .find(|row| row.plugin == original.summary.name)
                .unwrap();
            assert_eq!(row.state, State::Connected);
            let projected =
                project(&fixture.database.connection, fixture.host.profile(), row).unwrap();
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
        }
        let backup = std::fs::read_dir(fixture.directory.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(backup.len(), 1);
        let previous =
            Connection::open_with_flags(&backup[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        for (original, bytes) in originals {
            let retained: Vec<u8> = previous
                .query_row(
                    "SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2",
                    params![
                        original.summary.name,
                        original.summary.settings_revision as i64
                    ],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(retained, bytes);
        }
        previous.close().unwrap();
        fixture.upgrade().unwrap();
        assert_eq!(
            std::fs::read_dir(fixture.directory.path().join("backups"))
                .unwrap()
                .filter(|entry| entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "sqlite3"))
                .count(),
            1
        );
    }
}

#[test]
fn rejects_upgrade_without_writing() {
    let mut fixture = Fixture::new();
    let (mut info, _) = fixture.legacy("notion", false);
    let mut saved = settings::stored(&fixture.database.connection, &info).unwrap();
    saved.mcp = Some(plugin::mcp::Configuration {
        schema: plugin::mcp::SCHEMA.into(),
        servers: BTreeMap::from([(
            "notion".into(),
            plugin::mcp::Server::Http {
                url: "https://outside.example.com/mcp".into(),
                headers: BTreeMap::new(),
            },
        )]),
    });
    settings::persist(&fixture.database.connection, &mut info, &saved).unwrap();
    let before = fixture.history(&info);
    let inventory = encode(&required(&fixture.database.connection, "notion").unwrap()).unwrap();
    fixture
        .database
        .connection
        .execute_batch("DROP TABLE turn_connections;DROP TABLE plugin_connections;")
        .unwrap();
    assert!(fixture.upgrade().is_err());
    assert_eq!(fixture.database.connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('plugin_connections','turn_connections')",[],|row|row.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(fixture.history(&info), before);
    assert_eq!(
        encode(&required(&fixture.database.connection, "notion").unwrap()).unwrap(),
        inventory
    );

    assert!(!fixture.directory.path().join("backups").exists());
}

#[test]
fn suspends_changed_mcp_paths_and_queries() {
    for url in [
        "https://mcp.notion.com/other",
        "https://mcp.notion.com/mcp?account=other",
    ] {
        let mut fixture = Fixture::new();
        fixture.legacy("notion", false);
        fixture.upgrade().unwrap();
        let row = records(&fixture.database.connection)
            .unwrap()
            .into_iter()
            .find(|row| row.plugin == "notion")
            .unwrap();
        let info = required(&fixture.database.connection, "notion").unwrap();
        let configuration = plugin::mcp::Configuration {
            schema: plugin::mcp::SCHEMA.into(),
            servers: BTreeMap::from([(
                "notion".into(),
                plugin::mcp::Server::Http {
                    url: url.into(),
                    headers: BTreeMap::new(),
                },
            )]),
        };
        super::super::mcp::save(
            &fixture.database.connection,
            &info.summary.reference(),
            &configuration,
        )
        .unwrap();
        let projected =
            project(&fixture.database.connection, fixture.host.profile(), &row).unwrap();
        assert_eq!(projected.state, State::NeedsConfirmation);
        let public = serde_json::to_string(&projected.consent).unwrap();
        assert!(public.contains("notion"));
        assert!(!public.contains(url));
        assert!(
            current_target(
                &fixture.database.connection,
                fixture.host.profile(),
                row.id,
                row.revision,
                false
            )
            .is_err()
        );
        execute(
            &fixture.database.connection,
            fixture.host.profile(),
            &sailry_protocol::Request::new(
                fixture.database.node,
                Command::ConfirmConnectorConsent {
                    connection: row.id,
                    expected_revision: row.revision,
                    accept: true,
                },
            ),
        )
        .unwrap();
        assert!(
            read(&fixture.database.connection, row.id)
                .unwrap()
                .grant
                .is_none()
        );
    }
}

#[test]
fn suspends_changed_oauth_permissions() {
    for change in ["origins", "scopes", "client", "flow", "targets", "source"] {
        let mut fixture = Fixture::new();
        fixture.legacy("google-workspace", false);
        fixture.upgrade().unwrap();
        let row = records(&fixture.database.connection)
            .unwrap()
            .into_iter()
            .find(|row| row.plugin == "google-workspace")
            .unwrap();
        let mut info = required(&fixture.database.connection, "google-workspace").unwrap();
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
        super::super::save(&fixture.database.connection, &info, true).unwrap();
        let projection =
            project(&fixture.database.connection, fixture.host.profile(), &row).unwrap();
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
                &fixture.database.connection,
                fixture.host.profile(),
                row.id,
                row.revision,
                false
            )
            .is_err()
        );
        execute(
            &fixture.database.connection,
            fixture.host.profile(),
            &sailry_protocol::Request::new(
                fixture.database.node,
                Command::ConfirmConnectorConsent {
                    connection: row.id,
                    expected_revision: row.revision,
                    accept: true,
                },
            ),
        )
        .unwrap();
        assert!(
            read(&fixture.database.connection, row.id)
                .unwrap()
                .grant
                .is_none()
        );
    }
}

#[test]
fn preserves_reduced_permissions_and_rejects_stale_captures() {
    let mut fixture = Fixture::new();
    fixture.legacy("google-workspace", false);
    fixture.upgrade().unwrap();
    let db = &fixture.database.connection;
    let row = records(db)
        .unwrap()
        .into_iter()
        .find(|row| row.plugin == "google-workspace")
        .unwrap();
    let mut info = required(db, &row.plugin).unwrap();
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
        .is_ok()
    );
    let origin = info.origin.clone();
    info.origin = Some(plugin::Origin::Archive);
    super::super::save(db, &info, true).unwrap();
    assert!(
        resolve(
            db,
            fixture.host.profile(),
            &context,
            &row.connector,
            Some(row.id)
        )
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
    super::super::save(db, &info, true).unwrap();
    assert_eq!(
        project(db, fixture.host.profile(), &row).unwrap().state,
        State::NeedsConfirmation
    );
    execute(
        db,
        fixture.host.profile(),
        &sailry_protocol::Request::new(
            fixture.database.node,
            Command::ConfirmConnectorConsent {
                connection: row.id,
                expected_revision: row.revision,
                accept: true,
            },
        ),
    )
    .unwrap();
    let saved = read(db, row.id).unwrap();
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
        .is_err()
    );
    assert!(current_target(db, fixture.host.profile(), row.id, row.revision, false).is_err());
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
    assert!(consent_reduced(&old, &reduced));
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
        assert!(!consent_reduced(&old, &changed), "{change}");
    }
}
