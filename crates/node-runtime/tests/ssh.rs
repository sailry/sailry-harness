#![cfg(unix)]
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    ssh::{Authentication, Credential, Outcome, Profile},
    *,
};
use std::{sync::atomic::Ordering, time::Duration};

#[path = "ssh/server.rs"]
mod server;

#[path = "ssh/files.rs"]
mod files;
#[path = "ssh/install.rs"]
mod install;
#[path = "ssh/terminal.rs"]
mod terminal;
#[path = "ssh/transfer.rs"]
mod transfer;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

fn profile(port: u16) -> Profile {
    Profile {
        sharing: None,
        id: SshId::new(),
        revision: 0,
        name: "SSH fixture".into(),
        host: "127.0.0.1".into(),
        port,
        username: "fixture".into(),
        authentication: Authentication::Password,
        host_key: None,
    }
}

async fn save(client: &Client, profile: Profile, credential: Option<Credential>) -> Profile {
    let Output::SshProfile(profile) = execute(
        client,
        Command::SaveSsh {
            expected_revision: profile.revision,
            profile,
            credential,
        },
    )
    .await
    else {
        panic!("SSH profile expected")
    };
    profile
}

async fn trust(client: &Client, profile: &Profile, key: &ssh::HostKey) -> Profile {
    let Output::SshProfile(profile) = execute(
        client,
        Command::TrustSsh {
            profile: profile.id,
            expected_revision: profile.revision,
            key: key.clone(),
        },
    )
    .await
    else {
        panic!("SSH profile expected")
    };
    profile
}

fn run(profile: &Profile, command: &str) -> Command {
    Command::RunSsh {
        profile: profile.id,
        expected_revision: profile.revision,
        command: command.into(),
        timeout_ms: 5000,
    }
}

#[tokio::test]
#[ignore = "Requires SAILRY_TEST_SSH_HOST; performs only an unauthenticated handshake"]
async fn external_host_key() {
    struct Verifier;
    impl russh::client::Handler for Verifier {
        type Error = russh::Error;
        async fn check_server_key(
            &mut self,
            _: &russh::keys::PublicKey,
        ) -> Result<bool, Self::Error> {
            Ok(false)
        }
    }
    let host = std::env::var("SAILRY_TEST_SSH_HOST").expect("SSH test host required");
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        russh::client::connect(Default::default(), (host.as_str(), 22), Verifier),
    )
    .await
    .expect("SSH handshake timed out");
    match result {
        Err(russh::Error::UnknownKey) => {}
        Err(error) => panic!("SSH handshake failed: {error:?}"),
        Ok(_) => panic!("Untrusted host key was accepted"),
    }
}

#[tokio::test]
async fn preserves_authenticated_ownership() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let server = server::Server::start(directory.path().into(), 41).await;
        let node_path = directory.path().join("node");
        let node = Node::start(&node_path).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let invitation = node.link().invite().unwrap();
        let address = controller.handle().pair(invitation.ticket()).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let mut profile = save(
            &client,
            profile(server.port),
            Some(Credential::Password {
                password: Secret::new("isolated-ssh-password".into()),
            }),
        )
        .await;
        let mut stream = client.subscribe().await.unwrap();
        let probe = run(&profile, "printf x >> effects");
        assert_eq!(
            execute(&client, probe).await,
            Output::SshOutcome(Outcome::HostKeyRequired {
                key: server.key.clone(),
                changed: false
            })
        );
        assert_eq!(server.authentication.load(Ordering::SeqCst), 0);
        assert!(server.commands.lock().unwrap().is_empty());
        profile = trust(&client, &profile, &server.key).await;
        let mut projection = sailry_client::Projection::new(node.id(), 0);
        projection.reconnect(1).unwrap();
        loop {
            projection.apply(1, stream.next().await.unwrap()).unwrap();
            if projection
                .snapshot()
                .is_some_and(|s| s.ssh == [profile.clone()])
            {
                break;
            }
        }
        let request = client.prepare(run(
            &profile,
            "printf x >> effects; printf 'result 中文'; printf diagnostic >&2; exit 7",
        ));
        let expected = Output::SshOutcome(Outcome::Completed {
            exit_code: 7,
            stdout: "result 中文".into(),
            stderr: "diagnostic".into(),
            truncated: false,
        });
        assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
        assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
        assert_eq!(
            std::fs::read(directory.path().join("effects")).unwrap(),
            b"x"
        );
        let mut changed = profile.clone();
        changed.name = "Renamed".into();
        profile = save(&client, changed, None).await;
        assert_eq!(profile.host_key, Some(server.key.clone()));
        assert_eq!(
            client
                .execute(client.prepare(run(
                    &Profile {
                        revision: 1,
                        ..profile.clone()
                    },
                    "exit 0"
                )))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            execute(
                &client,
                Command::CheckSsh {
                    profile: profile.id,
                    expected_revision: profile.revision
                }
            )
            .await,
            Output::SshOutcome(Outcome::Connected)
        );
        drop(stream);
        node.shutdown().await.unwrap();
        let db = rusqlite::Connection::open(node_path.join("storage/node.sqlite3")).unwrap();
        let stored: String = db
            .query_row(
                "SELECT credential FROM ssh_profiles WHERE id=?1",
                [profile.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap()["password"],
            "isolated-ssh-password"
        );
        let mut records = db
            .prepare("SELECT body FROM requests UNION ALL SELECT body FROM events")
            .unwrap();
        for body in records
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
        {
            assert!(!String::from_utf8_lossy(&body.unwrap()).contains("isolated-ssh-password"));
        }
        drop(records);
        drop(db);
        let node = Node::start(&node_path).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            execute(&client, Command::ListSsh).await,
            Output::SshProfiles(vec![profile.clone()])
        );
        assert_eq!(client.execute(request).await.unwrap(), expected);
        assert_eq!(server.commands.lock().unwrap().len(), 1);
        execute(
            &client,
            Command::RemoveSsh {
                profile: profile.id,
                expected_revision: profile.revision,
            },
        )
        .await;
        assert_eq!(
            execute(&client, Command::ListSsh).await,
            Output::SshProfiles(vec![])
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        server.close().await;
    }
}

