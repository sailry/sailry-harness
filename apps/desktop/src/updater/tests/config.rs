use super::super::{
    config::{self, Channel, PREVIEW_SOURCE, STABLE_SOURCE},
    manifest,
};

#[test]
fn bundled_contract_matches_publisher() {
    let keys = config::trusted_keys().unwrap();
    assert!(!keys.is_empty());
    assert_eq!(Channel::Stable.sources(), &[STABLE_SOURCE]);
    assert_eq!(
        Channel::Experience.sources(),
        &[STABLE_SOURCE, PREVIEW_SOURCE]
    );

    for (channel, bytes) in [
        (
            Channel::Stable,
            include_bytes!("../../../../../update-v1.json").as_slice(),
        ),
        (
            Channel::Experience,
            include_bytes!("../../../../../update-v1-preview.json").as_slice(),
        ),
    ] {
        for release in manifest::parse(bytes, &keys).unwrap() {
            let version = semver::Version::parse(&release.version).unwrap();
            assert!(channel.accepts(&version));
        }
    }
}

#[test]
fn rejects_missing_invalid_and_weak_keys() {
    assert!(config::parse_keys("").is_err());
    assert!(config::parse_keys("not-base64").is_err());
    assert!(config::parse_keys("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").is_err());
}
