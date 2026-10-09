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
    let configured = resources.mcp().await;
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
        };
        let resources = Resources::new(
            host,
            vec![info],
            BTreeMap::from([("example".into(), Ok(settings))]),
            CancellationToken::new(),
        );
        let configured = resources.mcp().await;
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
    let configured = resources.mcp().await;
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
            .mcp()
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
    assert!(resources.mcp().await[0].launch.is_err());
    fs::rename(
        base.join("node/plugins/data/example"),
        base.join("retained-data"),
    )
    .unwrap();
    std::os::unix::fs::symlink(base.join("source"), base.join("node/plugins/data/example"))
        .unwrap();
    assert!(resources.mcp().await[0].launch.is_err());
}
