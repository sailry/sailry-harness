use super::*;
use serde_json::json;

mod metadata {
    use super::*;
    use cua_driver_sdk::{CuaDriver, DriverHostOptions};

    #[test]
    fn exposes_reviewed_tools_unmodified() {
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
        let reviewed: Vec<_> = original["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|tool| catalog::TOOLS.contains(&tool["name"].as_str().unwrap()))
            .cloned()
            .collect();
        // A renamed or removed SDK tool must fail review instead of vanishing.
        assert_eq!(reviewed.len(), catalog::TOOLS.len());
        assert_eq!(catalog().as_array().unwrap(), &reviewed);
        for tool in &reviewed {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(definition(name), Some(tool));
            assert!(tool["inputSchema"].is_object());
            if name != "clipboard_read" {
                assert_eq!(
                    read_only(name),
                    tool["annotations"]["readOnlyHint"]
                        .as_bool()
                        .or(Some(false))
                );
            }
        }
        assert_eq!(read_only("list_windows"), Some(true));
        assert_eq!(read_only("click"), Some(false));
        assert_eq!(read_only("clipboard_read"), Some(false));
        for hidden in [
            "page",
            "browser_navigate",
            "set_config",
            "install_ffmpeg",
            "start_recording",
            "replay_trajectory",
            "end_session",
            "check_permissions",
            "set_agent_cursor_theme",
            "computer_batch",
        ] {
            assert!(definition(hidden).is_none(), "{hidden}");
        }
    }

    #[test]
    fn package_adds_guidance_only() {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../../plugins/computer/plugin.json")).unwrap();
        let extension = &manifest["extensions"]["dev.sailry.platform"];
        assert!(extension.get("tools").is_none());
        assert!(
            extension["instructions"]
                .as_str()
                .is_some_and(|text| text.contains("untrusted"))
        );
        assert!(extension.get("host").is_none());
        assert_eq!(
            extension["actions"],
            json!(["computer.read", "computer.control"])
        );
    }
}

mod arguments {
    use super::*;

    #[test]
    fn rejects_host_paths_and_session() {
        assert!(execute::validate("get_window_state", &json!({"pid":1})).is_ok());
        for (name, arguments) in [
            (
                "get_window_state",
                json!({"pid":1,"screenshot_out_file":"~/.zshrc"}),
            ),
            (
                "get_desktop_state",
                json!({"screenshot_out_file":"/tmp/a.png"}),
            ),
            ("click", json!({"pid":1,"debug_image_out":"/tmp/a.png"})),
            ("clipboard_write", json!({"file_path":"/etc/hosts"})),
            ("clipboard_write", json!({"image_path":"/tmp/a.png"})),
            ("list_apps", json!({"session":"other"})),
        ] {
            assert!(
                execute::validate(name, &arguments).is_err(),
                "{name} {arguments}"
            );
        }
        assert!(execute::validate("page", &json!({})).is_err());
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
