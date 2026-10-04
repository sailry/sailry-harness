use super::*;
use serde_json::json;

mod metadata {
    use super::*;
    use cua_driver_sdk::{CuaDriver, DriverHostOptions};

    #[test]
    fn matches_original_inventory() {
        let original = CuaDriver::inspect_host_tools(DriverHostOptions {
            cursor: Default::default(),
            host_owns_permission_ux: true,
            host_bundle_id: None,
            claude_code_compatibility: false,
            prepare_desktop_environment: true,
            register_host_tools: None,
            authorization_host: None,
            activity_observer: None,
        });
        assert_eq!(catalog(), &original["tools"]);
        let tools = catalog().as_array().unwrap();
        assert!(!tools.is_empty());
        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(definition(name), Some(tool));
            assert_eq!(
                read_only(name),
                Some(
                    tool["annotations"]["readOnlyHint"]
                        .as_bool()
                        .unwrap_or(false)
                )
            );
            assert!(tool["inputSchema"].is_object());
        }
        assert_eq!(read_only("list_windows"), Some(true));
        assert_eq!(read_only("click"), Some(false));
        assert!(definition("computer_batch").is_none());
        assert!(definition("computer_observe").is_none());
    }

    #[test]
    fn package_has_no_custom_tools_or_prompt() {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../../plugins/computer/plugin.json")).unwrap();
        let extension = &manifest["extensions"]["dev.sailry.platform"];
        assert!(extension.get("tools").is_none());
        assert!(extension.get("instructions").is_none());
        assert!(extension.get("host").is_none());
        assert_eq!(
            extension["actions"],
            json!(["computer.read", "computer.control"])
        );
    }
}

mod admission {
    use super::*;

    #[tokio::test]
    async fn cancellation_prevents_dispatch() {
        let desktop = Desktop::default();
        let guard = desktop.state.lock().await;
        let stop = CancellationToken::new();
        stop.cancel();
        let result = desktop
            .execute(SessionId::new(), "list_windows", json!({}), &stop, async {
                panic!("cancelled call entered admission")
            })
            .await
            .unwrap_err();
        assert!(result.contains("cancelled"));
        drop(guard);
    }

    #[tokio::test]
    async fn rechecks_after_waiting() {
        let desktop = Desktop::default();
        assert_eq!(
            desktop
                .execute(
                    SessionId::new(),
                    "list_windows",
                    json!({}),
                    &CancellationToken::new(),
                    async { Err("disabled while waiting".into()) }
                )
                .await
                .unwrap_err(),
            "disabled while waiting"
        );
    }

    #[tokio::test]
    async fn shutdown_prevents_dispatch() {
        let desktop = Desktop::default();
        desktop.shutdown().await;
        assert!(
            desktop
                .execute(
                    SessionId::new(),
                    "click",
                    json!({}),
                    &CancellationToken::new(),
                    async { panic!("shutdown Node entered admission") }
                )
                .await
                .unwrap_err()
                .contains("shutting down")
        );
        desktop.shutdown().await;
    }
}

#[cfg(target_os = "macos")]
mod native;
