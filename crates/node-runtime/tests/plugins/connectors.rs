//! Bundled connector registration and settings use the same controller boundary.
use super::*;
use plugin::{McpTransport, oauth::Provider, settings::SecretUpdate};
use std::collections::BTreeMap;

#[tokio::test]
async fn installs_and_configures_declared_connectors() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        for name in [
            "notion",
            "linear",
            "figma",
            "slack",
            "feishu",
            "tencent-docs",
            "google-workspace",
            "microsoft-365",
            "wecom",
            "dingtalk",
            "yuque",
        ] {
            let info = info(
                execute(
                    &client,
                    Command::InstallBundledPlugin {
                        name: name.into(),
                        expected_revision: 0,
                    },
                )
                .await,
            );
            assert!(info.issues.is_empty(), "{name}: {:?}", info.issues);
            let extension = info.extension.as_ref().unwrap();
            assert_eq!(extension.api_version, "v1");
            assert_eq!(extension.display.as_ref().unwrap().locales.len(), 10);
            assert_eq!(extension.description.as_ref().unwrap().locales.len(), 10);
            if [
                "notion",
                "linear",
                "figma",
                "slack",
                "feishu",
                "tencent-docs",
            ]
            .contains(&name)
            {
                assert_eq!(info.mcp.len(), 1);
                assert_eq!(info.mcp[0].transport, McpTransport::StreamableHttp);
                assert!(extension.host.is_none());
            } else {
                assert!(extension.host.as_ref().unwrap().valid());
                assert_eq!(extension.tools.len(), 1);
            }
            let expected = match name {
                "google-workspace" => Some(Provider::Google),
                "microsoft-365" => Some(Provider::Microsoft),
                "dingtalk" => Some(Provider::Dingtalk),
                _ => None,
            };
            assert_eq!(
                extension
                    .oauth
                    .as_ref()
                    .map(|declaration| declaration.provider),
                expected
            );
            if let Some(schema) = &info.settings {
                let Output::PluginSettings(initial) = execute(
                    &client,
                    Command::ReadPluginSettings {
                        package: info.summary.reference(),
                    },
                )
                .await
                else {
                    panic!("settings expected");
                };
                let secrets: BTreeMap<_, _> = schema
                    .properties
                    .iter()
                    .filter(|(_, field)| field.secret.is_some())
                    .map(|(name, _)| {
                        (
                            name.clone(),
                            SecretUpdate::Replace(Secret::new("connector-private-marker".into())),
                        )
                    })
                    .collect();
                let Output::PluginSettings(saved) = execute(
                    &client,
                    Command::SavePluginSettings {
                        package: initial.package,
                        values: initial.values,
                        secrets,
                    },
                )
                .await
                else {
                    panic!("saved settings expected");
                };
                assert_eq!(
                    saved.configured.len(),
                    schema
                        .properties
                        .values()
                        .filter(|field| field.secret.is_some())
                        .count()
                );
                assert!(saved.ready);
                assert!(
                    !serde_json::to_string(&saved)
                        .unwrap()
                        .contains("connector-private-marker")
                );
            }
        }
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("connector-private-marker")
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
