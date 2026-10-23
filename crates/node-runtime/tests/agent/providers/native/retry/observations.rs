use sailry_protocol::{
    TurnId,
    conversation::observation::{Outcome, Request},
};

pub(super) fn records(profile: &std::path::Path, turn: TurnId, outcomes: &[Outcome]) -> String {
    let database = rusqlite::Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut query = database.prepare(
        "SELECT id,json_extract(body,'$.event_metadata.sailry_model_observation') FROM agent_events
         WHERE turn=?1 AND json_type(body,'$.event_metadata.sailry_model_observation')='text' ORDER BY sequence",
    ).unwrap();
    let records = query
        .query_map([turn.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (id, value) = row.unwrap();
            (id, serde_json::from_str::<Request>(&value).unwrap())
        })
        .collect::<Vec<_>>();
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
