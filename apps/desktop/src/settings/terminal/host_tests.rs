use super::*;
use core::prelude::v1::test;

fn saved() -> Settings {
    Settings {
        revision: 7,
        shell: r"C:\Windows\System32\wsl.exe".to_owned(),
        shell_arguments: vec!["--distribution".to_owned(), "Ubuntu".to_owned()],
        environment: [("SAVED".to_owned(), "value".to_owned())].into(),
    }
}

#[test]
fn preserves_arguments_when_editing_environment() {
    let saved = saved();
    let selected = terminal::Shell {
        path: saved.shell.clone(),
        arguments: saved.shell_arguments.clone(),
    };
    let environment = vec![("EDITED".to_owned(), "new value".to_owned())];
    let data = snapshot(Some(&saved), Some(&selected), &environment);
    assert_eq!(
        data,
        Settings {
            environment: environment.into_iter().collect(),
            ..saved.clone()
        },
    );
    assert_eq!(saved.shell_arguments, ["--distribution", "Ubuntu"]);
    assert!(data.validate().is_ok());
}

#[test]
fn retains_saved_choice_without_selection() {
    let saved = saved();
    let data = snapshot(Some(&saved), None, &[]);
    assert_eq!(
        data,
        Settings {
            environment: Default::default(),
            ..saved
        },
    );
}

#[test]
fn saves_selected_distribution_and_revision() {
    let saved = saved();
    let selected = terminal::Shell {
        path: saved.shell.clone(),
        arguments: vec!["--distribution".to_owned(), "Debian".to_owned()],
    };
    let environment = saved.environment.clone().into_iter().collect::<Vec<_>>();
    let data = snapshot(Some(&saved), Some(&selected), &environment);
    assert_eq!(
        data,
        Settings {
            shell_arguments: selected.arguments,
            ..saved
        },
    );
}

#[test]
fn default_clears_path_and_arguments() {
    let saved = saved();
    let environment = saved.environment.clone().into_iter().collect::<Vec<_>>();
    let data = snapshot(
        Some(&saved),
        Some(&terminal::Shell::default()),
        &environment,
    );
    assert_eq!(
        data,
        Settings {
            shell: String::new(),
            shell_arguments: Vec::new(),
            ..saved
        },
    );
    assert!(data.validate().is_ok());
}

#[test]
fn starts_with_host_default() {
    assert_eq!(snapshot(None, None, &[]), Settings::default());
}
