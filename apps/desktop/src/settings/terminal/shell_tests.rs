use super::*;
use core::prelude::v1::test;
use std::collections::BTreeSet;

fn descriptor(path: &str, arguments: &[&str]) -> terminal::Shell {
    terminal::Shell {
        path: path.to_owned(),
        arguments: arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
    }
}

#[test]
fn recognizes_execution_host_paths() {
    for (path, expected) in [
        (r"C:\Windows\System32\CMD.EXE", "terminal_shell_cmd"),
        (
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            "terminal_shell_windows_powershell",
        ),
        (
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            "terminal_shell_powershell",
        ),
        (
            r"D:\Tools\PortableGit\bin\bash.exe",
            "terminal_shell_git_bash",
        ),
        (
            "C:/Program Files/Git/bin/bash.exe",
            "terminal_shell_git_bash",
        ),
        (
            r"D:\Tools\PortableGit-2.47\bin\bash.exe",
            "terminal_shell_git_bash",
        ),
        (r"C:\msys64\usr\bin\bash.exe", "terminal_shell_msys_bash"),
        (r"C:\cygwin64\bin\bash.exe", "terminal_shell_cygwin_bash"),
        (r"C:\Tools\nu.exe", "terminal_shell_nu"),
        ("/bin/zsh", "terminal_shell_zsh"),
        ("/opt/homebrew/bin/bash", "terminal_shell_bash"),
        ("/usr/local/bin/fish", "terminal_shell_fish"),
        ("/bin/sh", "terminal_shell_sh"),
        ("/bin/csh", "terminal_shell_csh"),
        ("/bin/tcsh", "terminal_shell_tcsh"),
        ("/bin/ksh", "terminal_shell_ksh"),
    ] {
        assert_eq!(name(path), Some(expected), "{path}");
    }
    assert_eq!(name(r"C:\Windows\System32\wsl.exe"), None);
}

#[test]
fn uses_concise_labels() {
    for (value, expected) in [
        (descriptor(r"C:\Windows\System32\cmd.exe", &[]), "cmd"),
        (
            descriptor(
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                &[],
            ),
            "PowerShell",
        ),
        (
            descriptor(r"C:\Program Files\PowerShell\7\pwsh.exe", &[]),
            "pwsh",
        ),
        (
            descriptor(r"C:\Program Files\Git\bin\bash.exe", &[]),
            "Git Bash",
        ),
        (descriptor(r"C:\Tools\nu.exe", &[]), "Nushell"),
        (descriptor(r"C:\msys64\usr\bin\bash.exe", &[]), "MSYS2 Bash"),
        (descriptor(r"C:\cygwin64\bin\bash.exe", &[]), "Cygwin Bash"),
        (descriptor("/bin/zsh", &[]), "Zsh"),
        (descriptor("/bin/bash", &[]), "Bash"),
        (descriptor("/bin/sh", &[]), "sh"),
        (
            descriptor(
                r"C:\Windows\System32\wsl.exe",
                &["--distribution", "Ubuntu"],
            ),
            "Ubuntu(WSL)",
        ),
    ] {
        let items = Shell::items(std::slice::from_ref(&value), &value);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].value(), &value);
        assert_eq!(items[1].title().as_ref(), expected);
    }
}

#[test]
fn only_lists_detected_or_saved_choices() {
    let default = terminal::Shell::default();
    let items = Shell::items(&[], &default);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].value(), &default);
    assert_eq!(items[0].title(), tr("terminal_shell_default"));

    let detected = descriptor("/bin/zsh", &[]);
    let items = Shell::items(std::slice::from_ref(&detected), &default);
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].value(), &detected);
}

#[test]
fn retains_missing_selection() {
    let detected = descriptor("/bin/zsh", &[]);
    let selected = descriptor(r"D:\自定义 Shell\shell.exe", &["--login", "with spaces"]);
    let items = Shell::items(std::slice::from_ref(&detected), &selected);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].value(), &terminal::Shell::default());
    assert_eq!(items[1].value(), &detected);
    assert_eq!(items[2].value(), &selected);
    assert_eq!(items[2].title().as_ref(), "shell.exe");
}

#[test]
fn folds_only_windows_path_variants() {
    let detected = descriptor(r"C:\Program Files\PowerShell\7\pwsh.exe", &[]);
    let duplicate = descriptor("c:/program files/powershell/7/PWSH.EXE", &[]);
    let selected = descriptor(r"\\?\C:\PROGRAM FILES\PowerShell\7\pwsh.exe", &[]);
    let items = Shell::items(&[detected, duplicate], &selected);
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].value(), &selected);
    assert_eq!(items[1].title().as_ref(), "pwsh");
}

