use super::*;

async fn remove(client: &Client, id: SessionId) {
    let Output::Session(session) = client
        .execute(client.prepare(Command::ReadSession { session: id }))
        .await
        .unwrap()
    else {
        panic!("session expected");
    };
    client
        .execute(client.prepare(Command::RemoveSession {
            session: id,
            expected_revision: session.revision,
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn preserves_form_until_last_reference() {
    for remote in [false, true] {
        let mut form = draft(None, None, "Storage choices");
        form["html"] = json!(
            "<section><h1>Storage choices</h1><form data-name='choices'><fieldset data-name='storage'><legend>Storage</legend><label><input type='radio' name='storage' value='node'>Execution Node</label></fieldset><button type='submit'>Send</button></form></section>"
        );
        let server = Server::scripted_tools(vec![(plugin_tool(PACKAGE, "write"), form)]).await;
        let fixture = process::Fixture::new(remote, &server).await;
        install(&fixture, 0, "<html><body>Form template</body></html>").await;
        let publication = submit(&fixture, "Create the requested form").await;
        let page = finished(&fixture.client, fixture.session.id, publication).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let card = cards(&page).remove(0);
        let original = fixture.client.read_artifact(card.id, None).await.unwrap();
        assert!(original.html.contains("<form data-name=\"choices\""));
        assert!(original.html.contains("name=\"storage\""));
        let resources = original.view.as_ref().unwrap();
        assert_eq!(resources.styles, ["section { padding: 1rem }"]);
        assert_eq!(
            resources.scripts,
            ["document.documentElement.dataset.fixture='ready';"]
        );

        server.replace_tools(Vec::new());
        let following = submit(&fixture, "Continue after the form").await;
        finished(&fixture.client, fixture.session.id, following).await;
        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: following,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected");
        };
        let inherited = fixture
            .client
            .read_conversation(branch.id, None, 100)
            .await
            .unwrap();
        assert_eq!(cards(&inherited.page), [card.clone()]);
        assert_eq!(
            fixture
                .client
                .read_artifact_in(branch.id, card.id, Some(1))
                .await
                .unwrap(),
            original
        );
        let directory = fixture
            .node
            .profile()
            .join("artifacts")
            .join(fixture.session.id.to_string());
        assert!(directory.join(card.id.to_string()).join("1.html").is_file());
        remove(&fixture.client, fixture.session.id).await;
        assert!(directory.is_dir());
        let retained = fixture
            .client
            .read_artifact_in(branch.id, card.id, Some(1))
            .await
            .unwrap();
        assert_eq!(retained, original);
        assert_eq!(retained.view.as_ref().unwrap(), resources);
        remove(&fixture.client, branch.id).await;
        assert!(!directory.exists());
        assert_eq!(
            fixture
                .client
                .read_artifact_in(branch.id, card.id, Some(1))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
