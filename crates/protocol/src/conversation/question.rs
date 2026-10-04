//! User input is distinct from tool authorization, in every permission mode.
use crate::{ErrorCode, Fault, QuestionId, SessionId, TurnId};
use serde::{Deserialize, Serialize};

pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub mod form;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub prompt: String,
    pub input: Input,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Text {
        multiline: bool,
        max_bytes: usize,
    },
    Choice {
        options: Vec<String>,
        multiple: bool,
        allow_other: bool,
    },
    Plan,
    Form {
        fields: Vec<form::Field>,
    },
    Url {
        url: String,
        elicitation_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Answer {
    /// Consent to open an MCP URL, not proof that the external workflow completed.
    Opened,
    Form(serde_json::Map<String, serde_json::Value>),
    Text(String),
    Choices {
        selected: Vec<usize>,
        other: Option<String>,
    },
    /// An accepted plan points to the new coding turn admitted by the Node.
    Plan {
        turn: TurnId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Response {
    Answer(Answer),
    Decline,
    Cancel,
    StartCoding {
        expected_revision: u64,
        message: super::Input,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum State {
    Pending,
    Declined,
    Answered(Answer),
    Cancelled,
    Interrupted,
}

/// The prompt and input specification remain in the canonical history entry.
/// An accepted answer does not imply that ADK consumed it before interruption.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub id: QuestionId,
    pub session: SessionId,
    pub turn: TurnId,
    pub entry: String,
    pub index: usize,
    pub state: State,
}

/// Plan acceptance changes the session and admits its follow-up in one transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedPlan {
    pub question: Question,
    pub session: crate::Session,
    pub turn: crate::QueuedTurn,
}

impl Spec {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.prompt.trim().is_empty() || self.prompt.len() > 16 * 1024 {
            return Err(invalid("question prompt is empty or too large"));
        }
        match &self.input {
            Input::Url {
                url,
                elicitation_id,
            } => {
                if url.len() > 16 * 1024
                    || url.chars().any(char::is_control)
                    || elicitation_id.is_empty()
                    || elicitation_id.len() > 1024
                {
                    return Err(invalid("invalid MCP URL request"));
                }
                let parsed = url::Url::parse(url).map_err(|_| invalid("invalid MCP URL"))?;
                let local = parsed.host_str().is_some_and(|host| {
                    host == "localhost"
                        || host
                            .trim_matches(['[', ']'])
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|host| host.is_loopback())
                });
                if parsed.host().is_none()
                    || !matches!(parsed.scheme(), "http" | "https")
                    || parsed.scheme() == "http" && !local
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || url.split_once("://").is_some_and(|(_, rest)| {
                        rest.split(['/', '?', '#'])
                            .next()
                            .unwrap_or_default()
                            .contains('@')
                    })
                {
                    return Err(invalid(
                        "MCP URL requires HTTPS or loopback HTTP without user information",
                    ));
                }
                Ok(())
            }
            Input::Form { fields } => form::validate(fields),
            Input::Text { max_bytes, .. } if !(1..=MAX_TEXT_BYTES).contains(max_bytes) => Err(
                invalid("question text limit is outside the supported range"),
            ),
            Input::Choice { options, .. } => {
                let mut seen = std::collections::BTreeSet::new();
                if options.is_empty()
                    || options.len() > 64
                    || options.iter().any(|option| {
                        option.trim().is_empty() || option.len() > 1024 || !seen.insert(option)
                    })
                {
                    return Err(invalid("question options are empty, repeated or too large"));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn validate_answer(&self, answer: &Answer) -> Result<(), Fault> {
        self.validate()?;
        match (&self.input, answer) {
            (Input::Url { .. }, Answer::Opened) => {}
            (Input::Form { fields }, Answer::Form(values)) => {
                form::validate_answer(fields, values)?
            }
            (Input::Plan, Answer::Text(text)) if text.len() <= MAX_TEXT_BYTES => {}
            (
                Input::Text {
                    multiline,
                    max_bytes,
                },
                Answer::Text(text),
            ) => {
                if text.trim().is_empty()
                    || text.len() > *max_bytes
                    || !multiline && text.contains(['\n', '\r'])
                {
                    return Err(invalid(
                        "answer is empty, too large or requires multiple lines",
                    ));
                }
            }
            (
                Input::Choice {
                    options,
                    multiple,
                    allow_other,
                },
                Answer::Choices { selected, other },
            ) => {
                let count = selected.len() + usize::from(other.is_some());
                let mut seen = std::collections::BTreeSet::new();
                if count == 0
                    || !multiple && count != 1
                    || selected
                        .iter()
                        .any(|index| *index >= options.len() || !seen.insert(*index))
                    || other.as_ref().is_some_and(|text| {
                        !allow_other || text.trim().is_empty() || text.len() > MAX_TEXT_BYTES
                    })
                {
                    return Err(invalid("answer does not match the question choices"));
                }
            }
            _ => return Err(invalid("answer type does not match the question")),
        }
        Ok(())
    }
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests;
