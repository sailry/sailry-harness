use super::*;

#[test]
fn read_callbacks_reject_mutations_before_dispatch() {
    let source = r#"
import {getValue,setValue,prepareRequest,completeRequest} from 'sailry/sdk';
export async function run() {
  const entry = await getValue('seed');
  const mutations = [
    () => setValue('seed',2,'1'),
    () => prepareRequest({kind:'write_file',data:{worktree:'11111111-1111-4111-8111-111111111111',path:'untouched.txt',text:'Changed',expected_revision:null}}),
    () => prepareRequest({kind:'cancel_web_fetch',data:{request:'11111111-1111-4111-8111-111111111111'}})
  ];
  const errors=[];
  for (const mutation of mutations) {
    try { const result=await completeRequest(mutation()); errors.push(result.Err?.code ?? null); }
    catch (error) { errors.push(error.code); }
  }
  return {entry,errors};
}
"#;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let captured = calls.clone();
    let result = run_callback(
        bundle(source),
        "run",
        json!({}),
        Environment {
            context: plugin::Context {
                invocation: None,
                ..context()
            },
            target: NodeId([1; 32]),
            stop: CancellationToken::new(),
        },
        true,
        move |request| {
            captured.lock().unwrap().push(request.command.clone());
            assert!(matches!(
                request.command,
                sailry_protocol::Command::ReadPluginValue { .. }
            ));
            Ok(Output::PluginValue(plugin::storage::Entry {
                key: "seed".into(),
                revision: 1,
                value: json!(1),
                present: true,
            }))
        },
    )
    .unwrap();
    assert_eq!(result["entry"]["value"], 1);
    assert_eq!(
        result["errors"],
        json!([
            "permission_denied",
            "permission_denied",
            "permission_denied"
        ])
    );
    assert_eq!(calls.lock().unwrap().len(), 1);
}
