use super::*;
use sailry_protocol::{Output, plugin::storage::Entry};
use serde_json::json;

fn input(source: &str) -> Input {
    Input {
        entry: "fixture.js".into(),
        files: [("fixture.js".into(), source.into())].into(),
        handler: "run".into(),
        input: json!(null),
        target: target(),
        replies: vec![],
        policy: None,
        context: plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: plugin::Reference {
                name: "fixture".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            worktree: None,
            session: None,
        },
    }
}

fn reply() -> Reply {
    Reply {
        command: Command::ReadPluginValue { key: "seed".into() },
        response: Ok(Output::PluginValue(Entry {
            key: "seed".into(),
            revision: 0,
            present: false,
            value: Value::Null,
        })),
    }
}

#[test]
fn dispatches_through_the_production_sdk() {
    let mut input = input(
        "import {getValue} from 'sailry/sdk'; export async function run() {return getValue('seed');}",
    );
    let context = input.context.clone();
    input.replies.push(reply());
    let output = run(input).unwrap();
    assert_eq!(output.result.unwrap()["present"], false);
    assert_eq!(output.calls.len(), 1);
    assert_eq!(output.calls[0].plugin.as_ref(), Some(&context));
}

#[test]
fn rejects_incomplete_or_caught_service_mismatches() {
    let mut unused = input("export function run() {return null;}");
    unused.replies.push(reply());
    assert!(run(unused).is_err());
    let caught = input(
        "import {getValue} from 'sailry/sdk'; export async function run() {try {await getValue('unknown');} catch {} return 'caught';}",
    );
    assert!(run(caught).is_err());
}

#[test]
fn preserves_invocation_policy_and_staged_state() {
    let mut input = input(
        "import {callId,stageTurnState,readTurnState} from 'sailry/sdk'; export function run() {stageTurnState({ready:true}); return {id:callId(),state:readTurnState()};}",
    );
    let id = RequestId::new();
    input.policy = Some(Policy {
        call_id: Some(id),
        mode: WorkMode::Code,
        read_only: false,
        turn: Some(json!({})),
    });
    let output = run(input).unwrap();
    assert_eq!(
        output.result.unwrap(),
        json!({"id":id,"state":{"ready":true}})
    );
    assert_eq!(output.staged_turn, Some(json!({"ready":true})));
}
