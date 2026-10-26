use super::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use turso::{Connection, params};

const SOURCE: &str = "<!doctype html><html><head><style>button{border-radius:6px}</style></head><body><button id='next'>Next</button><output id='count'>0</output><script>let count=0;document.getElementById('next').onclick=()=>document.getElementById('count').textContent=String(++count);</script></body></html>";

#[tokio::test]
async fn retains_code_and_defaults_metadata() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            json!({"kind":"preview","title":"Interactive checklist","html":SOURCE}),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(
            &fixture,
            0,
            "<html><head></head><body>Document template</body></html>",
        )
        .await;
        let turn = submit(&fixture, "Create the requested interactive preview").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let preview = cards(&fixture.client, &page).await.remove(0);
        assert_eq!(preview.kind, artifact::Kind::Preview);
        assert_eq!(preview.size, SOURCE.len() as u64);
        let content = fixture
            .client
            .read_artifact(preview.id, None)
            .await
            .unwrap();
        assert_eq!(content.artifact, preview);
        assert_eq!(content.html, SOURCE);
        assert!(content.view.is_none());
        assert!(content.warnings.is_empty());
        let versions = fixture
            .client
            .list_artifact_revisions(preview.id, None, 64)
            .await
            .unwrap();
        assert_eq!(versions.items.len(), 1);
        assert_eq!(versions.items[0].kind, artifact::Kind::Preview);
        let assets = fixture
            .client
            .conversation_assets(
                fixture.session.id,
                conversation::assets::Query {
                    kind: Some(conversation::assets::Kind::Artifact),
                    before: None,
                    limit: 20,
                },
            )
            .await
            .unwrap();
        assert!(assets.groups.iter().flat_map(|group| &group.items).any(|item| {
            matches!(item, conversation::assets::Target::Document(value) if value == &preview)
        }));

        server.replace_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(None, None, "Report"),
        )]);
        let turn = submit(&fixture, "Create a structured report").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let document = cards(&fixture.client, &page)
            .await
            .into_iter()
            .find(|value| value.title == "Report")
            .unwrap();
        assert_eq!(document.kind, artifact::Kind::Document);
        let report = fixture
            .client
            .read_artifact(document.id, None)
            .await
            .unwrap();
        assert!(!report.html.contains("<script"));
        assert!(report.view.is_some());

        let profile = fixture.node.profile().to_owned();
        let session = fixture.session.id;
        let identity = fixture.node.id();
        drop(fixture.client);
        fixture.node.shutdown().await.unwrap();
        let path = profile.join("storage/node.turso");
        let before_backups = backups(&profile);
        let (database, db) = crate::database_fixture::open(&path, false).await;
        let captures = records(&db, "SELECT body FROM agent_events ORDER BY rowid").await;
        let requests = records(&db, "SELECT body FROM requests ORDER BY rowid").await;
        let outcomes = records(
            &db,
            "SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL ORDER BY rowid",
        )
        .await;
        let before = remove_kind(&db, "artifacts", "id", document.id).await;
        let revision = remove_kind(&db, "artifact_revisions", "artifact", document.id).await;
        crate::database_fixture::close(database, db).await;

        let node = Node::start_empty(&profile).await.unwrap();
        assert_eq!(node.id(), identity);
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let restored = client.read_artifact(document.id, None).await.unwrap();
        assert_eq!(restored, report);
        assert_eq!(
            client.read_artifact(preview.id, None).await.unwrap(),
            content
        );
        assert_eq!(
            client
                .list_artifacts(session, None, 64)
                .await
                .unwrap()
                .items
                .len(),
            2
        );
        drop(client);
        node.shutdown().await.unwrap();

        let saved = backups(&profile);
        let added: Vec<_> = saved
            .iter()
            .filter(|path| !before_backups.contains(path))
            .collect();
        assert_eq!(added.len(), 1);
        // The completed backup is immutable; inspection must not change its native family.
        let frozen = files(added[0].parent().unwrap());
        let (database, backup) = crate::database_fixture::open(added[0], true).await;
        assert_eq!(body(&backup, "artifacts", "id", document.id).await, before);
        assert_eq!(
            body(&backup, "artifact_revisions", "artifact", document.id).await,
            revision
        );
        crate::database_fixture::close(database, backup).await;
        assert_eq!(files(added[0].parent().unwrap()), frozen);
        assert_eq!(backups(&profile), saved);
        let (database, db) = crate::database_fixture::open(&path, false).await;
        assert_eq!(
            records(&db, "SELECT body FROM agent_events ORDER BY rowid").await,
            captures
        );
        assert_eq!(
            records(&db, "SELECT body FROM requests ORDER BY rowid").await,
            requests
        );
        assert_eq!(
            records(
                &db,
                "SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL ORDER BY rowid"
            )
            .await,
            outcomes
        );
        let stored: artifact::Summary =
            serde_json::from_slice(&body(&db, "artifacts", "id", document.id).await).unwrap();
        assert_eq!(stored, document);
        crate::database_fixture::close(database, db).await;
        for kind in [json!("unknown"), Value::Null] {
            let (database, db) = crate::database_fixture::open(&path, false).await;
            let mut invalid: Value =
                serde_json::from_slice(&body(&db, "artifacts", "id", document.id).await).unwrap();
            invalid["kind"] = kind;
            db.execute(
                "UPDATE artifacts SET body=?1 WHERE id=?2",
                params![
                    serde_json::to_vec(&invalid).unwrap(),
                    document.id.to_string()
                ],
            )
            .await
            .unwrap();
            crate::database_fixture::close(database, db).await;
            let bytes = fs::read(&path).unwrap();
            let family = files(path.parent().unwrap());
            let error = Node::start_empty(&profile)
                .await
                .err()
                .expect("invalid artifact kind must fail startup");
            assert!(error.to_string().contains("existing data was not changed"));
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(files(path.parent().unwrap()), family);
            assert_eq!(backups(&profile), saved);
            assert_eq!(
                fs::read_to_string(
                    profile
                        .join("artifacts")
                        .join(session.to_string())
                        .join(preview.id.to_string())
                        .join("1.html")
                )
                .unwrap(),
                SOURCE
            );
        }
        fixture.controller.close().await.unwrap();
    }
}

async fn records(db: &Connection, sql: &str) -> Vec<Vec<u8>> {
    let mut rows = db.query(sql, ()).await.unwrap();
    let mut values = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        values.push(row.get(0).unwrap());
    }
    values
}

async fn body(db: &Connection, table: &str, field: &str, id: ArtifactId) -> Vec<u8> {
    db.prepare(format!("SELECT body FROM {table} WHERE {field}=?1"))
        .await
        .unwrap()
        .query_row([id.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap()
}

async fn remove_kind(db: &Connection, table: &str, field: &str, id: ArtifactId) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(&body(db, table, field, id).await).unwrap();
    assert_eq!(
        value.as_object_mut().unwrap().remove("kind"),
        Some(json!("document"))
    );
    let before = serde_json::to_vec(&value).unwrap();
    db.execute(
        &format!("UPDATE {table} SET body=?1 WHERE {field}=?2"),
        params![before.as_slice(), id.to_string()],
    )
    .await
    .unwrap();
    before
}

fn backups(profile: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(profile.join("storage/backups"))
        .into_iter()
        .flatten()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect();
    paths.sort();
    paths
}

fn files(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| (entry.file_name().into(), fs::read(entry.path()).unwrap()))
        .collect()
}
