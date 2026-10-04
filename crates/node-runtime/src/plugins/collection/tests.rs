use super::*;
use std::fs;

#[test]
fn keeps_unmanaged() {
    let profile = tempfile::tempdir().unwrap();
    let packages = profile.path().join("plugins/packages");
    let removed = blake3::hash(b"removed").to_hex().to_string();
    let retained = blake3::hash(b"retained").to_hex().to_string();
    let file = blake3::hash(b"file").to_hex().to_string();
    let uppercase = blake3::hash(b"uppercase")
        .to_hex()
        .to_string()
        .to_uppercase();
    for name in [&removed, &retained, &uppercase, "notes", ".tmp-staging"] {
        fs::create_dir_all(packages.join(name)).unwrap();
        fs::write(packages.join(name).join("content"), name).unwrap();
    }
    fs::write(packages.join(&file), "unmanaged file").unwrap();
    let data = profile.path().join("plugins/data/example");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("state"), "persistent plugin data").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("source"), "original source").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(outside.path(), packages.join(&removed).join("outside")).unwrap();
        symlink(
            outside.path(),
            packages.join(blake3::hash(b"link").to_hex().as_str()),
        )
        .unwrap();
    }
    let host = Host::new(Some(profile.path().canonicalize().unwrap()));
    let keep = BTreeSet::from([retained.clone()]);
    host.collect(&keep).unwrap();
    host.collect(&keep).unwrap();
    assert!(!packages.join(removed).exists());
    for name in [&retained, &uppercase, "notes", ".tmp-staging"] {
        assert_eq!(
            fs::read_to_string(packages.join(name).join("content")).unwrap(),
            name
        );
    }
    assert_eq!(
        fs::read_to_string(packages.join(file)).unwrap(),
        "unmanaged file"
    );
    assert_eq!(
        fs::read_to_string(data.join("state")).unwrap(),
        "persistent plugin data"
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("source")).unwrap(),
        "original source"
    );
    #[cfg(unix)]
    assert!(
        fs::symlink_metadata(packages.join(blake3::hash(b"link").to_hex().as_str()))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn skips_missing() {
    let profile = tempfile::tempdir().unwrap();
    Host::new(Some(profile.path().canonicalize().unwrap()))
        .collect(&BTreeSet::new())
        .unwrap();
    assert!(!profile.path().join("plugins").exists());
    Host::new(None).collect(&BTreeSet::new()).unwrap();
}

#[test]
fn reports_failure() {
    let profile = tempfile::tempdir().unwrap();
    fs::create_dir(profile.path().join("plugins")).unwrap();
    let path = profile.path().join("plugins/packages");
    fs::write(&path, "unexpected file").unwrap();
    assert!(
        Host::new(Some(profile.path().canonicalize().unwrap()))
            .collect(&BTreeSet::new())
            .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "unexpected file");
}
