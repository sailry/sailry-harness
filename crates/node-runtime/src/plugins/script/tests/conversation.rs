use super::*;

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
