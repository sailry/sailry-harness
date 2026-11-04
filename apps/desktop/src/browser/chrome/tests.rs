use super::*;

fn encrypted() -> Vec<u8> {
    // Independent oracle: Python hashlib PBKDF2/SHA256 + OpenSSL AES-128-CBC.
    let hex = "763130cbe0d3934b1d9921743baffaf66d630303d3497336a024272f8327f166749f1af504654cc643d5543c05231db3432f4c";
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn profile_and_cookie_integrity() {
    for (scenario, check) in [
        (
            "verifies_key_domain_and_ciphertext",
            (|_scenario: &str| {
                let key = crypto::key(b"isolated-password");
                assert_eq!(
                    &*key,
                    &[
                        0x2f, 0x83, 0xf5, 0xd4, 0x15, 0x36, 0x05, 0x66, 0x72, 0xfa, 0xa0, 0x5b,
                        0xd4, 0xce, 0x17, 0xd6
                    ],
                    "verifies key domain and ciphertext"
                );
                assert_eq!(
                    crypto::decrypt(&key, ".example.test", &encrypted())
                        .unwrap()
                        .as_str(),
                    "fixture-login",
                    "verifies key domain and ciphertext"
                );
                assert!(
                    crypto::decrypt(&key, ".other.test", &encrypted()).is_err(),
                    "verifies key domain and ciphertext"
                );
                assert!(
                    crypto::decrypt(&[0; 16], ".example.test", &encrypted()).is_err(),
                    "verifies key domain and ciphertext"
                );
                assert!(
                    crypto::decrypt(&key, ".example.test", b"v20unsupported").is_err(),
                    "verifies key domain and ciphertext"
                );
            }) as fn(&str),
        ),
        (
            "imports_selected_sites",
            (|_scenario: &str| {
                let root = tempfile::tempdir().unwrap();
                let database = root.path().join("Cookies");
                let db = Connection::open(&database).unwrap();
                db.execute_batch("CREATE TABLE meta(key LONGVARCHAR, value LONGVARCHAR); INSERT INTO meta VALUES('version',24);
        CREATE TABLE cookies(host_key TEXT,name TEXT,value TEXT,encrypted_value BLOB,path TEXT,is_secure INTEGER,is_httponly INTEGER,samesite INTEGER,expires_utc INTEGER,has_expires INTEGER,top_frame_site_key TEXT);").unwrap();
                for (domain, name, expiry, partition, cipher) in [
                    (".example.test", "login", 0, "", encrypted()),
                    (".other.test", "excluded", 0, "", encrypted()),
                    (".example.test", "expired", 1, "", encrypted()),
                    (
                        ".example.test",
                        "partitioned",
                        0,
                        "https://other.test",
                        encrypted(),
                    ),
                    (
                        ".example.test",
                        "unsupported",
                        0,
                        "",
                        b"v20unsupported".to_vec(),
                    ),
                ] {
                    db.execute(
                        "INSERT INTO cookies VALUES(?1,?2,'',?3,'/',1,1,1,?4,?5,?6)",
                        rusqlite::params![domain, name, cipher, expiry, expiry != 0, partition],
                    )
                    .unwrap();
                }
                drop(db);
                let original = std::fs::read(&database).unwrap();
                let profile = Profile {
                    name: "Fixture".into(),
                    database,
                };
                assert_eq!(sites(&profile).unwrap().len(), 2, "imports selected sites");
                let prepared =
                    prepare(&profile, &BTreeSet::from([".example.test".into()])).unwrap();
                assert!(prepared.needs_key(), "imports selected sites");
                // Preparation reads app data only; credentials are requested separately and failures retain retry data.
                assert!(
                    prepared
                        .decrypt(|| Err("browser_chrome_key_denied"))
                        .is_err(),
                    "imports selected sites"
                );
                let unlocked = prepared
                    .decrypt(|| Ok(Zeroizing::new(b"isolated-password".to_vec())))
                    .unwrap();
                assert_eq!(
                    unlocked.cookies[0].value.as_str(),
                    "fixture-login",
                    "imports selected sites"
                );
                let result = prepared
                    .decrypt(|| Ok(Zeroizing::new(b"isolated-password".to_vec())))
                    .unwrap();
                assert_eq!(result.cookies.len(), 1, "imports selected sites");
                assert_eq!(result.skipped, 3, "imports selected sites");
                let cookie = &result.cookies[0];
                assert_eq!(
                    cookie.value.as_str(),
                    "fixture-login",
                    "imports selected sites"
                );
                assert!(cookie.secure && cookie.http_only, "imports selected sites");
                assert_eq!(cookie.same_site, 1, "imports selected sites");
                assert_eq!(cookie.expires, None, "imports selected sites");
                assert_eq!(
                    std::fs::read(&profile.database).unwrap(),
                    original,
                    "imports selected sites"
                );
                assert!(
                    prepared
                        .decrypt(|| Err("browser_chrome_key_denied"))
                        .is_err(),
                    "imports selected sites"
                );
                let empty = prepare(&profile, &BTreeSet::new())
                    .unwrap()
                    .decrypt(|| panic!("unselected sites must not request credentials"))
                    .unwrap();
                assert!(empty.cookies.is_empty(), "imports selected sites");
            }),
        ),
        (
            "confines_profile_discovery",
            (|_scenario: &str| {
                let root = tempfile::tempdir().unwrap();
                std::fs::create_dir(root.path().join("Default")).unwrap();
                std::fs::write(root.path().join("Default/Cookies"), []).unwrap();
                std::fs::write(root.path().join("Local State"), br#"{"profile":{"info_cache":{"Default":{"name":"Fixture"},"../outside":{"name":"Outside"}}}}"#).unwrap();
                let profiles = discover(root.path()).unwrap();
                assert_eq!(profiles.len(), 1, "confines profile discovery");
                assert_eq!(profiles[0].name, "Fixture", "confines profile discovery");
            }),
        ),
        (
            "distinguishes_missing_denied_and_unreadable_metadata",
            (|_scenario: &str| {
                use std::io::{Error, ErrorKind};
                assert_eq!(
                    discovery_error(Error::from(ErrorKind::NotFound)),
                    "browser_chrome_missing",
                    "distinguishes missing denied and unreadable metadata"
                );
                assert_eq!(
                    discovery_error(Error::from(ErrorKind::PermissionDenied)),
                    "browser_chrome_access_denied",
                    "distinguishes missing denied and unreadable metadata"
                );
                assert_eq!(
                    discovery_error(Error::from(ErrorKind::InvalidData)),
                    "browser_chrome_failed",
                    "distinguishes missing denied and unreadable metadata"
                );
                let root = tempfile::tempdir().unwrap();
                assert!(
                    matches!(discover(root.path()), Err("browser_chrome_missing")),
                    "distinguishes missing denied and unreadable metadata"
                );
                std::fs::create_dir(root.path().join("Local State")).unwrap();
                assert!(
                    matches!(discover(root.path()), Err("browser_chrome_failed")),
                    "distinguishes missing denied and unreadable metadata"
                );
            }),
        ),
        (
            "preserves_database_access_errors",
            (|_scenario: &str| {
                use std::os::unix::fs::PermissionsExt;

                let root = tempfile::tempdir().unwrap();
                let path = root.path().join("Cookies");
                assert!(
                    matches!(database(&path), Err("browser_chrome_missing")),
                    "preserves database access errors"
                );
                std::fs::write(&path, b"unreadable fixture").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
                assert!(
                    matches!(database(&path), Err("browser_chrome_access_denied")),
                    "preserves database access errors"
                );
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
                assert!(
                    matches!(database(&path), Err("browser_chrome_failed")),
                    "preserves database access errors"
                );
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    b"unreadable fixture",
                    "preserves database access errors"
                );
            }),
        ),
        (
            "discovers_network_cookie_storage_without_reading_cookies",
            (|_scenario: &str| {
                let root = tempfile::tempdir().unwrap();
                std::fs::create_dir_all(root.path().join("Profile 1/Network")).unwrap();
                std::fs::write(
                    root.path().join("Profile 1/Network/Cookies"),
                    b"unopened fixture",
                )
                .unwrap();
                std::fs::write(root.path().join("Local State"), br#"{"profile":{"info_cache":{"Profile 1":{"name":"Network"},"Default":{"name":"Absent"}}}}"#).unwrap();
                let profiles = discover(root.path()).unwrap();
                assert_eq!(
                    profiles.len(),
                    1,
                    "discovers network cookie storage without reading cookies"
                );
                assert_eq!(
                    profiles[0].name, "Network",
                    "discovers network cookie storage without reading cookies"
                );
                assert_eq!(
                    profiles[0].database,
                    root.path().join("Profile 1/Network/Cookies"),
                    "discovers network cookie storage without reading cookies"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[test]
#[ignore = "requires an installed local Chrome profile; reads metadata only"]
fn discovers_local_profiles() {
    let profiles = profiles().expect("local Chrome metadata must be readable");
    assert!(!profiles.is_empty(), "local Chrome profile expected");
    println!("Discovered {} local Chrome profiles", profiles.len());
}
