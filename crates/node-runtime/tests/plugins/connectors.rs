//! Bundled packages expose declared connection types on both controller paths.
use super::*;
#[path = "connectors/service.rs"]
mod service;

#[tokio::test]
async fn registers_every_bundled_connection() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        let Output::Plugins(installed) = execute(&client, Command::ListPlugins).await else {
            panic!("plugin inventory expected")
        };
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
            "github",
            "context7",
        ] {
            let package = info(
                execute(
                    &client,
                    Command::InstallBundledPlugin {
                        name: name.into(),
                        expected_revision: installed
                            .iter()
                            .find(|package| package.name == name)
                            .map_or(0, |package| package.revision),
                    },
                )
                .await,
            );
            assert!(package.issues.is_empty(), "{name}: {:?}", package.issues);
            let declarations = &package.extension.as_ref().unwrap().connectors;
            assert!(!declarations.is_empty(), "{name}");
            for declaration in declarations {
                assert!(declaration.valid());
            }
            if !package.summary.enabled {
                execute(
                    &client,
                    Command::SetPluginEnabled {
                        name: name.into(),
                        expected_revision: package.summary.revision,
                        enabled: true,
                    },
                )
                .await;
            }
        }
        let Output::ConnectorTypes(types) = execute(&client, Command::ListConnectorTypes).await
        else {
            panic!("connection types expected")
        };
        assert_eq!(types.len(), 13);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
