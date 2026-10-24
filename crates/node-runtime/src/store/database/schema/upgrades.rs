//! Current-format defaults for added native storage fields.
use sailry_protocol::hooks::Settings;
use std::sync::LazyLock;

static HOOKS: LazyLock<Vec<u8>> = LazyLock::new(|| {
    serde_json::to_vec(&Settings::default()).expect("default hook settings encode")
});

pub(super) fn column(table: &str, name: &str) -> Option<&'static [u8]> {
    match (table, name) {
        ("turns", "instructions") => Some(b"[]"),
        ("agent_approvals", "binding" | "prepared") => Some(b"null"),
        ("agent_runs", "hooks") => Some(HOOKS.as_slice()),
        _ => None,
    }
}

pub(super) fn matches(table: &str, name: &str, value: &[u8]) -> bool {
    match (table, name) {
        ("turns", "instructions") => value == b"[]",
        ("agent_runs", "hooks") => serde_json::from_slice::<Settings>(value)
            .is_ok_and(|settings| settings == Settings::default()),
        ("agent_approvals", "binding" | "prepared") => {
            serde_json::from_slice::<serde_json::Value>(value).is_ok_and(|value| value.is_null())
        }
        _ => false,
    }
}
