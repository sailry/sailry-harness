use super::*;
use serde_json::json;

#[test]
fn keeps_implementation_updates_within_the_owner() {
    let imported = |byte: &str| Origin::Imported {
        digest: byte.repeat(64),
    };
    let git = |repository: &str, path: &str| Origin::Git {
        repository: repository.into(),
        path: path.into(),
    };
    let official = git("https://github.com/sailry/sailry-plugins.git", "example");
    assert!(same_origin("example", &imported("a"), &imported("b")));
    assert!(same_origin("example", &Origin::Bundled, &official));
    assert!(same_origin("example", &official, &Origin::Bundled));
    for changed in [
        imported("a"),
        git("https://github.com/other/plugins.git", "example"),
        git("https://github.com/sailry/sailry-plugins.git", "other"),
        Origin::Worktree {
            worktree: sailry_protocol::WorktreeId::new(),
            path: "example".into(),
        },
    ] {
        assert!(!same_origin("example", &Origin::Bundled, &changed));
        assert!(!same_origin("example", &official, &changed));
    }
}

#[test]
fn trusts_installed_models_without_approval() {
    let mut provider: Provider = serde_json::from_value(json!({
        "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"Example",
        "api":"plugin","authentication":"plugin","endpoint":"https://example.invalid",
        "enabled":true,"models":[],"default_model":"","credential":null,
        "preset":{"source":{"kind":"package","data":{"name":"example"}},"id":"example","route":null},
        "binding":{"package":{"name":"example","digest":"a".repeat(64),"settings_revision":0},"origin":{"kind":"imported","digest":"a".repeat(64)}}
    })).unwrap();
    for scope in [Scope::Messages, Scope::Credentials] {
        check(&provider, scope).unwrap();
    }
    provider.consent = Some(Box::new(
        serde_json::from_value(json!({
            "package":{"name":"example","digest":"b".repeat(64),"settings_revision":0},
            "origin":null,"scopes":{"messages":false,"credentials":false}
        }))
        .unwrap(),
    ));
    for scope in [Scope::Messages, Scope::Credentials] {
        check(&provider, scope).unwrap();
    }
    provider
        .binding
        .as_mut()
        .unwrap()
        .package
        .as_mut()
        .unwrap()
        .name = "other".into();
    assert_eq!(
        check(&provider, Scope::Messages).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    provider
        .binding
        .as_mut()
        .unwrap()
        .package
        .as_mut()
        .unwrap()
        .name = "example".into();
    provider.binding.as_mut().unwrap().origin = None;
    assert_eq!(
        check(&provider, Scope::Credentials).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}
