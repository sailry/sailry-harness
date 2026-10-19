use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_legacy_grants_during_rehearsal() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |request| {
            match request.path.as_str() {
                "/token" => tokens(true),
                "/models" => Reply::Json(catalog::models(Authentication::Plugin)),
                _ => panic!("unexpected fixture route"),
            }
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, true).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut updates).await.state, login::State::Connected);
        drop(updates);
        let provider = saved(&fixture.client, provider.id).await;
        let Fixture {
            directory,
            node,
            controller,
            client,
        } = fixture;
        let profile = node.profile().to_path_buf();
        drop(client);
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        let bytes: Vec<u8> = db
            .query_row(
                "SELECT body FROM providers WHERE id=?1",
                [provider.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let mut stored: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let credential = provider.credential.as_ref().unwrap().id.to_string();
        let mut grant: serde_json::Value = serde_json::from_str(
            stored["authorizations"][&credential]["key"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        grant.as_object_mut().unwrap().remove("renewal_rejected");
        grant["record"]["target"]
            .as_object_mut()
            .unwrap()
            .remove("digest");
        let original = grant.clone();
        grant["record"]["issued"]["access"] = json!("");
        stored["authorizations"][&credential]["key"] = json!(grant.to_string());
        let invalid = serde_json::to_vec(&stored).unwrap();
        db.execute(
            "UPDATE providers SET body=?1 WHERE id=?2",
            rusqlite::params![invalid, provider.id.to_string()],
        )
        .unwrap();
        db.execute("DROP INDEX turns_session", []).unwrap();
        drop(db);
        assert!(
            Node::start_empty_with_authorization(&profile, &server.endpoint)
                .await
                .is_err()
        );
        let db = rusqlite::Connection::open(&path).unwrap();
        let unchanged: Vec<u8> = db
            .query_row(
                "SELECT body FROM providers WHERE id=?1",
                [provider.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, invalid);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='turns_session'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        stored["authorizations"][&credential]["key"] = json!(original.to_string());
        let legacy = serde_json::to_vec(&stored).unwrap();
        db.execute(
            "UPDATE providers SET body=?1 WHERE id=?2",
            rusqlite::params![legacy, provider.id.to_string()],
        )
        .unwrap();
        drop(db);
        let node = Node::start_empty_with_authorization(&profile, &server.endpoint)
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let current = saved(&client, provider.id).await;
        assert_eq!(current.credential, provider.credential);
        let requests = server.requests.lock().unwrap().len();
        let Output::DiscoveredModels(catalog) = client
            .execute(client.prepare(models(&current)))
            .await
            .unwrap()
        else {
            panic!("model catalog expected")
        };
        private(&catalog);
        assert_eq!(catalog.endpoint, server.endpoint);
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["fixture", "unknown"]
        );
        assert_eq!(catalog.models[0].context, Some(16384));
        assert_eq!(catalog.models[0].output, Some(1024));
        {
            let recorded = server.requests.lock().unwrap();
            assert_eq!(recorded.len(), requests + 1);
            assert_eq!(recorded.last().unwrap().path, "/models");
            assert_eq!(
                recorded
                    .iter()
                    .filter(|request| request.path == "/token")
                    .count(),
                1
            );
        }
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let retained: Vec<u8> = db
            .query_row(
                "SELECT body FROM providers WHERE id=?1",
                [provider.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, legacy);
        let backup = std::fs::read_dir(profile.join("storage/backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let before = rusqlite::Connection::open_with_flags(
            backup,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            before
                .query_row(
                    "SELECT body FROM providers WHERE id=?1",
                    [provider.id.to_string()],
                    |row| row.get::<_, Vec<u8>>(0)
                )
                .unwrap(),
            legacy
        );
        drop(before);
        drop(db);
        drop(directory);
    }
}
