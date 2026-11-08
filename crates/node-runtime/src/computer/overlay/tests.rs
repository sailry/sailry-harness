use super::*;
use serde_json::json;

#[test]
fn validates_session_identifiers() {
    assert_eq!(
        session_id(json!({"session_id":"run-fixture"})).unwrap(),
        "run-fixture"
    );
    for data in [
        json!({}),
        json!({"session_id":""}),
        json!({"session_id":"../other"}),
        json!({"session_id":"run-".repeat(20)}),
    ] {
        assert!(session_id(data).is_err());
    }
}

#[test]
fn defaults_to_enabled() {
    assert!(enabled(&BTreeMap::new()));
    assert!(enabled(&BTreeMap::from([(
        "cursor_overlay".into(),
        json!(true)
    )])));
    assert!(!enabled(&BTreeMap::from([(
        "cursor_overlay".into(),
        json!(false)
    )])));
}

#[cfg(unix)]
mod lifecycle {
    use super::*;
    use cap_std::{ambient_authority, fs::Dir};
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        directory: tempfile::TempDir,
        home: PathBuf,
        executable: PathBuf,
        overlay: Overlay,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            let home = root.join("agent-desktop");
            let executable = root.join("helper");
            std::fs::write(
                &executable,
                r#"#!/bin/sh
home="$AGENT_DESKTOP_HOME"
mode="$(cat "$home/mode" 2>/dev/null)"
printf '%s\n' "$*" >> "$home/calls"
case "$*" in
  'session gc --ended')
    if [ "$mode" = slow_steps ]; then sleep 6; fi
    printf '{"version":"2.6","ok":true,"command":"session","data":{"removed":[]}}\n' ;;
  'session start --no-trace --name sailry')
    case "$mode" in
      failed) printf '{"ok":false,"error":{"code":"INTERNAL","message":"private content"}}'; exit 1 ;;
      malformed) printf 'private content'; exit 0 ;;
    esac
    count="$(cat "$home/count" 2>/dev/null || printf 0)"
    count=$((count + 1))
    printf '%s' "$count" > "$home/count"
    session="run-fixture-$count"
    mkdir -p "$home/sessions/$session"
    printf '{"id":"%s","name":"sailry","created_at":1,"trace":"off","artifacts":"events"}' "$session" > "$home/sessions/$session/session.json"
    if [ "$mode" = config_failed ]; then mkdir "$home/sessions/$session/cursor.png"; fi
    if [ "$mode" = stalled_start ]; then exec sleep 30; fi
    if [ "$mode" = slow_steps ]; then sleep 6; fi
    if [ "$mode" = inherited_stdout ]; then
      sleep 30 &
      printf '%s' "$!" > "$home/stdout-child"
    fi
    printf '{"version":"2.6","ok":true,"command":"session","data":{"activation":{"environment":"AGENT_DESKTOP_SESSION","value":"%s"},"artifacts":"events","created_at":1,"name":"sailry","next":"export AGENT_DESKTOP_SESSION=%s","session_id":"%s","trace":"off"}}\n' "$session" "$session" "$session" ;;
  'session list') cat "$home/list" ;;
  'session end '*)
    if [ -f "$home/end-failed" ]; then
      printf '{"ok":false,"error":{"code":"INTERNAL"}}'
      exit 1
    fi
    printf '{"id":"%s","ended_at":1}' "$3" > "$home/sessions/$3/session.json"
    printf '{"version":"2.6","ok":true,"command":"session","data":{"ended_at":1,"session_id":"%s"}}\n' "$3" ;;
  *) exit 64 ;;
