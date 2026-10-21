use super::*;

fn bound() -> Bound {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../../plugins/google-workspace/plugin.json"
    ))
    .unwrap();
    let declaration: Declaration = serde_json::from_value(
        manifest["extensions"]["dev.sailry.platform"]["connectors"][0].clone(),
    )
    .unwrap();
    Bound {
        origin: Some(plugin::Origin::Online {
            source: plugin::skills::Resolved {
                repository: "https://github.com/owner/plugins.git".into(),
                git_ref: "main".into(),
                commit: "1".repeat(40),
            },
            path: "account".into(),
        }),
        auth: declaration.auth[0].clone(),
        origins: declaration.origins,
        inject: declaration.inject,
        targets: BTreeMap::new(),
    }
}

#[test]
fn preserves_updates_from_the_same_publisher() {
    let previous = bound();
    let mut current = previous.clone();
    let Some(plugin::Origin::Online { source, .. }) = &mut current.origin else {
        unreachable!()
    };
    source.commit = "2".repeat(40);
    source.git_ref = "release".into();
    assert!(same("example", Some(&previous), Some(&current)));
    assert!(changes("example", Some(&previous), Some(&current)).is_none());
    for changed in ["repository", "path"] {
        let mut current = current.clone();
        let Some(plugin::Origin::Online { source, path }) = &mut current.origin else {
            unreachable!()
        };
        if changed == "repository" {
            source.repository = "https://github.com/other/plugins.git".into();
        } else {
            *path = "other".into();
        }
        assert!(!same("example", Some(&previous), Some(&current)));
        assert!(
            changes("example", Some(&previous), Some(&current))
                .unwrap()
                .changes
                .contains(&Change::Source)
        );
    }
}

#[test]
fn detects_authentication_changes_within_the_publisher() {
    let previous = bound();
    for changed in ["issuer", "client", "scopes"] {
        let mut current = previous.clone();
        let Auth::Oauth2 {
            token,
            client,
            scopes,
            ..
        } = &mut current.auth
        else {
            unreachable!()
        };
        match changed {
            "issuer" => token.push_str("/changed"),
            "client" => {
                *client = Client::Bundled {
                    bundled: sailry_protocol::plugin::connector::PublicClient {
                        client_id: "other".into(),
                    },
                }
            }
            _ => scopes.push("additional.read".into()),
        }
        assert!(!same("example", Some(&previous), Some(&current)));
        assert!(changes("example", Some(&previous), Some(&current)).is_some());
    }
}
