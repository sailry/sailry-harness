use super::*;

#[test]
fn releases_lock_with_duplicate_handle() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profile");
    let profile = Profile::acquire(&path).unwrap();
    let inherited = profile._lock.try_clone().unwrap();
    drop(profile);
    let replacement = Profile::acquire(&path).unwrap();
    assert!(matches!(
        Profile::acquire(&path),
        Err(Error::ProfileInUse(_))
    ));
    drop(inherited);
    assert!(matches!(
        Profile::acquire(&path),
        Err(Error::ProfileInUse(_))
    ));
    drop(replacement);
}

#[test]
fn resolves_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let user = directory.path().join("用户 🙂");
    let path = from_home(Some(user.clone())).unwrap();
    assert_eq!(path, user.join(".sailry"));
    assert!(!user.exists());
}

#[test]
fn rejects_missing_home() {
    for directory in [None, Some(PathBuf::new()), Some(PathBuf::from("relative"))] {
        assert!(
            from_home(directory)
                .unwrap_err()
                .to_string()
                .contains("--data-dir")
        );
    }
}

#[test]
fn uses_platform_home() {
    assert_eq!(
        default_data_dir().unwrap(),
        dirs::home_dir().unwrap().join(".sailry")
    );
}
