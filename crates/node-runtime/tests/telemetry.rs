//! Anonymous consent uses the same durable command contract over both transports.
use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{
    telemetry::{Settings, Update as SettingsUpdate},
    *,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn preserves_consent_identity_and_replay() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start_empty(&profile).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let transports: [Arc<dyn Transport>; 2] = [node.local(), controller.handle().remote(address)];
    let mut expected = Client::new(node.local())
        .telemetry_settings()
        .await
        .unwrap();
    assert!(expected.errors && expected.metrics);
    assert!(!expected.installation_id.is_nil());
    for transport in transports {
        let client = Client::new(transport);
        assert_eq!(client.telemetry_settings().await.unwrap(), expected);
        let mut updates = client.subscribe().await.unwrap();
        let _ = updates.next().await.unwrap();
        let update = SettingsUpdate {
            revision: expected.revision,
            errors: false,
            metrics: true,
        };
        let old = client.prepare(Command::SaveTelemetrySettings(update.clone()));
        assert!(old.command.durable());
        let first = client.execute(old.clone()).await.unwrap();
        let Output::TelemetrySettings(saved) = first.clone() else {
            panic!("telemetry settings expected");
        };
        assert!(!saved.errors && saved.metrics);
        assert_eq!(saved.installation_id, expected.installation_id);
        assert_eq!(saved.revision, expected.revision + 1);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let sailry_protocol::Update::Event(envelope) = updates.next().await.unwrap()
                    && let Event::TelemetrySettingsChanged { revision } = envelope.event
                {
                    assert_eq!(revision, saved.revision);
                    break;
                }
            }
        })
        .await
        .unwrap();
        let disabled = client
            .save_telemetry(SettingsUpdate {
                revision: saved.revision,
                errors: false,
                metrics: false,
            })
            .await
            .unwrap();
        assert_eq!(client.execute(old).await.unwrap(), first);
        assert_eq!(client.telemetry_settings().await.unwrap(), disabled);
        assert_eq!(
            client.save_telemetry(update).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let reset = client.prepare(Command::ResetTelemetryIdentity {
            expected_revision: disabled.revision,
        });
        assert!(reset.command.durable());
        let output = client.execute(reset.clone()).await.unwrap();
        let Output::TelemetrySettings(reset_settings) = &output else {
            panic!("telemetry settings expected");
        };
        assert_ne!(reset_settings.installation_id, disabled.installation_id);
        assert!(!reset_settings.errors && !reset_settings.metrics);
        assert_eq!(reset_settings.revision, disabled.revision + 1);
        let changed = client
            .save_telemetry(SettingsUpdate {
                revision: reset_settings.revision,
                errors: true,
                metrics: false,
            })
            .await
            .unwrap();
        assert_eq!(client.execute(reset).await.unwrap(), output);
        assert_eq!(client.telemetry_settings().await.unwrap(), changed);
        assert_eq!(
            client
                .reset_telemetry(disabled.revision)
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        expected = changed;
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    assert_eq!(
        Client::new(node.local())
            .telemetry_settings()
            .await
            .unwrap(),
        expected
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn keeps_controller_and_host_profiles_separate() {
    let directory = tempfile::tempdir().unwrap();
    let desktop = Node::start_empty(directory.path().join("desktop"))
        .await
        .unwrap();
    let host = Node::start_empty(directory.path().join("host"))
        .await
        .unwrap();
    let address = desktop
        .link()
        .pair(host.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let local = Client::new(desktop.local());
    let remote = Client::new(desktop.link().remote(address));
    let local_settings = local.telemetry_settings().await.unwrap();
    let remote_settings = remote.telemetry_settings().await.unwrap();
    assert_ne!(
        local_settings.installation_id,
        remote_settings.installation_id
    );
    let changed = remote
        .save_telemetry(SettingsUpdate {
            revision: remote_settings.revision,
            errors: false,
            metrics: false,
        })
        .await
        .unwrap();
    assert_eq!(local.telemetry_settings().await.unwrap(), local_settings);
    let reset = local
        .reset_telemetry(local_settings.revision)
        .await
        .unwrap();
    assert_ne!(reset.installation_id, local_settings.installation_id);
    assert_eq!(remote.telemetry_settings().await.unwrap(), changed);
    desktop.shutdown().await.unwrap();
    host.shutdown().await.unwrap();
}

#[path = "telemetry/defaults.rs"]
mod defaults;
