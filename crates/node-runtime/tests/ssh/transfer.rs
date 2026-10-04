use super::*;
use sailry_protocol::ssh::{Direction, Transfer};

#[path = "../support/files.rs"]
#[allow(dead_code)]
mod files;

#[tokio::test]
async fn copies_confirmed_files() {
    for remote in [false, true] {
        let fixture = files::Fixture::start().await;
        let remote_files = fixture._temp.path().join("ssh");
        std::fs::create_dir(&remote_files).unwrap();
        let server = server::Server::start(remote_files.clone(), 44).await;
        let clients = fixture.clients();
        let client = &clients[usize::from(remote)];
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
        assert!(!remote_files.join("unexpected").exists());
        std::fs::write(fixture.root.join("source.bin"), "changed").unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
        assert_eq!(server.commands.lock().unwrap().len(), 1);
        let download = client.prepare(command(Direction::Download, "download.bin", remote_name));
        assert_eq!(client.execute(download.clone()).await.unwrap(), expected);
        assert_eq!(
            std::fs::read(fixture.root.join("download.bin")).unwrap(),
            data
        );
        assert_eq!(client.execute(download).await.unwrap(), expected);
        assert_eq!(server.commands.lock().unwrap().len(), 2);
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
        assert_eq!(
            client
                .execute(client.prepare(command(Direction::Download, "missing.bin", "absent")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!fixture.root.join("missing.bin").exists());
        std::fs::write(remote_files.join("empty"), []).unwrap();
        assert_eq!(
            execute(client, command(Direction::Download, "empty", "empty")).await,
            Output::SshOutcome(Outcome::Transferred { bytes: 0 })
        );
        assert_eq!(
            std::fs::metadata(fixture.root.join("empty")).unwrap().len(),
            0
        );
        drop(clients);
        let node_path = fixture._temp.path().join("node");
        fixture.node.shutdown().await.unwrap();
        let restored = Node::start(node_path).await.unwrap();
        let client = Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(restored.link().address())
        } else {
            restored.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), expected);
        assert_eq!(std::fs::read(remote_files.join(remote_name)).unwrap(), data);
        fixture.controller.close().await.unwrap();
        restored.shutdown().await.unwrap();
        server.close().await;
    }
}
