//! Adds isolated provider metadata after stopping the fixture Node.
//! The HTTP model fixture supplies tokens but does not expose ADK's optional cost field.
pub fn seed(profile: &std::path::Path) {
    let db = rusqlite::Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .unwrap();
    assert_eq!(db.execute(
        "UPDATE agent_events SET body=CAST(json_set(body,'$.usage_metadata.cost',0.125) AS BLOB)
         WHERE sequence=(SELECT min(sequence) FROM agent_events WHERE json_type(body,'$.usage_metadata')='object')",
        [],
    ).unwrap(), 1);
}
