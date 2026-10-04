use super::*;
use sailry_protocol::{NodeId, Output, Request};
use serde_json::json;
use std::sync::{Arc, Mutex};
mod conversation;
mod planning;

fn context() -> plugin::Context {
    plugin::Context {
        invocation: Some(sailry_protocol::RequestId::new()),
        turn: None,
        surface: Default::default(),
        package: plugin::Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        worktree: None,
        session: None,
    }
}

fn bundle(source: &str) -> Bundle {
    Bundle {
        entry: "dev.sailry.platform/host/main.js".into(),
        files: BTreeMap::from([("dev.sailry.platform/host/main.js".into(), source.into())]),
    }
}

fn execute(
    source: &str,
    stop: CancellationToken,
    timeout: Duration,
) -> Result<serde_json::Value, Fault> {
    evaluate(
        bundle(source),
        "run",
        json!({"value": 7}),
        Environment {
            context: context(),
            target: NodeId([1; 32]),
            stop,
        },
        |_| panic!("unexpected SDK call"),
        timeout,
        None,
    )
}

#[test]
fn supports_modules_promises_and_isolated_globals() {
    for _ in 0..2 {
        let mut code = bundle(
            "import { add } from './math.js'; export async function run(input) { globalThis.count = (globalThis.count ?? 0) + 1; return {value: await Promise.resolve(add(input.value)), count: globalThis.count}; }",
        );
        code.files.insert(
            "dev.sailry.platform/host/math.js".into(),
            "export const add = value => value + 1;".into(),
        );
        assert_eq!(
            run(
                code,
                "run",
                json!({"value":7}),
                context(),
                NodeId([1; 32]),
                CancellationToken::new(),
                |_| panic!()
            ),
            Ok(json!({"value":8,"count":1}))
        );
    }
}

#[test]
fn bounds_scripts_and_unresolved_promises() {
    let timeout = Duration::from_millis(30);
    for source in [
        "export function run() { while (true) {} }",
        "export async function run() { while (true) await Promise.resolve(); }",
    ] {
        assert_eq!(
            execute(source, CancellationToken::new(), timeout)
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
    }
    assert!(
        execute(
            "export function run() { return new Promise(() => {}); }",
            CancellationToken::new(),
            timeout
        )
        .is_err()
    );
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        execute("export function run() { return null; }", stop, timeout)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}

#[test]
fn restricts_ambient_access_and_results() {
    let result = execute("export function run() { return [typeof process, typeof fetch, typeof require, typeof document]; }", CancellationToken::new(), Duration::from_secs(2)).unwrap();
    assert_eq!(
        result,
        json!(["undefined", "undefined", "undefined", "undefined"])
    );
    let oversized = format!(
        "export function run() {{ return 'a'.repeat({}); }}",
        plugin::host::MAX_DATA_BYTES
    );
    assert!(execute(&oversized, CancellationToken::new(), Duration::from_secs(2)).is_err());
    for source in [
        "import 'std'; export function run() {}",
        "import '/etc/passwd'; export function run() {}",
        "export function run() { const a = {}; a.self = a; return a; }",
        "export function run() { const a = []; while (true) a.push(new Array(100000).fill('memory')); }",
    ] {
        assert!(execute(source, CancellationToken::new(), Duration::from_secs(2)).is_err());
    }
}

#[test]
fn keeps_revision_precision_and_original_requests() {
    let captured = Arc::new(Mutex::new(Vec::<Request>::new()));
    let requests = captured.clone();
    let scope = context();
    let result = run(
        bundle(
            r#"
        import {getValue, setValue, completeRequest, forgetRequest} from 'sailry/sdk';
        export async function run() {
            const old = await getValue('counter');
            const id = setValue('counter', 2, old.revision);
            const first = await completeRequest(id);
            const second = await completeRequest(id);
            forgetRequest(id);
            return {revision: old.revision, same: JSON.stringify(first) === JSON.stringify(second)};
        }
    "#,
        ),
        "run",
        json!(null),
        scope.clone(),
        NodeId([1; 32]),
        CancellationToken::new(),
        move |request| {
            requests.lock().unwrap().push(request);
            Ok(Output::PluginValue(plugin::storage::Entry {
                key: "counter".into(),
                revision: 9007199254740993,
                present: true,
                value: json!(1),
            }))
        },
    )
    .unwrap();
    assert_eq!(result, json!({"revision":"9007199254740993", "same":true}));
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1], requests[2]);
    assert!(
        requests
            .iter()
            .all(|request| request.plugin.as_ref() == Some(&scope))
    );
    assert!(matches!(
        requests[1].command,
        sailry_protocol::Command::WritePluginValue {
            expected_revision: 9007199254740993,
            ..
        }
    ));
}

