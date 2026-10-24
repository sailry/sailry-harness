use sailry_protocol::{
    TurnId,
    conversation::observation::{Outcome, Request},
};

pub(super) async fn records(
    profile: &std::path::Path,
    turn: TurnId,
    outcomes: &[Outcome],
) -> String {
    let (_database, database) =
        crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
    let mut query = database.prepare(
        "SELECT id,json_extract(body,'$.event_metadata.sailry_model_observation') FROM agent_events
         WHERE turn=?1 AND json_type(body,'$.event_metadata.sailry_model_observation')='text' ORDER BY sequence",
    ).await.unwrap();
    let mut rows = query.query([turn.to_string()]).await.unwrap();
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        let id: String = row.get(0).unwrap();
        let value: String = row.get(1).unwrap();
        records.push((id, serde_json::from_str::<Request>(&value).unwrap()));
    }
    assert_eq!(records.len(), outcomes.len() + 1);
    let first = &records[0].1;
    assert!(first.attempts.is_empty());
    assert!(first.id.parse::<sailry_protocol::RequestId>().is_ok());
    assert_eq!(records[0].0, format!("model:{}:request", first.id));
    for (index, ((id, request), outcome)) in records[1..].iter().zip(outcomes).enumerate() {
        assert_eq!(request.id, first.id);
        assert_eq!(request.boundary, first.boundary);
        assert_eq!(
            request.attempts.len(),
            1,
            "terminal events never repeat earlier attempts"
        );
        let attempt = &request.attempts[0];
        assert_eq!(attempt.index, index as u64 + 1);
        assert_eq!(attempt.outcome, *outcome);
        assert!(attempt.elapsed_us.is_some());
        assert_eq!(*id, format!("model:{}:{}", first.id, attempt.index));
    }
    first.id.clone()
}
