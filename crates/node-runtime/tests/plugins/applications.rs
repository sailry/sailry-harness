use super::*;
use plugin::{Context, connector::Kind, http};
#[path = "../support/connectors.rs"]
mod connectors;
use std::{collections::BTreeMap, sync::atomic::Ordering};
#[path = "../support/application_tokens.rs"]
mod issuer;

#[tokio::test]
async fn refreshes_application_http() {
    for remote in [false, true] {
        let server = issuer::Server::start(None).await;
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        node.set_connector_fixture(connectors::ORIGIN, &server.origin);
        connectors::configure(
            &directory.path().join("source/package"),
            connectors::application(false),
        );
        let package = info(execute(&client, install(worktree, 0)).await);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let row = connectors::connected(
            &client,
            connectors::start(
                &client,
                package.summary.reference(),
                Kind::Application,
                false,
            ),
        )
        .await;
        let context = Context {
            package: package.summary.reference(),
            invocation: None,
            turn: None,
            surface: Default::default(),
            worktree: Some(worktree),
            session: None,
        };
        let request = |url: &str| {
            client
                .prepare(Command::RequestPluginHttp(http::Request {
                    connector: Some("service".into()),
                    connection: Some(row.id),
                    idempotent: false,
                    url: url.into(),
                    method: "POST".into(),
                    headers: BTreeMap::new(),
                    body: Some("{}".into()),
                    credential: None,
                    timeout_ms: 2000,
                }))
                .with_plugin(context.clone())
        };
        let first = request("https://service.example.com/probe");
        let output = client.execute(first.clone()).await.unwrap();
        assert!(matches!(&output, Output::PluginHttp(response) if response.status == 200));
        assert_eq!(client.execute(first).await.unwrap(), output);
        assert_eq!(server.tokens.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let (left, right) = tokio::join!(
            client.execute(request("https://service.example.com/probe")),
            client.execute(request("https://service.example.com/probe"))
        );
        assert!(left.is_ok(), "{left:?}");
        assert!(right.is_ok(), "{right:?}");
        assert_eq!(server.tokens.load(Ordering::SeqCst), 2);
        let records = server.records.lock().unwrap().clone();
        let resource = records
            .iter()
            .filter(|(path, _, _)| path == "/probe")
            .collect::<Vec<_>>();
        assert_eq!(resource.len(), 3);
        assert_eq!(resource[0].1["x-access-token"], "application-token-1");
        for record in &resource[1..] {
            assert_eq!(record.1["x-access-token"], "application-token-2");
        }
        assert_eq!(
            client
                .execute(request("https://example.com/probe"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let stored = || -> Vec<u8> {
            rusqlite::Connection::open_with_flags(
                node.profile().join("storage/node.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap()
            .query_row(
                "SELECT body FROM plugin_connections WHERE id=?1",
                [row.id.to_string()],
                |row| row.get(0),
            )
            .unwrap()
        };
        let before = stored();
        server.status.store(503, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let failed = request("https://service.example.com/probe");
        let error = client.execute(failed.clone()).await.unwrap_err();
        assert!(!error.message.contains(issuer::SECRET));
        assert_eq!(client.execute(failed).await.unwrap_err(), error);
        assert_eq!(server.tokens.load(Ordering::SeqCst), 3);
        assert_eq!(stored(), before);
        server.status.store(200, Ordering::SeqCst);
        assert!(
            client
                .execute(request("https://service.example.com/probe"))
                .await
                .is_ok()
        );
        assert_eq!(server.tokens.load(Ordering::SeqCst), 4);

        let before = server
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _, _)| path == "/unauthorized")
            .count();
        let original = request("https://service.example.com/unauthorized");
        let rejected = client.execute(original.clone()).await.unwrap();
        assert!(matches!(&rejected, Output::PluginHttp(response) if response.status == 401));
        assert_eq!(client.execute(original).await.unwrap(), rejected);
        assert_eq!(
            server
                .records
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _, _)| path == "/unauthorized")
                .count()
                - before,
            1
        );
        assert_eq!(server.tokens.load(Ordering::SeqCst), 5);
        assert!(
            client
                .execute(request("https://service.example.com/probe"))
                .await
                .is_ok()
        );

        server.status.store(401, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(2100)).await;
        assert_eq!(
            client
                .execute(request("https://service.example.com/probe"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
            panic!("connections expected")
        };
        assert_eq!(rows[0].state, plugin::connector::State::Expired);
        assert!(rows[0].configured.contains(&"app_secret".into()));
        server.status.store(200, Ordering::SeqCst);
        let tokens = server.tokens.load(Ordering::SeqCst);
        let Output::Connection(recovered) =
            execute(&client, Command::ProbeConnection { connection: row.id }).await
        else {
            panic!("connection expected")
        };
        assert_eq!(recovered.state, plugin::connector::State::Connected);
        assert_eq!(server.tokens.load(Ordering::SeqCst), tokens + 1);
        assert!(
            client
                .execute(request("https://service.example.com/probe"))
                .await
                .is_ok()
        );
        let snapshot = execute(&client, Command::Snapshot).await;
        let public = serde_json::to_string(&(output, error, snapshot)).unwrap();
        assert!(!public.contains(issuer::SECRET));
        assert!(!public.contains("application-token-"));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