#[test]
fn uses_frozen_settings() {
    let mut scope = context();
    scope.package.settings_revision = 9007199254740993;
    let captured = scope.clone();
    let result = run(bundle(r#"
        import {context,readSettings} from 'sailry/sdk';
        export async function run() {
            const settings = await readSettings();
            return {scope:context().package.settings_revision,
                revision:settings.package.settings_revision};
        }
    "#), "run", json!(null), scope, NodeId([1;32]), CancellationToken::new(), move |request| {
        assert_eq!(request.plugin.as_ref(), Some(&captured));
        assert!(matches!(&request.command, sailry_protocol::Command::ReadPluginSettings {package} if *package == captured.package));
        Ok(Output::PluginSettings(plugin::settings::State {
            package: captured.package.clone(), values: Default::default(), configured: vec![], keepable: vec![], ready: true,
        }))
    }).unwrap();
    assert_eq!(result["scope"], "9007199254740993");
    assert_eq!(result["revision"], "9007199254740993");
}

#[test]
fn indexed_storage_preserves_exact_revisions_and_scope() {
    let captured = Arc::new(Mutex::new(Vec::<Request>::new()));
    let requests = captured.clone();
    let scope = context();
    let result=run(bundle(r#"
        import {searchValues,indexedValue,completeRequest} from 'sailry/sdk';
        export async function run() {
            const page=await searchValues({terms:['literal'],weights:[3,1],all:['opaque'],limit:1});
            const id=indexedValue('item',{changed:true},{fields:['literal',''],tags:['opaque'],order:page.now_ms},page.entries[0].revision);
            const result=await completeRequest(id);
            return {previous:page.entries[0].revision,next:result.Ok.data.revision};
        }
    "#),"run",json!(null),scope.clone(),NodeId([1;32]),CancellationToken::new(),move |request| {
        let search=matches!(request.command,sailry_protocol::Command::SearchPluginValues(_));
        requests.lock().unwrap().push(request);
        let entry=plugin::storage::Entry { key:"item".into(),revision:9007199254740993,present:true,value:json!({}) };
        if search { Ok(Output::PluginSearch(plugin::storage::SearchPage { entries:vec![entry],next:None,now_ms:100 })) }
        else { Ok(Output::PluginValue(plugin::storage::Entry { revision:9007199254740994,..entry })) }
    }).unwrap();
    assert_eq!(
        result,
        json!({"previous":"9007199254740993","next":"9007199254740994"})
    );
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.plugin.as_ref() == Some(&scope))
    );
    assert!(
        matches!(&requests[1].command,sailry_protocol::Command::WriteIndexedPluginValue { expected_revision:9007199254740993,index,.. } if index.order==100)
    );
}

#[test]
fn refuses_recursive_calls_and_write_reads() {
    for source in [
        "import {prepareRequest} from 'sailry/sdk'; export function run() { return prepareRequest({kind:'call_plugin',data:{handler:'run',input:null}}); }",
        "const bridge = globalThis.__sailry; export function run() { return JSON.parse(bridge('read', JSON.stringify({kind:'write_plugin_value',data:{key:'x',value:1,expected_revision:0}}))); }",
    ] {
        let result = execute(source, CancellationToken::new(), Duration::from_secs(2));
        assert!(result.is_err() || result.unwrap().get("Err").is_some());
    }
}

