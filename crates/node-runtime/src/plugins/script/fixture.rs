//! Production script and SDK execution with explicit, in-memory service replies.
//! This host does not start a Node or prove persistence, transport, or UI behavior.
use super::{Bundle, Environment, execution};
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Fault, NodeId, Request, RequestId, WorkMode, plugin};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub entry: String,
    pub files: std::collections::BTreeMap<String, String>,
    pub handler: String,
    pub input: Value,
    pub context: plugin::Context,
    #[serde(default = "target")]
    pub target: NodeId,
    #[serde(default)]
    pub replies: Vec<Reply>,
    pub policy: Option<Policy>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub command: Command,
    pub response: sailry_link::Response,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub call_id: Option<RequestId>,
    pub mode: WorkMode,
    pub read_only: bool,
    pub turn: Option<Value>,
}

#[derive(Serialize)]
pub struct Report {
    pub result: Result<Value, Fault>,
    pub calls: Vec<Request>,
    pub staged_turn: Option<Value>,
}

fn target() -> NodeId {
    NodeId([1; 32])
}

/// Rejects missing, unexpected, or unused replies even if a script catches the fault.
pub fn run(input: Input) -> Result<Report, Fault> {
    struct Service {
        replies: VecDeque<Reply>,
        calls: Vec<Request>,
        bytes: usize,
        mismatch: bool,
    }
    let service = Arc::new(Mutex::new(Service {
        replies: input.replies.into(),
        calls: Vec::new(),
        bytes: 0,
        mismatch: false,
    }));
    let dispatched = service.clone();
    let dispatch = move |request: Request| {
        let mut service = dispatched.lock().expect("fixture service lock");
        let bytes = serde_json::to_vec(&request)
            .map_err(|_| Fault::invalid("invalid fixture request"))?
            .len();
        if bytes > 8 * 1024 * 1024 - service.bytes {
            service.mismatch = true;
            return Err(Fault::invalid("plugin fixture call trace exceeds limit"));
        }
        service.bytes += bytes;
        let reply = service.replies.pop_front();
        let matches = reply
            .as_ref()
            .is_some_and(|reply| reply.command == request.command);
        service.calls.push(request);
        if !matches {
            service.mismatch = true;
            return Err(Fault::invalid(
                "plugin fixture received an unexpected request",
            ));
        }
        reply.expect("matched reply").response
    };
    let state = input.policy.map(|policy| {
        Arc::new(Mutex::new(execution::State {
            call_id: policy.call_id,
            mode: policy.mode,
            read_only: policy.read_only,
            turn: policy.turn,
            staged_turn: None,
        }))
    });
    let bundle = Bundle {
        entry: input.entry,
        files: input.files,
    };
    let stop = CancellationToken::new();
    let result = if let Some(state) = &state {
        super::run_tool(
            bundle,
            &input.handler,
            input.input,
            Environment {
                context: input.context,
                target: input.target,
                stop,
            },
            dispatch,
            state.clone(),
        )
    } else {
        super::run(
            bundle,
            &input.handler,
            input.input,
            input.context,
            input.target,
            stop,
            dispatch,
        )
    };
    let mut service = service.lock().expect("fixture service lock");
    if service.mismatch || !service.replies.is_empty() {
        return Err(Fault::invalid(
            "plugin fixture replies did not match all requests",
        ));
    }
    let staged_turn = state.as_ref().and_then(|state| {
        let state = state.lock().ok()?;
        state.staged_turn.clone()
    });
    Ok(Report {
        result,
        calls: std::mem::take(&mut service.calls),
        staged_turn,
    })
}
