use super::*;

#[test]
fn retains_retired_receipts_without_rewriting_requests() {
    for kind in [
        "github_login",
        "mcp_authorization",
        "mcp_login",
        "plugin_authorization",
        "plugin_login",
    ] {
        let fixture = Fixture::new();
        let original = json!({"package":{"name":"fixture","digest":"frozen"},"configured":true});
        let receipt = json!({"Ok":{"kind":kind,"data":original}});
        fixture.insert("requests", &bytes(&receipt));
        let body: Vec<u8> = fixture
            .db
            .query_row("SELECT body FROM requests", [], |row| row.get(0))
            .unwrap();
        assert!(reconcile(&fixture.db).unwrap());
        let output: Result<sailry_protocol::Output, sailry_protocol::Fault> =
            serde_json::from_slice(&fixture.body("requests")).unwrap();
        let sailry_protocol::Output::RetainedRecord(record) = output.unwrap() else {
            panic!("retained record expected")
        };
        assert_eq!(record.kind, kind);
        assert_eq!(record.data, original);
        assert_eq!(
            fixture
                .db
                .query_row("SELECT body FROM requests", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            body
        );
        assert!(!reconcile(&fixture.db).unwrap());
    }
}
