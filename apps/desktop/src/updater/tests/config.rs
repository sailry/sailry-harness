use super::super::config::{self, Channel, DOWNLOADS, RELEASES};

#[test]
fn publisher_contract() {
    for (scenario, check) in [
        (
            "bundled_contract_matches_publisher",
            (|_scenario: &str| {
                assert!(
                    !config::trusted_keys().unwrap().is_empty(),
                    "bundled contract matches publisher"
                );
                assert_eq!(
                    RELEASES, "https://api.github.com/repos/sailry/sailry-harness/releases",
                    "bundled contract matches publisher"
                );
                assert_eq!(
                    DOWNLOADS, "https://github.com/sailry/sailry-harness/releases/download",
                    "bundled contract matches publisher"
                );
                assert!(
                    !Channel::Stable.accepts(&semver::Version::parse("0.1.0-alpha.1").unwrap()),
                    "bundled contract matches publisher"
                );
                assert!(
                    Channel::Experience.accepts(&semver::Version::parse("0.1.0").unwrap()),
                    "bundled contract matches publisher"
                );
            }) as fn(&str),
        ),
        (
            "rejects_missing_invalid_and_weak_keys",
            (|_scenario: &str| {
                assert!(
                    config::parse_keys("").is_err(),
                    "rejects missing invalid and weak keys"
                );
                assert!(
                    config::parse_keys("not-base64").is_err(),
                    "rejects missing invalid and weak keys"
                );
                assert!(
                    config::parse_keys("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").is_err(),
                    "rejects missing invalid and weak keys"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
