use sailry_client::{Apply, Client, Projection};
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use serde_json::{Value, json};
use std::time::Duration;
#[path = "memory_support/mod.rs"]
mod support;
use support::{Entry, Kind, MemoryId, Summary, put};

fn entry(project: Option<ProjectId>, title: &str) -> Entry {
    Entry {
        summary: Summary {
            id: MemoryId::new(),
            project,
            title: title.into(),
            kind: Kind::Feedback,
            revision: 0,
            updated_at_ms: 0,
            archived: false,
        },
        body: "Use concise 中文 🙂 explanations".into(),
    }
}

async fn fixture(remote: bool) -> (tempfile::TempDir, Node, Link, Client) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
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
    let root = directory.path().join("fixture");
    support::prepare(&root.join("memory"));
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Memory fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let worktree = snapshot
        .worktrees
        .iter()
        .find(|item| item.project == Some(project.id))
        .unwrap()
        .id;
    support::install(&client, worktree, "memory").await;
    (directory, node, controller, client)
}
async fn search(client: &Client, project: Option<ProjectId>, query: &str) -> Vec<Entry> {
    support::run(
        client,
        &support::context(client).await,
        json!({"action":"search","project":project,"query":query}),
    )
    .await
    .unwrap()
}
async fn run<T: serde::de::DeserializeOwned>(client: &Client, input: Value) -> Result<T, Fault> {
    support::run(client, &support::context(client).await, input).await
}
async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
}

