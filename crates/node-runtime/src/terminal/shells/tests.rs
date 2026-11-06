use super::*;

fn executable(path: &std::path::Path) {
    std::fs::write(path, "fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn lists_executables_without_duplicates() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a shell.exe");
    let second = directory.path().join("资料 shell.exe");
    executable(&first);
    executable(&second);
    let folder = directory.path().join("folder.exe");
    std::fs::create_dir(&folder).unwrap();
    let paths = available(
        vec![
            second.clone(),
            directory.path().join("missing.exe"),
            folder,
            first.clone(),
            second.clone(),
        ],
        &crate::process::Environment::filtered([]),
    );
    assert_eq!(
        paths,
        vec![first, second]
            .into_iter()
            .map(|path| Shell {
                path: path.to_str().unwrap().into(),
                arguments: Vec::new(),
            })
            .collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn preserves_shell_invocation_names() {
    let directory = tempfile::tempdir().unwrap();
    let shell = directory.path().join("shell");
    let alias = directory.path().join("shell-alias");
    executable(&shell);
    std::os::unix::fs::symlink(&shell, &alias).unwrap();
    let shells = available(
        vec![shell.clone(), alias.clone()],
        &crate::process::Environment::filtered([]),
    );
    assert_eq!(
        shells,
        vec![shell, alias]
            .into_iter()
            .map(|path| Shell {
                path: path.to_str().unwrap().into(),
                arguments: Vec::new()
            })
            .collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn excludes_non_executable_files() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let shell = directory.path().join("shell");
    executable(&shell);
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(available(vec![shell], &crate::process::Environment::filtered([])).is_empty());
}

#[cfg(windows)]
#[test]
fn deduplicates_path_casing() {
    let directory = tempfile::tempdir().unwrap();
    let shell = directory.path().join("Shell.exe");
    executable(&shell);
    let paths = available(
        vec![shell.clone(), directory.path().join("SHELL.EXE")],
        &crate::process::Environment::filtered([]),
    );
    assert_eq!(
        paths,
        vec![Shell {
            path: shell.to_str().unwrap().into(),
            arguments: Vec::new()
        }]
    );
}
