use super::*;

#[test]
fn preserves_script_contracts() {
    for (scenario, check) in [
        (
            "queue_transactions_preserve_exact_revisions",
            (|scenario: &'static str| {
                use sailry_protocol::{
                    Command, SessionId,
                    conversation::{History, Page, Queue, QueueUpdate},
                };

                let session = SessionId::new();
                let mut scope = context();
                scope.session = Some(session);
                let captured = scope.clone();
                let result = run(
                    bundle(
                        r#"
import {readConversation,prepareTransaction,completeRequest} from 'sailry/sdk';
export async function run() {
  const history=await readConversation();
  const result=await completeRequest(prepareTransaction([{kind:'queue_pause',data:{
    session:history.page.session,expected_revision:history.page.queue.revision,paused:false
  }}]));
  return {previous:history.page.queue.revision,next:result.Ok.data[0].data.queue.revision};
}
"#,
                    ),
                    "run",
                    json!(null),
                    scope,
                    NodeId([1; 32]),
                    CancellationToken::new(),
                    move |request| {
                        assert_eq!(request.plugin.as_ref(), Some(&captured), "{scenario}");
                        match request.command {
                            Command::ReadConversation {
                                session: target, ..
                            } => {
                                assert_eq!(target, session, "{scenario}");
                                Ok(Output::Conversation(History {
                                    sequence: 0,
                                    page: Page {
                                        session,
                                        revision: 1,
                                        entries: vec![].into(),
                                        runs: vec![],
                                        queue: Queue {
                                            revision: 9007199254740993,
                                            paused: true,
                                            items: vec![],
                                        },
                                        approvals: vec![],
                                        questions: vec![],
                                        children: vec![],
                                        background: vec![],
                                        next_before: None,
                                    },
                                    missing: vec![],
                                }))
                            }
                            Command::PluginTransaction { operations } => {
                                assert_eq!(operations.len(), 1, "{scenario}");
                                assert_eq!(
                                    operations[0].command(&captured.package),
                                    Command::SetQueuePaused {
                                        session,
                                        expected_revision: 9007199254740993,
                                        paused: false,
                                    },
                                    "{scenario}"
                                );
                                Ok(Output::PluginTransaction(vec![Output::Queue(
                                    QueueUpdate {
                                        session,
                                        queue: Queue {
                                            revision: 9007199254740994,
                                            paused: false,
                                            items: vec![],
                                        },
                                        runs: vec![],
                                    },
                                )]))
                            }
                            _ => panic!("{scenario}: unexpected queue request"),
                        }
                    },
                )
                .unwrap();
                assert_eq!(
                    result,
                    json!({"previous":"9007199254740993","next":"9007199254740994"}),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "preserves_private_write_identity_and_revision_precision",
            (|scenario: &'static str| {
                let call = sailry_protocol::RequestId::new();
                let mut ids = Vec::new();
                for _ in 0..2 {
                    let captured = Arc::new(Mutex::new(Vec::<Request>::new()));
                    let requests = captured.clone();
                    let state = Arc::new(Mutex::new(execution::State {
                        call_id: Some(call),
                        mode: sailry_protocol::WorkMode::Code,
                        read_only: false,
                        turn: None,
                        staged_turn: None,
                    }));
                    let scope = context();
                    let result = run_tool(
                    bundle(
                r#"
import {getConversationValue,setConversationValue,deleteConversationValue,completeRequest} from 'sailry/sdk';
export async function run() {
  const old=await getConversationValue('private');
  const saved=await completeRequest(setConversationValue('private','changed',old.revision));
  const removed=await completeRequest(deleteConversationValue('private',saved.Ok.data.revision));
  return {previous:old.revision,saved:saved.Ok.data,removed:removed.Ok.data};
}
"#,
                    ),
            "run",
                    json!({}),
                    Environment { context: scope.clone(), target: NodeId([1; 32]), stop: CancellationToken::new(),
             deadline: Instant::now() + CALL_TIMEOUT,
            },
                    move |request| {
                        let (revision, present) = match request.command {
                            sailry_protocol::Command::ReadPluginConversationValue { .. } => {
                                (9007199254740993, true)
                            }
                            sailry_protocol::Command::WritePluginConversationValue {
                                expected_revision,
                                ..
                            } => {
                                assert_eq!(expected_revision, 9007199254740993, "{scenario}");
                                (9007199254740994, true)
                            }
                            sailry_protocol::Command::RemovePluginConversationValue {
                                expected_revision,
                                ..
                            } => {
                                assert_eq!(expected_revision, 9007199254740994, "{scenario}");
                                (9007199254740995, false)
                            }
                            _ => panic!("{scenario}: unexpected command"),
                        };
                        requests.lock().unwrap().push(request);
                        Ok(Output::PluginConversationValue(plugin::storage::ConversationEntry {
                            key: "private".into(),
                            revision,
                            present,
                            restored: false,
                            value: if present {json!("changed")} else {json!(null)},
                        }))
                    },
                    state,
                )
                .unwrap();
                    assert_eq!(result["previous"], "9007199254740993", "{scenario}");
                    assert_eq!(
                        result["saved"]["revision"], "9007199254740994",
                        "{scenario}"
                    );
                    assert_eq!(
                        result["removed"]["revision"], "9007199254740995",
                        "{scenario}"
                    );
                    assert_eq!(result["removed"]["present"], false, "{scenario}");
                    let requests = captured.lock().unwrap();
                    assert_eq!(requests.len(), 3, "{scenario}");
                    assert!(
                        requests
                            .iter()
                            .all(|request| request.plugin.as_ref() == Some(&scope)),
                        "{scenario}"
                    );
                    ids.push([requests[1].id, requests[2].id]);
                }
                assert_eq!(ids[0], ids[1], "{scenario}");
                assert_ne!(ids[0][0], ids[0][1], "{scenario}");
                assert_ne!(ids[0][0], call, "{scenario}");
            }),
        ),
        (
            "reads_run_metadata_with_exact_revisions",
            (|scenario: &'static str| {
                use sailry_protocol::{
                    Command, SessionId, TurnId,
                    conversation::{
                        Status,
                        runs::{Page, Summary},
                    },
                };
                let session = SessionId::new();
                let turn = TurnId::new();
                let mut scope = context();
                scope.session = Some(session);
                let captured = scope.clone();
                let result = run(
                    bundle(
                        r#"
import {readConversationRuns,readConversationRun} from 'sailry/sdk';
export async function run() {
  const page=await readConversationRuns(null,1);
  const run=await readConversationRun(page.runs[0].turn,page.revision);
  return {revision:page.revision,queue:page.queue_revision,status:run.status};
}
"#,
                    ),
                    "run",
                    json!(null),
                    scope,
                    NodeId([1; 32]),
                    CancellationToken::new(),
                    move |request| {
                        assert_eq!(request.plugin.as_ref(), Some(&captured), "{scenario}");
                        let run = Summary {
                            turn,
                            sequence: 1,
                            status: Status::Completed,
                            origin: None,
                            error_code: None,
                        };
                        match request.command {
                            Command::ReadConversationRuns {
                                session: target,
                                before,
                                limit,
                            } => {
                                assert_eq!(
                                    (target, before, limit),
                                    (session, None, 1),
                                    "{scenario}"
                                );
                                Ok(Output::ConversationRuns(Page {
                                    session,
                                    revision: 9007199254740993,
                                    runs: vec![run],
                                    next_before: None,
                                    queue_revision: 9007199254740994,
                                }))
                            }
                            Command::ReadConversationRun {
                                session: target,
                                turn: target_turn,
                                expected_revision,
                            } => {
                                assert_eq!(
                                    (target, target_turn, expected_revision),
                                    (session, turn, Some(9007199254740993)),
                                    "{scenario}"
                                );
                                Ok(Output::ConversationRun(run))
                            }
                            _ => panic!("{scenario}: metadata read expected"),
                        }
                    },
                )
                .unwrap();
                assert_eq!(
                    result,
                    json!({"revision":"9007199254740993","queue":"9007199254740994","status":"completed"}),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
