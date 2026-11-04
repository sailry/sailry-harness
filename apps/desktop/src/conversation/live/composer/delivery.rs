use sailry_protocol::{Command, SessionId, TurnId, conversation::Input};

/// Enter: idle input starts a turn; input sent while a task runs joins it at
/// the next model boundary. Tab queues input for after the current turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::conversation::live) enum Delivery {
    #[default]
    Submit,
    Queue,
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
            Self::Queue => Command::QueueTurn {
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
    fn captured_recipient() {
        for (scenario, check) in [
            (
                "enter_submits_idle_input",
                (|_scenario: &str| {
                    let session = SessionId::new();
                    assert!(
                        matches!(
                            Delivery::enter(None).command(session, 7, "Continue".into()),
                            Command::SubmitTurn { session: actual, expected_revision: 7, .. } if actual == session
                        ),
                        "enter submits idle input"
                    );
                }) as fn(&str),
            ),
            (
                "enter_preserves_captured_recipient",
                (|_scenario: &str| {
                    let recipient = TurnId::new();
                    let delivery = Delivery::enter(Some((recipient, 4)));
                    let message = Input {
                        text: "Use the attached sample".into(),
                        attachments: vec![sailry_protocol::AttachmentId::new()],
                        references: Vec::new(),
                    };
                    assert!(
                        matches!(
                            delivery.command(SessionId::new(), 9, message.clone()),
                            Command::GuideTurn { recipient: actual, expected_revision: 4, message: input }
                                if actual == recipient && input == message
                        ),
                        "enter preserves captured recipient"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }

    #[test]
    fn queues_input_with_attachments() {
        let session = SessionId::new();
        let message = Input {
            text: String::new(),
            attachments: vec![sailry_protocol::AttachmentId::new()],
            references: Vec::new(),
        };
        assert!(matches!(
            Delivery::Queue.command(session, 3, message.clone()),
            Command::QueueTurn { session: actual, expected_revision: 3, message: input }
                if actual == session && input == message
        ));
    }
}
