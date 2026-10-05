#![cfg(feature = "test-support")]

use sailry_client::{Client, Projection};
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{ModelApi, catalog},
    *,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[allow(dead_code)]
#[path = "discovery_support/mod.rs"]
mod discovery_support;
use discovery_support::{Reply, Server};
#[path = "catalog/completion.rs"]
mod completion;
#[path = "catalog/lifecycle.rs"]
mod lifecycle;
#[path = "catalog/lookup.rs"]
mod lookup;
#[path = "catalog/responses.rs"]
mod responses;

struct Fixture {
    _directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Arc<Client>,
}

impl Fixture {
    async fn new(remote: bool, server: &Server) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_with_catalog(
            directory.path().join("node"),
            &format!("{}/api.json", server.endpoint),
        )
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
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        Self {
            _directory: directory,
            node,
            controller,
            client,
        }
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

fn data() -> Value {
    json!({
        "fixture": {"models": {
            "a": {"id":"known", "name":"Known model\t", "limit":{"context":4096,"output":512},
                "modalities":{"input":["text","image","pdf"], "output":["text"]},
                "tool_call":true, "reasoning":true,
                "reasoning_options":[{"type":"toggle"}, {"type":"effort","values":[null,"low","high"]},
                    {"type":"budget_tokens","min":1024,"max":4096}],
                "cost":{"input":1.5,"output":6.0},
                "experimental":{"provider":{"body":{"must_not_execute":true}}}},
            "b": {"id":"unknown", "limit":{"context":0,"output":null}}
        }},
        "other": {"models":{"known":{"id":"known","limit":{"context":8192,"output":2048}}}}
    })
}

async fn execute(client: &Client, command: Command) -> Output {
    tokio::time::timeout(
        Duration::from_secs(5),
        client.execute(client.prepare(command)),
    )
    .await
    .unwrap()
    .unwrap()
}

async fn status(client: &Client) -> catalog::Status {
    let Output::CatalogStatus(status) = execute(client, Command::ReadCatalogStatus).await else {
        panic!("catalog status expected")
    };
    status
}

fn query() -> catalog::Query {
    catalog::Query {
        provider: Some("fixture".into()),
        ids: vec![],
        revision: None,
        after: None,
        limit: 1,
    }
}

async fn page(client: &Client, query: catalog::Query) -> catalog::Page {
    let Output::ModelCatalog(page) = execute(client, Command::ReadModelCatalog(query)).await else {
        panic!("catalog page expected")
    };
    page
}

async fn refresh(client: &Client) -> catalog::Status {
    let request = client.prepare(Command::RefreshModelCatalog);
    assert!(!request.command.durable());
    let admission = client.dispatch(request).await.unwrap();
    assert!(!admission.receipt.durable);
    let Output::CatalogStatus(status) = admission.completion.await.unwrap().unwrap() else {
        panic!("catalog status expected")
    };
    status
}

#[tokio::test]
async fn restores_cached_metadata() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
        let fixture = Fixture::new(remote, &server).await;
        assert_eq!(status(&fixture.client).await, catalog::Status::default());
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadModelCatalog(query())))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert!(server.requests.lock().unwrap().is_empty());
        let mut subscription = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.client.target(), 1);
        fixture.client.recover(&mut projection, 1).await.unwrap();
        let updated = refresh(&fixture.client).await;
        assert_eq!(updated.revision, 1);
        assert_eq!((updated.providers, updated.models), (2, 3));
        assert_eq!(updated.bytes, data().to_string().len() as u64);
        assert!(updated.updated_at_ms.is_some());
        tokio::time::timeout(Duration::from_secs(5), async {
            while projection.snapshot().unwrap().model_catalog != updated {
                projection
                    .apply(1, subscription.next().await.unwrap())
                    .unwrap();
            }
        })
        .await
        .unwrap();
        assert!(projection.snapshot().unwrap().providers.is_empty());
        let first = page(&fixture.client, query()).await;
        assert_eq!(first.revision, updated.revision);
        assert_eq!(first.next.as_deref(), Some("known"));
        let known = &first.models[0];
        assert_eq!(known.name, "Known model");
        assert_eq!((known.context, known.output), (Some(4096), Some(512)));
        assert_eq!(known.inputs, ["text", "image", "pdf"]);
        assert_eq!(known.outputs, ["text"]);
        assert_eq!((known.tools, known.reasoning), (Some(true), Some(true)));
        assert_eq!(known.options.len(), 3);
        assert!(
            matches!(&known.options[1], catalog::Reasoning::Effort { values } if values == &vec![None, Some("low".into()), Some("high".into())])
        );
        let mut next = query();
        next.revision = Some(first.revision);
        next.after = first.next.clone();
        let second = page(&fixture.client, next.clone()).await;
        assert!(second.next.is_none());
        assert_eq!(second.models[0].id, "unknown");
        assert_eq!(
            (second.models[0].context, second.models[0].output),
            (None, None)
        );
        assert_eq!(
            (second.models[0].tools, second.models[0].reasoning),
            (None, None)
        );
        let mut other = query();
        other.provider = Some("other".into());
        assert_eq!(
            page(&fixture.client, other).await.models[0].context,
            Some(8192)
        );
        let db = rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
            .unwrap();
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM requests", [], |row| row.get(0))
                .unwrap(),
            0
        );
        let body: Vec<u8> = db
            .query_row(
                "SELECT body FROM model_catalog WHERE provider='fixture' AND id='known'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["cost"]["input"],
            1.5
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let new_status = refresh(&fixture.client).await;
        assert_eq!(new_status.revision, 2);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadModelCatalog(next)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        drop(subscription);
        let path = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let reopened = Node::start(&path).await.unwrap();
        let client = Client::new(reopened.local());
        assert_eq!(status(&client).await, new_status);
        assert_eq!(page(&client, query()).await.models, first.models);
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.model_catalog, new_status);
        assert!(snapshot.providers.is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        for request in server.requests.lock().unwrap().iter() {
            assert_eq!(request.path, "/api.json");
            assert!(!request.headers.contains_key("authorization"));
            assert!(!request.headers.contains_key("x-api-key"));
        }
        fixture.controller.close().await.unwrap();
        reopened.shutdown().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires the public models.dev reference catalog"]
async fn refreshes_public_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let status = refresh(&client).await;
    assert!(status.providers > 0 && status.models > 0 && status.bytes > 0);
    for provider in ["openai", "anthropic", "google"] {
        let mut query = query();
        query.provider = Some(provider.into());
        assert!(!page(&client, query).await.models.is_empty());
    }
    println!(
        "Reference catalog: {} providers, {} models, {} bytes",
        status.providers, status.models, status.bytes
    );
    node.shutdown().await.unwrap();
}
