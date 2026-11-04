use super::*;
use crate::store::agent::Invocation;
use crate::store::agent::tests::command;

async fn append(fixture: &mut Fixture, invocation: &Invocation) {
    let inputs = std::iter::once((invocation.turn.id, &invocation.message)).chain(
        invocation
            .followups
            .iter()
            .map(|input| (input.turn.id, &input.message)),
    );
    for (turn, input) in inputs {
        let mut event = fixture::message(
            "user",
            canonical::Role::User,
            vec![BlockKind::Text {
                text: input.text.clone(),
            }],
        );
        event.turn = turn;
        Box::pin(crate::store::agent::history::append(
            &mut fixture.database,
            fixture.session,
            turn,
            event,
            &fixture.events,
        ))
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn batches_remaining_inputs_in_order() {
    for stopped in [false, true] {
        for claimed in [false, true] {
            let mut fixture = Fixture::new().await;
            let (active, first) = admitted(&mut fixture).await;
            let second = controller::guide(&mut fixture, active, "Second guidance").await;
            let third = controller::guide(&mut fixture, active, "Third guidance").await;
            let waiting = fixture.queued().await;
            // Only inputs the user sent join the successor; held items stay queued.
            fixture
                .database
                .connection
                .execute(
                    "UPDATE agent_runs SET ready=1 WHERE turn=?1",
                    [waiting.to_string()],
                )
                .await
                .unwrap();
            let turns = [first, second, third, waiting];
            let mut captures = Vec::new();
            for turn in turns {
                captures.push(message(&fixture.database.connection, turn).await.unwrap());
            }
            let controls = registered(&mut fixture, active).await;
            if claimed {
                let inputs = take(&mut fixture, active, &controls).await.unwrap();
                assert_eq!(
                    inputs.iter().map(|input| input.turn.id).collect::<Vec<_>>(),
                    turns[..3]
                );
            }
            if stopped {
                runs::stop(&fixture.database.connection, active)
                    .await
                    .unwrap();
                controls.cancel(active);
            }
            let expected = if stopped {
                Status::Cancelled
            } else {
                Status::Completed
            };
            assert_eq!(
                finish(
                    &mut fixture,
                    &controls,
                    active,
                    Status::Completed,
                    None,
                    None
                )
                .await,
                expected
            );
            let queue = read(&fixture.database.connection, fixture.session)
                .await
                .unwrap();
            assert!(!queue.paused);
            assert_eq!(
                queue.items.iter().map(|item| item.turn).collect::<Vec<_>>(),
                turns
            );
            assert!(queue.items.iter().all(|item| item.ready));
            let successor = Box::pin(runs::claim(&mut fixture.database, &fixture.events))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(successor.turn, captures[0].turn);
            assert_eq!(successor.message, captures[0].message);
            assert_eq!(
                successor
                    .followups
                    .iter()
                    .map(|input| input.turn.id)
                    .collect::<Vec<_>>(),
                turns[1..]
            );
            for (input, captured) in successor.followups.iter().zip(&captures[1..]) {
                assert_eq!(input.turn, captured.turn);
                assert_eq!(input.message, captured.message);
                let run = runs::get(&fixture.database.connection, input.turn.id)
                    .await
                    .unwrap();
                assert_eq!(run.status, Status::Continued);
                assert_eq!(run.joined, Some(first));
            }
            for turn in &turns[..3] {
                assert_eq!(
                    runs::get(&fixture.database.connection, *turn)
                        .await
                        .unwrap()
                        .guidance,
                    Some(active)
                );
            }
            assert!(
                read(&fixture.database.connection, fixture.session)
                    .await
                    .unwrap()
                    .items
                    .is_empty()
            );
            assert!(
                Box::pin(runs::claim(&mut fixture.database, &fixture.events))
                    .await
                    .unwrap()
                    .is_none()
            );
            append(&mut fixture, &successor).await;
            Box::pin(runs::finish(
                &mut fixture.database,
                first,
                Status::Completed,
                None,
                &fixture.events,
            ))
            .await
            .unwrap();
            assert!(
                Box::pin(runs::claim(&mut fixture.database, &fixture.events))
                    .await
                    .unwrap()
                    .is_none()
            );
            let history = fixture.read(None, 100).await;
            let inputs: Vec<_> = history
                .page
                .entries
                .iter()
                .filter(|entry| turns.contains(&entry.turn))
                .map(|entry| entry.turn)
                .collect();
            assert_eq!(inputs, turns);
            for turn in &turns[1..] {
                let run = runs::get(&fixture.database.connection, *turn)
                    .await
                    .unwrap();
                assert_eq!(run.status, Status::Continued);
                assert!(run.started_ms.is_none());
            }
        }
    }
}

#[tokio::test]
async fn stops_at_incompatible_capture() {
    let mut fixture = Fixture::new().await;
    let (active, first) = admitted(&mut fixture).await;
    let second = controller::guide(&mut fixture, active, "Compatible guidance").await;
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let mut config = session.config.clone();
    config.effort = Effort::High;
    let Output::Session(changed) = command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: fixture.session,
            expected_revision: session.revision,
            config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let incompatible = controller::guide(&mut fixture, active, "Different settings").await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: fixture.session,
            expected_revision: changed.revision,
            config: session.config,
        },
    )
    .await;
    let following = fixture.queued().await;
    runs::start(&fixture.database.connection, following)
        .await
        .unwrap();
    runs::stop(&fixture.database.connection, active)
        .await
        .unwrap();
    Box::pin(runs::finish(
        &mut fixture.database,
        active,
        Status::Cancelled,
        None,
        &fixture.events,
    ))
    .await
    .unwrap();
    let successor = Box::pin(runs::claim(&mut fixture.database, &fixture.events))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(successor.turn.id, first);
    assert_eq!(
        successor
            .followups
            .iter()
            .map(|input| input.turn.id)
            .collect::<Vec<_>>(),
        [second]
    );
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .iter()
            .map(|item| item.turn)
            .collect::<Vec<_>>(),
        [incompatible, following]
    );
    append(&mut fixture, &successor).await;
    Box::pin(runs::finish(
        &mut fixture.database,
        first,
        Status::Completed,
        None,
        &fixture.events,
    ))
    .await
    .unwrap();
    let next = Box::pin(runs::claim(&mut fixture.database, &fixture.events))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.turn.id, incompatible);
    assert!(next.followups.is_empty());
    assert_eq!(next.turn.config.effort, Effort::High);
    append(&mut fixture, &next).await;
    Box::pin(runs::finish(
        &mut fixture.database,
        incompatible,
        Status::Completed,
        None,
        &fixture.events,
    ))
    .await
    .unwrap();
    let last = Box::pin(runs::claim(&mut fixture.database, &fixture.events))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(last.turn.id, following);
    assert!(last.followups.is_empty());
    assert_eq!(last.turn.config.effort, Effort::Low);
}

