//! Adds an ignored model-reported amount after stopping the fixture Node.
//! Catalog estimates must remain unchanged by the provider's optional cost field.
pub async fn seed(profile: &std::path::Path) {
    let (database, db) =
        crate::database_fixture::open(&profile.join("storage/node.turso"), false).await;
    assert_eq!(db.execute(
        "UPDATE agent_events SET body=CAST(json_set(body,'$.usage.cost',0.125) AS BLOB)
         WHERE sequence=(SELECT min(sequence) FROM agent_events WHERE json_type(body,'$.usage')='object')",
        (),
    ).await.unwrap(), 1);
    crate::database_fixture::close(database, db).await;
}
