use super::*;
use sailry_link::Response;
use sailry_protocol::{Event, NodeId, plugin};
use tokio::sync::{broadcast, oneshot};

struct Fixture {
    _directory: tempfile::TempDir,
    database: Database,
    events: broadcast::Sender<sailry_protocol::EventEnvelope>,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().canonicalize().unwrap();
        let mut database = Database::open(&profile.join("node.turso"), NodeId([1; 32]), None)
            .await
            .unwrap();
        let host = crate::plugins::Host::new(Some(profile));
        let mut result = Ok(Output::Plugin(host.install_bundled("code").unwrap()));
        super::super::plugins::finish(
            &database.connection,
            host.profile(),
            &Command::InstallBundledPlugin {
                name: "code".into(),
                expected_revision: 0,
            },
            &mut result,
        )
        .await
        .unwrap();
        result.unwrap();
        initialize(&mut database.connection).await.unwrap();
        Self {
            _directory: directory,
            database,
            events: broadcast::channel(16).0,
        }
    }

    async fn execute(&mut self, request: Request) -> Response {
        let (reply, response) = oneshot::channel();
        let node = self.database.node;
        assert!(
            self.database
                .dispatch(node, request, reply, &self.events)
                .await
                .is_none()
        );
        response.await.unwrap().unwrap().completion.await.unwrap()
    }

    async fn package(&self) -> plugin::Info {
        super::super::plugins::get(&self.database.connection, "code")
            .await
            .unwrap()
            .unwrap()
    }

    async fn events(&self) -> Vec<Event> {
        let mut query = self
            .database
            .connection
            .prepare("SELECT body FROM events ORDER BY cursor")
            .await
            .unwrap();
        (async {
            let mut rows = query.query(()).await?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await? {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
                values.push(mapped);
            }
            Ok::<_, turso::Error>(values.into_iter())
        })
        .await
        .unwrap()
        .map(|body| serde_json::from_slice(&body.unwrap()).unwrap())
        .collect()
    }
}

#[tokio::test]
async fn rolls_back_both_switches_on_failure() {
    let mut fixture = Fixture::new().await;
    let original = fixture.package().await;
    let before = settings(&fixture.database.connection).await.unwrap();
    let events = fixture.events().await;
    fixture.database.connection.execute_batch("CREATE TRIGGER reject_code BEFORE INSERT ON code_settings BEGIN SELECT RAISE(ABORT,'fixture code failure'); END;").await.unwrap();
    let request = Request::new(
        fixture.database.node,
        Command::SetPluginEnabled {
            name: "code".into(),
            expected_revision: original.summary.revision,
            enabled: false,
        },
    );
    let failed = fixture.execute(request.clone()).await.unwrap_err();
    assert_eq!(fixture.package().await, original);
    assert_eq!(
        settings(&fixture.database.connection).await.unwrap(),
        before
    );
    assert_eq!(fixture.events().await, events);
    assert_eq!(fixture.execute(request).await.unwrap_err(), failed);
    assert_eq!(fixture.events().await, events);
    fixture
        .database
        .connection
        .execute_batch("DROP TRIGGER reject_code")
        .await
        .unwrap();
    let mut changed = before;
    changed.after_edit = false;
    let request = Request::new(
        fixture.database.node,
        Command::SaveCodeSettings {
            expected_revision: changed.revision,
            settings: changed,
        },
    );
    fixture.execute(request).await.unwrap();
    let before = settings(&fixture.database.connection).await.unwrap();
    let events = fixture.events().await;
    fixture.database.connection.execute_batch("CREATE TRIGGER reject_plugin BEFORE UPDATE ON plugins BEGIN SELECT RAISE(ABORT,'fixture plugin failure'); END;").await.unwrap();
    let mut changed = before.clone();
    changed.enabled = false;
    let request = Request::new(
        fixture.database.node,
        Command::SaveCodeSettings {
            expected_revision: changed.revision,
            settings: changed,
        },
    );
    let failed = fixture.execute(request.clone()).await.unwrap_err();
    assert_eq!(fixture.package().await, original);
    assert_eq!(
        settings(&fixture.database.connection).await.unwrap(),
        before
    );
    assert_eq!(fixture.events().await, events);
    assert_eq!(fixture.execute(request).await.unwrap_err(), failed);
    assert_eq!(fixture.events().await, events);
    fixture
        .database
        .connection
        .execute_batch("DROP TRIGGER reject_plugin")
        .await
        .unwrap();
    let mut changed = before;
    changed.enabled = false;
    let request = Request::new(
        fixture.database.node,
        Command::SaveCodeSettings {
            expected_revision: changed.revision,
            settings: changed,
        },
    );
    fixture.execute(request).await.unwrap();
    let package = fixture.package().await;
    assert!(!package.summary.enabled);
    assert_eq!(
        fixture.events().await.last(),
        Some(&Event::PluginChanged(package.summary))
    );
}

