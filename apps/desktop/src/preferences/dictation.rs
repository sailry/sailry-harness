use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Dictation {
    pub microphone: Option<String>,
    pub language: Language,
    #[serde(default)]
    pub model: Option<sailry_protocol::ModelId>,
}

pub(crate) use sailry_speech::Language;