#[tokio::test]
async fn restores_scoped_revisions() {
    for remote in [false, true] {
        let (directory, node, controller, client) = fixture(remote).await;
        let mut projects = vec![];
        for name in ["one", "two"] {
            let root = directory.path().join(name);
            std::fs::create_dir(&root).unwrap();
            let Output::Project(project) = client
                .execute(client.prepare(Command::RegisterProject {
                    name: name.into(),
                    path: root.to_str().unwrap().into(),
                }))
                .await
                .unwrap()
            else {
                panic!("project expected")
            };
            projects.push(project.id);
        }
        // The opposite transport receives only generic invalidations, then reads package-owned data.
        let observer = Client::new(if remote {
            node.local()
        } else {
            controller.handle().remote(node.link().address())
        });
        let mut updates = observer.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        assert_eq!(
            projection.apply(1, updates.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        let request = support::request(
            &client,
            &support::context(&client).await,
            json!({
                "action":"put","entry":entry(None,"Global preference"),"expected_revision":0
            }),
        );
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        drop(admitted);
        let saved = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), saved);
        let global: Entry = support::output(saved.clone()).unwrap();
        let first = put(&client, entry(Some(projects[0]), "First constraint"), 0)
            .await
            .unwrap();
        let second = put(&client, entry(Some(projects[1]), "Second constraint"), 0)
            .await
            .unwrap();
        assert_eq!(
            search(&client, None, "").await.as_slice(),
            std::slice::from_ref(&global)
        );
        assert!(
            search(&client, Some(projects[0]), "constraint")
                .await
                .iter()
                .all(|item| item.summary.id == first.summary.id)
        );
        assert_eq!(
            search(&client, Some(projects[1]), &second.summary.id.to_string())
                .await
                .as_slice(),
            std::slice::from_ref(&second)
        );
        assert!(
            search(&client, Some(projects[0]), &second.summary.id.to_string())
                .await
                .is_empty()
        );
        let mut changed = first.clone();
        changed.body = "Updated preference".into();
        let changed = put(&client, changed, 1).await.unwrap();
        assert_eq!(changed.summary.revision, 2);
        assert_eq!(run::<Entry>(&client, json!({"action":"merge","entry":changed,"sources":[{"id":second.summary.id,"revision":second.summary.revision}]})).await.unwrap_err().code, ErrorCode::InvalidRequest);
        assert_eq!(
            put(&client, first, 1).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let mut moved = changed.clone();
        moved.summary.project = None;
        assert_eq!(
            put(&client, moved, 2).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        let committed = snapshot(&client).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while projection.snapshot().unwrap().cursor < committed.cursor {
                let update = updates.next().await.unwrap();
                assert!(matches!(&update, Update::Event(envelope) if envelope.event == Event::PluginValuesChanged { name: "memory".into() }));
                projection.apply(1, update).unwrap();
            }
        }).await.unwrap();
        assert_eq!(projection.snapshot().unwrap(), &committed);
        assert_eq!(
            support::read(&observer, changed.summary.id).await.unwrap(),
            changed
        );
        let projected = serde_json::to_value(projection.snapshot().unwrap()).unwrap();
        assert!(projected.get("memories").is_none() && projected.get("memory_settings").is_none());
        assert!(!projected.to_string().contains("Updated preference"));
        // Generic indexed retrieval remains nondurable; the package supplies tokenizer and policy.
        let context = support::context(&client).await;
        let read = client
            .dispatch(
                client
                    .prepare(Command::SearchPluginValues(plugin::storage::Search {
                        terms: vec!["中文".into()],
                        weights: [3, 1],
                        all: vec!["state:active".into(), "scope:global".into()],
                        any: vec![],
                        offset: 0,
                        limit: 20,
                    }))
                    .with_plugin(context),
            )
            .await
            .unwrap();
        assert!(!read.receipt.durable);
        let Output::PluginSearch(page) = read.completion.await.unwrap().unwrap() else {
            panic!("search expected")
        };
        assert_eq!(page.entries.len(), 1);
        assert_eq!(
            page.entries[0].value["summary"]["id"],
            json!(global.summary.id)
        );
        assert_eq!(search(&client, None, "中文").await, [global]);
        let mut invalid = entry(None, "Too large");
        invalid.body = "x".repeat(8193);
        assert_eq!(
            put(&client, invalid, 0).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            put(&client, entry(Some(ProjectId::new()), "Missing project"), 0)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), saved);
        assert_eq!(
            search(&client, Some(projects[0]), "Updated")
                .await
                .as_slice(),
            std::slice::from_ref(&changed)
        );
        assert_eq!(
            run::<Vec<MemoryId>>(
                &client,
                json!({"action":"remove","id":changed.summary.id,"expected_revision":1})
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::RevisionConflict
        );
        let remove = support::request(
            &client,
            &support::context(&client).await,
            json!({"action":"remove","id":changed.summary.id,"expected_revision":2}),
        );
        let removed = client.execute(remove.clone()).await.unwrap();
        assert_eq!(
            support::output::<Vec<MemoryId>>(removed.clone()).unwrap(),
            [changed.summary.id]
        );
        assert_eq!(client.execute(remove).await.unwrap(), removed);
        assert!(
            search(&client, Some(projects[0]), "Updated")
                .await
                .is_empty()
        );
        let other = Node::start(directory.path().join("other")).await.unwrap();
        let other_client = Client::new(other.local());
        let root = directory.path().join("other-fixture");
        support::prepare(&root.join("memory"));
        other_client
            .execute(other_client.prepare(Command::RegisterProject {
                name: "Other".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap();
        let other_worktree = snapshot(&other_client).await.worktrees[0].id;
        support::install(&other_client, other_worktree, "memory").await;
        assert!(search(&other_client, None, "").await.is_empty());
        other.shutdown().await.unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_incompatible_profiles() {
    for schema in [
        "ALTER TABLE plugin_values RENAME COLUMN name TO incompatible_name",
        "ALTER TABLE plugin_values RENAME COLUMN key TO incompatible_key",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(schema).unwrap();
        drop(db);
        let original = std::fs::read(&path).unwrap();
        assert!(Node::start(&profile).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

#[tokio::test]
async fn applies_retrieval_configuration() {
    for remote in [false, true] {
        let (directory, node, controller, client) = fixture(remote).await;
        let mut settings = support::settings(&client).await.unwrap();
        settings.auto_write = true;
        settings.context_bytes = 4096;
        settings.review_after_days = 30;
        let request = support::request(
            &client,
            &support::context(&client).await,
            json!({"action":"saveSettings","settings":settings}),
        );
        let saved = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), saved);
        assert_eq!(
            support::save_settings(&client, settings)
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let mut invalid = support::settings(&client).await.unwrap();
        invalid.context_bytes = 1;
        assert_eq!(
            support::save_settings(&client, invalid)
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let mut first = entry(None, "Database convention");
        first.body = "数据库连接使用连接池，并限制最大连接数".into();
        let first = put(&client, first, 0).await.unwrap();
        let mut second = entry(None, "Database pool limit");
        second.body = "数据库连接使用连接池，并限制最大连接数为十个".into();
        second.summary.kind = Kind::Reference;
        let second = put(&client, second, 0).await.unwrap();
        let mut irrelevant = entry(None, "Output style");
        irrelevant.body = "用中文回答，保持简洁".into();
        let irrelevant = put(&client, irrelevant, 0).await.unwrap();
        let found = search(&client, None, "如何配置数据库的连接池").await;
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].summary.id, first.summary.id);
        assert!(
            found
                .iter()
                .any(|item| item.summary.id == second.summary.id)
        );
        let browsed: Vec<Summary> = run(&client,json!({"action":"browse","filter":{"project":null,"all_projects":true,"archived":false,"query":"数据库连接池"}})).await.unwrap();
        assert!(browsed.iter().any(|item| item.id == first.summary.id));
        let mut duplicate = entry(None, "Duplicated convention");
        duplicate.body = first.body.clone();
        duplicate.summary.kind = Kind::Reference;
        assert_eq!(
            put(&client, duplicate, 0).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        let mut invalid = first.clone();
        invalid.body = irrelevant.body;
        let source = json!({"id":second.summary.id,"revision":1});
        assert_eq!(
            run::<Entry>(
                &client,
                json!({"action":"merge","entry":invalid,"sources":[source]})
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            support::read(&client, second.summary.id).await.unwrap(),
            second
        );
        assert_eq!(
            search(&client, None, "十个").await.as_slice(),
            std::slice::from_ref(&second)
        );
        let mut merged = first;
        merged.body = "数据库连接池最多十个连接".into();
        assert_eq!(run::<Entry>(&client,json!({"action":"merge","entry":merged,"sources":[{"id":second.summary.id,"revision":9}]})).await.unwrap_err().code,ErrorCode::RevisionConflict);
        let merge = support::request(
            &client,
            &support::context(&client).await,
            json!({"action":"merge","entry":merged,"sources":[source]}),
        );
        let result = client.execute(merge.clone()).await.unwrap();
        assert_eq!(client.execute(merge).await.unwrap(), result);
        let merged: Entry = support::output(result).unwrap();
        assert_eq!(merged.body, "数据库连接池最多十个连接");
        let archived = support::read(&client, second.summary.id).await.unwrap();
        assert!(archived.summary.archived);
        assert_eq!(archived.body, second.body);
        assert!(
            search(&client, None, &second.summary.id.to_string())
                .await
                .is_empty()
        );
        assert!(
            support::list(&client)
                .await
                .unwrap()
                .iter()
                .any(|item| item.id == second.summary.id && item.archived)
        );
        assert!(support::settings(&client).await.unwrap().auto_write);
        let mut restored = archived;
        restored.summary.archived = false;
        let restored = put(&client, restored, 2).await.unwrap();
        assert!(!restored.summary.archived);
        // Age is a fixture mutation through the generic index, never a Node-owned Memory format.
        let old: Entry = run(
            &client,
            json!({"action":"age","id":restored.summary.id,"updated_at_ms":1}),
        )
        .await
        .unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            search(&client, None, "为十个")
                .await
                .iter()
                .filter(|item| item.summary.id == old.summary.id)
                .count(),
            1
        );
        assert_eq!(client.execute(request).await.unwrap(), saved);
        assert_eq!(
            support::settings(&client).await.unwrap(),
            support::output::<support::Settings>(saved).unwrap()
        );
        assert_eq!(support::read(&client, old.summary.id).await.unwrap(), old);
        assert!(
            !search(&client, None, &old.summary.id.to_string())
                .await
                .is_empty()
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn separates_archive_capacity() {
    for remote in [false, true] {
        let (_directory, node, controller, client) = fixture(remote).await;
        // Production mutations build the complete private catalog, in bounded fixture callbacks.
        let mut values = Vec::new();
        for index in 0..512 + 2048 - 1 {
            let mut value = entry(None, "Capacity fixture");
            value.body = format!("Distinct constraint {index}");
            value.summary.archived = index >= 512;
            values.push(value);
        }
        let mut seeded = Vec::new();
        for batch in values.chunks(32) {
            let result: Vec<Entry> = run(&client, json!({"action":"seed","entries":batch}))
                .await
                .unwrap();
            if seeded.is_empty() {
                seeded.extend(result.into_iter().take(2));
            }
        }
        assert_eq!(support::list(&client).await.unwrap().len(), 2559);
        let fresh = entry(None, "New constraint");
        assert_eq!(
            put(&client, fresh.clone(), 0).await.unwrap_err().code,
            ErrorCode::Busy
        );
        let mut first = seeded[0].clone();
        first.body = "Updated while full".into();
        let mut first = put(&client, first, 1).await.unwrap();
        first.summary.archived = true;
        let mut first = put(&client, first, 2).await.unwrap();
        let fresh = put(&client, fresh, 0).await.unwrap();
        assert_eq!(support::list(&client).await.unwrap().len(), 2560);
        first.summary.archived = false;
        assert_eq!(
            put(&client, first.clone(), 3).await.unwrap_err().code,
            ErrorCode::Busy
        );
        let mut second = seeded[1].clone();
        second.summary.archived = true;
        assert_eq!(
            put(&client, second, 1).await.unwrap_err().code,
            ErrorCode::Busy
        );
        run::<Vec<MemoryId>>(
            &client,
            json!({"action":"remove","id":fresh.summary.id,"expected_revision":1}),
        )
        .await
        .unwrap();
        let restored = put(&client, first, 3).await.unwrap();
        assert!(!restored.summary.archived);
        assert_eq!(restored.body, "Updated while full");
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
