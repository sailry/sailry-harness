use super::*;
use std::os::unix::fs::PermissionsExt;

mod requests {
    use super::*;

    #[test]
    fn accepts_only_the_current_share_envelope() {
        assert!(
            request_relay(
                "{\"version\":1,\"command\":\"share\",\"origin\":\"https://link.sailry.dev\"}\n"
            )
            .is_ok()
        );
        for request in [
            "{\"version\":2,\"command\":\"share\",\"origin\":\"https://link.sailry.dev\"}\n",
            "{\"version\":1,\"command\":\"start\",\"origin\":\"https://link.sailry.dev\"}\n",
            "{\"version\":1,\"command\":\"share\",\"origin\":\"http://example.invalid\"}\n",
            "{\"version\":1,\"command\":\"share\",\"origin\":\"https://link.sailry.dev\",\"extra\":true}\n",
            "{\"version\":1,\"command\":\"share\"}\n",
            "[]\n",
            "{}",
        ] {
            assert!(request_relay(request).is_err(), "{request}");
        }
        assert!(request_relay(&format!("{}\n", " ".repeat(8192))).is_err());
    }

    #[test]
    fn exposes_the_pin_only_when_ready() {
        let ready = response(&ShareState::Ready {
            code: "123456".into(),
            expires_at_ms: 60000,
        });
        assert_eq!(ready["version"], 1);
        assert_eq!(ready["state"], "ready");
        assert_eq!(ready["code"], "123456");
        for state in [
            ShareState::Preparing,
            ShareState::Paired,
            ShareState::Closed,
        ] {
            assert!(response(&state).get("code").is_none());
        }
    }
}

mod socket {
    use super::*;

    #[tokio::test]
    async fn replaces_stale_sockets_with_owner_only_access() {
        let directory = tempfile::tempdir().unwrap();
        let listener = listen(directory.path()).unwrap();
        let path = directory.path().join("control.sock");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(listener);
        let listener = listen(directory.path()).unwrap();
        drop(listener);
    }

    #[tokio::test]
    async fn preserves_non_socket_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        std::fs::write(&path, b"preserve").unwrap();
        assert!(listen(directory.path()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
    }
}

mod sharing {
    use super::*;

    #[tokio::test]
    async fn missing_service_does_not_create_a_profile() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("not-started");
        let result =
            share(["--data-dir".into(), profile.clone().into_os_string()].into_iter()).await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Host is not running")
        );
        assert!(!profile.exists());
    }

    #[tokio::test]
    async fn rejects_service_flags_and_invalid_origins() {
        for arguments in [
            vec!["--internet"],
            vec!["--bootstrap"],
            vec!["--data-dir", "relative"],
            vec!["--pairing-service", "http://example.invalid"],
            vec![
                "--pairing-service",
                "https://link.sailry.dev",
                "--pairing-service",
                "https://link.sailry.dev",
            ],
        ] {
            assert!(share(arguments.into_iter().map(Into::into)).await.is_err());
        }
    }
}
