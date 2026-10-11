//! Check typed stored JSON before startup changes journal or recovery state.
use crate::Error;
use rusqlite::Connection;
use sailry_protocol::{
    Event, Fault, Output, SessionConfig, SessionProfile, conversation::Provider, role,
};
use serde::de::DeserializeOwned;

pub(super) fn stored(db: &Connection) -> Result<(), Error> {
    records::<sailry_protocol::artifact::Summary>(
        db,
        "artifacts.body",
        "SELECT body FROM artifacts",
    )?;
    records::<sailry_protocol::artifact::Summary>(
        db,
        "artifact_revisions.body",
        "SELECT body FROM artifact_revisions",
    )?;
    crate::store::artifacts::validate_stored(db).map_err(|error| Error::Worker(error.message))?;
    records::<crate::store::backup::Stored>(
        db,
        "backup_settings.body",
        "SELECT body FROM backup_settings",
    )?;
    crate::store::backup::read(db)
        .map_err(|_| Error::Worker("incompatible stored backup configuration".into()))?;
    records::<crate::store::search::Stored>(
        db,
        "search_settings.body",
        "SELECT body FROM search_settings",
    )?;
    crate::store::search::validate_stored(db).map_err(|_| {
        Error::Worker(
            "incompatible stored search configuration; existing data was not changed".into(),
        )
    })?;
    records::<sailry_protocol::sandbox::Settings>(
        db,
        "sandbox_settings.body",
        "SELECT body FROM sandbox_settings",
    )?;
    records::<sailry_protocol::knowledge::Settings>(
        db,
        "knowledge_settings.body",
        "SELECT body FROM knowledge_settings",
    )?;
    records::<crate::store::knowledge::Stored>(
        db,
        "knowledge_collections.body",
        "SELECT body FROM knowledge_collections",
    )?;
    crate::store::knowledge::validate_stored(db).map_err(|_| {
        Error::Worker(
            "incompatible stored knowledge configuration; existing data was not changed".into(),
        )
    })?;
    records::<role::Profile>(db, "roles.body", "SELECT body FROM roles")?;
    records::<sailry_protocol::hooks::Settings>(
        db,
        "hook_settings.body",
        "SELECT body FROM hook_settings",
    )?;
    records::<sailry_protocol::hooks::Settings>(
        db,
        "agent_runs.hooks",
        "SELECT hooks FROM agent_runs",
    )?;
    records::<Result<sailry_protocol::process::Outcome, Fault>>(
        db,
        "hook_executions.result",
        "SELECT result FROM hook_executions WHERE result IS NOT NULL",
    )?;
    records::<sailry_protocol::code::Settings>(
        db,
        "code_settings.body",
        "SELECT body FROM code_settings",
    )?;
    records::<sailry_protocol::code::Startup>(
        db,
        "code_startups.body",
        "SELECT body FROM code_startups",
    )?;
    records::<sailry_protocol::conversation::Approval>(
        db,
        "agent_approvals.body",
        "SELECT body FROM agent_approvals",
    )?;
    records::<Option<crate::store::agent::ApprovalBinding>>(
        db,
        "agent_approvals.binding",
        "SELECT binding FROM agent_approvals",
    )?;
    records::<Option<crate::store::agent::connections::Prepared>>(
        db,
        "agent_approvals.prepared",
        "SELECT prepared FROM agent_approvals",
    )?;
    records::<crate::store::agent::ApprovalBinding>(
        db,
        "agent_approval_rules.binding",
        "SELECT binding FROM agent_approval_rules",
    )?;
    records::<sailry_protocol::conversation::ApprovalChoice>(
        db,
        "agent_approval_rules.choice",
        "SELECT choice FROM agent_approval_rules",
    )?;
    records::<sailry_protocol::notification::Notice>(
        db,
        "notifications.body",
        "SELECT body FROM notifications",
    )?;
    records::<sailry_protocol::dispatch::Handler>(
        db,
        "dispatch_handlers.body",
        "SELECT body FROM dispatch_handlers",
    )?;
    records::<sailry_protocol::dispatch::Schedule>(
        db,
        "dispatch_schedules.body",
        "SELECT body FROM dispatch_schedules",
    )?;
    records::<sailry_protocol::dispatch::Job>(
        db,
        "dispatch_jobs.body",
        "SELECT body FROM dispatch_jobs",
    )?;
    records::<sailry_protocol::dispatch::Callback>(
        db,
        "dispatch_jobs.callback",
        "SELECT callback FROM dispatch_jobs",
    )?;
    records::<sailry_protocol::dispatch::Event>(
        db,
        "dispatch_events.body",
        "SELECT body FROM dispatch_events",
    )?;
    records::<sailry_protocol::plugin::Info>(db, "plugins.body", "SELECT body FROM plugins")?;
    records::<sailry_protocol::plugin::Info>(
        db,
        "plugin_packages.body",
        "SELECT body FROM plugin_packages",
    )?;
    records::<crate::store::plugins::settings::Stored>(
        db,
        "plugin_settings.body",
        "SELECT body FROM plugin_settings",
    )?;
    records::<crate::store::agent::continuation::Fence>(
        db,
        "agent_continuations.body",
        "SELECT body FROM agent_continuations",
    )?;
    records::<super::super::providers::Stored>(db, "providers.body", "SELECT body FROM providers")?;
    super::super::providers::validate_stored(db).map_err(|_| {
        Error::Worker(
            "incompatible stored provider configuration; existing data was not changed".into(),
        )
    })?;
    records::<SessionConfig>(db, "defaults.config", "SELECT config FROM defaults")?;
    records::<SessionConfig>(
        db,
        "session_revisions.config",
        "SELECT config FROM session_revisions",
    )?;
    records::<Provider>(
        db,
        "agent_runs.provider",
        "SELECT provider FROM agent_runs WHERE provider IS NOT NULL",
    )?;
    records::<SessionProfile>(
        db,
        "session_revisions.profile",
        "SELECT profile FROM session_revisions WHERE profile IS NOT NULL",
    )?;
    records::<role::Snapshot>(
        db,
        "session_revisions.roles",
        "SELECT roles FROM session_revisions",
    )?;
    records::<crate::store::media::Snapshot>(
        db,
        "session_media.body",
        "SELECT body FROM session_media",
    )?;
    records::<sailry_protocol::media::Settings>(
        db,
        "media_settings.body",
        "SELECT body FROM media_settings",
    )?;
    records::<Event>(db, "events.body", "SELECT body FROM events")?;
    records::<Result<Output, Fault>>(
        db,
        "requests.result",
        "SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL",
    )?;
    Ok(())
}

