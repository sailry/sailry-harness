use super::*;
use serde_json::json;

fn package(source: Value, bin: &str) -> Package {
    Package {
        name: "fixture".into(),
        source,
        bin: BTreeMap::from([("fixture".into(), bin.into())]),
        homepage: None,
        licenses: vec!["MIT".into()],
    }
}

#[test]
fn freezes_the_literal_asset_and_complete_consent() {
    let environment = crate::process::Environment::ambient();
    let entry = package(
        json!({"id":"pkg:github/example/server@1.2.3","asset":{"file":"server-{{ version }}.gz"}}),
        "server-{{ version }}",
    );
    let plan = build(&entry, &environment).unwrap();
    assert_eq!(
        plan.consent.urls,
        vec!["https://github.com/example/server/releases/download/1.2.3/server-1.2.3.gz"]
    );
    assert_eq!(plan.consent.licenses, vec!["MIT"]);
    assert_eq!(plan.consent.revision.len(), 64);
    let mut changed = entry.clone();
    changed.licenses.push("Apache-2.0".into());
    assert_ne!(
        build(&changed, &environment).unwrap().consent.revision,
        plan.consent.revision
    );
    let mut rules = Rules {
        languages: vec![],
        servers: BTreeMap::new(),
        packages: vec![entry],
    };
    let mut forged = plan.consent.clone();
    forged.urls[0] = "https://example.invalid/replacement".into();
    assert!(matches!(
        prepare(&rules, &forged, &environment),
        Err(Fault {
            code: sailry_protocol::ErrorCode::RevisionConflict,
            ..
        })
    ));
    rules.packages[0].source["asset"]["file"] = json!("changed.gz");
    assert!(prepare(&rules, &plan.consent, &environment).is_err());
}

#[test]
fn rejects_unsupported_rules_without_inventing_an_install() {
    let environment = crate::process::Environment::ambient();
    for source in [
        json!({"id":"pkg:github/example/server@1","asset":{"file":"server.gz"},"build":"make"}),
        json!({"id":"pkg:github/example/server@1?build=true","asset":{"file":"server.gz"}}),
        json!({"id":"pkg:github/example/server@1","asset":{"file":"{{ unknown }}.gz"}}),
        json!({"id":"pkg:generic/server@1","download":{"files":{"server":"http://example.invalid/server"}}}),
    ] {
        let result = super::package(&package(source, "server"), &environment);
        assert!(result.install.is_none());
        assert!(result.unavailable.is_some());
    }
    for path in [
        "../server",
        "/server",
        "a/../server",
        "server.exe:stream",
        "a\\server",
        ".",
        "",
    ] {
        assert!(relative(path).is_err());
    }
    assert!(component(".install-test").is_ok());
    assert!(relative("archive/bin/server").is_ok());
}

#[test]
fn resolves_platform_assets_from_the_original_registry() {
    let fixture = fixture();
    assert!(
        fixture
            .asset_url
            .starts_with("https://github.com/rust-lang/rust-analyzer/releases/download/")
    );
    assert!(fixture.release_url.ends_with(&fixture.version));
}

#[test]
fn excludes_unsupported_original_archive_formats() {
    let rules = Rules::load().unwrap();
    let entry = rules
        .packages
        .iter()
        .find(|package| package.name == "dhall-lsp")
        .unwrap();
    let environment = crate::process::Environment::ambient();
    let result = super::package(entry, &environment);
    assert!(result.install.is_none());
    assert!(result.unavailable.is_some());
    assert_eq!(result.name, "dhall-lsp");
    assert!(result.homepage.is_some());
    for name in [
        "server.tar.bz2",
        "server.tar.zst",
        "server.7z",
        "server.xz",
        "server.tar",
    ] {
        assert!(archive(name).is_err());
    }
    for name in [
        "server.zip",
        "server.tar.gz",
        "server.tgz",
        "server.tar.xz",
        "server.gz",
        "server",
        "server.exe",
        "server.sh",
        "server.AppImage",
    ] {
        assert!(archive(name).is_ok());
    }
}
