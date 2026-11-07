use super::*;
use crate::file_fixture as files;
use sailry_protocol::ssh::{Direction, Transfer};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn copies_confirmed_files() {
    let _ = server::sftp_executable();
    let fixture = files::Fixture::empty().await;
    let remote_files = fixture._temp.path().join("ssh");
    std::fs::create_dir(&remote_files).unwrap();
    let server = server::Server::start(remote_files.clone(), 44).await;
    let clients = fixture.clients();
    let client = &clients[1];
    let profile = save(
        client,
        profile(server.port),
        Some(Credential::Password {
            password: Secret::new("isolated-ssh-password".into()),
        }),
    )
    .await;
    let profile = trust(client, &profile, &server.key).await;
    let command = |direction, path: &str, remote_path: &str| Command::TransferSsh {
        profile: profile.id,
        expected_revision: profile.revision,
        transfer: Transfer {
            worktree: fixture.worktree,
            path: path.into(),
            remote_path: remote_path.into(),
            direction,
        },
        timeout_ms: 5000,
    };
    let data: Vec<_> = (0..2 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    std::fs::write(fixture.root.join("source.bin"), &data).unwrap();
    let remote_name = "文件 ' $(touch unexpected).bin";
    let request = client.prepare(command(Direction::Upload, "source.bin", remote_name));
    let expected = Output::SshOutcome(Outcome::Transferred {
        bytes: data.len() as u64,
    });
    assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
    assert_eq!(std::fs::read(remote_files.join(remote_name)).unwrap(), data);
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(remote_files.join(remote_name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!remote_files.join("unexpected").exists());
    assert!(server.commands.lock().unwrap().is_empty());
    std::fs::write(fixture.root.join("source.bin"), "changed").unwrap();
    assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
    assert_eq!(server.subsystems.load(Ordering::SeqCst), 1);
    assert_eq!(server.authentication.load(Ordering::SeqCst), 1);
    let download = client.prepare(command(Direction::Download, "download.bin", remote_name));
    assert_eq!(client.execute(download.clone()).await.unwrap(), expected);
    assert_eq!(
        std::fs::read(fixture.root.join("download.bin")).unwrap(),
        data
    );
    assert_eq!(client.execute(download).await.unwrap(), expected);
    assert_eq!(server.subsystems.load(Ordering::SeqCst), 2);
    assert!(server.commands.lock().unwrap().is_empty());
    assert_eq!(
        client
            .execute(client.prepare(command(Direction::Download, "download.bin", remote_name)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    for path in ["../escape", ".git/config"] {
        assert!(
            client
                .execute(client.prepare(command(Direction::Download, path, remote_name)))
                .await
                .is_err()
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&remote_files, fixture.root.join("outside")).unwrap();
        assert!(
            client
                .execute(client.prepare(command(
                    Direction::Download,
                    "outside/escape",
                    remote_name
                )))
                .await
                .is_err()
        );
        assert!(!remote_files.join("escape").exists());
    }
    let missing = client.prepare(command(Direction::Download, "missing.bin", "absent"));
    let error = client.execute(missing.clone()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert!(
        error
            .message
            .contains("remote open: SFTP status=NoSuchFile (2)")
    );
    assert_eq!(client.execute(missing).await.unwrap_err(), error);
    assert!(!fixture.root.join("missing.bin").exists());
    let upload = client.prepare(command(
        Direction::Upload,
        "source.bin",
        "missing-parent/destination.bin",
    ));
    let error = client.execute(upload.clone()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert!(
        error
            .message
            .contains("remote open: SFTP status=NoSuchFile (2)")
    );
    assert!(!error.message.contains("remote stat:"));
    let subsystems = server.subsystems.load(Ordering::SeqCst);
    assert_eq!(client.execute(upload).await.unwrap_err(), error);
    assert_eq!(server.subsystems.load(Ordering::SeqCst), subsystems);
    assert!(!remote_files.join("missing-parent").exists());
    std::fs::write(remote_files.join("empty"), []).unwrap();
    assert_eq!(
        execute(client, command(Direction::Download, "empty", "empty")).await,
        Output::SshOutcome(Outcome::Transferred { bytes: 0 })
    );
    assert_eq!(
        std::fs::metadata(fixture.root.join("empty")).unwrap().len(),
        0
    );
    assert_eq!(
        execute(
            client,
            command(Direction::Upload, "empty", "uploaded-empty")
        )
        .await,
        Output::SshOutcome(Outcome::Transferred { bytes: 0 })
    );
    assert_eq!(
        std::fs::read(remote_files.join("uploaded-empty")).unwrap(),
        b""
    );
    std::fs::create_dir(remote_files.join("folder")).unwrap();
    assert_eq!(
        execute(client, command(Direction::Upload, "source.bin", "folder/")).await,
        Output::SshOutcome(Outcome::Transferred { bytes: 7 })
    );
    assert_eq!(
        std::fs::read(remote_files.join("folder/source.bin")).unwrap(),
        b"changed"
    );
    assert!(server.commands.lock().unwrap().is_empty());
    drop(clients);
    let node_path = fixture._temp.path().join("node");
    fixture.node.shutdown().await.unwrap();
    let restored = Node::start_empty(node_path).await.unwrap();
    let client = Client::new(
        fixture
            .controller
            .handle()
            .remote(restored.link().address()),
    );
    assert_eq!(client.execute(request).await.unwrap(), expected);
    assert_eq!(std::fs::read(remote_files.join(remote_name)).unwrap(), data);
    fixture.controller.close().await.unwrap();
    restored.shutdown().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn requires_confirmed_close() {
    let _ = server::sftp_executable();
    let fixture = files::Fixture::empty().await;
    let remote_files = fixture._temp.path().join("ssh");
    std::fs::create_dir(&remote_files).unwrap();
    let server = server::Server::without_close(remote_files.clone(), 45).await;
    let clients = fixture.clients();
    let client = &clients[1];
    let profile = save(
        client,
        profile(server.port),
        Some(Credential::Password {
            password: Secret::new("isolated-ssh-password".into()),
        }),
    )
    .await;
    let profile = trust(client, &profile, &server.key).await;
    let command = |direction, path: &str| Command::TransferSsh {
        profile: profile.id,
        expected_revision: profile.revision,
        transfer: Transfer {
            worktree: fixture.worktree,
            path: path.into(),
            remote_path: "unconfirmed.bin".into(),
            direction,
        },
        timeout_ms: 5000,
    };
    std::fs::write(fixture.root.join("source.bin"), b"unconfirmed").unwrap();
    let upload = client.prepare(command(Direction::Upload, "source.bin"));
    let error = client.execute(upload.clone()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert!(error.message.contains("remote close: I/O kind=Other"));
    assert!(!error.message.contains("SFTP status="));
    server.wait_processes(1).await;
    assert_eq!(
        std::fs::read(remote_files.join("unconfirmed.bin")).unwrap(),
        b"unconfirmed"
    );
    std::fs::write(fixture.root.join("source.bin"), b"changed").unwrap();
    assert_eq!(client.execute(upload).await.unwrap_err(), error);
    assert_eq!(server.subsystems.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(remote_files.join("unconfirmed.bin")).unwrap(),
        b"unconfirmed"
    );
    let download = client.prepare(command(Direction::Download, "download.bin"));
    let error = client.execute(download.clone()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert!(error.message.contains("remote close: I/O kind=Other"));
    assert!(!error.message.contains("SFTP status="));
    assert!(!fixture.root.join("download.bin").exists());
    server.wait_processes(1).await;
    assert_eq!(client.execute(download).await.unwrap_err(), error);
    assert_eq!(server.subsystems.load(Ordering::SeqCst), 2);
    assert!(server.commands.lock().unwrap().is_empty());
    assert!(!fixture.root.join("download.bin").exists());
    drop(clients);
    fixture.close().await;
    server.close().await;
}
