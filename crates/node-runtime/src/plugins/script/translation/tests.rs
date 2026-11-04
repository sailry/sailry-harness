use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn bundle(source: &str) -> Bundle {
    Bundle {
        entry: "dev.sailry.platform/adapter.js".into(),
        files: BTreeMap::from([("dev.sailry.platform/adapter.js".into(), source.into())]),
    }
}

#[test]
fn preserves_script_contracts() {
    for (scenario, check) in [
        (
            "loads_only_the_pure_adapter_helper",
            (|scenario: &'static str| {
                let mut session = Session::new(bundle(
        "import { version, parseJson } from 'sailry/adapter'; export function decode(input) {return {version, value:parseJson(input), io:[typeof fetch,typeof __sailryDispatch]};}",
            ), CancellationToken::new()).unwrap();
                assert_eq!(
                    session
                        .call("decode", &json!("{\"text\":\"fixture\"}"), 4096)
                        .unwrap(),
                    json!({"version":"v1","value":{"text":"fixture"},"io":["undefined","undefined"]}),
                    "{scenario}"
                );
                assert!(
                    session.call("decode", &json!("not JSON"), 4096).is_err(),
                    "{scenario}"
                );
                let mut replacement = bundle("export function prepare() {return null}");
                replacement.files.insert(
                    "sailry/adapter".into(),
                    "export const version='other';".into(),
                );
                assert!(
                    Session::new(replacement, CancellationToken::new()).is_err(),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "preserves_stream_state_without_host_access",
            (|scenario: &'static str| {
                let mut bundle = bundle(
                    r#"import { label } from './label.js';
        let count = 0;
        export function prepare(input) {
          return {label, input, unavailable: [typeof fetch, typeof process,
            typeof console, typeof __sailryDispatch, typeof require]};
        }
        export async function decode(input) { return {count: ++count, input}; }"#,
                );
                bundle.files.insert(
                    "dev.sailry.platform/label.js".into(),
                    "export const label = 'example';".into(),
                );
                let mut session = Session::new(bundle, CancellationToken::new()).unwrap();
                let prepared = session
                    .call("prepare", &json!({"model":"example"}), 4096)
                    .unwrap();
                assert_eq!(
                    prepared,
                    json!({"label":"example","input":{"model":"example"},
        "unavailable":["undefined","undefined","undefined","undefined","undefined"]}),
                    "{scenario}"
                );
                assert_eq!(
                    session.call("decode", &json!(1), 4096).unwrap(),
                    json!({"count":1,"input":1}),
                    "{scenario}"
                );
                assert_eq!(
                    session.call("decode", &json!(2), 4096).unwrap(),
                    json!({"count":2,"input":2}),
                    "{scenario}"
                );
            }),
        ),
        (
            "refuses_sdk_and_external_modules",
            (|scenario: &'static str| {
                for module in [
                    "sailry/sdk",
                    "sailry/models",
                    "node:fs",
                    "https://example.test/code.js",
                ] {
                    let result = Session::new(
                        bundle(&format!(
                            "import {{x}} from '{module}'; export function prepare() {{return x;}}"
                        )),
                        CancellationToken::new(),
                    );
                    assert!(result.is_err(), "{scenario}");
                }
                let mut replacement = bundle("export function prepare() {return null}");
                replacement
                    .files
                    .insert("sailry/sdk".into(), "export const x=1".into());
                assert!(
                    Session::new(replacement, CancellationToken::new()).is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "does_not_expose_thrown_values",
            (|scenario: &'static str| {
                let private = "private-authentication-exchange";
                let mut session = Session::new(
                    bundle("export function exchange(input) {throw new Error(input.token)}"),
                    CancellationToken::new(),
                )
                .unwrap();
                let error = session
                    .call("exchange", &json!({"token":private}), 4096)
                    .unwrap_err();
                assert!(
                    !serde_json::to_string(&error).unwrap().contains(private),
                    "{scenario}"
                );
                assert!(!format!("{error:?}").contains(private), "{scenario}");
                assert!(
                    session.call("exchange", &json!(null), 4096).is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "interrupts_execution_and_retained_sessions",
            (|scenario: &'static str| {
                let stop = CancellationToken::new();
                let mut session =
                    Session::new(bundle("export function decode() {return 1}"), stop.clone())
                        .unwrap();
                stop.cancel();
                assert_eq!(
                    session.call("decode", &json!(null), 4096).unwrap_err().code,
                    ErrorCode::Cancelled,
                    "{scenario}"
                );
                assert!(
                    Session::bounded(
                        bundle("while(true) {}"),
                        CancellationToken::new(),
                        Duration::ZERO
                    )
                    .is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_values_within_caller_budget",
            (|scenario: &'static str| {
                let mut session = Session::new(
                    bundle("export function prepare(input) {return input}"),
                    CancellationToken::new(),
                )
                .unwrap();
                let input = json!({"text":"x".repeat(8192)});
                assert_eq!(
                    session.call("prepare", &input, 16384).unwrap(),
                    input,
                    "{scenario}"
                );
                assert!(session.call("prepare", &input, 4096).is_err(), "{scenario}");
                let mut session = Session::new(
                    bundle("export function decode() {return 'x'.repeat(8192)}"),
                    CancellationToken::new(),
                )
                .unwrap();
                assert!(
                    session.call("decode", &json!(null), 4096).is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "bounds_callbacks_and_output_getters",
            (|scenario: &'static str| {
                for source in [
                    "export function decode(){while(true){}}",
                    "export function decode(){return {get value(){while(true){}}}}",
                    "export function decode(){JSON.stringify=()=>{while(true){}};return JSON.stringify({});}",
                ] {
                    let mut session =
                        Session::new(bundle(source), CancellationToken::new()).unwrap();
                    session.timeout = Duration::from_millis(20);
                    let error = session.call("decode", &json!(null), 4096).unwrap_err();
                    assert_eq!(error.code, ErrorCode::Cancelled, "{scenario}");
                    assert_eq!(
                        error.message, "plugin translation deadline exceeded",
                        "{scenario}"
                    );
                    assert!(
                        session.call("decode", &json!(null), 4096).is_err(),
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "retains_the_shared_callback_controls",
            (|scenario: &'static str| {
                let mut auth = Session::new(
                    bundle("export function exchange(input){return input}"),
                    CancellationToken::new(),
                )
                .unwrap();
                assert_eq!(auth.timeout, CALL_TIMEOUT, "{scenario}");
                assert_eq!(
                    auth.call("exchange", &json!({"access":"private"}), 4096)
                        .unwrap(),
                    json!({"access":"private"}),
                    "{scenario}"
                );
                assert!(!auth.has_export("classify").unwrap(), "{scenario}");
                assert!(auth.has_export("exchange").unwrap(), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
