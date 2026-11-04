//! Shared value adaptation for desktop and headless JavaScript SDKs.
use super::*;
use crate::{Fault, Output};
use serde_json::{Value, json};

pub fn decode(mut value: Value) -> Result<Vec<Operation>, Fault> {
    let operations = value.as_array_mut().ok_or_else(invalid)?;
    if operations.is_empty() || operations.len() > MAX_OPERATIONS {
        return Err(invalid());
    }
    for operation in operations {
        if operation["kind"] == "dispatch" {
            crate::plugin::conversation::sdk::handler(&mut operation["data"])?;
        }
        let pointer = match operation["kind"].as_str() {
            Some(
                "write"
                | "index"
                | "remove"
                | "conversation_write"
                | "conversation_remove"
                | "queue_pause"
                | "submit"
                | "continue",
            ) => Some("/data/expected_revision"),
            Some("dispatch") => match operation["data"]["kind"].as_str() {
                Some("save_handler" | "save_schedule") => Some("/data/data/revision"),
                Some("remove_handler" | "remove_schedule") => Some("/data/data/expected_revision"),
                _ => None,
            },
            _ => None,
        };
        if let Some(pointer) = pointer {
            let revision = operation.pointer_mut(pointer).ok_or_else(invalid)?;
            let exact = if let Some(text) = revision.as_str() {
                let number = text.parse::<u64>().map_err(|_| invalid())?;
                if text != number.to_string() {
                    return Err(invalid());
                }
                number
            } else {
                revision
                    .as_u64()
                    .filter(|number| *number <= 9_007_199_254_740_991)
                    .ok_or_else(invalid)?
            };
            *revision = json!(exact);
        }
    }
    let operations: Vec<Operation> = serde_json::from_value(value).map_err(|_| invalid())?;
    validate(&operations)?;
    Ok(operations)
}