#[tokio::test]
async fn initializes_declared_service_owner() {
    let mut fixture = Fixture::new().await;
    let mut package = fixture.package().await;
    let mut owner = package.clone();
    owner.summary.name = "language-tools".into();
    owner.summary.enabled = false;
    package.extension.as_mut().unwrap().enabled_service = None;
    fixture
        .database
        .connection
        .execute(
            "UPDATE plugins SET body=?1 WHERE name=?2",
            params![encode(&package).unwrap(), package.summary.name.as_str()],
        )
        .await
        .unwrap();
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,?2,1,?3)",
            params![
                owner.summary.name.as_str(),
                owner.summary.revision as i64,
                encode(&owner).unwrap()
            ],
        )
        .await
        .unwrap();
    let initialized = initialize(&mut fixture.database.connection).await.unwrap();
    assert!(!initialized.enabled);
    assert_eq!(fixture.package().await, package);
    assert_eq!(
        initialize(&mut fixture.database.connection).await.unwrap(),
        initialized
    );
    owner.summary.enabled = true;
    package.summary.enabled = false;
    for updated in [&owner, &package] {
        fixture
            .database
            .connection
            .execute(
                "UPDATE plugins SET body=?1 WHERE name=?2",
                params![encode(updated).unwrap(), updated.summary.name.as_str()],
            )
            .await
            .unwrap();
    }
    assert!(
        initialize(&mut fixture.database.connection)
            .await
            .unwrap()
            .enabled
    );
}

#[tokio::test]
async fn preserves_removed_package_choice_at_startup() {
    let mut fixture = Fixture::new().await;
    let original = fixture.package().await;
    let mut code = settings(&fixture.database.connection).await.unwrap();
    code.after_edit = false;
    let expected = code.revision;
    save(&fixture.database.connection, &code, expected)
        .await
        .unwrap();
    fixture
        .execute(Request::new(
            fixture.database.node,
            Command::RemovePlugin {
                name: "code".into(),
                expected_revision: original.summary.revision,
            },
        ))
        .await
        .unwrap();
    let before = settings(&fixture.database.connection).await.unwrap();
    let mut inconsistent = before.clone();
    inconsistent.enabled = true;
    save(
        &fixture.database.connection,
        &inconsistent,
        inconsistent.revision,
    )
    .await
    .unwrap();
    let initialized = initialize(&mut fixture.database.connection).await.unwrap();
    assert!(!initialized.enabled);
    assert!(!initialized.after_edit);
    assert_eq!(initialized.revision, before.revision + 2);
    assert!(
        super::super::plugins::get(&fixture.database.connection, "code")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        initialize(&mut fixture.database.connection).await.unwrap(),
        initialized
    );
    fixture
        .database
        .connection
        .execute("DELETE FROM plugins WHERE name='code'", ())
        .await
        .unwrap();
    assert_eq!(
        initialize(&mut fixture.database.connection).await.unwrap(),
        initialized
    );
}