#[tokio::test]
async fn keeps_pause_without_pending_guidance() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    runs::stop(&fixture.database.connection, source)
        .await
        .unwrap();
    let waiting = fixture.queued().await;
    runs::start(&fixture.database.connection, waiting)
        .await
        .unwrap();
    runs::stop(&fixture.database.connection, active)
        .await
        .unwrap();
    Box::pin(runs::finish(
        &mut fixture.database,
        active,
        Status::Completed,
        None,
        &fixture.events,
    ))
    .await
    .unwrap();
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, waiting);
    assert!(
        Box::pin(runs::claim(&mut fixture.database, &fixture.events))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn respects_pause_after_stop() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    runs::stop(&fixture.database.connection, active)
        .await
        .unwrap();
    let revision = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .revision;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetQueuePaused {
            session: fixture.session,
            expected_revision: revision,
            paused: true,
        },
    )
    .await;
    Box::pin(runs::finish(
        &mut fixture.database,
        active,
        Status::Completed,
        None,
        &fixture.events,
    ))
    .await
    .unwrap();
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(
        Box::pin(runs::claim(&mut fixture.database, &fixture.events))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn resume_preserves_cancelled_outcome() {
    let mut fixture = Fixture::new().await;
    let (active, _) = admitted(&mut fixture).await;
    runs::stop(&fixture.database.connection, active)
        .await
        .unwrap();
    pause(&fixture.database.connection, fixture.session, false)
        .await
        .unwrap();
    assert_eq!(
        Box::pin(runs::finish(
            &mut fixture.database,
            active,
            Status::Completed,
            None,
            &fixture.events
        ))
        .await
        .unwrap(),
        Status::Cancelled
    );
}