/// Writable revisions cross the JavaScript boundary as decimal strings.
pub fn output(output: Output) -> Value {
    let mut value = serde_json::to_value(&output).expect("protocol values are serializable");
    match output {
        // The model SDK lists definitions; execution availability stays in Node metadata.
        Output::LocalModels(catalog) => {
            value["data"] = json!(catalog.models);
        }
        Output::ConnectorTypes(types) => {
            for (value, kind) in value["data"]
                .as_array_mut()
                .expect("typed connector types")
                .iter_mut()
                .zip(types)
            {
                value["package"]["settings_revision"] =
                    json!(kind.package.settings_revision.to_string());
            }
        }
        Output::Connections(rows) => {
            for (value, row) in value["data"]
                .as_array_mut()
                .expect("typed connections")
                .iter_mut()
                .zip(rows)
            {
                value["revision"] = json!(row.revision.to_string());
                value["package"]["settings_revision"] =
                    json!(row.package.settings_revision.to_string());
            }
        }
        Output::Connection(row) => {
            value["data"]["revision"] = json!(row.revision.to_string());
            value["data"]["package"]["settings_revision"] =
                json!(row.package.settings_revision.to_string());
        }
        Output::ConnectorLogin(attempt) => {
            value["data"]["revision"] = json!(attempt.revision.to_string());
            value["data"]["package"]["settings_revision"] =
                json!(attempt.package.settings_revision.to_string());
        }
        Output::PluginTransaction(outputs) => {
            value["data"] = Value::Array(outputs.into_iter().map(self::output).collect());
        }
        Output::Installations(tasks) => {
            for (value, task) in value["data"]
                .as_array_mut()
                .expect("typed installations")
                .iter_mut()
                .zip(tasks)
            {
                value["revision"] = json!(task.revision.to_string());
                value["target"] = json!("node");
            }
        }
        Output::PluginValue(entry) => value["data"]["revision"] = json!(entry.revision.to_string()),
        Output::PluginConversationValue(entry) => {
            value["data"]["revision"] = json!(entry.revision.to_string())
        }
        Output::Session(session) => session_revisions(&mut value["data"], &session),
        Output::AssociatedSessions(page) => {
            for (value, session) in value["data"]["sessions"]
                .as_array_mut()
                .expect("typed sessions")
                .iter_mut()
                .zip(page.sessions)
            {
                session_revisions(value, &session);
            }
        }
        Output::Teams(teams) => {
            for (value, team) in value["data"]
                .as_array_mut()
                .expect("typed teams")
                .iter_mut()
                .zip(teams)
            {
                value["revision"] = json!(team.revision.to_string());
            }
        }
        Output::Team(team) => value["data"]["revision"] = json!(team.revision.to_string()),
        Output::QueuedTurn(turn) => {
            if let Some(team) = turn.roles.team {
                value["data"]["roles"]["team"]["revision"] = json!(team.revision.to_string());
            }
        }
        Output::Conversation(history) => {
            value["data"]["page"]["queue"]["revision"] =
                json!(history.page.queue.revision.to_string());
        }
        Output::ConversationRuns(page) => {
            value["data"]["revision"] = json!(page.revision.to_string());
            value["data"]["queue_revision"] = json!(page.queue_revision.to_string());
        }
        Output::Queue(update) => {
            value["data"]["queue"]["revision"] = json!(update.queue.revision.to_string());
        }
        Output::PluginSettings(settings) => {
            value["data"]["package"]["settings_revision"] =
                json!(settings.package.settings_revision.to_string());
        }
        Output::PluginSearch(page) => {
            for (value, entry) in value["data"]["entries"]
                .as_array_mut()
                .expect("typed entries")
                .iter_mut()
                .zip(page.entries)
            {
                value["revision"] = json!(entry.revision.to_string());
            }
        }
        Output::KnowledgeCollection(collection) => {
            value["data"]["revision"] = json!(collection.revision.to_string());
        }
        Output::KnowledgeCollections(collections) => {
            for (value, collection) in value["data"]
                .as_array_mut()
                .expect("typed collections")
                .iter_mut()
                .zip(collections)
            {
                value["revision"] = json!(collection.revision.to_string());
            }
        }
        Output::Dispatch(dispatch) => {
            if matches!(
                dispatch,
                dispatch::Output::Handler(_) | dispatch::Output::Schedule(_)
            ) {
                let record = &mut value["data"]["data"];
                record["revision"] = json!(
                    record["revision"]
                        .as_u64()
                        .expect("typed revision")
                        .to_string()
                );
            }
            callback_revisions(&mut value["data"]);
        }
        _ => {}
    }
    value
}

/// Adapt nested team selections without changing the dispatch record's revision shape.
pub fn callback_revisions(value: &mut Value) {
    match value["kind"].as_str() {
        Some("handler") => callback_revision(&mut value["data"]),
        Some("handlers") => {
            for record in value["data"].as_array_mut().expect("typed handlers") {
                callback_revision(record);
            }
        }
        _ => {}
    }
}

fn callback_revision(value: &mut Value) {
    if let Some(revision) = value
        .pointer_mut("/callback/command/data/team/revision")
        .filter(|value| !value.is_null())
    {
        *revision = json!(revision.as_u64().expect("typed team revision").to_string());
    }
}

fn session_revisions(value: &mut Value, session: &crate::Session) {
    value["revision"] = json!(session.revision.to_string());
    if let Some(team) = session.roles.team {
        value["roles"]["team"]["revision"] = json!(team.revision.to_string());
    }
}

