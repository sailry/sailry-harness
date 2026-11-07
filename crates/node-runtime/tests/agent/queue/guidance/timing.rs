use super::*;

#[tokio::test]
#[ignore = "opt-in admission timing"]
async fn measures_admission() {
    let (fixture, active) = asking().await;
    for index in 0..3 {
        let started = std::time::Instant::now();
        fixture
            .client
            .execute(fixture.client.prepare(Command::GuideTurn {
                recipient: active.id,
                expected_revision: active.revision,
                message: format!("Retain guidance input {index}").into(),
            }))
            .await
            .unwrap();
        eprintln!(
            "admission transport=remote operation=guide_turn duration_us={}",
            started.elapsed().as_micros()
        );
    }
    let started = std::time::Instant::now();
    fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::StopTurn { turn: active.id }),
        )
        .await
        .unwrap();
    eprintln!(
        "admission transport=remote operation=stop_turn duration_us={}",
        started.elapsed().as_micros()
    );
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}
