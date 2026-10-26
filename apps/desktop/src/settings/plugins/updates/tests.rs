use super::*;

mod handoff;
mod retry;

#[core::prelude::v1::test]
fn preserves_official_candidate_commit() {
    let listing: Listing = serde_json::from_value(serde_json::json!({
        "id":"example", "name":"example", "version":"0.1.1", "description":null,
        "display":null, "icon":null, "repository":"https://github.com/sailry/sailry-plugins",
        "bundled":false, "commit":"a".repeat(40),
    }))
    .unwrap();
    let source = official_source(&listing).unwrap();
    assert_eq!(source.git_ref.as_deref(), listing.commit.as_deref());
    assert_eq!(source.path.as_deref(), Some("example"));
}
