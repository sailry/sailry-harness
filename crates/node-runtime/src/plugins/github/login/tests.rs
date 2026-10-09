#![cfg(unix)]
use super::*;
use crate::process::environment::{EnvironmentFixture, Source};
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    directory: tempfile::TempDir,
    environment: Environment,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().canonicalize().unwrap();
        let shell = home.join("login-shell");
        for (path, script) in [
            (
                &shell,
                "#!/bin/sh\nfor command do :; done\neval \"$command\"\n",
            ),
            (
                &home.join("gh"),
                concat!(
                    "#!/bin/sh\n",
                    "case \"$*\" in\n",
                    "'auth status --active --hostname github.com --json hosts')\n",
                    "  if test -f \"$HOME/logout\"; then printf '{\"hosts\":{}}'; exit 0; fi\n",
                    "  login=fixture-user; state=success\n",
                    "  if test -f \"$HOME/switch\"; then login=other-user; fi\n",
                    "  if test -f \"$HOME/invalid\"; then state=error; fi\n",
                    "  printf '{\"hosts\":{\"github.com\":[{\"active\":true,\"host\":\"github.com\",\"login\":\"%s\",\"state\":\"%s\",\"tokenSource\":\"keyring\"}]}}' \"$login\" \"$state\";;\n",
                    "'auth token --hostname github.com --user fixture-user')\n",
                    "  if test -f \"$HOME/rotate\"; then printf 'new-private-token'; else printf 'original-private-token'; fi;;\n",
                    "*) exit 2;;\n",
                    "esac\n",
                ),
            ),
        ] {
            std::fs::write(path, script).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let environment = Environment::capture(Source::Fixture(EnvironmentFixture {
            home: home.clone(),
            shell,
            environment: [("PATH".into(), home.into_os_string())].into(),
            timeout: Duration::from_secs(2),
        }))
        .await;
        Self {
            directory,
            environment,
        }
    }
    fn set(&self, flag: &str) {
        std::fs::write(self.directory.path().join(flag), "").unwrap();
    }
    fn clear(&self, flag: &str) {
        std::fs::remove_file(self.directory.path().join(flag)).unwrap();
    }
}

#[tokio::test]
async fn rejects_changed_accounts_and_rotated_credentials() {
    let fixture = Fixture::new().await;
    let stop = CancellationToken::new();
    let identity = import(&fixture.environment, &stop).await.unwrap();
    assert_eq!(identity.login, "fixture-user");
    let token = credential(&fixture.environment, &identity, &stop)
        .await
        .unwrap();
    let binding = Binding {
        identity,
        fingerprint: blake3::hash(token.expose().as_bytes()),
    };
    binding.validate(&fixture.environment, &stop).await.unwrap();
    for flag in ["logout", "switch", "invalid", "rotate"] {
        fixture.set(flag);
        assert_eq!(
            binding
                .validate(&fixture.environment, &stop)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        fixture.clear(flag);
    }
    fixture.set("rotate");
    let refreshed = credential(&fixture.environment, &binding.identity, &stop)
        .await
        .unwrap();
    assert_eq!(refreshed.expose(), "new-private-token");
    assert_ne!(
        blake3::hash(refreshed.expose().as_bytes()),
        binding.fingerprint
    );
}

#[tokio::test]
async fn checks_json_authentication_instead_of_exit_status() {
    let fixture = Fixture::new().await;
    fixture.set("invalid");
    let (available, identity) = status(&fixture.environment, &CancellationToken::new())
        .await
        .unwrap();
    assert!(available);
    assert!(identity.is_none());
}