#[test]
fn restricts_execution_capabilities_and_commands() {
    for source in [
        "import {readTurnState} from 'sailry/sdk'; export function run() { return readTurnState(); }",
        "import {setValue} from 'sailry/sdk'; export function run() { return setValue('x',1,'0'); }",
        "import {prepareRequest} from 'sailry/sdk'; export function run() { return prepareRequest({kind:'call_plugin',data:{handler:'run',input:null}}); }",
    ] {
        let state = Arc::new(Mutex::new(execution::State {
            call_id: None,
            mode: sailry_protocol::WorkMode::Code,
            read_only: false,
            turn: None,
            staged_turn: None,
        }));
        assert!(
            run_tool(
                bundle(source),
                "run",
                json!({}),
                Environment {
                    context: context(),
                    target: NodeId([1; 32]),
                    stop: CancellationToken::new()
                },
                |_| panic!("forbidden dispatch"),
                state
            )
            .is_err()
        );
    }
    assert!(execute("import {readTurnState} from 'sailry/sdk'; export function run() {return readTurnState();}",
        CancellationToken::new(), Duration::from_secs(1)).is_err());
}

#[test]
fn private_tool_writes_reuse_the_captured_call() {
    let call = sailry_protocol::RequestId::new();
    let source = "import {setValue,completeRequest} from 'sailry/sdk'; export async function run() {return completeRequest(setValue('x',1,'0'));}";
    let mut ids = Vec::new();
    for _ in 0..2 {
        let state = Arc::new(Mutex::new(execution::State {
            call_id: Some(call),
            mode: sailry_protocol::WorkMode::Code,
            read_only: false,
            turn: None,
            staged_turn: None,
        }));
        let captured = Arc::new(Mutex::new(None));
        let result = run_tool(
            bundle(source),
            "run",
            json!({}),
            Environment {
                context: context(),
                target: NodeId([1; 32]),
                stop: CancellationToken::new(),
            },
            {
                let captured = captured.clone();
                move |request| {
                    assert!(matches!(
                        request.command,
                        sailry_protocol::Command::WritePluginValue { .. }
                    ));
                    assert_eq!(request.plugin.as_ref().unwrap().package.name, "example");
                    *captured.lock().unwrap() = Some(request.id);
                    Ok(sailry_protocol::Output::PluginValue(
                        plugin::storage::Entry {
                            key: "x".into(),
                            revision: 1,
                            value: json!(1),
                            present: true,
                        },
                    ))
                }
            },
            state,
        )
        .unwrap();
        assert_eq!(result["Ok"]["data"]["revision"], "1");
        ids.push(captured.lock().unwrap().unwrap());
    }
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], call);
}

#[test]
fn scopes_call_identity_to_tool_execution() {
    let id = sailry_protocol::RequestId::new();
    let source = "import {callId,context} from 'sailry/sdk'; export function run() {return {ids:[callId(),callId()], invocation:context().invocation};}";
    let state = Arc::new(Mutex::new(execution::State {
        call_id: Some(id),
        mode: sailry_protocol::WorkMode::Code,
        read_only: false,
        turn: None,
        staged_turn: None,
    }));
    let scope = context();
    for _ in 0..2 {
        let result = run_tool(
            bundle(source),
            "run",
            json!({}),
            Environment {
                context: scope.clone(),
                target: NodeId([1; 32]),
                stop: CancellationToken::new(),
            },
            |_| panic!("identity must not dispatch"),
            state.clone(),
        )
        .unwrap();
        assert_eq!(result, json!({"ids":[id,id],"invocation":scope.invocation}));
    }
    assert!(execute(source, CancellationToken::new(), Duration::from_secs(1)).is_err());
    state.lock().unwrap().call_id = None;
    assert!(
        run_tool(
            bundle(source),
            "run",
            json!({}),
            Environment {
                context: scope,
                target: NodeId([1; 32]),
                stop: CancellationToken::new()
            },
            |_| panic!("identity must not dispatch"),
            state,
        )
        .is_err()
    );
}
