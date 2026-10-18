use super::*;

#[test]
fn queue_transactions_preserve_exact_revisions() {
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
            assert_eq!(request.plugin.as_ref(), Some(&captured));
            match request.command {
                Command::ReadConversation {
                    session: target, ..
                } => {
                    assert_eq!(target, session);
                    Ok(Output::Conversation(History {
                        sequence: 0,
                        page: Page {
                            session,
                            revision: 1,
                            entries: vec![],
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
                    assert_eq!(operations.len(), 1);
                    assert_eq!(
                        operations[0].command(&captured.package),
                        Command::SetQueuePaused {
                            session,
                            expected_revision: 9007199254740993,
                            paused: false,
                        }
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
                _ => panic!("unexpected queue request"),
            }
        },
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"previous":"9007199254740993","next":"9007199254740994"})
    );
}

#[test]
fn preserves_private_write_identity_and_revision_precision() {
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
            Environment { context: scope.clone(), target: NodeId([1; 32]), stop: CancellationToken::new() },
            move |request| {
                let (revision, present) = match request.command {
                    sailry_protocol::Command::ReadPluginConversationValue { .. } => {
                        (9007199254740993, true)
                    }
                    sailry_protocol::Command::WritePluginConversationValue {
                        expected_revision,
                        ..
                    } => {
                        assert_eq!(expected_revision, 9007199254740993);
                        (9007199254740994, true)
                    }
                    sailry_protocol::Command::RemovePluginConversationValue {
                        expected_revision,
                        ..
                    } => {
                        assert_eq!(expected_revision, 9007199254740994);
                        (9007199254740995, false)
                    }
                    _ => panic!("unexpected command"),
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
        assert_eq!(result["previous"], "9007199254740993");
        assert_eq!(result["saved"]["revision"], "9007199254740994");
        assert_eq!(result["removed"]["revision"], "9007199254740995");
        assert_eq!(result["removed"]["present"], false);
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(
            requests
                .iter()
                .all(|request| request.plugin.as_ref() == Some(&scope))
        );
        ids.push([requests[1].id, requests[2].id]);
    }
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0][0], ids[0][1]);
    assert_ne!(ids[0][0], call);
}
