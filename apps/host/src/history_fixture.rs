use sailry_client::Client;
use sailry_protocol::{
    Command, Output, Session, TurnId, Update,
    conversation::{Change, Status},
};
use std::time::Duration;

pub(crate) async fn append(client: &Client, session: &Session, count: usize) -> Vec<TurnId> {
    let mut updates = client.subscribe_conversation(session.id).await.unwrap();
    let mut turns = Vec::with_capacity(count);
    for _ in 0..count {
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Append a completed turn".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let completed = match updates.next().await.unwrap() {
                    Update::ConversationSnapshot(snapshot) => snapshot
                        .page
                        .runs
                        .iter()
                        .find(|run| run.turn == turn.id)
                        .cloned(),
                    Update::ConversationFrame(frame) => match frame.change {
                        Change::Run(run) if run.turn == turn.id => Some(run),
                        _ => None,
                    },
                    Update::ResetRequired => {
                        updates = client.subscribe_conversation(session.id).await.unwrap();
                        None
                    }
                    _ => None,
                };
                if let Some(run) = completed
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
                {
                    assert_eq!(run.status, Status::Completed);
                    break;
                }
            }
        })
        .await
        .expect("history fixture completion deadline");
        turns.push(turn.id);
    }
    turns
}
