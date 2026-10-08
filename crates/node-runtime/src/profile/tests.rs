use super::*;

#[test]
fn prepares_private_temporary_storage() {
    let directory = tempfile::tempdir().unwrap();
    let profile = Profile::acquire(&directory.path().join("custom-profile")).unwrap();
    let path = temporary(&profile.path).unwrap();
    assert_eq!(path, profile.path.join("tmp"));
    assert!(path.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    }
}

#[cfg(unix)]
#[test]
fn rejects_linked_temporary_storage() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile");
    private_directory(&profile).unwrap();
    let outside = directory.path().join("outside");
    private_directory(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, profile.join("tmp")).unwrap();
    assert!(temporary(&profile).is_err());
    assert!(Profile::acquire(&profile).is_err());
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

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
