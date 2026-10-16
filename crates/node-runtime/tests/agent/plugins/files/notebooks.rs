use super::*;

#[tokio::test]
async fn preserves_outputs_and_restores_cell_edits() {
    for remote in [false, true] {
        let notebook = json!({"nbformat":4,"nbformat_minor":5,"metadata":{"custom":true},"cells":[{"cell_type":"code","id":"existing","metadata":{"tags":["kept"]},"source":"print(1)","outputs":[{"output_type":"stream","name":"stdout","text":"1\n"}],"execution_count":1}]});
        let original = notebook.to_string();
        let server = Server::tools(vec![
            (name("read_notebook"),json!({"path":"sample.ipynb"})),
            (name("edit_notebook"),json!({"path":"sample.ipynb","expected_revision":blake3::hash(original.as_bytes()).to_hex().to_string(),"edits":[{"kind":"replace","index":0,"source":"print(2)"},{"kind":"insert","index":1,"cell_type":"markdown","source":"# Added"}]}))
        ]).await;
        let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("sample.ipynb"), &original).unwrap();
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].source, ApprovalSource::Project);
        let output = results(&page);
        assert_eq!(output[0]["kind"], "notebook_page");
        assert_eq!(output[0]["data"]["cells"][0]["value"], notebook["cells"][0]);
        assert_eq!(output[1]["kind"], "file_written");
        let after = std::fs::read_to_string(fixture.root.join("sample.ipynb")).unwrap();
        let updated: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(
            updated["cells"][0]["outputs"],
            notebook["cells"][0]["outputs"]
        );
        assert_eq!(updated["cells"][0]["id"], "existing");
        let files = checkpoints(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(files.len(), 1);
        let Output::FileCheckpoint(content) = execute(
            &fixture.client,
            Command::ReadFileCheckpoint {
                session: fixture.session.id,
                checkpoint: files[0].id,
            },
        )
        .await
        else {
            panic!("checkpoint expected")
        };
        assert_eq!(content.before.as_deref(), Some(original.as_str()));
        assert_eq!(
            content.after, after,
            "inserted IDs must agree with the approved checkpoint"
        );
        execute(
            &fixture.client,
            Command::RestoreFileCheckpoint {
                session: fixture.session.id,
                checkpoint: files[0].id,
                worktree: fixture.session.worktree,
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("sample.ipynb")).unwrap(),
            original
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
