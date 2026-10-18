#![cfg(feature = "test-support")]
use crate::discovery_support::{Reply, Server};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, ErrorCode, NodeId, Output,
    conversation::{
        ModelApi,
        partners::{Catalog, Listing},
    },
};
use serde_json::Value;

#[tokio::test]
async fn reads_public_catalog_without_credentials() {
    for remote in [false, true] {
        let expected: Catalog =
            serde_json::from_slice(include_bytes!("fixtures/model-partners.json")).unwrap();
        let value: Value = serde_json::to_value(&expected).unwrap();
        let server = Server::start(ModelApi::Responses, move |path| {
            if path == "/v1" {
                Reply::Json(value.clone())
            } else {
                Reply::Raw("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into())
            }
        }).await;
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_model_partners_fixture(&server.endpoint).unwrap();
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
        let mut wrong = client.prepare(Command::ReadModelPartners);
        wrong.target = NodeId(*controller.handle().address().id.as_bytes());
        assert_ne!(wrong.target, node.id());
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        assert!(server.requests.lock().unwrap().is_empty());
        let request = client.prepare(Command::ReadModelPartners);
        assert!(!request.command.durable());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert_eq!(admission.receipt.id, request.id);
        assert!(!admission.receipt.durable);
        assert_eq!(
            admission.completion.await.unwrap().unwrap(),
            Output::ModelPartners(Listing {
                catalog: expected,
                unavailable: false
            })
        );
        let cached = client
            .execute(client.prepare(Command::ReadModelPartners))
            .await
            .unwrap();
        assert!(matches!(
            cached,
            Output::ModelPartners(Listing {
                unavailable: false,
                ..
            })
        ));
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].method, "GET");
            assert_eq!(requests[0].path, "/v1");
            assert!(requests[0].body.is_empty());
            assert!(!requests[0].headers.contains_key("authorization"));
            assert!(!requests[0].headers.contains_key("cookie"));
        }
        // A new source cannot reuse a previously validated document.
        node.set_model_partners_fixture(&format!("{}/unavailable", server.endpoint))
            .unwrap();
        let output = client
            .execute(client.prepare(Command::ReadModelPartners))
            .await
            .unwrap();
        let Output::ModelPartners(fallback) = output else {
            panic!("partner catalog expected");
        };
        assert!(fallback.unavailable);
        assert_eq!(fallback.catalog.version, 1);
        let bundled: Catalog = serde_json::from_slice(include_bytes!(
            "../../../assets/catalogs/model-partners.json"
        ))
        .unwrap();
        assert_eq!(fallback.catalog, bundled);
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadModelPartners))
                .await
                .unwrap(),
            Output::ModelPartners(fallback)
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let profile = node.profile().to_path_buf();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let database = rusqlite::Connection::open_with_flags(
            profile.join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            database
                .query_row::<i64, _, _>("SELECT count(*) FROM requests", [], |row| row.get(0))
                .unwrap(),
            0
        );
        drop(server);
    }
}
