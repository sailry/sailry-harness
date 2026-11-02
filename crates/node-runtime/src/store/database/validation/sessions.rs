use super::*;
use crate::Node;
use crate::store::database::{connect, driver, engine, schema};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_protocol::{
    Command, NodeId, QueuedTurn, Request, RequestOutcome, Session,
    conversation::{Association, Execution},
};
use std::path::{Path, PathBuf};

struct Fixture {
    _root: tempfile::TempDir,
    profile: PathBuf,
    controller: Link,
    remote: bool,
    node: NodeId,
    session: Session,
    turn: QueuedTurn,
    request: Request,
}

impl Fixture {
    async fn new(remote: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let controller = Link::controller(root.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let config = serde_json::from_value(serde_json::json!({
            "provider": sailry_protocol::ProviderId::new(), "model": "fixture",
            "mode": "code", "permission": "ask", "options": {}, "credential": null
        }))
        .unwrap();
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(config),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let request = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Preserved captured input".into(),
        });
        let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        client
            .execute(client.prepare(Command::RemoveQueuedTurn {
                turn: turn.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let identity = node.id();
        drop(client);
        node.shutdown().await.unwrap();
        Self {
            _root: root,
            profile,
            controller,
            remote,
            node: identity,
            session,
            turn,
            request,
        }
    }

    fn path(&self) -> PathBuf {
        self.profile.join("storage/node.turso")
    }

    async fn reopen(&self) -> (Node, Client) {
        let node = Node::start_empty(&self.profile).await.unwrap();
        let client = Client::new(if self.remote {
            self.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        (node, client)
    }

    fn backups(&self) -> Vec<PathBuf> {
        let directory = self.profile.join("storage/backups");
        if !directory.exists() {
            return Vec::new();
        }
        let mut paths = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path().join("node.turso"))
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    async fn verify(&self, client: &Client, association: Option<Association>) {
        let Output::Session(session) = client
            .execute(client.prepare(Command::ReadSession {
                session: self.session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(session.association, association);
        assert_eq!(session.config, self.session.config);
        assert_eq!(session.roles, self.session.roles);
        assert_eq!(session.roles.execution, Execution::Team);
        assert_eq!(
            client
                .read_turn_capture(session.id, self.turn.id)
                .await
                .unwrap(),
            self.turn
        );
        assert_eq!(
            client.outcome(&self.request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(Output::QueuedTurn(self.turn.clone()))))
        );
    }
}

async fn open(path: &Path) -> (turso::Database, Connection) {
    let handle = engine(path).await.unwrap();
    let db = connect(&handle).await.unwrap();
    (handle, db)
}

async fn read(path: &Path) -> (turso::Database, Connection) {
    let handle = turso::Builder::new_local(path.to_str().unwrap())
        .experimental_index_method(true)
        .read_only(true)
        .build()
        .await
        .unwrap();
    let db = connect(&handle).await.unwrap();
    (handle, db)
}

async fn close(handle: turso::Database, db: Connection) {
    driver::checkpoint(&db).await.unwrap();
    drop(db);
    drop(handle);
}

async fn captures(db: &Connection) -> Vec<Vec<u8>> {
    let mut rows = db
        .query(
            "SELECT roles FROM session_revisions ORDER BY session,revision",
            (),
        )
        .await
        .unwrap();
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        result.push(row.get(0).unwrap());
    }
    let mut rows = db
        .query(
            "SELECT roles FROM turns WHERE roles IS NOT NULL ORDER BY id",
            (),
        )
        .await
        .unwrap();
    while let Some(row) = rows.next().await.unwrap() {
        result.push(row.get(0).unwrap());
    }
    result
}

#[tokio::test]
async fn fills_schema_and_defaults_without_rewriting_captures() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let (handle, db) = open(&fixture.path()).await;
        db.execute_batch(
            "DROP TABLE session_associations;
             UPDATE session_revisions SET roles=CAST(json_remove(roles,'$.execution') AS BLOB);
             UPDATE turns SET roles=CAST(json_remove(roles,'$.execution') AS BLOB) WHERE roles IS NOT NULL;",
        )
        .await
        .unwrap();
        let original = captures(&db).await;
        assert!(!original.is_empty());
        for body in &original {
            assert!(
                serde_json::from_slice::<serde_json::Value>(body)
                    .unwrap()
                    .get("execution")
                    .is_none()
            );
            assert_eq!(
                serde_json::from_slice::<role::Snapshot>(body)
                    .unwrap()
                    .execution,
                Execution::Team
            );
        }
        close(handle, db).await;
        let (node, client) = fixture.reopen().await;
        fixture.verify(&client, None).await;
        drop(client);
        node.shutdown().await.unwrap();
        let backups = fixture.backups();
        assert_eq!(backups.len(), 1);
        let backup_bytes = std::fs::read(&backups[0]).unwrap();
        let (handle, db) = read(&backups[0]).await;
        assert!(
            db.prepare("SELECT namespace FROM session_associations")
                .await
                .is_err()
        );
        assert_eq!(captures(&db).await, original);
        drop(db);
        drop(handle);
        let (handle, db) = open(&fixture.path()).await;
        crate::store::database::validate_backup(&db, fixture.node)
            .await
            .unwrap();
        assert_eq!(captures(&db).await, original);
        close(handle, db).await;
        let (node, client) = fixture.reopen().await;
        fixture.verify(&client, None).await;
        drop(client);
        node.shutdown().await.unwrap();
        assert_eq!(fixture.backups(), backups);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), backup_bytes);
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_associations_and_rejects_invalid_upgrades() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let association = Association {
            namespace: "fixture".into(),
            kind: "task".into(),
            id: "scheduled-1".into(),
        };
        let (handle, db) = open(&fixture.path()).await;
        crate::store::sessions::associations::bind(&db, fixture.session.id, &association)
            .await
            .unwrap();
        crate::store::database::validate_backup(&db, fixture.node)
            .await
            .unwrap();
        close(handle, db).await;
        let (node, client) = fixture.reopen().await;
        fixture.verify(&client, Some(association.clone())).await;
        let Output::AssociatedSessions(page) = client
            .execute(client.prepare(Command::ListAssociatedSessions {
                association: association.clone(),
                before: None,
                limit: 10,
            }))
            .await
            .unwrap()
        else {
            panic!("associated sessions expected")
        };
        assert_eq!(page.sessions.len(), 1);
        assert_eq!(page.sessions[0].id, fixture.session.id);
        drop(client);
        node.shutdown().await.unwrap();
        assert!(fixture.backups().is_empty());

        let saved = schema::backup::preview(&fixture.path()).unwrap();
        let (handle, db) = read(saved.path()).await;
        crate::store::database::validate_backup(&db, fixture.node)
            .await
            .unwrap();
        assert_eq!(
            crate::store::sessions::associations::read(&db, fixture.session.id)
                .await
                .unwrap(),
            Some(association)
        );
        drop(db);
        drop(handle);
        let (handle, db) = open(&fixture.path()).await;
        db.execute(
            "UPDATE session_associations SET identity=' private-association-marker '",
            (),
        )
        .await
        .unwrap();
        let error = stored(&db).await.unwrap_err().to_string();
        assert!(error.contains("session association"), "{error}");
        assert!(!error.contains("private-association-marker"));
        assert!(
            crate::store::database::validate_backup(&db, fixture.node)
                .await
                .is_err()
        );
        db.execute("DROP INDEX session_association_identity", ())
            .await
            .unwrap();
        close(handle, db).await;
        let original = std::fs::read(fixture.path()).unwrap();
        let error = Node::start_empty(&fixture.profile)
            .await
            .err()
            .expect("invalid upgrade must fail");
        assert!(error.to_string().contains("session association"));
        assert!(!error.to_string().contains("private-association-marker"));
        assert_eq!(std::fs::read(fixture.path()).unwrap(), original);
        assert!(fixture.backups().is_empty());
        let (handle, db) = open(&fixture.path()).await;
        let count: i64 = db
            .prepare("SELECT count(*) FROM sqlite_schema WHERE name='session_association_identity'")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get(0)
            .unwrap();
        assert_eq!(count, 0);
        let identity: String = db
            .prepare("SELECT identity FROM session_associations")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get(0)
            .unwrap();
        assert_eq!(identity, " private-association-marker ");
        close(handle, db).await;
        fixture.controller.close().await.unwrap();
    }
}