#[test]
fn preserves_unix_path_choices() {
    let detected = [
        descriptor("/bin/bash", &[]),
        descriptor("/usr/local/bin/bash", &[]),
        descriptor("/opt/homebrew/bin/bash", &[]),
        descriptor("/Tools/bin/bash", &[]),
        descriptor("/tools/bin/bash", &[]),
        descriptor(r"/Tools\Case/bin/bash", &[]),
        descriptor("/tools/case/bin/bash", &[]),
    ];
    let items = Shell::items(&detected, &detected[2]);
    assert_eq!(items.len(), detected.len() + 1);
    assert_eq!(
        items[1..]
            .iter()
            .map(|item| item.value().clone())
            .collect::<Vec<_>>(),
        detected,
    );
    assert_eq!(
        items
            .iter()
            .map(|item| item.title().to_string())
            .collect::<BTreeSet<_>>()
            .len(),
        items.len()
    );
}

#[test]
fn distinguishes_versions_and_architectures() {
    let detected = [
        descriptor(r"C:\Program Files\PowerShell\7\pwsh.exe", &[]),
        descriptor(r"C:\Program Files\PowerShell\8\pwsh.exe", &[]),
        descriptor(r"C:\Program Files\PowerShell\7-preview\pwsh.exe", &[]),
        descriptor(r"C:\Program Files (x86)\PowerShell\7\pwsh.exe", &[]),
        descriptor(r"D:\arm64\PowerShell\7\pwsh.exe", &[]),
        descriptor(r"D:\x64\PowerShell\7\pwsh.exe", &[]),
    ];
    let items = Shell::items(&detected, &detected[0]);
    assert_eq!(items.len(), detected.len() + 1);
    for (item, expected) in items[1..].iter().zip([
        "pwsh",
        "pwsh(8)",
        "pwsh(Preview)",
        "pwsh(x86)",
        "pwsh(arm64)",
        "pwsh(x64)",
    ]) {
        assert_eq!(item.title().as_ref(), expected);
    }
}

#[test]
fn identifies_windows_powershell_architecture() {
    let detected = [
        descriptor(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            &[],
        ),
        descriptor(
            r"C:\Windows\SysWOW64\WindowsPowerShell\v1.0\powershell.exe",
            &[],
        ),
    ];
    let items = Shell::items(&detected, &detected[0]);
    assert_eq!(items.len(), 3);
    assert_eq!(items[1].title().as_ref(), "PowerShell");
    assert_eq!(items[2].title().as_ref(), "PowerShell(x86)");
}

#[test]
fn uses_short_installation_identifiers() {
    let detected = [
        descriptor(r"D:\Toolchains\Stable\bin\nu.exe", &[]),
        descriptor(r"D:\Toolchains\Next\bin\nu.exe", &[]),
    ];
    let items = Shell::items(&detected, &detected[0]);
    assert_eq!(items.len(), 3);
    assert_eq!(items[1].title().as_ref(), "Nushell(Stable)");
    assert_eq!(items[2].title().as_ref(), "Nushell(Next)");
}

#[test]
fn keeps_ambiguous_installations_distinct() {
    let detected = [
        descriptor(r"C:\A\B\nu.exe", &[]),
        descriptor(r"C:\A\D\nu.exe", &[]),
        descriptor(r"C:\F\B\nu.exe", &[]),
        descriptor(r"C:\F\D\nu.exe", &[]),
    ];
    let items = Shell::items(&detected, &detected[0]);
    assert_eq!(items.len(), detected.len() + 1);
    let titles = items
        .iter()
        .map(|item| item.title().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(titles.len(), items.len());
    assert!(titles.iter().all(|title| !title.contains(['/', '\\'])));
}

#[test]
fn retains_distributions_on_the_same_executable() {
    let detected = descriptor(
        r"C:\Windows\System32\wsl.exe",
        &["--distribution", "Ubuntu"],
    );
    let selected = descriptor(
        r"C:\Windows\System32\wsl.exe",
        &["--distribution", "开发 Linux"],
    );
    let items = Shell::items(std::slice::from_ref(&detected), &selected);
    assert_eq!(items.len(), 3);
    assert_eq!(items[1].value(), &detected);
    assert_eq!(items[1].title().as_ref(), "Ubuntu(WSL)");
    assert_eq!(items[2].value(), &selected);
    assert_eq!(items[2].title().as_ref(), "开发 Linux(WSL)");
}

#[test]
fn preserves_distinct_arguments() {
    let detected = descriptor("/bin/zsh", &[]);
    let selected = descriptor("/bin/zsh", &["--login"]);
    let items = Shell::items(std::slice::from_ref(&detected), &selected);
    assert_eq!(items.len(), 3);
    assert_eq!(items[1].value(), &detected);
    assert_eq!(items[2].value(), &selected);
    assert_ne!(items[1].title(), items[2].title());
}
