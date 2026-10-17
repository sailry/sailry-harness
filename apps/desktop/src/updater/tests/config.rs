use super::super::config::{self, Channel, DOWNLOADS, RELEASES};

#[test]
fn bundled_contract_matches_publisher() {
    assert!(!config::trusted_keys().unwrap().is_empty());
    assert_eq!(
        RELEASES,
        "https://api.github.com/repos/sailry/sailry-harness/releases"
    );
    assert_eq!(
        DOWNLOADS,
        "https://github.com/sailry/sailry-harness/releases/download"
    );
    assert!(!Channel::Stable.accepts(&semver::Version::parse("0.1.0-alpha.1").unwrap()));
    assert!(Channel::Experience.accepts(&semver::Version::parse("0.1.0").unwrap()));
}

#[test]
fn rejects_missing_invalid_and_weak_keys() {
    assert!(config::parse_keys("").is_err());
    assert!(config::parse_keys("not-base64").is_err());
    assert!(config::parse_keys("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").is_err());
}
