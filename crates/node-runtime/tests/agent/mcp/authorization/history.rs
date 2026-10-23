//! Current authorization cannot make captured package metadata disappear.
use super::*;
use sailry_protocol::plugin::{connector::State, mcp::Availability};

#[tokio::test]
async fn reads_removed_versions() {
    for remote in [false, true] {
        let peer = input::Server::url("https://example.invalid/continue", false).await;
        let issuer = issuer::Server::start(&peer.endpoint).await;
        let model = Server::start(false).await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        isolate(&fixture.client).await;
        configure(&fixture, &issuer.endpoint, false, false);
        let installed = install(&fixture, 0).await;
        assert!(installed.mcp_source.is_none());
        let (_, attempt, url) = begin(&fixture.client, installed.summary.reference(), false).await;
        let account = complete(&fixture.client, &attempt, &url).await;
        assert!(account.enabled);
        assert_eq!(account.state, State::Connected);
        let Output::PluginMcp(configuration) = execute(
            &fixture.client,
            Command::ReadPluginMcp {
                package: installed.summary.reference(),
            },
        )
        .await
        else {
            panic!("MCP configuration expected")
        };
        let Output::Plugin(configured) = execute(
            &fixture.client,
            Command::SavePluginMcp {
                package: configuration.package,
                configuration: configuration.configuration.clone(),
            },
        )
        .await
        else {
            panic!("configured plugin expected")
        };
        assert_eq!(configured.mcp[0].availability, Availability::Available);
        let reference = configured.summary.reference();
        assert!(reference.settings_revision > 0);
        let (_, captured) = submit(&fixture, true).await;
        assert!(captured.plugins.contains(&reference));
        let profile = fixture.node.profile().to_owned();
        let package = profile.join("plugins/packages").join(&reference.digest);
        let declaration = fs::read(package.join("mcp.json")).unwrap();
        let skill = fs::read(package.join("skills/analysis/SKILL.md")).unwrap();
        let account_body = lifecycle::stored(&fixture.node, account.id);
        let db = rusqlite::Connection::open_with_flags(
            profile.join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let settings = || {
            db.query_row(
                "SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2",
                rusqlite::params![reference.name, reference.settings_revision as i64],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap()
        };
        let captures = || {
            let mut query = db
                .prepare("SELECT plugin,connector,connection,revision FROM turn_connections WHERE turn=?1 ORDER BY connection")
                .unwrap();
            query
                .query_map([captured.id.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .unwrap()
                .map(|row| row.unwrap())
                .collect::<Vec<_>>()
        };
        let saved = settings();
        let frozen = captures();
        assert!(!frozen.is_empty());
        let saved_value: Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(
            saved_value["mcp"],
            serde_json::to_value(&configuration.configuration).unwrap()
        );
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: configured.summary.name,
                expected_revision: configured.summary.revision,
            },
        )
        .await;
        for restored in [false, true] {
            if restored {
                fixture.node.shutdown().await.unwrap();
                fixture.node = Node::start_empty(&profile).await.unwrap();
                fixture.client = Client::new(if remote {
                    fixture
                        .controller
                        .handle()
                        .remote(fixture.node.link().address())
                } else {
                    fixture.node.local()
                });
            }
            let Output::Plugin(retained) = execute(
                &fixture.client,
                Command::ReadPluginVersion {
                    package: reference.clone(),
                },
            )
            .await
            else {
                panic!("retained plugin expected")
            };
            assert_eq!(retained.summary.reference(), reference);
            assert!(retained.summary.enabled);
            assert_eq!(retained.skills, configured.skills);
            assert!(retained.mcp[0].allowed);
            assert_eq!(retained.mcp[0].availability, Availability::Unavailable);
            assert_eq!(fs::read(package.join("mcp.json")).unwrap(), declaration);
            assert_eq!(
                fs::read(package.join("skills/analysis/SKILL.md")).unwrap(),
                skill
            );
            assert_eq!(settings(), saved);
            assert_eq!(captures(), frozen);
            assert_eq!(lifecycle::stored(&fixture.node, account.id), account_body);
            let Output::QueuedMessage(queued) = execute(
                &fixture.client,
                Command::ReadQueuedTurn { turn: captured.id },
            )
            .await
            else {
                panic!("captured turn expected")
            };
            assert_eq!(queued.turn.plugins, captured.plugins);
            for command in [
                Command::ReadPlugin {
                    name: reference.name.clone(),
                },
                Command::ReadPluginMcp {
                    package: reference.clone(),
                },
                Command::CheckPluginMcp {
                    package: reference.clone(),
                    server: "input".into(),
                },
            ] {
                assert_eq!(
                    fixture
                        .client
                        .execute(fixture.client.prepare(command))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::NotFound
                );
            }
        }
        let (_, next) = submit(&fixture, true).await;
        assert!(
            next.plugins
                .iter()
                .all(|package| package.name != reference.name)
        );
        assert!(model.requests.lock().unwrap().is_empty());
        assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
        drop(db);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
