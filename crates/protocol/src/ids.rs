use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Transport-independent public endpoint identity; never contains private key material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub [u8; 32]);

macro_rules! identifier {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "Uuid", into = "Uuid")]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self { Self(Uuid::new_v4()) }
        }

        impl Default for $name {
            fn default() -> Self { Self::new() }
        }

        impl TryFrom<Uuid> for $name {
            type Error = &'static str;
            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                if value.is_nil() { Err("identifier must not be nil") } else { Ok(Self(value)) }
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self { value.0 }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl std::str::FromStr for $name {
            type Err = &'static str;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map_err(|_| "invalid identifier").and_then(Self::try_from)
            }
        }
    )+ };
}

identifier!(
    RequestId,
    ProjectId,
    WorktreeId,
    SessionId,
    TurnId,
    ApprovalId,
    CheckpointId,
    QuestionId,
    TerminalId,
    StreamId,
    AttachmentId,
    CredentialId,
    RoleId,
    SshId,
    DatabaseId,
    ProviderId,
    EventId,
    JobId,
    NotificationId,
    ScheduleId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nil_wire_identifier() {
        assert!(
            serde_json::from_str::<RequestId>("\"00000000-0000-0000-0000-000000000000\"").is_err()
        );
    }

    #[test]
    fn identity_round_trip() {
        let id = RequestId::new();
        assert_eq!(
            serde_json::from_str::<RequestId>(&serde_json::to_string(&id).unwrap()).unwrap(),
            id
        );
    }
}
