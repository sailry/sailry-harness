use super::*;
use crate::plugins::fixture::Fixture;
use core::prelude::v1::test;
use sailry_protocol::{Command, Output, plugin};
use std::sync::atomic::Ordering;

use crate::ssh_fixture as server;

#[test]
fn inspects_original_upload_receipt_without_replaying() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let root = fixture.directory.path().join("peer");
        std::fs::create_dir(&root).unwrap();
        let peer = fixture
            .runtime
            .block_on(server::Server::start(root.clone(), 73));
        let Output::SshProfile(profile) = fixture.execute(Command::SaveSsh {
            profile: ssh::Profile {
                id: sailry_protocol::SshId::new(),
                revision: 0,
                name: "Transfer peer".into(),
                host: "127.0.0.1".into(),
                port: peer.port,
                username: "fixture".into(),
                authentication: ssh::Authentication::Password,
                host_key: None,
                sharing: None,
            },
            expected_revision: 0,
            credential: Some(ssh::Credential::Password {
                password: sailry_protocol::Secret::new("isolated-ssh-password".into()),
            }),
        }) else {
            panic!("profile expected");
        };
        let Output::SshProfile(profile) = fixture.execute(Command::TrustSsh {
            profile: profile.id,
            expected_revision: profile.revision,
            key: peer.key.clone(),
        }) else {
            panic!("trusted profile expected");
        };
        let Output::Plugin(package) = fixture.execute(Command::ReadPlugin { name: "ssh".into() })
        else {
            panic!("package expected");
        };
        let access = worker::Access {
            client: fixture.binding.client.clone(),
            context: plugin::Context {
                package: package.summary.reference(),
                surface: plugin::desktop::Surface::Workspace,
                worktree: None,
                session: None,
                turn: None,
                invocation: None,
            },
            profile,
        };
        let source = fixture.directory.path().join("chosen.txt");
        std::fs::write(&source, "Original bytes 中文 🙂").unwrap();
        let destination = root.join("received.txt").to_str().unwrap().to_owned();
        fixture.transport.mode.store(14, Ordering::SeqCst);
        let (plan, status) = fixture.runtime.block_on(worker::upload(
            access.clone(),
            source.clone(),
            worker::Destination {
                path: destination.clone(),
                overwrite: false,
            },
            Default::default(),
            false,
            CancellationToken::new(),
            tokio::sync::watch::channel(Status::Preparing).0,
        ));
        assert!(matches!(
            status,
            Status::Failed(Fault {
                code: ErrorCode::OutcomeUnknown,
                ..
            })
        ));
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "Original bytes 中文 🙂"
        );
        fixture.transport.mode.store(10, Ordering::SeqCst);
        let (plan, status) = fixture.runtime.block_on(worker::upload(
            access.clone(),
            source.clone(),
            worker::Destination {
                path: destination.clone(),
                overwrite: false,
            },
            plan,
            true,
            CancellationToken::new(),
            tokio::sync::watch::channel(Status::Preparing).0,
        ));
        assert!(
            matches!(
                status,
                Status::Failed(Fault {
                    code: ErrorCode::OutcomeUnknown,
                    ..
                })
            ),
            "a failed inspection must keep the original receipt uncertain"
        );
        let (_, status) = fixture.runtime.block_on(worker::upload(
            access.clone(),
            source.clone(),
            worker::Destination {
                path: destination.clone(),
                overwrite: false,
            },
            plan,
            true,
            CancellationToken::new(),
            tokio::sync::watch::channel(Status::Preparing).0,
        ));
        assert!(matches!(status, Status::Done));
        // A worker interruption must not turn a missing receipt into a new upload.
        let (_, status) = fixture.runtime.block_on(worker::upload(
            access,
            source,
            worker::Destination {
                path: destination,
                overwrite: false,
            },
            Default::default(),
            true,
            CancellationToken::new(),
            tokio::sync::watch::channel(Status::Preparing).0,
        ));
        assert!(matches!(
            status,
            Status::Failed(Fault {
                code: ErrorCode::OutcomeUnknown,
                ..
            })
        ));
        let requests = fixture.transport.requests.lock().unwrap();
        let publications: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::FinishSshUpload { .. }))
            .collect();
        assert_eq!(publications.len(), 1);
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(request.command, Command::StageSshUpload(_)))
                .count(),
            1
        );
        assert!(
            publications[0]
                .plugin
                .as_ref()
                .is_some_and(|context| context.package == package.summary.reference()
                    && context.worktree.is_none())
        );
        drop(requests);
        fixture.runtime.block_on(peer.close());
        fixture.close();
    }
}