#[tokio::test]
async fn preserves_completion_when_refresh_fails() {
    use crate::Node;
    use sailry_client::Client;
    use sailry_link::{Link, NetworkScope};

    for remote in [false, true] {
        for install in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let node = Node::start_empty(directory.path().join("profile"))
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
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::InstallBundledPlugin {
                    name: "code".into(),
                    expected_revision: 0,
                }))
                .await
                .unwrap()
            else {
                panic!("plugin expected")
            };
            let original = node.with_storage(|db| Box::pin(async move {
                let transaction = db.transaction().await.map_err(storage_error)?;
                let original = encode(&settings(&transaction).await?)?;
                transaction.execute(
                    "INSERT INTO code_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",
                    [original.as_slice()],
                ).await.map_err(storage_error)?;
                transaction.commit().await.map_err(storage_error)?;
                Ok(original)
            })).await.unwrap();
            let mut changed: sailry_protocol::code::Settings =
                serde_json::from_slice(&original).unwrap();
            changed.enabled = false;
            let request = client.prepare(if install {
                Command::InstallBundledPlugin {
                    name: "code".into(),
                    expected_revision: package.summary.revision,
                }
            } else {
                Command::SaveCodeSettings {
                    expected_revision: changed.revision,
                    settings: changed,
                }
            });
            // Corrupt only the disposable fixture's refresh input after durable completion.
            let id = request.id;
            node.with_storage(move |db| Box::pin(async move {
                db.execute_batch(format!(
                    "CREATE TRIGGER fail_refresh AFTER UPDATE OF status ON requests WHEN NEW.id='{id}' AND NEW.status='completed' BEGIN UPDATE code_settings SET body=x'7b'; END;"
                )).await.map_err(storage_error)?;
                Ok(())
            })).await.unwrap();
            let response = client.execute(request.clone()).await.unwrap();
            assert!(matches!(
                response,
                Output::CodeSettings(_) | Output::Plugin(_)
            ));
            let (invalid, events, stored) = node
                .with_storage(move |db| {
                    Box::pin(async move {
                        let invalid = settings(db).await.is_err();
                        let events = db
                            .prepare("SELECT count(*) FROM events")
                            .await
                            .map_err(storage_error)?
                            .query_row(())
                            .await
                            .map_err(storage_error)?
                            .get::<i64>(0)
                            .map_err(storage_error)?;
                        let stored = db
                            .prepare(
                                "SELECT result FROM requests WHERE id=?1 AND status='completed'",
                            )
                            .await
                            .map_err(storage_error)?
                            .query_row([id.to_string()])
                            .await
                            .map_err(storage_error)?
                            .get::<String>(0)
                            .map_err(storage_error)?;
                        Ok((invalid, events, stored))
                    })
                })
                .await
                .unwrap();
            assert!(invalid, "the post-commit refresh must fail");
            let stored: sailry_link::Response = serde_json::from_str(&stored).unwrap();
            assert_eq!(stored, Ok(response.clone()));
            assert_eq!(client.execute(request).await.unwrap(), response);
            let (count, last) = node
                .with_storage(|db| {
                    Box::pin(async move {
                        let count = db
                            .prepare("SELECT count(*) FROM events")
                            .await
                            .map_err(storage_error)?
                            .query_row(())
                            .await
                            .map_err(storage_error)?
                            .get::<i64>(0)
                            .map_err(storage_error)?;
                        let last = db
                            .prepare("SELECT body FROM events ORDER BY cursor DESC LIMIT 1")
                            .await
                            .map_err(storage_error)?
                            .query_row(())
                            .await
                            .map_err(storage_error)?
                            .get::<Vec<u8>>(0)
                            .map_err(storage_error)?;
                        Ok((count, last))
                    })
                })
                .await
                .unwrap();
            assert_eq!(count, events);
            assert!(matches!(
                serde_json::from_slice::<Event>(&last).unwrap(),
                Event::PluginChanged(_)
            ));
            node.with_storage(move |db| {
                Box::pin(async move {
                    let transaction = db.transaction().await.map_err(storage_error)?;
                    transaction
                        .execute_batch("DROP TRIGGER fail_refresh")
                        .await
                        .map_err(storage_error)?;
                    transaction
                        .execute("UPDATE code_settings SET body=?1", [original])
                        .await
                        .map_err(storage_error)?;
                    transaction.commit().await.map_err(storage_error)?;
                    Ok(())
                })
            })
            .await
            .unwrap();
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}
