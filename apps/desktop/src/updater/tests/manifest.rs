use super::{
    super::{config::Channel, manifest},
    fixture,
};

#[test]
fn trust_and_selection() {
    for (scenario, check) in [
        (
            "authenticates_exact_payload_bytes",
            (|_scenario: &str| {
                let release = fixture::release();
                let bytes = fixture::signed(std::slice::from_ref(&release));
                let selection = manifest::select(&bytes, &fixture::config())
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    selection.release, release,
                    "authenticates exact payload bytes"
                );
                assert_eq!(
                    selection.manifest, bytes,
                    "authenticates exact payload bytes"
                );
                assert_eq!(
                    selection.channel,
                    Channel::Experience,
                    "authenticates exact payload bytes"
                );
                let altered = String::from_utf8(bytes).unwrap().replace("9.9.9", "9.9.8");
                assert_eq!(
                    manifest::select(altered.as_bytes(), &fixture::config())
                        .unwrap_err()
                        .key,
                    "updates_signature_invalid",
                    "authenticates exact payload bytes"
                );
            }) as fn(&str),
        ),
        (
            "stable_filters_prereleases",
            (|_scenario: &str| {
                let mut stable = fixture::release();
                stable.version = "9.0.0".into();
                let mut preview = fixture::release();
                preview.version = "10.0.0-alpha.1".into();
                let bytes = fixture::signed(&[stable.clone(), preview.clone()]);
                let mut config = fixture::config();
                config.channel = Channel::Stable;
                assert_eq!(
                    manifest::select(&bytes, &config)
                        .unwrap()
                        .unwrap()
                        .release
                        .version,
                    stable.version,
                    "stable filters prereleases"
                );

                config.channel = Channel::Experience;
                assert_eq!(
                    manifest::select(&bytes, &config)
                        .unwrap()
                        .unwrap()
                        .release
                        .version,
                    preview.version,
                    "stable filters prereleases"
                );
                let mut final_release = preview;
                final_release.version = "10.0.0".into();
                let bytes = fixture::signed(&[stable, final_release.clone()]);
                assert_eq!(
                    manifest::select(&bytes, &config)
                        .unwrap()
                        .unwrap()
                        .release
                        .version,
                    final_release.version,
                    "stable filters prereleases"
                );
            }),
        ),
        (
            "chooses_compatible_before_newer_system_requirement",
            (|_scenario: &str| {
                let mut compatible = fixture::release();
                compatible.version = "8.0.0".into();
                let mut incompatible = fixture::release();
                incompatible.version = "9.0.0".into();
                incompatible.minimum_system = "100.0".into();
                assert_eq!(
                    manifest::select(
                        &fixture::signed(&[compatible.clone(), incompatible]),
                        &fixture::config()
                    )
                    .unwrap()
                    .unwrap()
                    .release
                    .version,
                    compatible.version,
                    "chooses compatible before newer system requirement"
                );
            }),
        ),
        (
            "uses_precedence_without_build_metadata",
            (|_scenario: &str| {
                let mut release = fixture::release();
                release.version = "9.9.9+two".into();
                let bytes = fixture::signed(&[release]);
                let mut config = fixture::config();
                config.version = "9.9.9+one".into();
                assert!(
                    manifest::select(&bytes, &config).unwrap().is_none(),
                    "uses precedence without build metadata"
                );
                config.version = "10.0.0".into();
                assert!(
                    manifest::select(&bytes, &config).unwrap().is_none(),
                    "uses precedence without build metadata"
                );
            }),
        ),
        (
            "retains_first_equal_precedence_source",
            (|_scenario: &str| {
                let mut stable = fixture::release();
                stable.version = "2.0.0+stable".into();
                let stable_bytes = fixture::signed(std::slice::from_ref(&stable));
                let mut preview = stable;
                preview.version = "2.0.0+preview".into();
                let preview_bytes = fixture::signed(&[preview]);
                let config = fixture::config();
                let stable = manifest::select(&stable_bytes, &config).unwrap().unwrap();
                let preview = manifest::select(&preview_bytes, &config).unwrap().unwrap();
                let selected = manifest::prefer(Some(stable), preview).unwrap();
                assert_eq!(
                    selected.manifest, stable_bytes,
                    "retains first equal precedence source"
                );
                assert_eq!(
                    selected.release.version, "2.0.0+stable",
                    "retains first equal precedence source"
                );
            }),
        ),
        (
            "rejects_unknown_publisher_and_empty_keys",
            (|_scenario: &str| {
                let bytes = fixture::signed(&[fixture::release()]);
                let mut config = fixture::config();
                config.keys = vec![
                    ed25519_dalek::SigningKey::from_bytes(&[8; 32])
                        .verifying_key()
                        .to_bytes(),
                ];
                assert_eq!(
                    manifest::select(&bytes, &config).unwrap_err().key,
                    "updates_signature_invalid",
                    "rejects unknown publisher and empty keys"
                );
                config.keys.clear();
                assert!(
                    manifest::select(&bytes, &config).is_err(),
                    "rejects unknown publisher and empty keys"
                );
            }),
        ),
        (
            "rejects_target_and_system_mismatch",
            (|_scenario: &str| {
                let mut release = fixture::release();
                release.target = "x86_64-pc-windows-msvc".into();
                release.bundle = "Sailry".into();
                release.executable = "sailry-desktop.exe".into();
                assert_eq!(
                    manifest::select(&fixture::signed(&[release]), &fixture::config())
                        .unwrap_err()
                        .key,
                    "updates_platform",
                    "rejects target and system mismatch"
                );
                let mut config = fixture::config();
                config.system = "12.7".into();
                assert_eq!(
                    manifest::select(&fixture::signed(&[fixture::release()]), &config)
                        .unwrap_err()
                        .key,
                    "updates_system_old",
                    "rejects target and system mismatch"
                );
            }),
        ),
        (
            "refuses_downgrade_and_tampered_staging",
            (|_scenario: &str| {
                let release = fixture::release();
                let bytes = fixture::signed(std::slice::from_ref(&release));
                let mut config = fixture::config();
                config.version = "9.9.9".into();
                assert!(
                    manifest::select(&bytes, &config).unwrap().is_none(),
                    "refuses downgrade and tampered staging"
                );
                let mut selection = manifest::select(&bytes, &fixture::config())
                    .unwrap()
                    .unwrap();
                selection.release.url = "https://example.test/another.zip".into();
                assert_eq!(
                    manifest::revalidate(&selection, &fixture::config())
                        .unwrap_err()
                        .key,
                    "updates_manifest_invalid",
                    "refuses downgrade and tampered staging"
                );
                let mut stable = fixture::config();
                stable.channel = Channel::Stable;
                assert_eq!(
                    manifest::revalidate(&selection, &stable).unwrap_err().key,
                    "updates_manifest_invalid",
                    "refuses downgrade and tampered staging"
                );
            }),
        ),
        (
            "rejects_ambiguous_or_unsafe_package_contract",
            (|_scenario: &str| {
                let release = fixture::release();
                assert!(
                    manifest::select(
                        &fixture::signed(&[release.clone(), release.clone()]),
                        &fixture::config()
                    )
                    .is_err(),
                    "rejects ambiguous or unsafe package contract"
                );
                for name in ["../package.zip", "C:\\package.zip", "/package.zip"] {
                    let mut value = release.clone();
                    value.name = name.into();
                    assert!(
                        manifest::select(&fixture::signed(&[value]), &fixture::config()).is_err(),
                        "rejects ambiguous or unsafe package contract"
                    );
                }
                for value in [
                    "http://updates.example.test/update.json",
                    "https://user:password@example.test/update.json",
                ] {
                    assert!(
                        super::super::config::validate_url(value).is_err(),
                        "rejects ambiguous or unsafe package contract"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
