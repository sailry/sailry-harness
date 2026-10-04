use super::*;
use sailry_protocol::{RequestId, TurnId};

pub(super) fn call(
    turn: TurnId,
    package: &plugin::Reference,
    tool: &str,
    call: &str,
) -> adk_core::Result<RequestId> {
    let encoded = serde_json::to_vec(&(turn, package, tool, call))
        .map_err(|_| AdkError::tool("invalid plugin tool identity"))?;
    let mut identity = blake3::Hasher::new_derive_key("Sailry plugin tool call v1");
    identity.update(&encoded);
    let mut bytes: [u8; 16] = identity.finalize().as_bytes()[..16].try_into().unwrap();
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    RequestId::try_from(uuid::Uuid::from_bytes(bytes))
        .map_err(|_| AdkError::tool("invalid plugin tool identity"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_only_the_captured_call() {
        let turn = TurnId::new();
        let package = plugin::Reference {
            name: "reminders".into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        };
        let original = call(turn, &package, "reminders", "create-1").unwrap();
        assert_eq!(
            original,
            call(turn, &package, "reminders", "create-1").unwrap()
        );
        assert_ne!(
            original,
            call(turn, &package, "reminders", "create-2").unwrap()
        );
        assert_ne!(
            original,
            call(TurnId::new(), &package, "reminders", "create-1").unwrap()
        );
        assert_ne!(
            original,
            call(turn, &package, "another", "create-1").unwrap()
        );
        let mut other = package;
        other.digest = "b".repeat(64);
        assert_ne!(
            original,
            call(turn, &other, "reminders", "create-1").unwrap()
        );
    }
}
