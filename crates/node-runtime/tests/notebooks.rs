use super::file_fixture::Fixture;
use sailry_protocol::*;
use serde_json::{Value, json};

async fn page(
    client: &sailry_client::Client,
    worktree: WorktreeId,
    offset: usize,
) -> notebook::Page {
    let Output::NotebookPage(page) = client
        .execute(client.prepare(Command::ReadNotebook {
            worktree,
            options: notebook::Read {
                path: "sample.ipynb".into(),
                offset,
                limit: 1,
            },
        }))
        .await
        .unwrap()
    else {
        panic!("notebook page expected")
    };
    page
}

#[tokio::test]
async fn shares_revision_checked_cell_edits() {
    let fixture = Fixture::start().await;
    let original = json!({"nbformat":4,"nbformat_minor":5,"metadata":{"custom":[1,2]},"cells":[
        {"cell_type":"markdown","id":"intro","metadata":{"tags":["a"]},"source":["# Notebook\n"]},
        {"cell_type":"code","id":"code","metadata":{"custom":true},"source":"print(1)","execution_count":12,"outputs":[{"output_type":"stream","name":"stdout","text":"1\n"}]}
    ]});
    std::fs::write(fixture.root.join("sample.ipynb"), original.to_string()).unwrap();
    let [local, remote] = fixture.clients();
    for client in [&local, &remote] {
        let first = page(client, fixture.worktree, 0).await;
        assert_eq!(first.cells[0].index, 0);
        assert_eq!(first.cells[0].value, original["cells"][0]);
        assert_eq!(first.next_offset, Some(1));
        let second = page(client, fixture.worktree, first.next_offset.unwrap()).await;
        assert_eq!(second.cells[0].value, original["cells"][1]);
        assert_eq!(second.next_offset, None);
        assert_eq!(first.revision, second.revision);
    }
    let first = page(&local, fixture.worktree, 0).await;
    let request = remote.prepare(Command::EditNotebook {
        worktree: fixture.worktree,
        edit: notebook::Edit {
            path: "sample.ipynb".into(),
            expected_revision: first.revision.clone(),
            edits: vec![
                notebook::Change::Replace {
                    index: 1,
                    source: "print(2)\n".into(),
                },
                notebook::Change::Insert {
                    index: 2,
                    cell_type: notebook::Kind::Raw,
                    source: "New cell".into(),
                },
            ],
        },
    });
    let result = remote.execute(request.clone()).await.unwrap();
    assert!(matches!(result, Output::FileWritten(_)));
    assert_eq!(remote.execute(request.clone()).await.unwrap(), result);
    assert_eq!(
        remote.outcome(&request).await.unwrap(),
        RequestOutcome::Completed(Box::new(Ok(result.clone())))
    );
    let after: Value =
        serde_json::from_slice(&std::fs::read(fixture.root.join("sample.ipynb")).unwrap()).unwrap();
    assert_eq!(after["metadata"], original["metadata"]);
    assert_eq!(after["cells"][0], original["cells"][0]);
    let mut expected = original["cells"][1].clone();
    expected["source"] = json!("print(2)\n");
    assert_eq!(after["cells"][1], expected);
    let stale = local.prepare(Command::EditNotebook {
        worktree: fixture.worktree,
        edit: notebook::Edit {
            path: "sample.ipynb".into(),
            expected_revision: first.revision,
            edits: vec![notebook::Change::Delete { index: 0 }],
        },
    });
    assert_eq!(
        local.execute(stale).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let current = page(&remote, fixture.worktree, 0).await;
    let invalid = local.prepare(Command::EditNotebook {
        worktree: fixture.worktree,
        edit: notebook::Edit {
            path: "../sample.ipynb".into(),
            expected_revision: current.revision.clone(),
            edits: vec![notebook::Change::Delete { index: 0 }],
        },
    });
    assert_eq!(
        local.execute(invalid).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let deleted = local.prepare(Command::EditNotebook {
        worktree: fixture.worktree,
        edit: notebook::Edit {
            path: "sample.ipynb".into(),
            expected_revision: current.revision,
            edits: vec![notebook::Change::Delete { index: 2 }],
        },
    });
    local.execute(deleted.clone()).await.unwrap();
    assert_eq!(page(&remote, fixture.worktree, 0).await.total, 2);
    let stable = std::fs::read(fixture.root.join("sample.ipynb")).unwrap();
    local.execute(deleted).await.unwrap();
    assert_eq!(
        std::fs::read(fixture.root.join("sample.ipynb")).unwrap(),
        stable
    );
    fixture.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_links_and_preserves_failed_batches() {
    let fixture = Fixture::start().await;
    let original = json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"raw","id":"kept","metadata":{},"source":"Kept"}]}).to_string();
    std::fs::write(fixture.root.join("sample.ipynb"), &original).unwrap();
    std::os::unix::fs::symlink(
        fixture.root.join("sample.ipynb"),
        fixture.root.join("linked.ipynb"),
    )
    .unwrap();
    for client in fixture.clients() {
        let read = Command::ReadNotebook {
            worktree: fixture.worktree,
            options: notebook::Read {
                path: "linked.ipynb".into(),
                offset: 0,
                limit: 1,
            },
        };
        assert!(client.execute(client.prepare(read)).await.is_err());
        let edit = Command::EditNotebook {
            worktree: fixture.worktree,
            edit: notebook::Edit {
                path: "sample.ipynb".into(),
                expected_revision: blake3::hash(original.as_bytes()).to_hex().to_string(),
                edits: vec![
                    notebook::Change::Replace {
                        index: 0,
                        source: "Changed".into(),
                    },
                    notebook::Change::Delete { index: 20 },
                ],
            },
        };
        assert_eq!(
            client.execute(client.prepare(edit)).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("sample.ipynb")).unwrap(),
            original
        );
    }
    fixture.close().await;
}
