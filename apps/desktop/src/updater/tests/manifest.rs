use super::{
    super::{config::Channel, manifest},
    fixture,
};

#[test]
fn authenticates_exact_payload_bytes() {
    let release = fixture::release();
    let bytes = fixture::signed(std::slice::from_ref(&release));
    let selection = manifest::select(&bytes, &fixture::config())
        .unwrap()
        .unwrap();
    assert_eq!(selection.release, release);
    assert_eq!(selection.manifest, bytes);
    assert_eq!(selection.channel, Channel::Experience);
    let altered = String::from_utf8(bytes).unwrap().replace("9.9.9", "9.9.8");
    assert_eq!(
        manifest::select(altered.as_bytes(), &fixture::config())
            .unwrap_err()
            .key,
        "updates_signature_invalid"
    );
}

#[test]
fn stable_filters_prereleases() {
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
        stable.version
    );

    config.channel = Channel::Experience;
    assert_eq!(
        manifest::select(&bytes, &config)
            .unwrap()
            .unwrap()
            .release
            .version,
        preview.version
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
        final_release.version
    );
}

#[test]
fn chooses_compatible_before_newer_system_requirement() {
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
        compatible.version
    );
}

#[test]
fn uses_precedence_without_build_metadata() {
    let mut release = fixture::release();
    release.version = "9.9.9+two".into();
    let bytes = fixture::signed(&[release]);
    let mut config = fixture::config();
    config.version = "9.9.9+one".into();
    assert!(manifest::select(&bytes, &config).unwrap().is_none());
    config.version = "10.0.0".into();
    assert!(manifest::select(&bytes, &config).unwrap().is_none());
}

#[test]
fn retains_first_equal_precedence_source() {
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
    assert_eq!(selected.manifest, stable_bytes);
    assert_eq!(selected.release.version, "2.0.0+stable");
}

#[test]
fn rejects_unknown_publisher_and_empty_keys() {
    let bytes = fixture::signed(&[fixture::release()]);
    let mut config = fixture::config();
    config.keys = vec![
        ed25519_dalek::SigningKey::from_bytes(&[8; 32])
            .verifying_key()
            .to_bytes(),
    ];
    assert_eq!(
        manifest::select(&bytes, &config).unwrap_err().key,
        "updates_signature_invalid"
    );
    config.keys.clear();
    assert!(manifest::select(&bytes, &config).is_err());
}

#[test]
fn rejects_target_and_system_mismatch() {
    let mut release = fixture::release();
    release.target = "x86_64-pc-windows-msvc".into();
    release.bundle = "Sailry".into();
    release.executable = "sailry-desktop.exe".into();
    assert_eq!(
        manifest::select(&fixture::signed(&[release]), &fixture::config())
            .unwrap_err()
            .key,
        "updates_platform"
    );
    let mut config = fixture::config();
    config.system = "12.7".into();
    assert_eq!(
        manifest::select(&fixture::signed(&[fixture::release()]), &config)
            .unwrap_err()
            .key,
        "updates_system_old"
    );
}

#[test]
fn refuses_downgrade_and_tampered_staging() {
    let release = fixture::release();
    let bytes = fixture::signed(std::slice::from_ref(&release));
    let mut config = fixture::config();
    config.version = "9.9.9".into();
    assert!(manifest::select(&bytes, &config).unwrap().is_none());
    let mut selection = manifest::select(&bytes, &fixture::config())
        .unwrap()
        .unwrap();
    selection.release.url = "https://example.test/another.zip".into();
    assert_eq!(
        manifest::revalidate(&selection, &fixture::config())
            .unwrap_err()
            .key,
        "updates_manifest_invalid"
    );
    let mut stable = fixture::config();
    stable.channel = Channel::Stable;
    assert_eq!(
        manifest::revalidate(&selection, &stable).unwrap_err().key,
        "updates_manifest_invalid"
    );
}

#[test]
fn rejects_ambiguous_or_unsafe_package_contract() {
    let release = fixture::release();
    assert!(
        manifest::select(
            &fixture::signed(&[release.clone(), release.clone()]),
            &fixture::config()
        )
        .is_err()
    );
    for name in ["../package.zip", "C:\\package.zip", "/package.zip"] {
        let mut value = release.clone();
        value.name = name.into();
        assert!(manifest::select(&fixture::signed(&[value]), &fixture::config()).is_err());
    }
    for value in [
        "http://updates.example.test/update.json",
        "https://user:password@example.test/update.json",
    ] {
        assert!(super::super::config::validate_url(value).is_err());
    }
}
