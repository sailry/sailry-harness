use super::*;
use crate::startup::Options;
use sailry_client::Client;
use sailry_protocol::{Command, Output};

#[test]
fn reopens_owned_profile() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node");
    let owner = Owner::start(Options {
        data_dir: path.clone(),
        relays: vec![],
    })
    .unwrap();
    let id = owner.node.id();
    assert_eq!(owner.node.profile(), path.canonicalize().unwrap());
    let client = Client::new(owner.services.local.clone());
    assert!(
        matches!(owner.services.runtime.block_on(client.execute(client.prepare(Command::Snapshot))).unwrap(), Output::Snapshot(snapshot) if snapshot.projects.is_empty())
    );
    assert!(
        Owner::start(Options {
            data_dir: path.clone(),
            relays: vec![]
        })
        .is_err()
    );
    owner
        .services
        .runtime
        .block_on(owner.node.shutdown())
        .unwrap();
    let reopened = Owner::start(Options {
        data_dir: path,
        relays: vec![],
    })
    .unwrap();
    assert_eq!(reopened.node.id(), id);
    reopened
        .services
        .runtime
        .block_on(reopened.node.shutdown())
        .unwrap();
}

#[test]
fn update_handoff_drains_only_the_shared_local_owner() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local");
    let local = Owner::start(Options {
        data_dir: path.clone(),
        relays: vec![],
    })
    .unwrap();
    let remote = Owner::start(Options {
        data_dir: directory.path().join("remote"),
        relays: vec![],
    })
    .unwrap();
    let id = local.node.id();
    let runtime = local.services.runtime.clone();
    let lifecycle = Lifecycle::new(local.node);
    let quitting = lifecycle.clone();
    runtime.block_on(async {
        let (install, quit) = tokio::join!(lifecycle.stop(), quitting.stop());
        install.unwrap();
        quit.unwrap();
        assert!(lifecycle.owner.lock().await.is_none());
    });
    let remote_client = Client::new(remote.services.local.clone());
    assert!(matches!(
        remote
            .services
            .runtime
            .block_on(remote_client.execute(remote_client.prepare(Command::Snapshot)))
            .unwrap(),
        Output::Snapshot(_)
    ));
    let reopened = Owner::start(Options {
        data_dir: path,
        relays: vec![],
    })
    .unwrap();
    assert_eq!(reopened.node.id(), id);
    reopened
        .services
        .runtime
        .block_on(reopened.node.shutdown())
        .unwrap();
    remote
        .services
        .runtime
        .block_on(remote.node.shutdown())
        .unwrap();
}
