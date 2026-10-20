use super::*;
use serde_json::json;

#[test]
fn rejects_inactive_accounts() {
    let status = CliStatus {
        argv: vec!["status".into()],
        account: "/login".into(),
        condition: Some(sailry_protocol::plugin::model_auth::Condition {
            pointer: "/active".into(),
            values: vec![json!(true)],
        }),
    };
    assert_eq!(
        account(br#"{"login":"fixture-user","active":true}"#, &status).unwrap(),
        "fixture-user"
    );
    assert!(account(br#"{"login":"other-user","active":false}"#, &status).is_err());
    assert!(account(br#"{"login":"$(unsafe)","active":true}"#, &status).is_err());
}

#[cfg(unix)]
mod process {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn executable(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credential-cli");
        std::fs::write(&path, source).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        (directory, path)
    }
    #[tokio::test]
    async fn preserves_literal_arguments_and_clears_environment() {
        let (_directory, path) =
            executable("#!/bin/sh\nprintf '%s|%s|%s' \"$1\" \"$DECLARED\" \"${GH_TOKEN-unset}\"\n");
        let environment = crate::process::Environment::ambient();
        let output = run(
            &path,
            &["$(touch never); literal".into()],
            &BTreeMap::from([("DECLARED".into(), "public".into())]),
            &environment,
            1000,
            1024,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(&**output, b"$(touch never); literal|public|unset");
    }
    #[tokio::test]
    async fn bounds_output_and_execution() {
        let (_directory, path) = executable("#!/bin/sh\nwhile :; do printf 'output'; done\n");
        let environment = crate::process::Environment::ambient();
        assert!(
            run(
                &path,
                &[],
                &BTreeMap::new(),
                &environment,
                1000,
                16,
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
        let (_directory, path) = executable("#!/bin/sh\nwhile :; do :; done\n");
        assert!(
            run(
                &path,
                &[],
                &BTreeMap::new(),
                &environment,
                20,
                1024,
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
    }
}
