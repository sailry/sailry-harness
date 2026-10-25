use super::*;

async fn read(client: &Client) -> code_index::Settings {
    let Output::CodeIndexSettings(settings) = client
        .execute(client.prepare(Command::ReadCodeIndexSettings))
        .await
        .unwrap()
    else {
        panic!("code index settings expected");
    };
    settings
}

async fn save(client: &Client, settings: code_index::Settings) -> code_index::Settings {
    let Output::CodeIndexSettings(saved) = client
        .execute(client.prepare(Command::SaveCodeIndexSettings {
            expected_revision: settings.revision,
            settings,
        }))
        .await
        .unwrap()
    else {
        panic!("code index settings expected");
    };
    saved
}

#[tokio::test]
async fn gates_indexing_and_preserves_file_reads() {
    let fixture = Fixture::empty().await;
    let source = "pub fn enabled_symbol() {}\n";
    std::fs::write(fixture.root.join("main.rs"), source).unwrap();
    for client in fixture.clients() {
        let mut settings = read(&client).await;
        assert!(settings.enabled);
        settings.enabled = false;
        let disabled = save(&client, settings.clone()).await;
        assert!(!disabled.enabled);
        assert!(disabled.revision > settings.revision);
        assert_eq!(
            client
                .outline_code(fixture.worktree, "main.rs")
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(
            client
                .search_code(fixture.worktree, code_index::Search::default())
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        let lines = client
            .read_file_lines(fixture.worktree, "main.rs", 0, 1, Some(revision(source)))
            .await
            .unwrap();
        assert_eq!(lines.lines[0].text, source);
        assert_eq!(
            client
                .execute(client.prepare(Command::SaveCodeIndexSettings {
                    settings,
                    expected_revision: disabled.revision - 1,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let mut enabled = read(&client).await;
        enabled.enabled = true;
        save(&client, enabled).await;
        let outline = client
            .outline_code(fixture.worktree, "main.rs")
            .await
            .unwrap();
        assert_eq!(outline.status, code_index::Status::Parsed);
    }
    fixture.close().await;
}
