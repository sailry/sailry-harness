use super::*;

fn file(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "fixture").unwrap();
    path.to_owned()
}

#[test]
fn discovers_common_installations() {
    let directory = tempfile::tempdir().unwrap();
    let system = directory.path().join("Windows");
    let programs = directory.path().join("Program Files");
    let local = directory.path().join("用户 AppData");
    let expected = [
        file(&system.join("System32/cmd.exe")),
        file(&system.join("System32/WindowsPowerShell/v1.0/powershell.exe")),
        file(&programs.join("PowerShell/7/pwsh.exe")),
        file(&programs.join("PowerShell/7-preview/pwsh.exe")),
        file(&programs.join("Git/bin/bash.exe")),
        file(&local.join("Programs/Git/usr/bin/bash.exe")),
        file(&local.join("Microsoft/WinGet/Links/pwsh.exe")),
    ];
    file(&programs.join("Git/usr/bin/bash.exe"));
    file(&programs.join("Windows Terminal/WindowsTerminal.exe"));
    file(&system.join("System32/bash.exe"));
    let paths = locations(|name| match name {
        "SystemRoot" => Some(system.as_os_str().into()),
        "ProgramFiles" => Some(programs.as_os_str().into()),
        "LOCALAPPDATA" => Some(local.as_os_str().into()),
        _ => None,
    });
    let actual = super::super::available(paths, &Environment::filtered([]));
    assert_eq!(
        actual
            .into_iter()
            .map(PathBuf::from)
            .collect::<std::collections::BTreeSet<_>>(),
        expected
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
}

#[test]
fn finds_portable_git_layouts() {
    let root = Path::new(r"C:\Tools\PortableGit");
    for executable in ["cmd/git.exe", "bin/git.exe", "mingw64/bin/git.exe"] {
        assert_eq!(git_root(&root.join(executable)), Some(root));
    }
}

#[test]
fn excludes_missing_shells() {
    let directory = tempfile::tempdir().unwrap();
    let programs = directory.path().join("Program Files");
    file(&programs.join("PowerShell/7/unrelated.exe"));
    file(&programs.join("Git/git-bash.exe"));
    let paths = locations(|name| (name == "ProgramFiles").then(|| programs.as_os_str().to_owned()));
    assert!(super::super::available(paths, &Environment::filtered([])).is_empty());
}
