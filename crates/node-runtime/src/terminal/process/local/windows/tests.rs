use super::*;

fn settings(path: &str, arguments: &[&str]) -> Settings {
    Settings {
        shell: path.into(),
        shell_arguments: arguments
            .iter()
            .map(|argument| (*argument).into())
            .collect(),
        ..Default::default()
    }
}

#[test]
fn passes_native_arguments_without_command_parsing() {
    let settings = settings(
        r"C:\Program Files\nu\bin\nu.exe",
        &["--login", "资料 with spaces; $() & 'quotes'"],
    );
    let mut command = CommandBuilder::new_default_prog();
    configure(&settings, Path::new(r"C:\项目 with spaces"), &mut command).unwrap();
    assert_eq!(
        command.get_argv(),
        &[
            OsString::from(&settings.shell),
            OsString::from("--login"),
            OsString::from("资料 with spaces; $() & 'quotes'"),
        ]
    );
}

#[test]
fn passes_windows_directory_to_wsl() {
    let settings = settings(
        r"C:\Windows\System32\wsl.exe",
        &["--distribution", "Ubuntu 资料"],
    );
    for root in [
        r"C:\项目 with spaces & 'quotes'",
        r"\\?\C:\项目 with spaces & 'quotes'",
    ] {
        let mut command = CommandBuilder::new_default_prog();
        configure(&settings, Path::new(root), &mut command).unwrap();
        assert_eq!(
            command.get_argv(),
            &[
                OsString::from(&settings.shell),
                "--distribution".into(),
                "Ubuntu 资料".into(),
                "--cd".into(),
                r"C:\项目 with spaces & 'quotes'".into(),
            ]
        );
        assert!(command.get_cwd().is_none());
    }
}

#[test]
fn maps_matching_distribution_unc_paths() {
    for root in [
        r"\\wsl$\Ubuntu\home\资料 with spaces",
        r"\\wsl.localhost\Ubuntu\home\资料 with spaces",
        r"\\?\UNC\wsl.localhost\Ubuntu\home\资料 with spaces",
    ] {
        assert_eq!(
            directory(Path::new(root), Some("Ubuntu")).unwrap(),
            OsString::from("/home/资料 with spaces")
        );
        let settings = settings(
            r"C:\Windows\System32\wsl.exe",
            &["--distribution", "Ubuntu"],
        );
        let mut command = CommandBuilder::new_default_prog();
        configure(&settings, Path::new(root), &mut command).unwrap();
        assert_eq!(
            command.get_cwd(),
            Some(&OsString::from(r"C:\Windows\System32"))
        );
        assert_eq!(
            command.get_argv().last(),
            Some(&OsString::from("/home/资料 with spaces"))
        );
    }
    assert_eq!(
        directory(Path::new(r"\\wsl$\Ubuntu"), Some("ubuntu")).unwrap(),
        OsString::from("/")
    );
}

#[test]
fn rejects_unrelated_unc_and_distribution_paths() {
    for (root, selected) in [
        (r"\\wsl$\Ubuntu\project", Some("Debian")),
        (r"\\wsl.localhost\Ubuntu\project", None),
        (r"\\server\share\project", Some("Ubuntu")),
        (r"relative\project", Some("Ubuntu")),
        (r"C:relative\project", Some("Ubuntu")),
    ] {
        assert_eq!(
            directory(Path::new(root), selected).unwrap_err().code,
            sailry_protocol::ErrorCode::InvalidRequest
        );
    }
}
