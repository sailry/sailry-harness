use super::*;

fn environment() -> Environment {
    let system = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows system root"));
    let bin = system.join("System32");
    let shell = bin.join("cmd.exe");
    assert!(shell.is_file(), "Windows command interpreter missing");
    Environment::filtered([
        ("SystemRoot".into(), system.into_os_string()),
        ("COMSPEC".into(), shell.into_os_string()),
        ("PATH".into(), bin.into_os_string()),
        ("PATHEXT".into(), ".EXE;.CMD".into()),
    ])
}

async fn run(launch: Launch) -> Completion {
    execute(
        launch,
        &environment(),
        &CancellationToken::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn preserves_cmd_quoting() {
    let directory = tempfile::tempdir().unwrap();
    for (command, expected) in [
        (r#"echo "quoted & value""#, "\"quoted & value\""),
        (r#"for %G in ("quoted value") do @echo %~G"#, "quoted value"),
        (r#"echo "pipe value" | findstr "pipe""#, "\"pipe value\""),
        (r#"echo escaped ^& value"#, "escaped & value"),
    ] {
        let output = run(launch(directory.path(), command)).await;
        assert_eq!(
            output.outcome,
            Outcome::Exited(0),
            "{command}: {:?}",
            output.stderr
        );
        assert_eq!(output.stdout.text.trim(), expected, "{command}");
        assert!(
            output.stderr.text.is_empty(),
            "{command}: {:?}",
            output.stderr
        );
    }
}

#[tokio::test]
async fn starts_quoted_script_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("项目 with spaces");
    std::fs::create_dir(&root).unwrap();
    let script = root.join("quoted script.cmd");
    std::fs::write(&script, "@echo off\r\necho script-marker\r\nexit /b 7\r\n").unwrap();
    let mut request = launch(directory.path(), &format!("\"{}\"", script.display()));
    request.cwd = "项目 with spaces".into();
    let output = run(request).await;
    assert_eq!(output.outcome, Outcome::Exited(7));
    assert_eq!(output.stdout.text.trim(), "script-marker");
    assert!(output.stderr.text.is_empty());
}

#[tokio::test]
async fn captures_unicode_and_exit_status() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("utf8.txt"), "中文 🙂").unwrap();
    let output = run(launch(
        directory.path(),
        "type utf8.txt & echo error-marker 1>&2& exit /b 7",
    ))
    .await;
    assert_eq!(output.outcome, Outcome::Exited(7));
    assert_eq!(output.stdout.text, "中文 🙂");
    assert_eq!(output.stderr.text.trim(), "error-marker");
    assert!(!output.stdout.invalid_utf8);
    assert!(!output.stderr.invalid_utf8);
}

#[tokio::test]
async fn captures_localized_system_output() {
    let directory = tempfile::tempdir().unwrap();
    let output = run(launch(directory.path(), "cmd.exe /d /c ver")).await;
    let reference = std::process::Command::new(environment().shell())
        .args(["/d", "/u", "/c", "ver"])
        .output()
        .unwrap();
    assert!(reference.status.success());
    assert_eq!(reference.stdout.len() % 2, 0);
    let wide: Vec<_> = reference
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    assert_eq!(output.outcome, Outcome::Exited(0), "{:?}", output.stderr);
    assert_eq!(output.stdout.text, String::from_utf16(&wide).unwrap());
    assert!(!output.stdout.invalid_utf8);
    assert!(output.stderr.text.is_empty() && !output.stderr.invalid_utf8);
}
