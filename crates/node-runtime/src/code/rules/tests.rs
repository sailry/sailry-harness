use super::*;
use sha2::{Digest, Sha256};
#[test]
fn preserves_upstream_candidates_and_globs() {
    let rules = Rules::load().unwrap();
    let rust = rules.language("src/main.rs", "").unwrap();
    assert_eq!(rust.id, "rust");
    assert_eq!(rust.servers.first().unwrap(), "rust-analyzer");
    let python = rules.language("main.py", "").unwrap();
    assert_eq!(python.servers, vec!["ty", "ruff", "jedi", "pylsp", "zuban"]);
    assert_eq!(rules.language("Dockerfile", "").unwrap().name, "dockerfile");
    assert_eq!(
        rules
            .language("script", "#!/usr/bin/env python\n")
            .unwrap()
            .name,
        "python"
    );
    assert_eq!(
        rules
            .language("script", "#!/usr/bin/env -S python -u\n")
            .unwrap()
            .name,
        "python"
    );
    assert_eq!(
        rules.package("rust-analyzer").unwrap().name,
        "rust-analyzer"
    );
}
#[test]
fn verifies_original_snapshot_bytes() {
    let source: Value = serde_json::from_str(include_str!("../upstream/SOURCE.json")).unwrap();
    for (key, bytes, license) in [
        (
            "helix",
            include_bytes!("../upstream/languages.toml").as_slice(),
            include_bytes!("../upstream/helix-LICENSE").as_slice(),
        ),
        (
            "mason",
            include_bytes!("../upstream/registry.json.zip").as_slice(),
            include_bytes!("../upstream/mason-LICENSE").as_slice(),
        ),
    ] {
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            source[key]["sha256"].as_str().unwrap()
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(license)),
            source[key]["license_sha256"].as_str().unwrap()
        );
    }
}
#[test]
fn ignores_unknown_fields_and_bad_individual_entries() {
    let source = "[[language]]\nname='valid'\nfile-types=['ok']\nlanguage-servers=['server']\nunknown='ignored'\n[[language]]\nfile-types=['bad']\n[language-server.server]\ncommand='tool'\nunknown={future=true}\n[language-server.invalid]\nargs=['no-command']\n";
    let rules = Rules::parse(source, include_bytes!("../upstream/registry.json.zip")).unwrap();
    assert_eq!(
        rules.language("file.ok", "").unwrap().servers,
        vec!["server"]
    );
    assert_eq!(rules.languages.len(), 1);
    assert_eq!(rules.servers.len(), 1);
}

#[test]
#[ignore = "The snapshot update script supplies an isolated candidate directory"]
fn parses_candidate() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("SAILRY_RULES_CANDIDATE").expect("snapshot candidate directory required"),
    );
    let languages = std::fs::read_to_string(directory.join("languages.toml")).unwrap();
    let registry = std::fs::read(directory.join("registry.json.zip")).unwrap();
    let rules = Rules::parse(&languages, &registry).unwrap();
    assert!(!rules.languages.is_empty());
    assert!(!rules.servers.is_empty());
    assert!(!rules.packages.is_empty());
    let source: Value =
        serde_json::from_slice(&std::fs::read(directory.join("SOURCE.json")).unwrap()).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(languages.as_bytes())),
        source["helix"]["sha256"].as_str().unwrap()
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(registry)),
        source["mason"]["sha256"].as_str().unwrap()
    );
}
