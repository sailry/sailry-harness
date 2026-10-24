use super::*;

#[tokio::test]
async fn rejects_receipts_without_canonical_history() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    let receipt = card::Receipt {
        session: sailry_protocol::SessionId::new(),
        turn: sailry_protocol::TurnId::new(),
        call: sailry_protocol::RequestId::new(),
        block: 0,
    };
    assert!(captured(&db, receipt).await.is_err());
    let request = Request::new(
        NodeId([1; 32]),
        Command::ReadPluginSettings {
            package: plugin::Reference {
                name: "example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
        },
    );
    assert_eq!(
        check(&db, request.target, NodeId([2; 32]), &request)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
}
