//! Package-owned fixtures execute the same pure VM without SDK or service replies.
use super::{Bundle, Report};
use sailry_link::CancellationToken;
use sailry_protocol::Fault;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub entry: String,
    pub files: BTreeMap<String, String>,
    pub handler: String,
    pub input: Value,
}

pub fn run(input: Input) -> Result<Report, Fault> {
    let mut session = super::super::translation::Session::new(
        Bundle {
            entry: input.entry,
            files: input.files,
        },
        CancellationToken::new(),
    )?;
    let result = session.call(
        &input.handler,
        &input.input,
        sailry_protocol::MAX_FRAME_BYTES,
    );
    Ok(Report {
        result,
        calls: Vec::new(),
        staged_turn: None,
    })
}
