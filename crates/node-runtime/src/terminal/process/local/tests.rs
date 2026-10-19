use super::*;

fn private(path: &Path) {
    crate::profile::private_directory(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}

#[test]
fn restricts_scratch_and_profile_temporary_storage() {
    let profile = tempfile::tempdir().unwrap();
    for source in [Some(profile.path()), None] {
        let temporary = scratch(source).unwrap();
        private(temporary.path());
        if let Some(profile) = source {
            assert_eq!(
                temporary.path().parent(),
                Some(profile.join("tmp").as_path())
            );
            private(&profile.join("tmp"));
        }
    }
}

#[cfg(unix)]
#[test]
fn restricts_shell_integration_directories() {
    let scratch = scratch(None).unwrap();
    for parent in [Some(scratch.path()), None] {
        let mut command = CommandBuilder::new("/bin/zsh");
        let directory = integration::configure(&mut command, parent)
            .unwrap()
            .unwrap();
        private(directory.path());
        assert!(directory.path().join(".zshenv").is_file());
    }
}