esac
"#,
            )
            .unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let data = Dir::open_ambient_dir(&root, ambient_authority()).unwrap();
            crate::computer::helper::prepare_home(&data, &home).unwrap();
            Self {
                directory,
                home,
                executable,
                overlay: Overlay::default(),
            }
        }

        fn launch(&self) -> Launch {
            let root = self.directory.path().canonicalize().unwrap();
            Launch::Stdio {
                command: self.executable.clone(),
                args: vec!["mcp".into()],
                env: BTreeMap::from([(
                    "AGENT_DESKTOP_HOME".into(),
                    self.home.to_str().unwrap().into(),
                )]),
                cwd: root.clone(),
                roots: (
                    Dir::open_ambient_dir(&root, ambient_authority()).unwrap(),
                    Dir::open_ambient_dir(&root, ambient_authority()).unwrap(),
                ),
                settings: None,
                python: None,
                computer: true,
            }
        }

        async fn attach(&self, enabled: bool) -> Option<String> {
            let mut launch = self.launch();
            self.overlay
                .attach("computer", enabled, &mut launch, &Environment::ambient())
                .await;
            let Launch::Stdio { env, .. } = launch else {
                unreachable!()
            };
            env.get(SESSION).cloned()
        }

        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.home.join("calls"))
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }
    }

    #[tokio::test]
    async fn shares_concurrent_starts() {
        let fixture = Fixture::new();
        let sessions = futures::future::join_all((0..8).map(|_| fixture.attach(true))).await;
        assert!(
            sessions
                .iter()
                .all(|session| session.as_deref() == Some("run-fixture-1"))
        );
        assert_eq!(
            fixture.calls(),
            [
                "session gc --ended",
                "session start --no-trace --name sailry"
            ]
        );
        let manifest = manifest(&fixture.home, "run-fixture-1").await.unwrap();
        assert_eq!(manifest["name"], "sailry");
        assert_eq!(manifest["created_at"], 1);
        assert_eq!(manifest["trace"], "off");
        assert_eq!(manifest["artifacts"], "events");
        assert!(manifest["ended_at"].is_null());
        let overlay = &manifest["cursor_overlay"];
        assert_eq!(overlay["enabled"], true);
        assert!(overlay.get("label").is_none());
        assert!(overlay.get("multi_agent").is_none());
        #[cfg(target_os = "macos")]
        {
            let cursor = &overlay["style"]["images"]["arrow"];
            assert_eq!(cursor["hotspot"], json!({"x": 16, "y": 16}));
            assert_eq!(
                cursor["path"],
                fixture
                    .home
                    .join("sessions/run-fixture-1/cursor.png")
                    .to_str()
                    .unwrap()
            );
            let image = image::open(cursor["path"].as_str().unwrap()).unwrap();
            assert_eq!((image.width(), image.height()), (32, 32));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(overlay["style"]["size"], 0.75);
            assert!(overlay["style"].get("images").is_none());
        }
        fixture.overlay.shutdown().await;
        assert_eq!(fixture.calls().last().unwrap(), "session end run-fixture-1");
        assert_eq!(fixture.attach(true).await, None);
        assert_eq!(fixture.calls().len(), 3);
    }

    #[tokio::test]
    async fn replaces_ended_or_missing_sessions() {
        let fixture = Fixture::new();
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-1"));
        let manifest = fixture.home.join("sessions/run-fixture-1/session.json");
        std::fs::write(&manifest, r#"{"id":"run-fixture-1","ended_at":1}"#).unwrap();
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-2"));
        std::fs::remove_file(fixture.home.join("sessions/run-fixture-2/session.json")).unwrap();
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-3"));
        assert_eq!(
            fixture
                .calls()
                .iter()
                .filter(|call| *call == "session gc --ended")
                .count(),
            1
        );
        fixture.overlay.shutdown().await;
    }

    #[tokio::test]
    async fn gives_each_step_its_deadline() {
        let fixture = Fixture::new();
        std::fs::write(fixture.home.join("mode"), "slow_steps").unwrap();
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-1"));
        fixture.overlay.shutdown().await;
    }

    #[tokio::test]
    async fn accepts_response_before_stdout_closes() {
        let fixture = Fixture::new();
        std::fs::write(fixture.home.join("mode"), "inherited_stdout").unwrap();
        let session = tokio::time::timeout(Duration::from_secs(3), fixture.attach(true)).await;
        let pid: i32 = std::fs::read_to_string(fixture.home.join("stdout-child"))
            .unwrap()
            .parse()
            .unwrap();
        unsafe { libc::kill(pid, libc::SIGTERM) };
        fixture.overlay.shutdown().await;
        assert_eq!(session.unwrap().as_deref(), Some("run-fixture-1"));
    }

    #[tokio::test]
    async fn cleans_only_unrecorded_owned_sessions() {
        let fixture = Fixture::new();
        std::fs::write(fixture.home.join("mode"), "stalled_start").unwrap();
        std::fs::write(
            fixture.home.join("list"),
            serde_json::to_vec(&json!({"ok": true, "data": {"sessions": [
                {"session_id": "run-fixture-1", "name": "sailry", "ended_at": null},
                {"session_id": "manual", "name": "operator", "ended_at": null},
                {"session_id": "ended", "name": "sailry", "ended_at": 1},
                {"session_id": "../other", "name": "sailry", "ended_at": null}
            ]}}))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(fixture.attach(true).await, None);
        assert_eq!(
            fixture.calls(),
            [
                "session gc --ended",
                "session start --no-trace --name sailry",
                "session list",
                "session end run-fixture-1",
            ]
        );
        assert!(!active(&fixture.home, "run-fixture-1").await);
        std::fs::remove_file(fixture.home.join("mode")).unwrap();
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-2"));
        fixture.overlay.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_interrupts_pending_start() {
        let fixture = Fixture::new();
        std::fs::write(fixture.home.join("mode"), "stalled_start").unwrap();
        std::fs::write(
            fixture.home.join("list"),
            serde_json::to_vec(&json!({
                "version": "2.6",
                "ok": true,
                "command": "session",
                "data": {"sessions": [{
                    "artifacts": "events",
                    "created_at": 1,
                    "ended_at": null,
                    "name": "sailry",
                    "session_id": "run-fixture-1",
                    "trace": "off"
                }]}
            }))
            .unwrap(),
        )
        .unwrap();
        let shutdown = async {
            while !fixture
                .home
                .join("sessions/run-fixture-1/session.json")
                .exists()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            fixture.overlay.shutdown().await;
        };
        let (session, ()) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(fixture.attach(true), shutdown)
        })
        .await
        .expect("shutdown must interrupt the helper start");
        assert_eq!(session, None);
        assert!(!active(&fixture.home, "run-fixture-1").await);
        assert_eq!(fixture.calls().last().unwrap(), "session end run-fixture-1");
        assert_eq!(fixture.attach(true).await, None);
    }

    #[tokio::test]
    async fn retains_failed_cleanup_for_retry() {
        let fixture = Fixture::new();
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-1"));
        std::fs::write(fixture.home.join("end-failed"), "").unwrap();
        fixture.overlay.retain(Some("computer"), &[]).await;
        fixture
            .overlay
            .retain(Some("computer"), &["computer".into()])
            .await;
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-1"));
        assert_eq!(
            std::fs::read_to_string(fixture.home.join("count")).unwrap(),
            "1"
        );
        std::fs::remove_file(fixture.home.join("end-failed")).unwrap();
        fixture.overlay.shutdown().await;
        assert_eq!(
            fixture
                .calls()
                .iter()
                .filter(|call| *call == "session end run-fixture-1")
                .count(),
            2
        );
        assert!(!active(&fixture.home, "run-fixture-1").await);
    }

    #[tokio::test]
    async fn preserves_headless_launch_on_failure() {
        for mode in ["failed", "malformed"] {
            let fixture = Fixture::new();
            std::fs::write(fixture.home.join("mode"), mode).unwrap();
            assert_eq!(fixture.attach(true).await, None);
            let Launch::Stdio { args, env, .. } = fixture.launch() else {
                unreachable!()
            };
            assert_eq!(args, ["mcp"]);
            assert!(!env.contains_key(SESSION));
            fixture.overlay.shutdown().await;
            assert!(
                !fixture
                    .calls()
                    .iter()
                    .any(|call| call.starts_with("session end"))
            );
        }
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn releases_unconfigured_sessions() {
        let fixture = Fixture::new();
        std::fs::write(fixture.home.join("mode"), "config_failed").unwrap();
        std::fs::write(fixture.home.join("end-failed"), "").unwrap();
        assert_eq!(fixture.attach(true).await, None);
        assert_eq!(fixture.calls().last().unwrap(), "session end run-fixture-1");
        assert_eq!(fixture.attach(true).await, None);
        assert_eq!(
            std::fs::read_to_string(fixture.home.join("count")).unwrap(),
            "1"
        );
        std::fs::remove_file(fixture.home.join("end-failed")).unwrap();
        std::fs::remove_file(fixture.home.join("mode")).unwrap();
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-2"));
        assert!(!active(&fixture.home, "run-fixture-1").await);
        fixture.overlay.shutdown().await;
        assert!(!active(&fixture.home, "run-fixture-2").await);
    }

    #[tokio::test]
    async fn respects_current_settings_and_declared_environment() {
        let fixture = Fixture::new();
        assert_eq!(fixture.attach(false).await, None);
        assert!(fixture.calls().is_empty());
        let mut launch = fixture.launch();
        let Launch::Stdio { env, .. } = &mut launch else {
            unreachable!()
        };
        env.insert(SESSION.into(), "declared-session".into());
        fixture
            .overlay
            .attach("computer", true, &mut launch, &Environment::ambient())
            .await;
        let Launch::Stdio { env, .. } = launch else {
            unreachable!()
        };
        assert_eq!(env[SESSION], "declared-session");
        assert!(fixture.calls().is_empty());
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-1"));
        fixture.overlay.retain(Some("computer"), &[]).await;
        assert_eq!(fixture.calls().last().unwrap(), "session end run-fixture-1");
        assert_eq!(fixture.attach(true).await, None);
        fixture
            .overlay
            .retain(Some("computer"), &["computer".into()])
            .await;
        assert_eq!(fixture.attach(true).await.as_deref(), Some("run-fixture-2"));
        fixture.overlay.retain(None, &[]).await;
        assert_eq!(fixture.calls().last().unwrap(), "session end run-fixture-2");
        assert_eq!(fixture.attach(true).await, None);
        fixture.overlay.shutdown().await;
    }

    #[test]
    fn prepares_private_directories() {
        let fixture = Fixture::new();
        for directory in [fixture.directory.path(), fixture.home.as_path()] {
            assert_eq!(
                std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }
}
