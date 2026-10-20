//! Bounded, read-only local decision inference on the selected Node.
use crate::Fault;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_QUESTIONS: usize = 16;
pub const MAX_OPTIONS: usize = 16;
pub const MAX_STATE_BYTES: usize = 16 * 1024;
pub const MAX_INSTRUCTIONS_BYTES: usize = 1024;
pub const MAX_OPTION_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub state: String,
    pub questions: BTreeMap<String, Question>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Question {
    Noul {
        instructions: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        criteria: BTreeMap<String, String>,
    },
    Choice {
        instructions: String,
        criteria: BTreeMap<String, Option<String>>,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

impl Request {
    pub fn validate(&self) -> Result<(), Fault> {
        if self
            .model
            .as_ref()
            .is_some_and(|id| !crate::models::valid_id(id))
            || !text(&self.state, MAX_STATE_BYTES)
            || self.questions.is_empty()
            || self.questions.len() > MAX_QUESTIONS
        {
            return Err(Fault::invalid("invalid decision request"));
        }
        for (id, question) in &self.questions {
            if id.is_empty() || !text(id, 128) {
                return Err(Fault::invalid("invalid decision question identity"));
            }
            let instructions = match question {
                Question::Noul {
                    instructions,
                    criteria,
                } => {
                    if criteria.iter().any(|(key, value)| {
                        !matches!(key.as_str(), "false" | "true") || !text(value, MAX_OPTION_BYTES)
                    }) {
                        return Err(Fault::invalid("invalid binary decision criteria"));
                    }
                    instructions
                }
                Question::Choice {
                    instructions,
                    criteria,
                } => {
                    if criteria.is_empty()
                        || criteria.len() > MAX_OPTIONS
                        || criteria.iter().any(|(key, value)| {
                            key.is_empty()
                                || !text(key, MAX_OPTION_BYTES)
                                || value
                                    .as_ref()
                                    .is_some_and(|value| !text(value, MAX_OPTION_BYTES))
                        })
                    {
                        return Err(Fault::invalid("invalid decision choices"));
                    }
                    instructions
                }
                Question::Score {
                    instructions,
                    criteria,
                } => {
                    if !(2..=10).contains(&criteria.len())
                        || criteria.iter().any(|value| !text(value, MAX_OPTION_BYTES))
                    {
                        return Err(Fault::invalid("invalid decision score criteria"));
                    }
                    instructions
                }
            };
            if instructions.trim().is_empty() || !text(instructions, MAX_INSTRUCTIONS_BYTES) {
                return Err(Fault::invalid("invalid decision instructions"));
            }
        }
        Ok(())
    }
}

fn text(value: &str, limit: usize) -> bool {
    value.len() <= limit && !value.contains('\0')
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Noul {
        noul: f32,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f32>,
        confidence: f32,
    },
    Score {
        score: f32,
        legend: Vec<String>,
        probabilities: Vec<f32>,
        confidence: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[cfg(test)]
mod tests;
