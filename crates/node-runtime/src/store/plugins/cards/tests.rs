use super::*;

#[test]
fn rejects_receipts_without_canonical_history() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    let receipt = card::Receipt {
        session: sailry_protocol::SessionId::new(),
        turn: sailry_protocol::TurnId::new(),
        call: sailry_protocol::RequestId::new(),
        block: 0,
    };
    assert!(captured(&db, receipt).is_err());
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
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
}
