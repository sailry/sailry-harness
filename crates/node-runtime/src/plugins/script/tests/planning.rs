use super::*;

#[test]
fn denies_private_mutations_before_dispatch() {
    for (mode, read_only) in [
        (sailry_protocol::WorkMode::Plan, false),
        (sailry_protocol::WorkMode::Code, true),
    ] {
        let state = Arc::new(Mutex::new(execution::State {
            call_id: Some(sailry_protocol::RequestId::new()),
            mode,
            read_only,
            turn: None,
            staged_turn: None,
        }));
        let result = run_tool(
        bundle(
            r#"
import {getValue,setValue,deleteValue,setConversationValue,deleteConversationValue,indexedValue,prepareTransaction,completeRequest} from 'sailry/sdk';
export async function run() {
  const read = await getValue('seed');
  const writes = [
    () => setValue('new',1,'0'),
    () => deleteValue('seed','1'),
    () => indexedValue('indexed',1,{fields:['title','body'],tags:[],order:0},'0'),
    () => prepareTransaction([{kind:'write',data:{key:'bulk',value:1,expected_revision:'0'}}]),
    () => setConversationValue('scoped',1,'0'),
    () => deleteConversationValue('scoped','1'),
    () => prepareTransaction([{kind:'conversation_write',data:{key:'bulk-scoped',value:1,expected_revision:'0'}}])
  ];
  const errors=[];
  for (const write of writes) {
    try { await completeRequest(write()); errors.push(null); }
    catch (error) { errors.push(error.code); }
  }
  return {read,errors};
}
"#,
        ),
        "run",
        json!({}),
        Environment { context: context(), target: NodeId([1; 32]), stop: CancellationToken::new() },
        |request| {
            assert!(matches!(
                request.command,
                sailry_protocol::Command::ReadPluginValue { .. }
            ));
            Ok(Output::PluginValue(plugin::storage::Entry {
                key: "seed".into(),
                revision: 1,
                present: true,
                value: json!("original"),
            }))
        },
        state,
    )
    .unwrap();
        assert_eq!(result["read"]["value"], "original");
        assert_eq!(result["errors"], json!(vec!["permission_denied"; 7]));
    }
}
