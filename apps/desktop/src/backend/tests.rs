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
fn restores_relays_and_respects_explicit_startup_options() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let initial = Owner::start(Options {
        data_dir: profile.clone(),
        relays: vec![],
    })
    .unwrap();
    initial
        .services
        .runtime
        .block_on(initial.node.shutdown())
        .unwrap();
    let stored = vec!["https://relay.example.test".into()];
    let data = crate::preferences::Data {
        iroh_relays: Some(stored.clone()),
        ..Default::default()
    };
    std::fs::create_dir_all(profile.join("desktop")).unwrap();
    std::fs::write(
        profile.join("desktop/preferences.json"),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    for (explicit, expected) in [
        (vec![], stored),
        (
            vec!["https://explicit.example.test".into()],
            vec!["https://explicit.example.test".into()],
        ),
    ] {
        let owner = Owner::start(Options {
            data_dir: profile.clone(),
            relays: explicit,
        })
        .unwrap();
        assert_eq!(
            owner.services.link.relay_selection().unwrap(),
            Some(sailry_link::RelaySelection::Custom(expected))
        );
        owner
            .services
            .runtime
            .block_on(owner.node.shutdown())
            .unwrap();
    }
}

#[test]
fn handoff_drains_only_local_owner() {
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
