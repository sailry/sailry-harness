use super::*;
use sailry_protocol::{RequestId, TurnId};

pub(super) fn call(
    turn: TurnId,
    package: &plugin::Reference,
    tool: &str,
    call: &str,
) -> adk_core::Result<RequestId> {
    crate::plugins::cards::identity(turn, package, tool, call)
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
