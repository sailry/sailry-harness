use super::*;
use serde_json::json;

#[test]
fn preserves_shell_arguments_on_the_wire() {
    let shell = Shell {
        path: r"C:\Windows\System32\wsl.exe".into(),
        arguments: vec!["--distribution".into(), "Ubuntu 资料".into()],
    };
    let output = crate::Output::Shells(vec![shell.clone()]);
    let encoded = json!({
        "kind": "shells",
        "data": [{ "path": shell.path, "arguments": shell.arguments }],
    });
    assert_eq!(serde_json::to_value(&output).unwrap(), encoded);
    assert_eq!(
        serde_json::from_value::<crate::Output>(encoded).unwrap(),
        output
    );
    assert!(!crate::Command::ListShells.durable());
    let settings = Settings {
        shell: shell.path,
        shell_arguments: shell.arguments,
        ..Default::default()
    };
    let command = crate::Command::SaveTerminalSettings(settings);
    assert!(command.durable());
    assert_eq!(
        serde_json::from_value::<crate::Command>(serde_json::to_value(&command).unwrap()).unwrap(),
        command
    );
}

#[test]
fn defaults_missing_arguments_without_changing_paths() {
    let settings: Settings = serde_json::from_value(json!({
        "revision": 7,
        "shell": r"C:\Removed Shell\pwsh.exe",
        "environment": { "FIXTURE": "preserved" },
    }))
    .unwrap();
    assert_eq!(settings.revision, 7);
    assert_eq!(settings.shell, r"C:\Removed Shell\pwsh.exe");
    assert!(settings.shell_arguments.is_empty());
    assert_eq!(settings.environment["FIXTURE"], "preserved");
    assert!(settings.validate().is_ok());
    let encoded = r#"{"kind":"save_terminal_settings","data":{"revision":7,"shell":"C:\\Removed Shell\\pwsh.exe","environment":{"FIXTURE":"preserved"}}}"#;
    let command = crate::Command::SaveTerminalSettings(settings);
    assert_eq!(
        serde_json::from_str::<crate::Command>(encoded).unwrap(),
        command
    );
    assert_eq!(serde_json::to_string(&command).unwrap(), encoded);
}

#[test]
fn validates_arguments_without_parsing_them() {
    let settings = Settings {
        shell: "/shell".into(),
        shell_arguments: vec!["".into(), "资料 with spaces; $() & 'quotes'".into()],
        ..Default::default()
    };
    assert!(settings.validate().is_ok());
    for arguments in [
        vec!["null\0byte".into()],
        vec![String::new(); 129],
        vec!["a".repeat(64 * 1024 + 1)],
    ] {
        let invalid = Settings {
            shell_arguments: arguments,
            ..settings.clone()
        };
        assert_eq!(
            invalid.validate().unwrap_err().code,
            crate::ErrorCode::InvalidRequest
        );
    }
    let invalid = Settings {
        shell: String::new(),
        ..settings
    };
    assert_eq!(
        invalid.validate().unwrap_err().code,
        crate::ErrorCode::InvalidRequest
    );
}
