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
        file(&system.join("SysWOW64/WindowsPowerShell/v1.0/powershell.exe")),
        file(&programs.join("nu/bin/nu.exe")),
        file(&programs.join("MSYS2/usr/bin/bash.exe")),
        file(&programs.join("Cygwin/bin/bash.exe")),
        file(&programs.join("Cygwin64/bin/bash.exe")),
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
            .map(|shell| PathBuf::from(shell.path))
            .collect::<std::collections::BTreeSet<_>>(),
        expected
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
}

#[test]
fn discovers_drive_and_user_installations() {
    let directory = tempfile::tempdir().unwrap();
    let drive = directory.path().join("drive");
    let local = directory.path().join("用户 AppData");
    let expected = [
        file(&drive.join("msys64/usr/bin/bash.exe")),
        file(&drive.join("cygwin64/bin/bash.exe")),
        file(&local.join("Programs/nushell/nu.exe")),
        file(&local.join("Microsoft/WinGet/Links/nu.exe")),
    ];
    let paths = locations(|name| match name {
        "SystemDrive" => Some(drive.as_os_str().into()),
        "LOCALAPPDATA" => Some(local.as_os_str().into()),
        _ => None,
    });
    assert_eq!(
        super::super::available(paths, &Environment::filtered([]))
            .into_iter()
            .map(|shell| PathBuf::from(shell.path))
            .collect::<std::collections::BTreeSet<_>>(),
        expected.into_iter().collect()
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
fn excludes_launchers_and_terminal_emulators() {
    let directory = tempfile::tempdir().unwrap();
    let paths = [
        "Windows/System32/bash.exe",
        "Windows/SysWOW64/bash.exe",
        "Windows/Sysnative/bash.exe",
        "Windows/System32/wsl.exe",
        "Users/用户/AppData/Local/Microsoft/WindowsApps/bash.exe",
        "Users/用户/AppData/Local/mIcRoSoFt/wInDoWsApPs/BaSh.ExE",
        "Windows Terminal/WindowsTerminal.exe",
        "wt.exe",
        "iTerm2",
    ]
    .map(|path| file(&directory.path().join(path)));
    assert!(super::super::available(paths.into(), &Environment::filtered([])).is_empty());
}

#[test]
fn preserves_other_aliases_and_bash_installations() {
    let directory = tempfile::tempdir().unwrap();
    let paths = [
        "Users/用户/AppData/Local/Microsoft/WindowsApps/pwsh.exe",
        "Users/用户/AppData/Local/Microsoft/WindowsApps/nu.exe",
        "Tools/WindowsApps/bash.exe",
        "Microsoft/WindowsApps/MSYS2/usr/bin/bash.exe",
    ]
    .map(|path| file(&directory.path().join(path)));
    let shells = super::super::available(paths.clone().into(), &Environment::filtered([]));
    assert_eq!(
        shells
            .into_iter()
            .map(|shell| PathBuf::from(shell.path))
            .collect::<std::collections::BTreeSet<_>>(),
        paths.into_iter().collect()
    );
}

#[test]
fn preserves_versions_and_architectures() {
    let directory = tempfile::tempdir().unwrap();
    let paths = [
        "PowerShell/7/pwsh.exe",
        "PowerShell/7-preview/pwsh.exe",
        "Program Files (x86)/PowerShell/7/pwsh.exe",
    ]
    .map(|path| file(&directory.path().join(path)));
    let shells = super::super::available(paths.clone().into(), &Environment::filtered([]));
    assert_eq!(
        shells
            .into_iter()
            .map(|shell| PathBuf::from(shell.path))
            .collect::<std::collections::BTreeSet<_>>(),
        paths.into_iter().collect()
    );
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
