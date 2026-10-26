#![cfg(any(target_os = "macos", target_os = "linux"))]
use super::*;
use sailry_protocol::hooks::{Phase, Script, Settings};

async fn fixture(remote: bool) -> (Fixture, sailry_link::CancellationToken) {
    let release = sailry_link::CancellationToken::new();
    let parent = Server::turn_tools(vec![(
        plugin_tool("delegation", "spawn_agent"),
        json!({"role":"review","task":"Wait for the held response","background":true}),
    )])
    .await;
    let child = Server::markdown_held("Final child evidence".into(), release.clone()).await;
    (Fixture::start(remote, parent, child).await, release)
}

async fn configure(fixture: &Fixture, phase: Phase, failure: bool) {
    let script = Script {
        command: format!(
            "test \"$SAILRY_HOOK_SESSION\" = '{}' || exit 0; printf once >> hook.txt; \
             while test ! -f hook-release.txt; do sleep 0.01; done; exit {}",
            fixture.session.id,
            if failure { 9 } else { 0 }
        ),
        timeout_ms: 5_000,
        ..Default::default()
    };
    let settings = Settings {
        enabled: true,
        after_tool: (phase == Phase::AfterTool).then_some(script.clone()),
        turn_end: (phase == Phase::TurnEnd).then_some(script),
        ..Default::default()
    };
    let Output::HookSettings(_) = fixture
        .client
        .execute(fixture.client.prepare(Command::SaveHooks {
            worktree: fixture.session.worktree,
            expected_revision: 0,
            settings,
        }))
        .await
        .unwrap()
    else {
        panic!("hook settings expected")
    };
}

async fn waiting(fixture: &Fixture, active: &QueuedTurn) -> Session {
    let child = children(&fixture.client, fixture.session.id, 1)
        .await
        .pop()
        .unwrap();
    fixture.child.wait_count(1).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.root().join("hook.txt").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("completion hook deadline");
    let page = history(&fixture.client, fixture.session.id).await;
    assert_eq!(status(&page, active.id), Status::Running);
    assert_eq!(page.children[0].run.status, Status::Running);
    child
}

#[tokio::test]
async fn successful_hook_keeps_background_child() {
    for remote in [false, true] {
        let (fixture, release) = fixture(remote).await;
        configure(&fixture, Phase::TurnEnd, false).await;
        let active = fixture.push("Complete after the owned hook").await;
        let child = waiting(&fixture, &active).await;
        std::fs::write(fixture.root().join("hook-release.txt"), "release").unwrap();
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Running);
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("hook.txt")).unwrap(),
            "once"
        );
        release.cancel();
        let page = history(&fixture.client, child.id).await;
        let page = finished(&fixture.client, child.id, page.runs[0].turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        fixture.close().await;
    }
}

#[tokio::test]
async fn failures_settle_background_child() {
    for remote in [false, true] {
        for phase in [Phase::AfterTool, Phase::TurnEnd] {
            let (fixture, release) = fixture(remote).await;
            configure(&fixture, phase, true).await;
            let active = fixture.push("Retain the actual hook failure").await;
            let child = waiting(&fixture, &active).await;
            std::fs::write(fixture.root().join("hook-release.txt"), "release").unwrap();
            let parent = finished(&fixture.client, fixture.session.id, active.id).await;
            assert_eq!(status(&parent, active.id), Status::Failed);
            let run = parent
                .runs
                .iter()
                .find(|run| run.turn == active.id)
                .unwrap();
            assert_eq!(run.error.as_ref().unwrap().code, ErrorCode::Unavailable);
            assert!(
                run.error
                    .as_ref()
                    .unwrap()
                    .message
                    .contains(&format!("{} hook failed: Exited(9)", phase.name()))
            );
            assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
            assert_eq!(parent.children[0].run.status, Status::Cancelled);
            let response = results(&fixture.client, &parent).await;
            assert_eq!(response.len(), 1);
            assert_eq!(response[0]["session"], json!(child.id));
            assert!(!response[0].to_string().contains("hook failed"));
            let child = history(&fixture.client, child.id).await;
            assert_eq!(child.runs[0].status, Status::Cancelled);
            assert_eq!(
                std::fs::read_to_string(fixture.root().join("hook.txt")).unwrap(),
                "once"
            );
            release.cancel();
            fixture.close().await;
        }
    }
}
