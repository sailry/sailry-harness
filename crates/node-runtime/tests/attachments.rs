use crate::file_fixture as files;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_node_runtime::Node;
use sailry_protocol::{attachment::*, *};
use std::time::Duration;

use files::Fixture;

#[path = "attachments/conversation.rs"]
mod conversation;
#[path = "attachments/guards.rs"]
mod guards;
#[path = "attachments/lifecycle.rs"]
mod lifecycle;
#[path = "attachments/transfers.rs"]
mod transfers;

fn spec(worktree: WorktreeId, name: &str, data: &[u8]) -> Spec {
    Spec {
        worktree,
        name: name.into(),
        media_type: "application/octet-stream".into(),
        size: data.len() as u64,
        revision: blake3::hash(data).to_hex().to_string(),
    }
}

async fn prepare(client: &Client, spec: Spec) -> Upload {
    let admission = client
        .dispatch(client.prepare(Command::UploadAttachment(spec.clone())))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    let Output::AttachmentUpload(upload) = admission.completion.await.unwrap().unwrap() else {
        panic!("attachment upload expected")
    };
    assert_eq!(upload.spec, spec);
    upload
}

async fn stage(client: &Client, upload: &Upload, data: &[u8]) {
    let mut progress = Vec::new();
    client
        .upload_attachment(upload, &mut &data[..], CancellationToken::new(), |size| {
            progress.push(size)
        })
        .await
        .unwrap();
    assert_eq!(progress.first(), Some(&0));
    assert_eq!(progress.last(), Some(&(data.len() as u64)));
    assert!(
        progress
            .windows(2)
            .all(|pair| pair[1] > pair[0] && pair[1] - pair[0] <= FILE_TRANSFER_CHUNK_BYTES as u64)
    );
}

fn finish(client: &Client, upload: &Upload) -> Request {
    client.prepare(Command::FinishAttachmentUpload {
        worktree: upload.spec.worktree,
        stream: upload.stream,
    })
}

async fn publish(client: &Client, upload: &Upload) -> Attachment {
    let admission = client.dispatch(finish(client, upload)).await.unwrap();
    assert!(admission.receipt.durable);
    let Output::Attachment(attachment) = admission.completion.await.unwrap().unwrap() else {
        panic!("attachment expected")
    };
    assert_eq!(attachment.spec, upload.spec);
    attachment
}

async fn download(client: &Client, attachment: &Attachment) -> Vec<u8> {
    let command = Command::DownloadAttachment {
        worktree: attachment.spec.worktree,
        attachment: attachment.id,
    };
    let admission = client.dispatch(client.prepare(command)).await.unwrap();
    assert!(!admission.receipt.durable);
    let Output::AttachmentDownload(download) = admission.completion.await.unwrap().unwrap() else {
        panic!("attachment download expected")
    };
    assert_eq!(&download.attachment, attachment);
    let mut bytes = Vec::new();
    client
        .download_attachment(&download, &mut bytes, CancellationToken::new(), |_| {})
        .await
        .unwrap();
    bytes
}

fn read(attachment: &Attachment) -> Command {
    Command::ReadAttachment {
        worktree: attachment.spec.worktree,
        attachment: attachment.id,
    }
}

fn count(database: &rusqlite::Connection, table: &str) -> i64 {
    database
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}