pub(in crate::store) fn records<T: DeserializeOwned>(
    db: &Connection,
    source: &str,
    sql: &str,
) -> Result<(), Error> {
    let mut query = db.prepare(sql)?;
    for (index, body) in query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))?
        .enumerate()
    {
        serde_json::from_slice::<T>(&body?).map_err(|error| {
            // Serde messages can include invalid values. Report the location, not the payload.
            Error::Worker(format!(
                "invalid stored JSON in {source}, record {}, line {}, column {} ({:?}); existing data was not changed",
                index + 1,
                error.line(),
                error.column(),
                error.classify(),
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::database::Database;
    use rusqlite::params;
    use sailry_protocol::{
        Effort, NodeId, ProviderId, RequestId,
        conversation::{Model, ModelApi},
    };

    #[test]
    fn incompatible_dispatch_callback_preserves_profile() {
        use sailry_protocol::{EventId, JobId, dispatch::*};
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let node = NodeId([52; 32]);
        let database = Database::open(&path, node, None).unwrap();
        let job = Job {
            id: JobId::new(),
            event: Event {
                id: EventId::new(),
                source: Source {
                    package: "fixture".into(),
                    topic: "fixture".into(),
                },
                payload: serde_json::Value::Null,
                timestamp_ms: 0,
                schedule: None,
                scheduled_ms: None,
            },
            handler: "fixture".into(),
            queue: "fixture".into(),
            request: RequestId::new(),
            status: Status::Queued,
            created_ms: 0,
            started_ms: None,
            finished_ms: None,
            error: None,
        };
        let mut callback = serde_json::to_value(Callback {
            scope: Scope::default(),
            completion: Completion::Command,
            command: Box::new(sailry_protocol::Command::Snapshot),
            bindings: Default::default(),
        })
        .unwrap();
        callback.as_object_mut().unwrap().remove("completion");
        database.connection.execute("INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference) VALUES(?1,'fixture','fixture','queued',?2,?3,?4)", params![job.id.to_string(), serde_json::to_vec(&job).unwrap(), serde_json::to_vec(&callback).unwrap(), b"{}".as_slice()]).unwrap();
        database.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(Database::open(&path, node, None).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn missing_continuation_scope_preserves_profile() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let node = NodeId([58; 32]);
        let database = Database::open(&path, node, None).unwrap();
        database
            .connection
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        let fence = serde_json::json!({"package":{"name":"fixture","digest":"fixture","settings_revision":0},"key":"state","revision":1});
        database
            .connection
            .execute(
                "INSERT INTO agent_continuations(turn,body) VALUES(?1,?2)",
                params![
                    sailry_protocol::TurnId::new().to_string(),
                    serde_json::to_vec(&fence).unwrap()
                ],
            )
            .unwrap();
        database.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        let error = Database::open(&path, node, None).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("invalid stored JSON in agent_continuations.body")
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn adds_checkpoint_content_to_empty_tables() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("database.sqlite3");
        let node = NodeId([52; 32]);
        let database = Database::open(&path, node, None).unwrap();
        database
            .connection
            .execute_batch("ALTER TABLE file_checkpoints DROP COLUMN after_text")
            .unwrap();
        database.close().unwrap();
        let database = Database::open(&path, node, None).unwrap();
        assert!(
            database
                .connection
                .prepare("SELECT after_text FROM file_checkpoints")
                .is_ok()
        );
        database.close().unwrap();
        let backups = path.parent().unwrap().join("backups");
        let files = std::fs::read_dir(&backups)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        let previous =
            Connection::open_with_flags(&files[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        assert!(
            previous
                .prepare("SELECT after_text FROM file_checkpoints")
                .is_err()
        );
        assert_eq!(
            previous
                .query_row("SELECT count(*) FROM file_checkpoints", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(previous);
        Database::open(&path, node, None).unwrap().close().unwrap();
        let snapshots = std::fs::read_dir(backups)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "sqlite3")
            })
            .collect::<Vec<_>>();
        assert_eq!(snapshots, files);
    }

    #[test]
    fn rejects_incompatible_schema() {
        for table in ["sessions", "worktrees"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("database.sqlite3");
            let node = NodeId([48; 32]);
            let database = Database::open(&path, node, None).unwrap();
            let schema: String = database
                .connection
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            database
                .connection
                .execute_batch(&format!(
                    "PRAGMA foreign_keys=OFF; DROP TABLE {table}; {};",
                    schema.replace(
                        "project TEXT REFERENCES",
                        "project TEXT NOT NULL REFERENCES"
                    ),
                ))
                .unwrap();
            database.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(Database::open(&path, node, None).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn rejects_invalid_authentication_fields() {
        for case in ["record", "authentication", "grant"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("database.sqlite3");
            let node = NodeId([49; 32]);
            let database = Database::open(&path, node, None).unwrap();
            let provider = ProviderId::new();
            let authorization = sailry_protocol::CredentialId::new();
            let mut value = serde_json::json!({
                "id": provider,
                "provider": null,
                "listed": false,
                "authorizations": {
                    authorization.to_string(): {
                        "authentication": sailry_protocol::Authentication::Copilot,
                        "revision": 1,
                        "key": "invalid-grant",
                        "expires_at_ms": null
                    }
                }
            });
            if case == "record" {
                value.as_object_mut().unwrap().remove("authorizations");
            } else if case == "authentication" {
                value["authorizations"][authorization.to_string()]["authentication"] =
                    serde_json::json!("unknown");
            }
            database
                .connection
                .execute(
                    "INSERT INTO providers(id,body) VALUES(?1,?2)",
                    params![provider.to_string(), serde_json::to_vec(&value).unwrap()],
                )
                .unwrap();
            database
                .connection
                .execute(
                    "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                    params![
                        &node.0[..],
                        RequestId::new().to_string(),
                        b"fixture".as_slice()
                    ],
                )
                .unwrap();
            database.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(Database::open(&path, node, None).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            let db = Connection::open(&path).unwrap();
            assert_eq!(
                db.query_row::<i64, _, _>(
                    "SELECT count(*) FROM requests WHERE status='admitted'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
                1
            );
        }
    }

    #[test]
    fn rejects_invalid_models() {
        for owner in ["provider", "event", "receipt"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("database.sqlite3");
            let node = NodeId([37; 32]);
            let database = Database::open(&path, node, None).unwrap();
            let provider = Provider {
                oauth: None,
                options: None,
                id: ProviderId::new(),
                revision: 1,
                name: "fixture".into(),
                api: ModelApi::Gemini,
                authentication: sailry_protocol::Authentication::ApiKey,
                endpoint: "http://127.0.0.1:9".into(),
                enabled: true,
                credential: None,
                default_model: "fixture".into(),
                models: vec![Model {
                    id: "fixture".into(),
                    display_name: None,
                    context: 4096,
                    output: 2048,
                    vision: false,
                    tools: false,
                    reasoning: true,
                    web_search: false,
                    generates: vec![],
                    efforts: vec![Effort::High],
                    capabilities: None,
                    custom_efforts: false,
                    default_effort: Effort::High,
                }],
            };
            let mut value = serde_json::to_value(&provider).unwrap();
            value["models"][0]["default_effort"] = serde_json::json!("invalid-choice");
            match owner {
                "provider" => {
                    database
                        .connection
                        .execute(
                            "INSERT INTO providers(id,body) VALUES(?1,?2)",
                            params![
                                provider.id.to_string(),
                                serde_json::to_vec(&serde_json::json!({
                                    "id": provider.id,
                                    "provider": value,
                                    "listed": true,
                                    "authorizations": {}
                                }))
                                .unwrap()
                            ],
                        )
                        .unwrap();
                }
                "event" => {
                    database
                        .connection
                        .execute(
                            "INSERT INTO events(body) VALUES(?1)",
                            [serde_json::to_vec(
                                &serde_json::json!({"kind":"provider_changed","data":value}),
                            )
                            .unwrap()],
                        )
                        .unwrap();
                }
                "receipt" => {
                    database.connection.execute("INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)", params![&node.0[..], RequestId::new().to_string(), b"fixture".as_slice(), serde_json::json!({"Ok":{"kind":"provider","data":value}}).to_string()]).unwrap();
                }
                _ => unreachable!(),
            }
            database
                .connection
                .execute(
                    "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                    params![
                        &node.0[..],
                        RequestId::new().to_string(),
                        b"fixture".as_slice()
                    ],
                )
                .unwrap();
            database.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            let error = Database::open(&path, node, None).err().unwrap();
            assert!(error.to_string().contains("invalid stored JSON"));
            assert_eq!(std::fs::read(&path).unwrap(), before);
            let db = Connection::open(&path).unwrap();
            let admitted: i64 = db
                .query_row(
                    "SELECT count(*) FROM requests WHERE status='admitted'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(admitted, 1);
        }
    }

    mod contracts {
        use super::*;
        use sailry_protocol::plugin;
        use serde_json::{Value, json};
        use std::path::{Path, PathBuf};

        fn fixture() -> (tempfile::TempDir, PathBuf, NodeId, Database) {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("database.sqlite3");
            let node = NodeId([57; 32]);
            let database = Database::open(&path, node, None).unwrap();
            database
                .connection
                .execute(
                    "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                    params![
                        &node.0[..],
                        RequestId::new().to_string(),
                        b"diagnostic-secret-marker".as_slice()
                    ],
                )
                .unwrap();
            (directory, path, node, database)
        }

        fn rejects_without_writes(database: Database, path: &Path, node: NodeId) {
            let sessions: i64 = database
                .connection
                .query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
                .unwrap();
            assert_eq!(sessions, 0);
            database.close().unwrap();
            let connection = Connection::open(path).unwrap();
            connection
                .pragma_update(None, "journal_mode", "DELETE")
                .unwrap();
            connection.close().unwrap();
            let before = std::fs::read(path).unwrap();

            let error = Database::open(path, node, None).err().unwrap().to_string();

            assert!(error.contains("invalid stored JSON"), "{error}");
            assert!(error.contains("existing data was not changed"), "{error}");
            assert!(!error.contains("diagnostic-secret-marker"), "{error}");
            assert_eq!(std::fs::read(path).unwrap(), before);
            let connection =
                Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .unwrap();
            let journal: String = connection
                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                .unwrap();
            assert_eq!(journal, "delete");
            let admitted: i64 = connection
                .query_row(
                    "SELECT count(*) FROM requests WHERE status='admitted'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(admitted, 1);
        }

        fn schema() -> Value {
            let value = json!({
                "$schema": plugin::settings::SCHEMA,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "token": {"type": "string", "x-sailry-secret": {
                        "header": "Authorization", "origin": "https://example.test"
                    }}
                },
                "required": ["token"]
            });
            serde_json::from_value::<plugin::settings::Schema>(value.clone()).unwrap();
            value
        }

        fn info() -> Value {
            let value = json!({
                "summary": {
                    "name": "fixture", "revision": 1, "digest": "a".repeat(64),
                    "settings_revision": 0, "enabled": true
                },
                "extension": {"api_version": plugin::API_VERSION, "actions": []},
                "settings": schema(),
                "skills": [], "mcp": [], "issues": []
            });
            serde_json::from_value::<plugin::Info>(value.clone()).unwrap();
            value
        }

        fn accepts_without_rewrites(database: Database, path: &Path, node: NodeId, sql: &str) {
            let before: Vec<Vec<u8>> = database
                .connection
                .prepare(sql)
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            database.close().unwrap();
            let database = Database::open(path, node, None).unwrap();
            let after: Vec<Vec<u8>> = database
                .connection
                .prepare(sql)
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(after, before);
            database.close().unwrap();
        }

        #[test]
        fn ignores_extra_session_fields() {
            for field in ["unrecognized", "future_setting"] {
                let (_directory, path, node, database) = fixture();
                let config = SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Low,
                    mode: sailry_protocol::WorkMode::Code,
                    permission: sailry_protocol::Permission::Ask,
                    credential: None,
                };
                let mut value = serde_json::to_value(config).unwrap();
                value[field] = json!(true);
                value.as_object_mut().unwrap().remove("effort");
                assert_eq!(
                    serde_json::from_value::<SessionConfig>(value.clone())
                        .unwrap()
                        .effort,
                    Effort::Default
                );
                database
                    .connection
                    .execute(
                        "INSERT INTO defaults(singleton,revision,config) VALUES(1,1,?1)",
                        [serde_json::to_vec(&value).unwrap()],
                    )
                    .unwrap();
                accepts_without_rewrites(database, &path, node, "SELECT config FROM defaults");
            }
        }

        #[test]
        fn reads_metadata_without_changing_records() {
            for (owner, field) in [
                ("inventory", "future_metadata"),
                ("package", "future_metadata"),
                ("inventory", "future_binding"),
                ("package", "future_binding"),
                ("settings", "future_binding"),
            ] {
                let (_directory, path, node, database) = fixture();
                let mut value = if owner == "settings" {
                    let value = json!({
                        "schema": schema(), "values": {}, "slots": {}, "authorizations": {}
                    });
                    serde_json::from_value::<crate::store::plugins::settings::Stored>(
                        value.clone(),
                    )
                    .unwrap();
                    value
                } else {
                    info()
                };
                if field == "future_metadata" {
                    value["extension"][field] = json!([]);
                } else {
                    let key = if owner == "settings" {
                        "schema"
                    } else {
                        "settings"
                    };
                    value[key]["properties"]["token"]["x-sailry-secret"][field] = json!("fixture");
                }
                let body = serde_json::to_vec(&value).unwrap();
                match owner {
                    "inventory" => {
                        database.connection.execute(
                            "INSERT INTO plugins(name,revision,installed,body) VALUES('fixture',1,1,?1)",
                            [body],
                        ).unwrap();
                    }
                    "package" => {
                        database.connection.execute(
                            "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,'fixture',?2)",
                            params!["a".repeat(64), body],
                        ).unwrap();
                    }
                    "settings" => {
                        database.connection.execute(
                            "INSERT INTO plugin_settings(plugin,revision,body) VALUES('fixture',1,?1)",
                            [body],
                        ).unwrap();
                    }
                    _ => unreachable!(),
                }
                let sql = match owner {
                    "inventory" => "SELECT body FROM plugins",
                    "package" => "SELECT body FROM plugin_packages",
                    "settings" => "SELECT body FROM plugin_settings",
                    _ => unreachable!(),
                };
                accepts_without_rewrites(database, &path, node, sql);
            }
        }

        #[test]
        fn unknown_output_rejects_before_writes() {
            let (_directory, path, node, database) = fixture();
            for table in ["defaults", "plugins", "plugin_packages", "plugin_settings"] {
                let count: i64 = database
                    .connection
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, 0);
            }
            let result = json!({"Ok": {
                "kind": "unrecognized", "data": {"token": "diagnostic-secret-marker"}
            }});
            database.connection.execute(
                "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
                params![&node.0[..], RequestId::new().to_string(), b"fixture".as_slice(), result.to_string()],
            ).unwrap();
            assert!(crate::store::outcomes::decode(&result.to_string()).is_err());
            rejects_without_writes(database, &path, node);
        }

        #[test]
        fn unknown_event_rejects_before_writes() {
            let (_directory, path, node, database) = fixture();
            let value = json!({
                "kind": "unrecognized", "data": {"token": "diagnostic-secret-marker"}
            });
            database
                .connection
                .execute(
                    "INSERT INTO events(body) VALUES(?1)",
                    [serde_json::to_vec(&value).unwrap()],
                )
                .unwrap();
            assert!(serde_json::from_value::<Event>(value).is_err());
            rejects_without_writes(database, &path, node);
        }

        #[test]
        fn missing_permissions_rejects_before_writes() {
            let (_directory, path, node, database) = fixture();
            let config = json!({
                "provider": ProviderId::new(), "model": "fixture", "mode": "code",
                "unrecognized": "diagnostic-secret-marker"
            });
            database
                .connection
                .execute(
                    "INSERT INTO defaults(singleton,revision,config) VALUES(1,1,?1)",
                    [serde_json::to_vec(&config).unwrap()],
                )
                .unwrap();
            rejects_without_writes(database, &path, node);
        }

        #[test]
        fn malformed_known_records_include_only_the_location() {
            let (_directory, path, node, database) = fixture();
            let value =
                json!({"kind":"session_config", "data":{"permission":"diagnostic-secret-marker"}});
            let result = json!({"Ok":value});
            database.connection.execute("INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)", params![&node.0[..], RequestId::new().to_string(), b"fixture".as_slice(), result.to_string()]).unwrap();
            let error = stored(&database.connection).unwrap_err().to_string();
            assert!(error.contains("requests.result"), "{error}");
            assert!(error.contains("record 1"), "{error}");
            assert!(!error.contains("diagnostic-secret-marker"), "{error}");
            rejects_without_writes(database, &path, node);
        }
    }

    mod schema {
        use super::*;
        use std::path::{Path, PathBuf};

        fn fixture(sql: &str) -> (tempfile::TempDir, PathBuf, NodeId) {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join(if cfg!(unix) {
                    "database\nfixture.sqlite3"
                } else {
                    "database-fixture.sqlite3"
                });
            let node = NodeId([53; 32]);
            let database = Database::open(&path, node, None).unwrap();
            database
                .connection
                .execute(
                    "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                    params![
                        &node.0[..],
                        RequestId::new().to_string(),
                        b"diagnostic-secret-marker".as_slice()
                    ],
                )
                .unwrap();
            database.connection.execute_batch(sql).unwrap();
            database.close().unwrap();
            let connection = Connection::open(&path).unwrap();
            connection
                .pragma_update(None, "journal_mode", "DELETE")
                .unwrap();
            connection.close().unwrap();
            (directory, path, node)
        }

        fn rejects_without_writes(path: &Path, node: NodeId, reason: &str) {
            let before = std::fs::read(path).unwrap();
            let error = Database::open(path, node, None).err().unwrap().to_string();

            assert!(
                error.contains("incompatible development database"),
                "{error}"
            );
            assert!(error.contains(&format!("{path:?}")), "{error}");
            assert!(error.contains(reason), "{error}");
            assert!(error.contains("existing data was not changed"), "{error}");
            assert!(
                error.contains("--data-dir <new absolute private directory>"),
                "{error}"
            );
            assert!(!error.contains('\n'), "{error}");
            assert!(!error.contains("diagnostic-secret-marker"), "{error}");
            assert_eq!(std::fs::read(path).unwrap(), before);

            let connection =
                Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .unwrap();
            let journal: String = connection
                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                .unwrap();
            assert_eq!(journal, "delete");
            let admitted: i64 = connection
                .query_row(
                    "SELECT count(*) FROM requests WHERE status='admitted'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(admitted, 1);
        }

        #[test]
        fn wrong_application_preserves_profile() {
            for (sql, reason) in [
                (
                    "PRAGMA application_id=0",
                    "application_id is 0x0, expected 0x5341494c",
                ),
                (
                    "PRAGMA application_id=42",
                    "application_id is 0x2a, expected 0x5341494c",
                ),
            ] {
                let (_directory, path, node) = fixture(sql);
                rejects_without_writes(&path, node, reason);
            }
        }

        #[test]
        fn wrong_version_preserves_profile() {
            for (sql, reason) in [
                ("PRAGMA user_version=0", "user_version is 0, expected 1"),
                ("PRAGMA user_version=2", "user_version is 2, expected 1"),
            ] {
                let (_directory, path, node) = fixture(sql);
                rejects_without_writes(&path, node, reason);
            }
        }

        #[test]
        fn current_schema_reopens() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("database.sqlite3");
            let node = NodeId([54; 32]);
            Database::open(&path, node, None).unwrap().close().unwrap();
            let database = Database::open(&path, node, None).unwrap();
            let application: i64 = database
                .connection
                .pragma_query_value(None, "application_id", |row| row.get(0))
                .unwrap();
            let version: i64 = database
                .connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(application, crate::store::database::APPLICATION_ID);
            assert_eq!(version, 1);
            database.close().unwrap();
        }
    }
}
