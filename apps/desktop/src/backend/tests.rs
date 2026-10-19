use super::*;
use crate::startup::Options;
use sailry_client::Client;
use sailry_protocol::{Command, Output};

mod unwind {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Guard<'a>(&'a AtomicUsize);

    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[inline(never)]
    fn nested(depth: usize, drops: &AtomicUsize) {
        let _guard = Guard(drops);
        if depth == 0 {
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            assert!(trace.contains("unwind::nested"), "{trace}");
            panic!("unwind test");
        }
        nested(depth - 1, drops);
        std::hint::black_box(depth);
    }

    #[test]
    fn catches_panics_and_runs_destructors() {
        let drops = AtomicUsize::new(0);
        let result = std::panic::catch_unwind(|| nested(8, &drops));
        let panic = result.expect_err("nested call must panic");
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"unwind test"));
        assert_eq!(drops.load(Ordering::SeqCst), 9);
    }
}

fn profile() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix("sailry-profile-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir().unwrap()
}

#[test]
fn reopens_owned_profile() {
    let directory = profile();
    let path = directory.path().to_path_buf();
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
    let directory = profile();
    let profile = directory.path().to_path_buf();
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
    let directory = profile();
    let path = directory.path().to_path_buf();
    let local = Owner::start(Options {
        data_dir: path.clone(),
        relays: vec![],
    })
    .unwrap();
    let remote_directory = profile();
    let remote = Owner::start(Options {
        data_dir: remote_directory.path().to_path_buf(),
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