#[tokio::test]
async fn connection_failure_includes_cause() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let invitation = node.link().invite().unwrap();
        let address = controller.handle().pair(invitation.ticket()).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let profile = save(
            &client,
            profile(port),
            Some(Credential::Password {
                password: Secret::new("isolated-ssh-password".into()),
            }),
        )
        .await;
        let error = client
            .execute(client.prepare(Command::CheckSsh {
                profile: profile.id,
                expected_revision: profile.revision,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        let cause = error
            .message
            .strip_prefix("SSH connection failed: ")
            .unwrap();
        assert!(!cause.is_empty());
        assert!(!error.message.contains("isolated-ssh-password"));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reports_authenticated_uncertainty() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let server = server::Server::start(directory.path().into(), 42).await;
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let invitation = node.link().invite().unwrap();
        let address = controller.handle().pair(invitation.ticket()).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let encoded = server::key(43)
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap();
        let path = directory.path().join("key");
        std::fs::write(&path, encoded.as_str()).unwrap();
        let mut profile = profile(server.port);
        for credential in [
            Credential::PrivateKey {
                key: Secret::new(encoded.to_string()),
                passphrase: None,
            },
            Credential::KeyPath {
                path: Secret::new(path.to_str().unwrap().into()),
                passphrase: None,
            },
        ] {
            profile.authentication = credential.authentication();
            profile = save(&client, profile, Some(credential)).await;
            profile = trust(&client, &profile, &server.key).await;
            assert_eq!(
                execute(
                    &client,
                    Command::CheckSsh {
                        profile: profile.id,
                        expected_revision: profile.revision
                    }
                )
                .await,
                Output::SshOutcome(Outcome::Connected)
            );
        }
        let Output::SshOutcome(Outcome::Completed {
            stdout, truncated, ..
        }) = execute(&client, run(&profile, "large")).await
        else {
            panic!("SSH output expected")
        };
        assert_eq!(stdout.len(), 128 * 1024);
        assert!(truncated);
        assert_eq!(
            client
                .execute(client.prepare(run(&profile, "unknown")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        let request = client.prepare(run(&profile, "hold"));
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !server.commands.lock().unwrap().iter().any(|c| c == "hold") {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        execute(&client, Command::Snapshot).await;
        execute(
            &client,
            Command::CancelSsh {
                request: request.id,
            },
        )
        .await;
        assert_eq!(
            pending.completion.await.unwrap().unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        let key = server::key(44);
        let changed = ssh::HostKey {
            algorithm: key.public_key().algorithm().to_string(),
            fingerprint: key
                .public_key()
                .fingerprint(russh::keys::HashAlg::Sha256)
                .to_string(),
        };
        profile = trust(&client, &profile, &changed).await;
        let attempts = server.authentication.load(Ordering::SeqCst);
        assert_eq!(
            execute(
                &client,
                Command::CheckSsh {
                    profile: profile.id,
                    expected_revision: profile.revision
                }
            )
            .await,
            Output::SshOutcome(Outcome::HostKeyRequired {
                key: server.key.clone(),
                changed: true
            })
        );
        assert_eq!(server.authentication.load(Ordering::SeqCst), attempts);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        server.close().await;
    }
}
