use super::*;

#[test]
fn preserves_interrupted_dispatch_outcomes() {
    for code in [ErrorCode::OutcomeUnknown, ErrorCode::PermissionDenied] {
        let expected = Fault::new(code, "original SDK outcome");
        let outcome = expected.clone();
        let calls = Arc::new(Mutex::new(0));
        let observed = calls.clone();
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let result = run_callback(
            bundle(
                r#"
                import {setValue,completeRequest} from 'sailry/sdk';
                export async function run() {
                    await completeRequest(setValue('once',true,'0'));
                    return 'not an execution receipt';
                }
            "#,
            ),
            "run",
            json!(null),
            Environment {
                context: context(),
                target: NodeId([1; 32]),
                stop,
                deadline: Instant::now() + CALL_TIMEOUT,
            },
            false,
            move |_| {
                *observed.lock().unwrap() += 1;
                cancelled.cancel();
                Err(outcome.clone())
            },
        );
        assert_eq!(result, Err(expected));
        assert_eq!(*calls.lock().unwrap(), 1);
    }
}

#[test]
fn read_callbacks_reject_mutations_before_dispatch() {
    let source = r#"
import {getValue,setValue,prepareRequest,prepareProviderHttp,completeRequest} from 'sailry/sdk';
export async function run() {
  const entry = await getValue('seed');
  const mutations = [
    () => setValue('seed',2,'1'),
    () => prepareProviderHttp('11111111-1111-4111-8111-111111111111','1','claim'),
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
            deadline: Instant::now() + CALL_TIMEOUT,
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
            "permission_denied",
            "permission_denied"
        ])
    );
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn retains_provider_operation_identity() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let result = run_callback(
        bundle(
            r#"
import {requestProviderHttp,prepareProviderHttp,completeRequest} from 'sailry/sdk';
export async function run() {
  const provider='11111111-1111-4111-8111-111111111111';
  const status=await requestProviderHttp(provider,'9007199254740993','status');
  let invalid=false;
  try { prepareProviderHttp(provider,'01','claim'); } catch { invalid=true; }
  const id=prepareProviderHttp(provider,'9007199254740993','claim',{date:'today'});
  const first=await completeRequest(id);
  const second=await completeRequest(id);
  return {status,first,second,invalid};
}
"#,
        ),
        "run",
        json!(null),
        Environment {
            context: context(),
            target: NodeId([1; 32]),
            stop: CancellationToken::new(),
            deadline: Instant::now() + CALL_TIMEOUT,
        },
        false,
        move |request| {
            observed.lock().unwrap().push(request);
            Ok(Output::PluginHttp(plugin::http::Response {
                status: 200,
                headers: Default::default(),
                body: "public status".into(),
            }))
        },
    )
    .unwrap();
    assert_eq!(result["status"]["status"], 200);
    assert_eq!(result["first"], result["second"]);
    assert_eq!(result["invalid"], true);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].command.durable());
    assert!(requests[1].command.durable());
    assert_eq!(requests[1], requests[2]);
    let sailry_protocol::Command::ExecuteProviderHttp {
        expected_revision,
        request,
        input,
        ..
    } = &requests[1].command
    else {
        panic!("provider mutation expected")
    };
    assert_eq!(*expected_revision, 9007199254740993);
    assert_eq!(request, "claim");
    assert_eq!(input.get("date").map(String::as_str), Some("today"));
    assert_eq!(
        requests[1].plugin.as_ref().unwrap().package,
        context().package
    );
}