fn invalid() -> Fault {
    Fault::invalid("invalid plugin transaction or revision")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "nested_team_revisions",
                (|scenario: &'static str| {
                    let revision = 9_007_199_254_740_993_u64;
                    let handler = json!({
                        "name":"run","revision":"0","enabled":true,"source":{"package":"fixture","topic":"tick"},"queue":"tasks",
                        "callback":{"scope":{"worktree":null,"session":null},"completion":"turn","bindings":{},
                            "command":{"kind":"start_session","data":{"project":null,"worktree":null,"config":null,"title":"Task", "message":{"text":"Run","attachments":[]},
                                "execution":"team","team":{"id":crate::TeamId::new(),"revision":revision.to_string()}}}}
                    });
                    let operations = decode(
                        json!([{"kind":"dispatch","data":{"kind":"save_handler","data":handler}}]),
                    )
                    .unwrap();
                    let Operation::Dispatch(dispatch::Command::SaveHandler(handler)) =
                        &operations[0]
                    else {
                        panic!("{scenario}: handler expected")
                    };
                    let Command::StartSession(start) = &*handler.callback.command else {
                        panic!("{scenario}: start expected")
                    };
                    assert_eq!(start.team.unwrap().revision, Some(revision), "{scenario}");
                    for result in [
                        dispatch::Output::Handler(handler.clone()),
                        dispatch::Output::Handlers(vec![handler.clone()]),
                    ] {
                        let value = output(Output::Dispatch(result));
                        let record = if value["data"]["kind"] == "handlers" {
                            assert_eq!(value["data"]["data"][0]["revision"], 0, "{scenario}");
                            &value["data"]["data"][0]
                        } else {
                            assert_eq!(value["data"]["data"]["revision"], "0", "{scenario}");
                            &value["data"]["data"]
                        };
                        assert_eq!(
                            record["callback"]["command"]["data"]["team"]["revision"],
                            revision.to_string(),
                            "{scenario}"
                        );
                    }
                }) as fn(&'static str),
            ),
            (
                "session_team_revisions",
                (|scenario: &'static str| {
                    use crate::*;
                    let revision = 9_007_199_254_740_993_u64;
                    let team = team::Profile {
                        id: TeamId::new(),
                        revision,
                        key: "fixture".into(),
                        name: "Fixture".into(),
                        appearance: None,
                        description: String::new(),
                        model: None,
                        instructions: String::new(),
                        members: vec![],
                    };
                    let session = Session {
                        id: SessionId::new(),
                        association: None,
                        creation_order: 1,
                        archived: false,
                        activity: Default::default(),
                        project: None,
                        worktree: WorktreeId::new(),
                        revision,
                        config: SessionConfig {
                            features: Default::default(),
                            assistant: None,
                            resource: None,
                            provider: ProviderId::new(),
                            model: "fixture".into(),
                            effort: Effort::Default,
                            mode: WorkMode::Code,
                            options: Default::default(),
                            permission: Permission::Ask,
                            credential: None,
                        },
                        roles: role::Snapshot {
                            team: Some(team.reference()),
                            ..Default::default()
                        },
                        profile: None,
                        fork: None,
                        delegation: None,
                    };
                    let single = output(Output::Session(session.clone()));
                    assert_eq!(
                        single["data"]["revision"],
                        revision.to_string(),
                        "{scenario}"
                    );
                    assert_eq!(
                        single["data"]["roles"]["team"]["revision"],
                        revision.to_string(),
                        "{scenario}"
                    );
                    let page = output(Output::AssociatedSessions(
                        conversation::AssociatedSessions {
                            sessions: vec![session],
                            next_before: None,
                        },
                    ));
                    assert_eq!(page["data"]["sessions"][0], single["data"], "{scenario}");
                    let catalog = output(Output::Teams(vec![team.clone()]));
                    assert_eq!(
                        catalog["data"][0]["revision"],
                        revision.to_string(),
                        "{scenario}"
                    );
                    assert_eq!(
                        output(Output::Team(team))["data"],
                        catalog["data"][0],
                        "{scenario}"
                    );
                }),
            ),
            (
                "installations_keep_exact_revisions",
                (|scenario: &'static str| {
                    let task = crate::installation::Task {
                        request: crate::RequestId::new(),
                        revision: 9_007_199_254_740_993,
                        resource: crate::installation::Resource::Model {
                            kind: crate::models::Kind::Ocr,
                            id: "fixture".into(),
                        },
                        state: crate::installation::State::Queued,
                    };
                    let value = output(Output::Installations(vec![task]));
                    assert_eq!(
                        value["data"][0]["revision"], "9007199254740993",
                        "{scenario}"
                    );
                    assert_eq!(value["data"][0]["target"], "node", "{scenario}");
                }),
            ),
            (
                "queue_revisions",
                (|scenario: &'static str| {
                    use crate::conversation::{History, Page, Queue, QueueUpdate};

                    let session = crate::SessionId::new();
                    let revision = 9007199254740993;
                    let operations = decode(json!([{"kind":"queue_pause","data":{
                        "session":session,"expected_revision":revision.to_string(),"paused":false
                    }}]))
                    .unwrap();
                    let package = super::super::super::Reference {
                        name: "example".into(),
                        digest: "a".repeat(64),
                        settings_revision: 0,
                    };
                    assert_eq!(
                        operations[0].command(&package),
                        Command::SetQueuePaused {
                            session,
                            expected_revision: revision,
                            paused: false,
                        },
                        "{scenario}"
                    );
                    for invalid in [
                        json!(9007199254740992u64),
                        json!("01"),
                        json!("-1"),
                        json!("18446744073709551616"),
                        json!(1.5),
                    ] {
                        assert!(
                            decode(json!([{"kind":"queue_pause","data":{
                                "session":session,"expected_revision":invalid,"paused":false
                            }}]))
                            .is_err(),
                            "{scenario}"
                        );
                    }
                    let queue = Queue {
                        revision,
                        paused: true,
                        items: vec![],
                    };
                    let history = output(Output::Conversation(History {
                        sequence: 0,
                        page: Page {
                            session,
                            revision: 1,
                            entries: vec![].into(),
                            runs: vec![],
                            queue: queue.clone(),
                            approvals: vec![],
                            questions: vec![],
                            children: vec![],
                            background: vec![],
                            next_before: None,
                        },
                        missing: vec![],
                    }));
                    assert_eq!(
                        history["data"]["page"]["queue"]["revision"], "9007199254740993",
                        "{scenario}"
                    );
                    assert_eq!(history["data"]["page"]["revision"], 1, "{scenario}");
                    let update = Output::Queue(QueueUpdate {
                        session,
                        queue,
                        runs: vec![],
                    });
                    assert_eq!(
                        output(update.clone())["data"]["queue"]["revision"],
                        "9007199254740993",
                        "{scenario}"
                    );
                    assert_eq!(
                        output(Output::PluginTransaction(vec![update]))["data"][0]["data"]["queue"]
                            ["revision"],
                        "9007199254740993",
                        "{scenario}"
                    );
                }),
            ),
            (
                "revisions_remain_exact",
                (|scenario: &'static str| {
                    let operations = decode(json!([
                    {"kind":"write","data":{"key":"item","value":{},"expected_revision":"9007199254740993"}},
                    {"kind":"dispatch","data":{"kind":"save_schedule","data":{"id":crate::ScheduleId::new(),"revision":"9007199254740993","enabled":true,"topic":"tasks","payload":null,"timing":{"kind":"once","data":{"at_ms":1}},"next_ms":null}}}
                ])).unwrap();
                    assert!(
                        matches!(
                            operations[0],
                            Operation::Write {
                                expected_revision: 9007199254740993,
                                ..
                            }
                        ),
                        "{scenario}"
                    );
                    assert!(
                        matches!(&operations[1], Operation::Dispatch(dispatch::Command::SaveSchedule(schedule)) if schedule.revision == 9007199254740993),
                        "{scenario}"
                    );
                    for revision in [
                        json!(9007199254740992u64),
                        json!("01"),
                        json!("-1"),
                        json!("1.0"),
                        json!("18446744073709551616"),
                        json!(1.5),
                    ] {
                        assert!(
                        decode(
                            json!([{"kind":"remove","data":{"key":"item","expected_revision":revision}}])
                        )
                        .is_err(), "{scenario}");
                    }
                    let output = output(Output::PluginTransaction(vec![
                        Output::PluginValue(super::super::super::storage::Entry {
                            key: "item".into(),
                            revision: 9007199254740993,
                            value: json!(true),
                            present: true,
                        }),
                        Output::Dispatch(dispatch::Output::Schedule(dispatch::Schedule {
                            id: crate::ScheduleId::new(),
                            revision: 9007199254740993,
                            enabled: true,
                            topic: "tasks".into(),
                            payload: Value::Null,
                            timing: dispatch::Timing::Once { at_ms: 1 },
                            next_ms: None,
                        })),
                    ]));
                    assert_eq!(
                        output["data"][0]["data"]["revision"], "9007199254740993",
                        "{scenario}"
                    );
                    assert_eq!(
                        output["data"][1]["data"]["data"]["revision"], "9007199254740993",
                        "{scenario}"
                    );
                }),
            ),
            (
                "rejects_nontransactional_operations",
                (|scenario: &'static str| {
                    for operations in [
                        json!([]),
                        json!([{"kind":"call_plugin","data":{"handler":"run","input":null}}]),
                        json!([{"kind":"dispatch","data":{"kind":"list_handlers"}}]),
                    ] {
                        assert!(decode(operations).is_err(), "{scenario}");
                    }
                }),
            ),
            (
                "indexed_values_share_the_exact_revision_boundary",
                (|scenario: &'static str| {
                    let operations=decode(json!([{"kind":"index","data":{"key":"item","value":{},"index":{"fields":["title","body"],"tags":["opaque"],"order":1},"expected_revision":"9007199254740993"}}])).unwrap();
                    assert!(
                        matches!(
                            operations[0],
                            Operation::Index {
                                expected_revision: 9007199254740993,
                                ..
                            }
                        ),
                        "{scenario}"
                    );
                    let page = output(Output::PluginSearch(
                        super::super::super::storage::SearchPage {
                            entries: vec![super::super::super::storage::Entry {
                                key: "item".into(),
                                revision: 9007199254740993,
                                value: json!({}),
                                present: true,
                            }],
                            next: None,
                            now_ms: 1,
                            mode: super::super::super::storage::Mode::Keyword,
                            indexing: false,
                        },
                    ));
                    assert_eq!(
                        page["data"]["entries"][0]["revision"], "9007199254740993",
                        "{scenario}"
                    );
                }),
            ),
            (
                "conversation_revisions",
                (|scenario: &'static str| {
                    let operations = decode(json!([
                    {"kind":"conversation_write","data":{"key":"state","value":{"phase":"initial"},"expected_revision":"9007199254740993"}},
                    {"kind":"conversation_remove","data":{"key":"state","expected_revision":"9007199254740994"}}
                ])).unwrap();
                    assert!(
                        matches!(
                            operations[0],
                            Operation::ConversationWrite {
                                expected_revision: 9007199254740993,
                                ..
                            }
                        ),
                        "{scenario}"
                    );
                    assert!(
                        matches!(
                            operations[1],
                            Operation::ConversationRemove {
                                expected_revision: 9007199254740994,
                                ..
                            }
                        ),
                        "{scenario}"
                    );
                    let value = output(Output::PluginConversationValue(
                        super::super::super::storage::ConversationEntry {
                            key: "state".into(),
                            revision: 9007199254740993,
                            value: Value::Null,
                            present: false,
                            restored: true,
                        },
                    ));
                    assert_eq!(value["data"]["revision"], "9007199254740993", "{scenario}");
                    assert_eq!(value["data"]["restored"], true, "{scenario}");
                    assert_eq!(value["data"]["present"], false, "{scenario}");
                }),
            ),
            (
                "continuation_scopes",
                (|scenario: &'static str| {
                    let session = crate::SessionId::new();
                    let mut operation = json!({"kind":"continue","data":{
                        "session":session,"after":null,"message":{"text":"Continue","references":[],"attachments":[]},
                        "key":"state","expected_revision":"1"
                    }});
                    assert!(decode(json!([operation.clone()])).is_err(), "{scenario}");
                    operation["data"]["scope"] = json!("conversation");
                    let operations = decode(json!([operation])).unwrap();
                    assert!(
                        matches!(
                            operations[0],
                            Operation::Continue {
                                scope: super::super::super::storage::Scope::Conversation,
                                ..
                            }
                        ),
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
