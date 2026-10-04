use super::{super::manifest, fixture};

#[test]
fn authenticates_exact_payload_bytes() {
    let release = fixture::release();
    let bytes = fixture::signed(std::slice::from_ref(&release));
    assert_eq!(
        manifest::select(&bytes, &fixture::config())
            .unwrap()
            .unwrap()
            .release,
        release
    );
    let altered = String::from_utf8(bytes).unwrap().replace("9.9.9", "9.9.8");
    assert_eq!(
        manifest::select(altered.as_bytes(), &fixture::config())
            .unwrap_err()
            .key,
        "updates_signature_invalid"
    );
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
