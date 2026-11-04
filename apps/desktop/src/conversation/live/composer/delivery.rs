use sailry_protocol::{Command, SessionId, TurnId, conversation::Input};

/// One send action: idle input starts a turn; input sent while a task runs
/// joins it at the next model boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::conversation::live) enum Delivery {
    #[default]
    Submit,
    Guide {
        recipient: TurnId,
        revision: u64,
    },
}

impl Delivery {
    pub(in crate::conversation::live) fn enter(recipient: Option<(TurnId, u64)>) -> Self {
        recipient.map_or(Self::Submit, |(recipient, revision)| Self::Guide {
            recipient,
            revision,
        })
    }

    pub(in crate::conversation::live) fn command(
        self,
        session: SessionId,
        revision: u64,
        message: Input,
    ) -> Command {
        match self {
            Self::Submit => Command::SubmitTurn {
                session,
                expected_revision: revision,
                message,
            },
            Self::Guide {
                recipient,
                revision,
            } => Command::GuideTurn {
                recipient,
                expected_revision: revision,
                message,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_submits_idle_input() {
        let session = SessionId::new();
        assert!(matches!(
            Delivery::enter(None).command(session, 7, "Continue".into()),
            Command::SubmitTurn { session: actual, expected_revision: 7, .. } if actual == session
        ));
    }

    #[test]
    fn enter_preserves_captured_recipient() {
        let recipient = TurnId::new();
        let delivery = Delivery::enter(Some((recipient, 4)));
        let message = Input {
            text: "Use the attached sample".into(),
            attachments: vec![sailry_protocol::AttachmentId::new()],
            references: Vec::new(),
        };
        assert!(matches!(
            delivery.command(SessionId::new(), 9, message.clone()),
            Command::GuideTurn { recipient: actual, expected_revision: 4, message: input }
                if actual == recipient && input == message
        ));
    }
}
