use super::Format;
use crate::activity::fixture::Fixture;
use sailry_client::Client;
use sailry_link::{CancellationToken, Transport};
use sailry_protocol::{
    Command, Output,
    attachment::{Attachment, Spec},
    conversation::{Input, Status},
};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const BODY: &[u8] = b"Published attachment bytes\n";

#[test]
fn prepares_html_assets_on_both_nodes() {
    let fixture = Fixture::new();
    for index in 0..2 {
        let attachment = seed(&fixture, index);
        let output = tempfile::tempdir().unwrap();
        let prepared = fixture
            .runtime
            .block_on(super::prepare(
                client(&fixture, index),
                fixture.sessions[index].clone().into(),
                "Captured export".into(),
                Format::Html,
                output.path().join("reply.html"),
                CancellationToken::new(),
            ))
            .unwrap();
        assert!(
            prepared
                .html
                .contains(&format!("Captured export from Node {index}"))
        );
        assert!(
            !prepared
                .html
                .contains(&format!("Captured export from Node {}", 1 - index))
        );
        assert!(prepared.html.contains("<section class=\"message user\">"));
        assert!(
            prepared
                .html
                .contains("<section class=\"message assistant\">")
        );
        assert_eq!(
            std::fs::read(prepared.staging.path().join(
                sailry_client::conversation::export::attachment_name(&attachment)
            ))
            .unwrap(),
            BODY
        );
        for asset in ["mathjax.js", "mermaid.js", "echarts.js", "static.js"] {
            assert!(prepared.staging.path().join(asset).is_file());
        }
        // Publication receives the native renderer's standalone document, not its assets.
        let document = b"<!doctype html><html><body><svg></svg></body></html>".to_vec();
        fixture
            .runtime
            .block_on(super::publish(
                prepared,
                document.clone(),
                CancellationToken::new(),
            ))
            .unwrap();
        assert_eq!(entries(output.path()), [output.path().join("reply.html")]);
        assert_eq!(
            std::fs::read(output.path().join("reply.html")).unwrap(),
            document
        );
    }
    fixture.close();
}

fn client(fixture: &Fixture, index: usize) -> Arc<Client> {
    Arc::new(Client::new(transport(fixture, index)))
}

fn transport(fixture: &Fixture, index: usize) -> Arc<dyn Transport> {
    if index == 0 {
        fixture.nodes[0].local()
    } else {
        fixture.nodes[0]
            .link()
            .remote(fixture.nodes[1].link().address())
    }
}

fn seed(fixture: &Fixture, index: usize) -> Attachment {
    let client = client(fixture, index);
    let session = &fixture.sessions[index];
    let attachment = fixture.runtime.block_on(async {
        let spec = Spec {
            worktree: session.worktree,
            name: "report.txt".into(),
            media_type: "text/plain".into(),
            size: BODY.len() as u64,
            revision: blake3::hash(BODY).to_hex().to_string(),
        };
        let Output::AttachmentUpload(upload) = client
            .execute(client.prepare(Command::UploadAttachment(spec)))
            .await
            .unwrap()
        else {
            panic!("attachment upload expected");
        };
        let mut bytes = BODY;
        client
            .upload(&upload, &mut bytes, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        let Output::Attachment(attachment) = client
            .execute(client.prepare(Command::FinishAttachmentUpload {
                worktree: session.worktree,
                stream: upload.stream,
            }))
            .await
            .unwrap()
        else {
            panic!("published attachment expected");
        };
        client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: Input {
                    text: format!("Captured export from Node {index}"),
                    attachments: vec![attachment.id],
                    references: vec![],
                },
            }))
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let history = client
                .read_conversation(session.id, None, 20)
                .await
                .unwrap();
            if history.page.questions.iter().any(|question| {
                question.state == sailry_protocol::conversation::question::State::Pending
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "export fixture question deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        attachment
    });
    fixture.answer(index);
    fixture.runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let history = client
                .read_conversation(session.id, None, 20)
                .await
                .unwrap();
            if history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Completed)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "export fixture completion deadline: {:?}",
                history.page.runs
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    attachment
}

fn entries(path: &Path) -> Vec<std::path::PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    entries
}
