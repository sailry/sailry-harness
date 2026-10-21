//! Permission access retains the captured package and execution Node authority.
use crate::plugins::support::{execute, info};
use sailry_protocol::{
    Command, ErrorCode, Output,
    plugin::{self, Context},
};

#[tokio::test]
async fn reads_permissions_without_prompting() {
    #[cfg(target_os = "macos")]
    isolated().await;
    #[cfg(not(target_os = "macos"))]
    inspect().await;
}

#[cfg(not(target_os = "macos"))]
async fn inspect() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _worktree) =
            crate::plugins::support::packages(remote, &["computer"]).await;
        let package = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "computer".into(),
                },
            )
            .await,
        );
        assert!(!package.summary.enabled);
        let context = Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Settings,
            package: package.summary.reference(),
            worktree: None,
            session: None,
        };
        let Output::ComputerPermissions(permissions) = client
            .execute(
                client
                    .prepare(Command::ReadComputerPermissions)
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("permissions expected")
        };
        assert_eq!(permissions.node, node.id());
        assert_eq!(permissions.local, !remote);
        assert_eq!(permissions.platform, std::env::consts::OS);
        assert_eq!(permissions.screen_capture, None);
        assert_eq!(permissions.accessibility, None);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::RequestComputerPermission {
                            permission: sailry_protocol::computer::Permission::Accessibility
                        })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            if remote {
                ErrorCode::PermissionDenied
            } else {
                ErrorCode::Unavailable
            }
        );
        denied_surface(&client, context).await;
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

async fn denied_surface(client: &sailry_client::Client, mut context: Context) {
    context.surface = Default::default();
    assert_eq!(
        client
            .execute(
                client
                    .prepare(Command::ReadComputerPermissions)
                    .with_plugin(context)
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
}

#[cfg(target_os = "macos")]
async fn isolated() {
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let helpers = root.join("helpers");
    let executable = helpers.join("Sailry Computer Use.app/Contents/MacOS/OpenComputerUse");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, r#"#!/bin/sh
fixture="$(cd "$(dirname "$0")/../../../.." && pwd)"
printf '%s|%s|%s\n' "$*" "$OPEN_COMPUTER_USE_AGENT_SOCKET_NAMESPACE" "$OPEN_COMPUTER_USE_CONFIG_FILE" >> "$fixture/calls"
case "$*" in
  'doctor') printf 'permission setup\n' ;;
  'doctor --json')
    case "$(cat "$fixture/mode")" in
      missing) printf '{"screenCaptureGranted":false,"accessibilityTrusted":false}' ;;
      granted) printf '{"screenCaptureGranted":true,"accessibilityTrusted":true}' ;;
      malformed) printf 'private diagnostic content' ;;
      failed) printf 'private diagnostic content' >&2; exit 17 ;;
      *) exit 64 ;;
    esac ;;
  *) exit 64 ;;
esac
"#).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(120),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "plugins::actions::computer::diagnostics_peer",
                "--ignored",
                "--nocapture",
            ])
            .env("SAILRY_HELPERS_DIR", helpers)
            .env("SAILRY_TEST_OCU_PERMISSIONS", root)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "permission subprocess failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "launched by the isolated permission service fixture"]
fn diagnostics_peer() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let root = std::path::PathBuf::from(
            std::env::var_os("SAILRY_TEST_OCU_PERMISSIONS").expect("isolated permission fixture"),
        );
        for remote in [false, true] {
            let (directory, node, controller, client, _worktree) =
                crate::plugins::support::packages(remote, &["computer"]).await;
            let package = info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "computer".into(),
                    },
                )
                .await,
            );
            assert!(!package.summary.enabled);
            assert!(package.issues.is_empty(), "{:?}", package.issues);
            let context = Context {
                invocation: None,
                turn: None,
                surface: plugin::desktop::Surface::Settings,
                package: package.summary.reference(),
                worktree: None,
                session: None,
            };
            std::fs::write(root.join("calls"), "").unwrap();
            for (mode, granted) in [("missing", false), ("granted", true)] {
                std::fs::write(root.join("mode"), mode).unwrap();
                let request = client
                    .prepare(Command::ReadComputerPermissions)
                    .with_plugin(context.clone());
                assert!(!request.command.durable());
                let Output::ComputerPermissions(permissions) =
                    client.execute(request).await.unwrap()
                else {
                    panic!("permissions expected")
                };
                assert_eq!(permissions.node, node.id());
                assert_eq!(permissions.local, !remote);
                assert_eq!(permissions.platform, "macos");
                assert_eq!(permissions.screen_capture, Some(granted));
                assert_eq!(permissions.accessibility, Some(granted));
            }
            let calls = std::fs::read_to_string(root.join("calls")).unwrap();
            assert_eq!(calls.lines().count(), 2);
            let namespace = directory
                .path()
                .join("node/plugins/data/computer")
                .canonicalize()
                .unwrap();
            let config = directory
                .path()
                .join("node/plugins/packages")
                .join(&context.package.digest)
                .join("ocu.json")
                .canonicalize()
                .unwrap();
            for call in calls.lines() {
                assert_eq!(
                    call,
                    format!("doctor --json|{}|{}", namespace.display(), config.display())
                );
            }
            let request = client
                .prepare(Command::RequestComputerPermission {
                    permission: sailry_protocol::computer::Permission::Accessibility,
                })
                .with_plugin(context.clone());
            assert!(!request.command.durable());
            if remote {
                assert_eq!(
                    client.execute(request).await.unwrap_err().code,
                    ErrorCode::PermissionDenied
                );
                assert_eq!(std::fs::read_to_string(root.join("calls")).unwrap(), calls);
            } else {
                let Output::ComputerPermissions(permissions) =
                    client.execute(request).await.unwrap()
                else {
                    panic!("permissions expected")
                };
                assert_eq!(permissions.screen_capture, Some(true));
                assert_eq!(permissions.accessibility, Some(true));
                let current = std::fs::read_to_string(root.join("calls")).unwrap();
                assert_eq!(current.lines().count(), 4);
                assert!(current.lines().nth(2).unwrap().starts_with("doctor|"));
                assert!(
                    current
                        .lines()
                        .nth(3)
                        .unwrap()
                        .starts_with("doctor --json|")
                );
            }
            for mode in ["malformed", "failed"] {
                std::fs::write(root.join("mode"), mode).unwrap();
                let error = client
                    .execute(
                        client
                            .prepare(Command::ReadComputerPermissions)
                            .with_plugin(context.clone()),
                    )
                    .await
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::Unavailable);
                assert!(!error.message.contains("private diagnostic content"));
            }
            denied_surface(&client, context).await;
            drop(client);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    });
}
