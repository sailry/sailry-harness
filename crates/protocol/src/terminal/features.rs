use super::*;

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct Features {
    pub focus_reporting: bool,
    pub alternate_scroll: bool,
    pub title: String,
    pub directory: String,
    pub bell: u64,
    pub clipboard: Option<Clipboard>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Clipboard {
    pub sequence: u64,
    pub text: String,
}
