use super::*;

#[test]
fn preserves_script_contracts() {
    for (scenario, check) in [
        (
            "preserves_interrupted_dispatch_outcomes",
            (|scenario: &'static str| {
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
                    let fault = result.unwrap_err();
                    assert_eq!(fault.code, expected.code, "{scenario}");
                    assert_eq!(fault.message, expected.message, "{scenario}");
                    let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) =
                        fault.details
                    else {
                        panic!("{scenario}: safe callback diagnostic expected");
                    };
                    assert_eq!(report.code, code, "{scenario}");
                    assert_eq!(
                        report.source,
                        sailry_protocol::notification::SystemSource::PluginCallback,
                        "{scenario}"
                    );
                    assert!(
                        report.budget_ms.is_some_and(|budget| budget > 0),
                        "{scenario}"
                    );
                    assert!(report.elapsed_ms.is_some(), "{scenario}");
                    assert!(
                        !serde_json::to_string(&report)
                            .unwrap()
                            .contains("original SDK outcome"),
                        "{scenario}"
                    );
                    assert_eq!(*calls.lock().unwrap(), 1, "{scenario}");
                }
            }) as fn(&'static str),
        ),
        (
            "read_callbacks_reject_mutations_before_dispatch",
            (|scenario: &'static str| {
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
                        assert!(
                            matches!(
                                request.command,
                                sailry_protocol::Command::ReadPluginValue { .. }
                            ),
                            "{scenario}"
                        );
                        Ok(Output::PluginValue(plugin::storage::Entry {
                            key: "seed".into(),
                            revision: 1,
                            value: json!(1),
                            present: true,
                        }))
                    },
                )
                .unwrap();
                assert_eq!(result["entry"]["value"], 1, "{scenario}");
                assert_eq!(
                    result["errors"],
                    json!([
                        "permission_denied",
                        "permission_denied",
                        "permission_denied",
                        "permission_denied"
                    ]),
                    "{scenario}"
                );
                assert_eq!(calls.lock().unwrap().len(), 1, "{scenario}");
            }),
        ),
        (
            "retains_provider_operation_identity",
            (|scenario: &'static str| {
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
                assert_eq!(result["status"]["status"], 200, "{scenario}");
                assert_eq!(result["first"], result["second"], "{scenario}");
                assert_eq!(result["invalid"], true, "{scenario}");
                let requests = requests.lock().unwrap();
                assert_eq!(requests.len(), 3, "{scenario}");
                assert!(!requests[0].command.durable(), "{scenario}");
                assert!(requests[1].command.durable(), "{scenario}");
                assert_eq!(requests[1], requests[2], "{scenario}");
                let sailry_protocol::Command::ExecuteProviderHttp {
                    expected_revision,
                    request,
                    input,
                    ..
                } = &requests[1].command
                else {
                    panic!("{scenario}: provider mutation expected")
                };
                assert_eq!(*expected_revision, 9007199254740993, "{scenario}");
                assert_eq!(request, "claim", "{scenario}");
                assert_eq!(
                    input.get("date").map(String::as_str),
                    Some("today"),
                    "{scenario}"
                );
                assert_eq!(
                    requests[1].plugin.as_ref().unwrap().package,
                    context().package,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
