use super::*;
use serde_json::json;
use std::fs;

fn fixture(servers: serde_json::Value) -> (tempfile::TempDir, Resources) {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    fs::create_dir(base.join("node")).unwrap();
    fs::create_dir_all(base.join("source/bin/sub")).unwrap();
    fs::write(base.join("source/plugin.json"), json!({"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "example"}).to_string()).unwrap();
    fs::write(
        base.join("source/mcp.json"),
        json!({"$schema": mcp::SCHEMA, "mcpServers": servers}).to_string(),
    )
    .unwrap();
    fs::write(base.join("source/bin/server"), "fixture executable").unwrap();
    let host = Host::new(Some(base.join("node")));
    let info = host.install(&base.join("source"), "", "example").unwrap();
    (
        directory,
        Resources::new(host, vec![info], BTreeMap::new(), CancellationToken::new()),
    )
}

#[tokio::test]
async fn resolves_roots_with_literal_arguments() {
    let (_directory, resources) = fixture(json!({
        "relative": {"type": "stdio", "command": "./bin/server", "cwd": "./bin/sub/..", "args": ["${PLUGIN_ROOT}", "${PLUGIN_DATA}", "../../opaque", "${HOME}"], "env": {"PUBLIC": "${PLUGIN_ROOT}|${PLUGIN_DATA}"}},
        "default": {"type": "stdio", "command": "runtime"},
        "data": {"type": "stdio", "command": "runtime", "cwd": "${PLUGIN_DATA}"}
    }));
    let package = resources.directory(0).await.unwrap();
    let configured = resources.mcp(&crate::process::Environment::ambient()).await;
    assert_eq!(configured.len(), 3);
    for config in configured {
        let Launch::Stdio {
            command,
            cwd,
            args,
            env,
            ..
        } = config.launch.unwrap()
        else {
            panic!("stdio expected")
        };
        assert_eq!(env["PLUGIN_ROOT"], package.path.to_str().unwrap());
        match config.name.as_str() {
            "relative" => {
                assert_eq!(command, package.path.join("bin/server"));
                assert_eq!(cwd, package.path.join("bin"));
                assert_eq!(
                    args,
                    [
                        &env["PLUGIN_ROOT"],
                        &env["PLUGIN_DATA"],
                        "../../opaque",
                        "${HOME}"
                    ]
                );
            }
            "default" => assert_eq!(cwd, package.path),
            "data" => assert_eq!(cwd, PathBuf::from(&env["PLUGIN_DATA"])),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn fills_only_declared_header_slots() {
    for prefix in [None, Some("Bearer ")] {
        let (directory, resources) = fixture(json!({
            "service":{"type":"streamable-http","url":"https://example.com/mcp","headers":{"Authorization":""}},
            "legacy":{"type":"sse","url":"https://example.com/events","headers":{"authorization":""}},
            "other":{"type":"streamable-http","url":"https://other.example.com/mcp","headers":{"Authorization":""}}
        }));
        let source = directory.path().canonicalize().unwrap().join("source");
        fs::create_dir(source.join("dev.sailry.platform")).unwrap();
        fs::write(
            source.join("plugin.json"),
            json!({
                "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name":"example",
                "extensions":{"dev.sailry.platform":{
                    "api_version":"v1","settings_schema":"dev.sailry.platform/settings.json"
                }}
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            source.join("dev.sailry.platform/settings.json"),
            json!({
                "$schema":sailry_protocol::plugin::settings::SCHEMA,
                "type":"object","additionalProperties":false,
                "properties":{
                    "token":{"type":"string","x-sailry-secret":{
                        "server":"service","header":"Authorization","prefix":prefix
                    }},
                    "legacy_token":{"type":"string","x-sailry-secret":{
                        "server":"legacy","header":"authorization","prefix":prefix
                    }}
                }
            })
            .to_string(),
        )
        .unwrap();
        let host = resources.host.clone();
        let info = host.install(&source, "", "example").unwrap();
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        assert!(info.settings.is_some());
        let settings = crate::plugins::settings::Resolved {
            values: BTreeMap::new(),
            secrets: BTreeMap::from([
                (
                    "token".into(),
                    sailry_protocol::Secret::new("synthetic-token".into()),
                ),
                (
                    "legacy_token".into(),
                    sailry_protocol::Secret::new("synthetic-legacy".into()),
                ),
            ]),
            authorizations: BTreeMap::new(),
            mcp: None,
            github: None,
        };
        let resources = Resources::new(
            host,
            vec![info],
            BTreeMap::from([("example".into(), Ok(settings))]),
            CancellationToken::new(),
        );
        let configured = resources.mcp(&crate::process::Environment::ambient()).await;
        assert_eq!(configured.len(), 3);
        for config in configured {
            let Launch::Http {
                url, headers, sse, ..
            } = config.launch.unwrap()
            else {
                panic!("HTTP launch expected")
            };
            let (expected_url, header, token, expected_sse) = match config.name.as_str() {
                "service" => (
                    "https://example.com/mcp",
                    "Authorization",
                    "synthetic-token",
                    false,
                ),
                "legacy" => (
                    "https://example.com/events",
                    "authorization",
                    "synthetic-legacy",
                    true,
                ),
                "other" => {
                    assert_eq!(url, "https://other.example.com/mcp");
                    assert_eq!(headers["Authorization"], "");
                    assert!(!sse);
                    continue;
                }
                _ => unreachable!(),
            };
            assert_eq!(url, expected_url);
            assert_eq!(sse, expected_sse);
            assert_eq!(headers[header], format!("{}{token}", prefix.unwrap_or("")));
        }
    }
}

#[tokio::test]
async fn validates_paths_and_invalidates_cache() {
    let (_directory, resources) = fixture(json!({
        "valid": {"type": "stdio", "command": "runtime"},
        "escape": {"type": "stdio", "command": "./../server"},
        "directory": {"type": "stdio", "command": "./bin"},
        "cwd": {"type": "stdio", "command": "runtime", "cwd": "${PLUGIN_DATA}/.."}
    }));
    let configured = resources.mcp(&crate::process::Environment::ambient()).await;
    assert_eq!(
        configured
            .iter()
            .filter(|config| config.launch.is_ok())
            .count(),
        1
    );
    let package = resources.directory(0).await.unwrap();
    fs::write(package.path.join("mcp.json"), "{}").unwrap();
    assert!(
        resources
            .mcp(&crate::process::Environment::ambient())
            .await
            .iter()
            .all(|config| config.launch.is_err())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_unconfined_roots() {
    let (directory, resources) = fixture(
        json!({"service": {"type": "stdio", "command": "runtime", "cwd": "${PLUGIN_DATA}/linked"}}),
    );
    let base = directory.path().canonicalize().unwrap();
    fs::create_dir_all(base.join("node/plugins/data/example")).unwrap();
    std::os::unix::fs::symlink(
        base.join("source"),
        base.join("node/plugins/data/example/linked"),
    )
    .unwrap();
    assert!(
        resources.mcp(&crate::process::Environment::ambient()).await[0]
            .launch
            .is_err()
    );
    fs::rename(
        base.join("node/plugins/data/example"),
        base.join("retained-data"),
    )
    .unwrap();
    std::os::unix::fs::symlink(base.join("source"), base.join("node/plugins/data/example"))
        .unwrap();
    assert!(
        resources.mcp(&crate::process::Environment::ambient()).await[0]
            .launch
            .is_err()
    );
}

#[tokio::test]
async fn confines_cli_bindings_and_resolves_private_headers() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::new(Some(directory.path().canonicalize().unwrap()));
    let info = host.install_bundled("github").unwrap();
    let resources = Resources::new(
        host.clone(),
        vec![info.clone()],
        BTreeMap::new(),
        CancellationToken::new(),
    );
    let package = resources.directory(0).await.unwrap();
    let settings = crate::plugins::settings::Resolved {
        values: BTreeMap::new(),
        secrets: BTreeMap::from([(
            "token".into(),
            sailry_protocol::Secret::new("transient-cli-token".into()),
        )]),
        authorizations: BTreeMap::new(),
        mcp: None,
        github: Some(crate::plugins::github::login::Identity {
            login: "fixture-user".into(),
            source: "keyring".into(),
        }),
    };
    let config = Server::Http {
        url: "https://api.githubcopilot.com/mcp/".into(),
        headers: BTreeMap::from([("Authorization".into(), String::new())]),
    };
    let Launch::Http {
        headers,
        github: Some(binding),
        ..
    } = prepare(&host, &package, &info, "github", config.clone(), &settings).unwrap()
    else {
        panic!("bound HTTP launch expected")
    };
    assert_eq!(headers["Authorization"], "Bearer transient-cli-token");
    assert_eq!(binding.identity.login, "fixture-user");
    assert_eq!(binding.fingerprint, blake3::hash(b"transient-cli-token"));
    let mut renamed = info.clone();
    renamed.summary.name = "repository-tools".into();
    assert!(matches!(
        prepare(
            &host,
            &package,
            &renamed,
            "github",
            config.clone(),
            &settings,
        )
        .unwrap(),
        Launch::Http {
            github: Some(_),
            ..
        }
    ));
    renamed.extension.as_mut().unwrap().credential_source = None;
    for (declaration, server, url) in [
        (&renamed, "github", "https://api.githubcopilot.com/mcp/"),
        (&info, "other", "https://api.githubcopilot.com/mcp/"),
        (&info, "github", "https://changed.invalid/mcp/"),
    ] {
        let mut changed = config.clone();
        let Server::Http { url: target, .. } = &mut changed else {
            unreachable!()
        };
        *target = url.into();
        assert_eq!(
            prepare(&host, &package, declaration, server, changed, &settings)
                .err()
                .unwrap()
                .code,
            ErrorCode::PermissionDenied
        );
    }
}

#[cfg(unix)]
mod github {
    use super::*;
    use crate::process::environment::{EnvironmentFixture, Source};
    use std::{os::unix::fs::PermissionsExt, time::Duration};

    const TOKEN: &str = "synthetic-automatic-gh-token";

    struct Fixture {
        directory: tempfile::TempDir,
        environment: crate::process::Environment,
        host: Host,
        info: Info,
    }

    impl Fixture {
        async fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let home = directory.path().canonicalize().unwrap();
            let shell = home.join("login-shell");
            for (path, source) in [
                (
                    shell.clone(),
                    "#!/bin/sh\nfor command do :; done\neval \"$command\"\n",
                ),
                (
                    home.join("gh"),
                    concat!(
                        "#!/bin/sh\n",
                        "printf '%s\\n' \"$*\" >> \"$HOME/gh-calls\"\n",
                        "case \"$*\" in\n",
                        "'auth status --active --hostname github.com --json hosts')\n",
                        "  printf '{\"hosts\":{\"github.com\":[{\"active\":true,\"host\":\"github.com\",\"login\":\"fixture-user\",\"state\":\"success\",\"tokenSource\":\"keyring\"}]}}';;\n",
                        "'auth token --hostname github.com --user fixture-user')\n",
                        "  printf 'synthetic-automatic-gh-token';;\n",
                        "*) exit 2;;\n",
                        "esac\n",
                    ),
                ),
            ] {
                fs::write(&path, source).unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            let environment =
                crate::process::Environment::capture(Source::Fixture(EnvironmentFixture {
                    home: home.clone(),
                    shell,
                    environment: [("PATH".into(), home.clone().into_os_string())].into(),
                    timeout: Duration::from_secs(2),
                }))
                .await;
            fs::create_dir(home.join("node")).unwrap();
            let host = Host::new(Some(home.join("node")));
            let info = host.install_bundled("github").unwrap();
            assert!(info.issues.is_empty(), "{:?}", info.issues);
            Self {
                directory,
                environment,
                host,
                info,
            }
        }

        fn resources(&self, token: Option<&str>, endpoint: Option<&str>) -> Resources {
            let settings = crate::plugins::settings::Resolved {
                values: BTreeMap::new(),
                secrets: token
                    .map(|token| {
                        BTreeMap::from([(
                            "token".into(),
                            sailry_protocol::Secret::new(token.into()),
                        )])
                    })
                    .unwrap_or_default(),
                authorizations: BTreeMap::new(),
                mcp: endpoint.map(|url| sailry_protocol::plugin::mcp::Configuration {
                    schema: mcp::SCHEMA.into(),
                    servers: BTreeMap::from([(
                        "github".into(),
                        Server::Http {
                            url: url.into(),
                            headers: BTreeMap::from([("Authorization".into(), String::new())]),
                        },
                    )]),
                }),
                github: None,
            };
            Resources::new(
                self.host.clone(),
                vec![self.info.clone()],
                BTreeMap::from([("github".into(), Ok(settings))]),
                CancellationToken::new(),
            )
        }

        fn calls(&self, action: &str) -> usize {
            fs::read_to_string(self.directory.path().join("gh-calls"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.starts_with(&format!("auth {action} ")))
                .count()
        }
    }

    #[tokio::test]
    async fn uses_current_account_without_saved_binding() {
        let fixture = Fixture::new().await;
        let resources = fixture.resources(None, None);
        let mut configured = resources.mcp(&fixture.environment).await;
        assert_eq!(configured.len(), 1);
        let launch = configured.pop().unwrap();
        assert_eq!(launch.plugin, "github");
        assert_eq!(launch.name, "github");
        let Launch::Http {
            url,
            headers,
            authorization,
            github: Some(binding),
            ..
        } = launch.launch.unwrap()
        else {
            panic!("automatic GitHub CLI launch expected")
        };
        assert_eq!(url, "https://api.githubcopilot.com/mcp/");
        assert_eq!(headers["Authorization"], format!("Bearer {TOKEN}"));
        assert_eq!(binding.identity.login, "fixture-user");
        assert_eq!(binding.fingerprint, blake3::hash(TOKEN.as_bytes()));
        assert!(authorization.is_none());
        assert_eq!(fixture.calls("token"), 1);
        assert!(fixture.calls("status") > 0);
        let captured = resources.settings["github"].as_ref().unwrap();
        assert!(captured.github.is_none());
        assert!(captured.secrets.is_empty());
    }

    #[tokio::test]
    async fn keeps_manual_token_without_cli_calls() {
        let fixture = Fixture::new().await;
        let resources = fixture.resources(Some("synthetic-manual-token"), None);
        let mut configured = resources.mcp(&fixture.environment).await;
        assert_eq!(configured.len(), 1);
        let Launch::Http {
            headers,
            github,
            authorization,
            ..
        } = configured.pop().unwrap().launch.unwrap()
        else {
            panic!("manual GitHub launch expected")
        };
        assert_eq!(headers["Authorization"], "Bearer synthetic-manual-token");
        assert!(github.is_none());
        assert!(authorization.is_none());
        assert_eq!(fixture.calls("status"), 0);
        assert_eq!(fixture.calls("token"), 0);
    }

    #[tokio::test]
    async fn keeps_configured_header_without_cli_calls() {
        let fixture = Fixture::new().await;
        for header in ["Authorization", "authorization"] {
            let mut resources = fixture.resources(None, Some("https://api.githubcopilot.com/mcp/"));
            let settings = resources
                .settings
                .get_mut("github")
                .unwrap()
                .as_mut()
                .unwrap();
            let Server::Http { headers, .. } = settings
                .mcp
                .as_mut()
                .unwrap()
                .servers
                .get_mut("github")
                .unwrap()
            else {
                panic!("GitHub HTTP configuration expected")
            };
            *headers = BTreeMap::from([(header.into(), "Bearer synthetic-header-token".into())]);
            let mut configured = resources.mcp(&fixture.environment).await;
            assert_eq!(configured.len(), 1);
            let Launch::Http {
                headers,
                github,
                authorization,
                ..
            } = configured.pop().unwrap().launch.unwrap()
            else {
                panic!("configured GitHub launch expected")
            };
            assert_eq!(headers[header], "Bearer synthetic-header-token");
            assert!(github.is_none());
            assert!(authorization.is_none());
            assert_eq!(fixture.calls("status"), 0);
            assert_eq!(fixture.calls("token"), 0);
        }
    }

    #[tokio::test]
    async fn rejects_other_hosts_before_launch() {
        let fixture = Fixture::new().await;
        for url in [
            "https://changed.invalid/mcp/",
            "https://api.githubcopilot.com.changed.invalid/mcp/",
        ] {
            let resources = fixture.resources(None, Some(url));
            let mut configured = resources.mcp(&fixture.environment).await;
            assert_eq!(configured.len(), 1);
            let error = configured
                .pop()
                .unwrap()
                .launch
                .err()
                .expect("GitHub CLI credentials require the official endpoint");
            assert_eq!(error.code, ErrorCode::PermissionDenied);
            assert!(!serde_json::to_string(&error).unwrap().contains(TOKEN));
        }
    }
}
