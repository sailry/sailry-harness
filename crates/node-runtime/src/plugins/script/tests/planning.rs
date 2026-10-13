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
    () => prepareTransaction([{kind:'conversation_write',data:{key:'bulk-scoped',value:1,expected_revision:'0'}}]),
    () => prepareTransaction([{kind:'queue_pause',data:{session:'11111111-1111-4111-8111-111111111111',expected_revision:'1',paused:false}}])
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
        assert_eq!(result["errors"], json!(vec!["permission_denied"; 8]));
    }
}

#[test]
fn admits_scoped_control_transactions_with_stable_identity() {
    let captured = Arc::new(Mutex::new(Vec::<Request>::new()));
    let requests = captured.clone();
    let state = Arc::new(Mutex::new(execution::State {
        call_id: Some(sailry_protocol::RequestId::new()),
        mode: sailry_protocol::WorkMode::Code,
        read_only: false,
        turn: None,
        staged_turn: None,
    }));
    run_tool(
        bundle(r#"
import {prepareTransaction,completeRequest} from 'sailry/sdk';
export async function run() {
  const id = prepareTransaction([
    {kind:'conversation_write',data:{key:'item',value:1,expected_revision:'0'}},
    {kind:'conversation_remove',data:{key:'item',expected_revision:'1'}},
    {kind:'queue_pause',data:{session:'11111111-1111-4111-8111-111111111111',expected_revision:'1',paused:false}},
    {kind:'continue',data:{session:'11111111-1111-4111-8111-111111111111',after:null,key:'item',scope:'conversation',expected_revision:'1',message:{text:'Continue',references:[],attachments:[]}}},
    {kind:'stop',data:{turn:'22222222-2222-4222-8222-222222222222'}}
  ]);
  await completeRequest(id); await completeRequest(id); return null;
}
"#),
        "run", json!({}),
        Environment { context: context(), target: NodeId([1; 32]), stop: CancellationToken::new() },
        move |request| {
            requests.lock().unwrap().push(request);
            Ok(Output::PluginTransaction(vec![]))
        }, state,
    ).unwrap();
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert!(
        matches!(&requests[0].command, sailry_protocol::Command::PluginTransaction { operations } if operations.len() == 5)
    );
}

#[test]
fn denies_mixed_transactions_before_dispatch() {
    for (mode, read_only) in [
        (sailry_protocol::WorkMode::Plan, false),
        (sailry_protocol::WorkMode::Code, true),
        (sailry_protocol::WorkMode::Code, false),
    ] {
        let state = Arc::new(Mutex::new(execution::State {
            call_id: Some(sailry_protocol::RequestId::new()),
            mode,
            read_only,
            turn: None,
            staged_turn: None,
        }));
        let result = run_tool(
            bundle(r#"
import {prepareTransaction} from 'sailry/sdk';
export function run() {
  const errors=[];
  for (const operation of [
    {kind:'write',data:{key:'node',value:1,expected_revision:'0'}},
    {kind:'remove',data:{key:'node',expected_revision:'1'}},
    {kind:'index',data:{key:'node',value:1,index:{fields:['title','body'],tags:[],order:0},expected_revision:'0'}},
    {kind:'submit',data:{session:'11111111-1111-4111-8111-111111111111',expected_revision:'1',message:{text:'Submit',references:[],attachments:[]}}},
    {kind:'notify',data:{title:'Notice',message:'Do not publish',kind:'info',session:null}},
    {kind:'dispatch',data:{kind:'publish',data:{topic:'test',payload:{}}}}
  ]) {
    try { prepareTransaction([{kind:'conversation_write',data:{key:'scoped',value:1,expected_revision:'0'}},operation]); errors.push(null); }
    catch(error) { errors.push(error.code); }
  }
  return errors;
}
"#),
            "run", json!({}),
            Environment { context: context(), target: NodeId([1; 32]), stop: CancellationToken::new() },
            |_| panic!("denied mixed transactions must not dispatch or partially write"), state,
        ).unwrap();
        assert_eq!(result, json!(vec!["permission_denied"; 6]));
    }
}
