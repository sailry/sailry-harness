use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_node_runtime::Node;

#[test]
fn keeps_settings_on_the_local_profile() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap(),
    );
    runtime.block_on(async {
        let local = Node::start_empty(directory.path().join("desktop"))
            .await
            .unwrap();
        let remote = Node::start_empty(directory.path().join("host"))
            .await
            .unwrap();
        let address = local
            .link()
            .pair(remote.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let services = Services {
            profile: local.profile().to_path_buf(),
            runtime: runtime.clone(),
            link: local.link(),
            local: local.local(),
            relay_enabled: true,
        };
        let selected = Client::new(services.link.remote(address));
        let saved = selected.telemetry_settings().await.unwrap();
        let remote_settings = selected
            .save_telemetry(Update {
                revision: saved.revision,
                errors: false,
                metrics: false,
            })
            .await
            .unwrap();
        let binding = bind(&services);
        assert_eq!(binding.client.target(), local.id());
        assert_ne!(binding.client.target(), selected.target());
        let saved = binding.client.telemetry_settings().await.unwrap();
        assert!(saved.errors && saved.metrics);
        assert_ne!(saved.installation_id, remote_settings.installation_id);
        let changed = binding
            .client
            .save_telemetry(Update {
                revision: saved.revision,
                errors: false,
                metrics: true,
            })
            .await
            .unwrap();
        assert!(!changed.errors && changed.metrics);
        assert_eq!(
            selected.telemetry_settings().await.unwrap(),
            remote_settings
        );
        local.shutdown().await.unwrap();
        remote.shutdown().await.unwrap();
    });
}
