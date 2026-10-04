//! Explicitly opted-in, billable validation against a read-only provider source.
use super::*;

#[tokio::test]
#[ignore = "requires an explicitly selected provider and incurs model usage"]
async fn configured_provider() {
    let source = std::env::var("SAILRY_SMOKE_DATABASE").expect("source database required");
    let id = std::env::var("SAILRY_SMOKE_PROVIDER").expect("provider ID required");
    let model = std::env::var("SAILRY_SMOKE_MODEL").expect("model ID required");
    let prompt = std::env::var_os("SAILRY_SMOKE_PROMPT").map(|path| {
        let prompt = std::fs::read_to_string(path).expect("read game prompt");
        let state: serde_json::Value = serde_json::from_str(prompt.lines().last().unwrap())
            .expect("game state expected");
        let choices = state["choices"].as_array().expect("legal choices expected").len();
        (prompt, choices)
    }).unwrap_or_else(|| (
        "You are bidding in Dou Dizhu. Your hand is 3 4 5 6 7 8 9 10 J Q K A 2 2 Joker Joker 3. Choose one legal move: [0,1,2,3], where 0 passes and 3 is the highest bid. Reply only with JSON {\"move\":N} where N is the index of your choice, from 0 through 3.".into(), 4));
    // The caller supplies a closed profile; immutable mode never creates SQLite sidecars.
    assert!(
        !std::path::Path::new(&format!("{source}-wal")).exists(),
        "close the source Node before validation"
    );
    let db = rusqlite::Connection::open_with_flags(
        format!("file:{source}?mode=ro&immutable=1"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let body: Vec<u8> = db
        .query_row("SELECT body FROM providers WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    let stored: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(stored["listed"], true);
    let mut provider: conversation::Provider =
        serde_json::from_value(stored["provider"].clone()).unwrap();
    assert_eq!(provider.authentication, Authentication::ApiKey);
    assert!(
        provider
            .models
            .iter()
            .any(|candidate| candidate.id == model)
    );
    let credential = provider.credential.as_ref().unwrap();
    let secret = stored["authorizations"][credential.id.to_string()]["key"]
        .as_str()
        .expect("inline API key required")
        .to_owned();
    drop(db);
    provider.id = ProviderId::new();
    provider.revision = 0;
    provider.credential = None;
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        execute(
            &client,
            Command::SaveProvider {
                provider: provider.clone(),
                expected_revision: 0,
                secret: Some(Secret::new(secret.clone())),
            },
        )
        .await;
        let mut context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::GenerateText],
        )
        .await;
        context.worktree = None;
        let started = std::time::Instant::now();
        let request = client
            .prepare(Command::GeneratePluginText {
                effort: None,
                model: format!("{}/{}", provider.id, model),
                prompt: prompt.0.clone(),
            })
            .with_plugin(context);
        let mut result = client.execute(request.clone()).await;
        if matches!(&result, Err(error) if error.code == ErrorCode::OutcomeUnknown) {
            let deadline = std::time::Instant::now() + Duration::from_secs(600);
            loop {
                match client.outcome(&request).await.unwrap() {
                    RequestOutcome::Completed(completed) => {
                        result = *completed;
                        break;
                    }
                    RequestOutcome::Admitted => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "model completion deadline"
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    other => panic!("unconfirmed model outcome: {other:?}"),
                }
            }
        }
        let Output::PluginText(text) = result.unwrap() else {
            panic!("plugin text expected")
        };
        let value: serde_json::Value = serde_json::from_str(
            text.text
                .trim()
                .trim_start_matches("```json")
                .trim_end_matches("```")
                .trim(),
        )
        .expect("legal game JSON expected");
        assert!(
            value["move"]
                .as_u64()
                .is_some_and(|index| index < prompt.1 as u64)
        );
        assert!(text.tokens > 0);
        println!(
            "{} provider completion passed: model={}, tokens={}, elapsed_ms={}",
            if remote { "remote" } else { "local" },
            text.model,
            text.tokens,
            started.elapsed().as_millis()
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
