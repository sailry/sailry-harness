use super::*;

#[tokio::test]
async fn preserves_paths_and_defaults_missing_arguments() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(
        "CREATE TABLE terminal_settings(singleton INTEGER PRIMARY KEY, body BLOB NOT NULL)",
    )
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join("Removed 资料 Shell.exe")
        .to_str()
        .unwrap()
        .to_owned();
    let body = serde_json::to_vec(&serde_json::json!({
        "revision": 5,
        "shell": path,
        "environment": { "FIXTURE": "preserved" },
    }))
    .unwrap();
    db.execute(
        "INSERT INTO terminal_settings(singleton, body) VALUES(1, ?1)",
        [body],
    )
    .await
    .unwrap();
    let mut settings = read(&db).await.unwrap();
    assert_eq!(settings.shell, path);
    assert!(settings.shell_arguments.is_empty());
    assert_eq!(settings.environment["FIXTURE"], "preserved");
    settings.shell_arguments = vec!["--distribution".into(), "Removed 资料".into()];
    let (Output::TerminalSettings(saved), event) = save(&db, &settings).await.unwrap() else {
        panic!("terminal settings expected")
    };
    assert_eq!(saved.revision, 6);
    assert_eq!(saved.shell, path);
    assert_eq!(saved.shell_arguments, settings.shell_arguments);
    assert_eq!(read(&db).await.unwrap(), saved);
    assert_eq!(event, Some(Event::TerminalSettingsChanged { revision: 6 }));
    assert_eq!(
        save(&db, &settings).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
}
